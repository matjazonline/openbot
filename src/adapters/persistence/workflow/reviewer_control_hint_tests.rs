//! A current operator acts on retained run facts without the former actor's account.
use super::*;
use crate::application::workflow::{
    CancelCommand, CancelResult, IdempotencyKey, WorkflowInspection, WorkflowRunTransitions,
};
use crate::use_cases::company_invite::CompanyInvitePersistence;

struct FormerActorDecision {
    fixture: FinalFixture,
    former: Uuid,
    command: SubmitDecision,
}

async fn former_actor_decision() -> FormerActorDecision {
    let s = Box::pin(Intents::new()).await;
    let former = member(&s.fixture, "admin").await;
    let source = business_source(s.owner_principal(), BusinessContinuation::NextIoStep);
    let fixture = Box::pin(FinalFixture::from_case(
        s,
        FinalCase {
            source,
            contract: literal_contract("Complete"),
            policy: None,
            run_as: Some(former),
            publication: FinalPublication::Standard,
            delivery_policy: DeliveryPolicy::Deliver,
        },
    ))
    .await;
    fixture
        .p
        .park_decision(fixture.scope, &approval_test_support::notices(&fixture.p))
        .await
        .unwrap()
        .unwrap();
    let decision = DecisionId(
        sqlx::query_scalar("SELECT id FROM workflow_decisions WHERE company_id=$1 AND run_id=$2 AND subject_kind='business'")
            .bind(fixture.scope.company.as_uuid()).bind(fixture.scope.run.as_uuid())
            .fetch_one(fixture.p.pool()).await.unwrap(),
    );
    let command = fixture.decision_command(decision, "continue").await;
    let actor: Uuid =
        sqlx::query_scalar("SELECT actor_id FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(fixture.scope.company.as_uuid())
            .bind(fixture.scope.run.as_uuid())
            .fetch_one(fixture.p.pool())
            .await
            .unwrap();
    assert_eq!(actor, former);
    assert_ne!(actor, fixture.s.owner().user_id());
    FormerActorDecision {
        fixture,
        former,
        command,
    }
}

#[derive(Clone, Copy)]
enum DecisionStage {
    Open,
    Settled,
}

#[tokio::test]
async fn workflow_reviewer_follow_former_run_actor_removal_preserves_owner_history_and_vote() {
    for stage in [DecisionStage::Open, DecisionStage::Settled] {
        let f = Box::pin(former_actor_decision()).await;
        let p = &f.fixture.p;
        if matches!(stage, DecisionStage::Settled) {
            p.submit_decision(f.command.clone()).await.unwrap();
        }
        let before = p
            .read_decision(f.command.company, f.command.actor, f.command.decision)
            .await
            .unwrap();
        p.remove_member(f.command.company.as_uuid(), f.former)
            .await
            .unwrap();
        assert_eq!(
            p.read_decision(f.command.company, f.command.actor, f.command.decision)
                .await
                .unwrap(),
            before,
            "former actor removal must preserve the whole current/settled owner projection",
        );
        let submitted = p.submit_decision(f.command.clone()).await.unwrap();
        assert_eq!(
            submitted.disposition,
            match stage {
                DecisionStage::Open => DecisionDisposition::Applied,
                DecisionStage::Settled => DecisionDisposition::Replayed,
            }
        );
        let current = p
            .read_decision(f.command.company, f.command.actor, f.command.decision)
            .await
            .unwrap();
        let mut old = f.command.clone();
        old.actor = WorkflowActor::authenticated(f.former).unwrap();
        assert!(matches!(
            p.submit_decision(old).await,
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            p.read_decision(
                f.command.company,
                WorkflowActor::authenticated(f.former).unwrap(),
                f.command.decision
            )
            .await,
            Err(AppError::NotFound(_)),
        ));
        assert_eq!(
            p.read_decision(f.command.company, f.command.actor, f.command.decision)
                .await
                .unwrap(),
            current
        );
    }
}

async fn cancel_command(f: &FormerActorDecision) -> CancelCommand {
    let scope = f.fixture.scope;
    CancelCommand {
        company_id: scope.company,
        run_id: scope.run,
        actor: f.fixture.s.owner(),
        command_key: IdempotencyKey::parse("former-actor-cancel").unwrap(),
        expected_revision: f
            .fixture
            .p
            .head(scope.company, scope.run)
            .await
            .unwrap()
            .unwrap()
            .revision,
    }
}

async fn control_state(f: &FormerActorDecision) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object('run',(SELECT to_jsonb(run) FROM workflow_runs AS run WHERE company_id=$1 AND id=$2), \
         'commands',(SELECT COALESCE(jsonb_agg(to_jsonb(command) ORDER BY command_key),'[]') FROM workflow_control_commands AS command WHERE company_id=$1 AND run_id=$2), \
         'events',(SELECT COALESCE(jsonb_agg(to_jsonb(event) ORDER BY sequence),'[]') FROM workflow_run_events AS event WHERE company_id=$1 AND run_id=$2))",
    )
    .bind(f.fixture.scope.company.as_uuid()).bind(f.fixture.scope.run.as_uuid())
    .fetch_one(f.fixture.p.pool()).await.unwrap()
}

#[tokio::test]
async fn workflow_reviewer_follow_owner_cancel_and_replay_do_not_lock_former_run_actor() {
    let f = Box::pin(former_actor_decision()).await;
    f.fixture
        .p
        .remove_member(f.fixture.scope.company.as_uuid(), f.former)
        .await
        .unwrap();
    // Reply contexts currently retain an actor FK. Exercise actual membership removal and
    // its account-lock prefix without claiming a full account deletion can commit here.
    let mut former_account = f.fixture.p.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(f.former)
        .execute(&mut *former_account)
        .await
        .unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *former_account)
        .await
        .unwrap();
    let command = cancel_command(&f).await;
    let cancel = f.fixture.p.cancel(command.clone());
    tokio::pin!(cancel);
    let first = tokio::select! {
        result = cancel.as_mut() => result.unwrap(),
        _ = blocked_by(f.fixture.p.pool(), blocker, "%FROM users%FOR SHARE%") => {
            panic!("a current owner's cancel must not wait for the former run actor account")
        }
    };
    assert!(matches!(first, CancelResult::Applied { .. }));
    let stored = control_state(&f).await;
    assert_eq!(stored["run"]["state"], "cancelled");
    assert_eq!(stored["commands"].as_array().unwrap().len(), 1);
    assert_eq!(
        stored["commands"][0]["actor_id"],
        json!(f.fixture.s.owner().user_id())
    );
    assert_eq!(f.fixture.p.cancel(command.clone()).await.unwrap(), first);
    assert_eq!(control_state(&f).await, stored);
    let mut changed = command.clone();
    changed.expected_revision.0 += 1;
    assert!(matches!(
        f.fixture.p.cancel(changed).await,
        Err(AppError::Conflict(_))
    ));
    former_account.rollback().await.unwrap();
    let mut retired = command;
    retired.actor = WorkflowActor::authenticated(f.former).unwrap();
    assert!(matches!(
        f.fixture.p.cancel(retired).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(control_state(&f).await, stored);
}
