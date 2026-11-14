use super::*;
#[path = "escalation_attention_tests.rs"]
mod attention;
#[path = "escalation_cancellation_tests.rs"]
mod cancellation;
#[path = "escalation_history_tests.rs"]
mod history;
use crate::application::workflow::escalation::{
    EscalationChoice, EscalationDecisionId, EscalationSubmission, EscalationView, SubmitEscalation,
    WorkflowEscalations,
};
use crate::entities::company_member::CompanyAccessRole;
use crate::use_cases::company_invite::CompanyInvitePersistence;

async fn pending(count: usize) -> (GuardFixture, EscalationDecisionId) {
    let people = Box::pin(PeopleFixture::new(count)).await;
    let f = Box::pin(GuardFixture::admit(people, Some(policy()), None)).await;
    assert_eq!(
        f.p().fail_run_on_guard(f.failure()).await.unwrap(),
        GuardFailureOutcome::Failed
    );
    assert_eq!(f.worker().drain_once().await.unwrap(), 1);
    assert_eq!(f.worker().drain_once().await.unwrap(), count);
    let decision: Uuid = sqlx::query_scalar(
        "SELECT current_decision_id FROM workflow_escalation_progress WHERE company_id=$1",
    )
    .bind(f.scope.company.as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    (f, EscalationDecisionId::new(decision))
}

fn command(
    f: &GuardFixture,
    decision: EscalationDecisionId,
    actor: Uuid,
    key: &str,
    choice: EscalationChoice,
) -> SubmitEscalation {
    SubmitEscalation {
        company: f.scope.company,
        actor: WorkflowActor::authenticated(actor).unwrap(),
        decision,
        command: DecisionCommandKey::parse(key).unwrap(),
        choice,
    }
}

async fn view(f: &GuardFixture, decision: EscalationDecisionId, actor: Uuid) -> EscalationView {
    f.p()
        .read_escalation(
            f.scope.company,
            WorkflowActor::authenticated(actor).unwrap(),
            decision,
        )
        .await
        .unwrap()
}

async fn facts(f: &GuardFixture) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object( \
        'commands',(SELECT count(*) FROM workflow_escalation_commands WHERE company_id=$1), \
        'assignments',(SELECT count(*) FROM workflow_escalation_decisions WHERE company_id=$1), \
        'hops',(SELECT count(*) FROM workflow_run_escalations WHERE company_id=$1), \
        'notices',(SELECT count(*) FROM workflow_escalation_notices WHERE company_id=$1), \
        'active',(SELECT count(*) FROM notifications WHERE company_id=$1 AND state='active'), \
        'state',progress.state,'manual',progress.manual_assignments) \
        FROM workflow_escalation_progress AS progress WHERE company_id=$1",
    )
    .bind(f.scope.company.as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap()
}

async fn eligible_member(f: &GuardFixture) -> (Uuid, PrincipalId) {
    let fixture = &f.people.role.admission.binding.fixture;
    let user = crate::adapters::persistence::workflow::tests::message_tests::support::member(
        fixture, "member",
    )
    .await;
    sqlx::query("UPDATE users SET email_verified_at=clock_timestamp() WHERE id=$1")
        .bind(user)
        .execute(f.p().pool())
        .await
        .unwrap();
    let principal = PrincipalId::new(fixture.principal(user).await);
    crate::adapters::persistence::test_support::department_use_cases(f.p())
        .add_member(
            f.people.role.admission.binding.target.actor.user_id(),
            f.scope.company.as_uuid(),
            f.people.role.department,
            principal,
        )
        .await
        .unwrap();
    (user, principal)
}

#[tokio::test]
async fn workflow_escalation_decision_assigned_member_acknowledges_once_and_conflicting_replay_has_no_effect()
 {
    let (f, decision) = Box::pin(pending(1)).await;
    let actor = f.people.users[0];
    f.p()
        .update_member_role(f.scope.company.as_uuid(), actor, CompanyAccessRole::Member)
        .await
        .unwrap();
    let before = view(&f, decision, actor).await;
    assert!(
        !before.inspect_run,
        "assignment grants no workflow inspection"
    );
    let html = crate::adapters::http::pages::workflow_escalation_page(&before);
    assert!(html.contains(&format!(
        "<span class=\"font-mono\" title=\"Run {}\">",
        f.scope.run.as_uuid()
    )));
    assert!(before.can_acknowledge && before.can_reassign);
    assert_eq!(
        before.cancel_revision, None,
        "assignment grants no run control"
    );
    let request = command(&f, decision, actor, "ack", EscalationChoice::Acknowledge);
    assert_eq!(
        f.p().submit_escalation(request.clone()).await.unwrap(),
        EscalationSubmission::Applied
    );
    assert_eq!(
        f.p().submit_escalation(request.clone()).await.unwrap(),
        EscalationSubmission::Replayed
    );
    let mut wrong = request;
    wrong.choice = EscalationChoice::Reassign {
        principal: f.people.principals[0],
    };
    assert!(matches!(
        f.p().submit_escalation(wrong).await,
        Err(AppError::Conflict(_))
    ));
    let after = view(&f, decision, actor).await;
    assert!(after.settled_at.is_some() && !after.can_acknowledge && !after.can_reassign);
    let audit = after.command.as_ref().unwrap();
    assert_eq!(audit.actor_user, actor);
    assert_eq!(audit.actor_principal, f.people.principals[0]);
    assert_eq!(audit.choice, EscalationChoice::Acknowledge);
    assert_eq!(audit.next_decision, None);
    assert!(
        crate::adapters::http::pages::workflow_escalation_page(&after).contains(&format!(
            "Account {actor} · principal {}",
            audit.actor_principal
        ))
    );
    assert_eq!(
        facts(&f).await,
        json!({"commands":1,"assignments":1,"hops":1,"notices":1,"active":0,"state":"settled","manual":0})
    );
    assert_eq!(f.worker().drain_once().await.unwrap(), 0);
    f.p()
        .remove_member(f.scope.company.as_uuid(), actor)
        .await
        .unwrap();
    let history = view(
        &f,
        decision,
        f.people.role.admission.binding.target.actor.user_id(),
    )
    .await;
    assert_eq!(history.command.unwrap().actor_user, actor);
}

#[tokio::test]
async fn workflow_escalation_decision_reassignment_creates_one_manual_notice_and_replay_ignores_retired_target()
 {
    let (f, decision) = Box::pin(pending(2)).await;
    let actor = f.people.users[0];
    let (target_user, target) = eligible_member(&f).await;
    let request = command(
        &f,
        decision,
        actor,
        "assign",
        EscalationChoice::Reassign { principal: target },
    );
    assert_eq!(
        f.p().submit_escalation(request.clone()).await.unwrap(),
        EscalationSubmission::Applied
    );
    assert_eq!(
        facts(&f).await,
        json!({"commands":1,"assignments":2,"hops":1,"notices":3,"active":0,"state":"manual","manual":1})
    );
    assert_eq!(f.worker().drain_once().await.unwrap(), 1);
    let next: Uuid = sqlx::query_scalar(
        "SELECT current_decision_id FROM workflow_escalation_progress WHERE company_id=$1",
    )
    .bind(f.scope.company.as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    let next = EscalationDecisionId::new(next);
    let current = view(&f, next, target_user).await;
    assert_eq!(current.recipients.len(), 1);
    assert_eq!(current.recipients[0].principal, target);
    let label = current.recipients[0].label.clone();
    f.p()
        .remove_member(f.scope.company.as_uuid(), target_user)
        .await
        .unwrap();
    assert_eq!(
        f.p().submit_escalation(request).await.unwrap(),
        EscalationSubmission::Replayed
    );
    let history = view(
        &f,
        next,
        f.people.role.admission.binding.target.actor.user_id(),
    )
    .await;
    assert_eq!(history.recipients[0].label, label);
    assert!(
        f.p()
            .submit_escalation(command(
                &f,
                next,
                target_user,
                "retired",
                EscalationChoice::Acknowledge
            ))
            .await
            .is_err()
    );
    assert_eq!(facts(&f).await["commands"], 1);
}

#[tokio::test]
async fn workflow_escalation_decision_visible_unassigned_member_and_foreign_actor_cannot_settle() {
    let (f, decision) = Box::pin(pending(1)).await;
    let (outsider, _) = eligible_member(&f).await;
    let read = view(&f, decision, outsider).await;
    assert!(!read.can_acknowledge && !read.can_reassign && read.cancel_revision.is_none());
    let before = facts(&f).await;
    for actor in [outsider, Uuid::new_v4()] {
        assert!(
            f.p()
                .submit_escalation(command(
                    &f,
                    decision,
                    actor,
                    "unassigned",
                    EscalationChoice::Acknowledge
                ))
                .await
                .is_err()
        );
    }
    assert_eq!(facts(&f).await, before);
    let mut foreign = command(
        &f,
        decision,
        outsider,
        "foreign",
        EscalationChoice::Acknowledge,
    );
    foreign.company = CompanyId::new(Uuid::new_v4());
    assert!(f.p().submit_escalation(foreign).await.is_err());
}

#[tokio::test]
async fn workflow_escalation_decision_terminal_cancel_is_audited_without_reviving_the_run() {
    let (f, decision) = Box::pin(pending(1)).await;
    let owner = f.people.role.admission.binding.target.actor.user_id();
    let revision = view(&f, decision, owner).await.cancel_revision.unwrap();
    let before = f.state().await;
    let request = command(
        &f,
        decision,
        owner,
        "cancel",
        EscalationChoice::Cancel {
            expected_run_revision: revision,
        },
    );
    assert_eq!(
        f.p().submit_escalation(request.clone()).await.unwrap(),
        EscalationSubmission::Applied
    );
    assert_eq!(
        f.p().submit_escalation(request).await.unwrap(),
        EscalationSubmission::Replayed
    );
    assert_eq!(f.state().await, before);
    let settlement: String = sqlx::query_scalar(
        "SELECT settlement FROM workflow_escalation_commands WHERE company_id=$1",
    )
    .bind(f.scope.company.as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    assert_eq!(settlement, "already_terminal");
}

#[tokio::test]
async fn workflow_escalation_decision_competing_current_operators_leave_exactly_one_settlement() {
    let (f, decision) = Box::pin(pending(2)).await;
    let left = command(
        &f,
        decision,
        f.people.users[0],
        "left",
        EscalationChoice::Acknowledge,
    );
    let right = command(
        &f,
        decision,
        f.people.users[1],
        "right",
        EscalationChoice::Reassign {
            principal: f.people.principals[0],
        },
    );
    let (a, b) = tokio::join!(
        f.p().submit_escalation(left),
        f.p().submit_escalation(right)
    );
    assert_eq!(
        usize::from(a.is_ok()) + usize::from(b.is_ok()),
        1,
        "{a:?} {b:?}"
    );
    let loser = if a.is_err() { a } else { b };
    assert!(matches!(loser, Err(AppError::Conflict(_))), "{loser:?}");
    let after = facts(&f).await;
    assert_eq!(after["commands"], 1);
    assert_eq!(after["hops"], 1);
}

#[tokio::test]
async fn workflow_escalation_decision_receipt_failure_rolls_back_settlement_and_notice_withdrawal()
{
    let (f, decision) = Box::pin(pending(1)).await;
    let before = facts(&f).await;
    sqlx::raw_sql(
        "CREATE FUNCTION fail_escalation_receipt() RETURNS trigger LANGUAGE plpgsql AS $$ \
        BEGIN RAISE EXCEPTION 'receipt fault'; END $$; \
        CREATE TRIGGER fail_escalation_receipt BEFORE INSERT ON workflow_escalation_commands \
        FOR EACH ROW EXECUTE FUNCTION fail_escalation_receipt();",
    )
    .execute(f.p().pool())
    .await
    .unwrap();
    let request = command(
        &f,
        decision,
        f.people.users[0],
        "atomic",
        EscalationChoice::Acknowledge,
    );
    assert!(f.p().submit_escalation(request.clone()).await.is_err());
    assert_eq!(facts(&f).await, before);
    assert!(view(&f, decision, f.people.users[0]).await.can_acknowledge);
    sqlx::raw_sql("DROP TRIGGER fail_escalation_receipt ON workflow_escalation_commands; DROP FUNCTION fail_escalation_receipt();")
        .execute(f.p().pool()).await.unwrap();
    assert_eq!(
        f.p().submit_escalation(request).await.unwrap(),
        EscalationSubmission::Applied
    );
}

#[tokio::test]
async fn workflow_escalation_decision_manual_assignments_are_bounded_and_acknowledgement_still_works()
 {
    let (f, mut decision) = Box::pin(pending(1)).await;
    for ordinal in 1..=32 {
        let request = command(
            &f,
            decision,
            f.people.users[0],
            &format!("manual-{ordinal}"),
            EscalationChoice::Reassign {
                principal: f.people.principals[0],
            },
        );
        assert_eq!(
            f.p().submit_escalation(request).await.unwrap(),
            EscalationSubmission::Applied
        );
        let current: Uuid = sqlx::query_scalar(
            "SELECT current_decision_id FROM workflow_escalation_progress WHERE company_id=$1",
        )
        .bind(f.scope.company.as_uuid())
        .fetch_one(f.p().pool())
        .await
        .unwrap();
        decision = EscalationDecisionId::new(current);
    }
    let current = view(&f, decision, f.people.users[0]).await;
    assert!(current.can_acknowledge && !current.can_reassign);
    assert_eq!(
        current.deadline, None,
        "manual assignments have no automatic timer"
    );
    let before = facts(&f).await;
    let error = f
        .p()
        .submit_escalation(command(
            &f,
            decision,
            f.people.users[0],
            "over-limit",
            EscalationChoice::Reassign {
                principal: f.people.principals[0],
            },
        ))
        .await
        .unwrap_err();
    assert!(
        matches!(error, AppError::Conflict(ref code) if code=="escalation.manual_assignment_limit"),
        "{error:?}"
    );
    assert_eq!(facts(&f).await, before);
    f.p()
        .submit_escalation(command(
            &f,
            decision,
            f.people.users[0],
            "final-ack",
            EscalationChoice::Acknowledge,
        ))
        .await
        .unwrap();
    assert_eq!(facts(&f).await["commands"], 33);
    assert_eq!(facts(&f).await["hops"], 1);
}

#[tokio::test]
async fn workflow_escalation_decision_database_requires_command_receipts_and_keeps_them_immutable()
{
    let (f, decision) = Box::pin(pending(1)).await;
    let mut tx = f.p().pool().begin().await.unwrap();
    sqlx::query("UPDATE workflow_escalation_decisions SET settled_at=clock_timestamp(),settlement='acknowledged' WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid()).bind(decision.as_uuid()).execute(&mut *tx).await.unwrap();
    let error = tx.commit().await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("workflow_escalation_command_complete")
    );
    let request = command(
        &f,
        decision,
        f.people.users[0],
        "immutable",
        EscalationChoice::Acknowledge,
    );
    f.p().submit_escalation(request).await.unwrap();
    for sql in [
        "UPDATE workflow_escalation_commands SET command_key='other' WHERE company_id=$1",
        "DELETE FROM workflow_escalation_commands WHERE company_id=$1",
    ] {
        let error = sqlx::query(sql)
            .bind(f.scope.company.as_uuid())
            .execute(f.p().pool())
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().constraint(),
            Some("workflow_escalation_command_guard")
        );
    }
}
