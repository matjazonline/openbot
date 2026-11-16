use super::*;
use crate::adapters::persistence::workflow::tests::message_tests::support::member;
use crate::application::workflow::waits::*;

fn source(kind: &str) -> Value {
    let mut source: Value = serde_json::from_str(&registry::example(kind).unwrap().source).unwrap();
    source["steps"]["start"]["with"] =
        json!({"value":{"literal":{"result":42}},"output_schema":{"literal":true}});
    source
}

#[tokio::test]
async fn channel_reply_completion_revoked_send_role_still_permits_suppression() {
    for kind in ["data.map", "channel.no_reply"] {
        let s = Intents::new().await;
        let admin = member(&s.fixture, "admin").await;
        let f = FinalFixture::from_case(
            s,
            FinalCase {
                source: source(kind),
                contract: literal_contract("Done"),
                policy: None,
                run_as: Some(admin),
                publication: FinalPublication::Standard,
                delivery_policy: DeliveryPolicy::Deliver,
            },
        )
        .await;
        sqlx::query("UPDATE company_members SET role='member' WHERE company_id=$1 AND user_id=$2")
            .bind(f.scope.company.as_uuid())
            .bind(admin)
            .execute(f.p.pool())
            .await
            .unwrap();
        let done = f.advance().await;
        if kind == "channel.no_reply" {
            assert_eq!(done.disposition, BatchDisposition::Completed);
            assert_eq!(f.state().await["decision"], "suppressed");
        } else {
            assert_eq!(done.disposition, BatchDisposition::Failed);
            assert_eq!(f.state().await["decision"], Value::Null);
        }
    }
}

#[tokio::test]
async fn channel_reply_completion_opposite_reviewers_have_one_formal_winner() {
    let s = Intents::new().await;
    let a = member(&s.fixture, "admin").await;
    let b = member(&s.fixture, "admin").await;
    let policy = ReplyApprovalPolicy::new(
        vec![s.fixture.principal(a).await, s.fixture.principal(b).await],
        60,
    )
    .unwrap();
    let f = FinalFixture::from_case(
        s,
        FinalCase {
            source: source("data.map"),
            contract: literal_contract("Done"),
            policy: Some(policy),
            run_as: None,
            publication: FinalPublication::Standard,
            delivery_policy: DeliveryPolicy::Deliver,
        },
    )
    .await;
    f.advance().await;
    let mut left = f.command("approve").await;
    left.actor = WorkflowActor::authenticated(a).unwrap();
    let mut right = left.clone();
    right.actor = WorkflowActor::authenticated(b).unwrap();
    right.command_key = DecisionCommandKey::parse("opposite-reviewer").unwrap();
    let barrier = tokio::sync::Barrier::new(2);
    let submit = |command| async {
        barrier.wait().await;
        f.p.submit_decision(command).await
    };
    let (left, right) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(submit(left), submit(right))
    })
    .await
    .unwrap();
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    assert!(matches!(
        left.err().or_else(|| right.err()).unwrap(),
        AppError::Conflict(_)
    ));
    assert!(matches!(
        f.p.resume_finalization(f.scope).await.unwrap(),
        FinalizationProgress::Completed(_)
    ));
}

#[tokio::test]
async fn channel_reply_completion_review_timeout_never_publishes_unapproved_output() {
    let s = Intents::new().await;
    let policy = ReplyApprovalPolicy::new(vec![s.owner_principal()], 1).unwrap();
    let f = FinalFixture::from_case(
        s,
        FinalCase {
            source: source("data.map"),
            contract: literal_contract("Done"),
            policy: Some(policy),
            run_as: None,
            publication: FinalPublication::Standard,
            delivery_policy: DeliveryPolicy::Deliver,
        },
    )
    .await;
    f.advance().await;
    tokio::time::sleep(Duration::from_millis(1100)).await;
    f.p.resume_wait(f.scope).await.unwrap();
    assert_eq!(f.state().await["run"], "failed");
    assert_eq!(f.state().await["decision"], Value::Null);
    assert!(matches!(
        f.p.resume_finalization(f.scope).await.unwrap(),
        FinalizationProgress::Refused
    ));
}

#[tokio::test]
async fn channel_reply_completion_approved_job_cannot_claim_a_handler_with_sql() {
    let f = FinalFixture::new("data.map", true).await;
    f.advance().await;
    f.p.submit_decision(f.command("approve").await)
        .await
        .unwrap();
    let error=sqlx::query("UPDATE background_tasks SET status='processing',worker_id=$3,execution_generation=$4,locked_at=clock_timestamp(),lock_expires_at=clock_timestamp()+interval '30 seconds' WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid()).bind(f.scope.job.0).bind(Uuid::new_v4()).bind(Uuid::new_v4()).execute(f.p.pool()).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("workflow_guard:guard_channel_reply_job_claim")
    );
    assert!(matches!(
        f.p.resume_finalization(f.scope).await.unwrap(),
        FinalizationProgress::Completed(_)
    ));
}

#[tokio::test]
async fn channel_reply_completion_private_result_cannot_cross_public_reply_clearance() {
    let s = Intents::new().await;
    let recipient = verified_recipient(&s).await;
    let f = FinalFixture::configured(s, source("data.map"), literal_contract("Done"), false).await;
    let ceiling: i16 = sqlx::query_scalar(
        "SELECT knowledge_ceiling::smallint FROM workflow_runs WHERE company_id=$1 AND id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.scope.run.as_uuid())
    .fetch_one(f.p.pool())
    .await
    .unwrap();
    assert!(
        ceiling >= 5,
        "the verified sender admitted a classified run"
    );
    sqlx::query(
        "UPDATE workflow_runs SET content_level=knowledge_ceiling WHERE company_id=$1 AND id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.scope.run.as_uuid())
    .execute(f.p.pool())
    .await
    .unwrap();
    sqlx::query("UPDATE company_members SET access_level=0 WHERE company_id=$1 AND user_id=$2")
        .bind(f.scope.company.as_uuid())
        .bind(recipient)
        .execute(f.p.pool())
        .await
        .unwrap();
    assert_eq!(f.advance().await.disposition, BatchDisposition::Failed);
    assert_eq!(f.state().await["proposals"], 0);
    assert_eq!(f.state().await["decision"], Value::Null);
}

#[derive(sqlx::FromRow)]
struct Sender {
    principal_id: Uuid,
    email: String,
}
pub(super) async fn verified_recipient(s: &Intents) -> Uuid {
    let sender: Sender=sqlx::query_as("SELECT identity.principal_id,identity.subject::text AS email FROM participant_identities AS identity \
        JOIN thread_principals AS party ON party.company_id=identity.company_id AND party.principal_id=identity.principal_id \
        JOIN principals AS principal ON principal.company_id=identity.company_id AND principal.id=identity.principal_id \
        WHERE party.company_id=$1 AND party.thread_id=$2 AND identity.transport='email' AND principal.kind='external' ORDER BY identity.id LIMIT 1")
        .bind(s.fixture.target.company.as_uuid()).bind(s.thread).fetch_one(s.pool()).await.unwrap();
    let account = s
        .p()
        .create_user(&Uuid::new_v4().simple().to_string(), &sender.email, "hash")
        .await
        .unwrap();
    sqlx::query("UPDATE users SET email_verified_at=clock_timestamp() WHERE id=$1")
        .bind(account.id)
        .execute(s.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO company_members(id,company_id,user_id,role,access_level) VALUES($1,$2,$3,'member',5)")
        .bind(Uuid::new_v4()).bind(s.fixture.target.company.as_uuid()).bind(account.id).execute(s.pool()).await.unwrap();
    sqlx::query("UPDATE principals SET kind='person',user_id=$3 WHERE company_id=$1 AND id=$2")
        .bind(s.fixture.target.company.as_uuid())
        .bind(sender.principal_id)
        .bind(account.id)
        .execute(s.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE participant_identities SET status='verified',provenance='account',claim_metadata='{\"version\":1,\"kind\":\"account\"}'::jsonb WHERE company_id=$1 AND principal_id=$2")
        .bind(s.fixture.target.company.as_uuid()).bind(sender.principal_id).execute(s.pool()).await.unwrap();
    account.id
}

#[tokio::test]
async fn channel_reply_completion_privileged_reviewers_still_require_payload_clearance() {
    for initial_level in [1, 5] {
        let s = Intents::new().await;
        verified_recipient(&s).await;
        let reviewer = member(&s.fixture, "admin").await;
        sqlx::query("UPDATE company_members SET access_level=$3::smallint WHERE company_id=$1 AND user_id=$2")
            .bind(s.fixture.target.company.as_uuid()).bind(reviewer).bind(initial_level as i16).execute(s.pool()).await.unwrap();
        let policy =
            ReplyApprovalPolicy::new(vec![s.fixture.principal(reviewer).await], 60).unwrap();
        let f = FinalFixture::from_case(
            s,
            FinalCase {
                source: source("data.map"),
                contract: literal_contract("Done"),
                policy: Some(policy),
                run_as: None,
                publication: FinalPublication::Standard,
                delivery_policy: DeliveryPolicy::Deliver,
            },
        )
        .await;
        sqlx::query("UPDATE workflow_runs SET content_level=5 WHERE company_id=$1 AND id=$2")
            .bind(f.scope.company.as_uuid())
            .bind(f.scope.run.as_uuid())
            .execute(f.p.pool())
            .await
            .unwrap();
        let result = f.advance().await;
        if initial_level == 1 {
            assert_eq!(result.disposition, BatchDisposition::Failed);
            assert_eq!(f.state().await["proposals"], 0);
            continue;
        }
        assert_eq!(result.disposition, BatchDisposition::Deferred);
        let mut command = f.command("approve").await;
        command.actor = WorkflowActor::authenticated(reviewer).unwrap();
        sqlx::query("UPDATE company_members SET access_level=1 WHERE company_id=$1 AND user_id=$2")
            .bind(f.scope.company.as_uuid())
            .bind(reviewer)
            .execute(f.p.pool())
            .await
            .unwrap();
        let view =
            f.p.read_decision(f.scope.company, command.actor, command.decision)
                .await
                .unwrap();
        assert!(view.payload_restricted);
        assert_eq!(view.proposal, Value::Null);
        assert!(!view.can_submit);
        assert!(matches!(
            f.p.submit_decision(command).await,
            Err(AppError::NotFound(_))
        ));
        assert_eq!(f.state().await["run"], "waiting");
        assert_eq!(f.state().await["decision"], Value::Null);
    }
}

#[tokio::test]
async fn channel_reply_completion_cancellation_and_finalization_share_one_terminal_owner() {
    let f = FinalFixture::new("data.map", false).await;
    let head =
        f.p.head(f.scope.company, f.scope.run)
            .await
            .unwrap()
            .unwrap();
    let command = CancelCommand {
        company_id: f.scope.company,
        run_id: f.scope.run,
        actor: f.s.owner(),
        command_key: IdempotencyKey::parse("cancel-final-race").unwrap(),
        expected_revision: head.revision,
    };
    let barrier = tokio::sync::Barrier::new(2);
    let complete = async {
        barrier.wait().await;
        f.p.advance_pure(
            f.scope,
            BatchBudget::new(4, Duration::from_millis(100)).unwrap(),
        )
        .await
    };
    let cancel = async {
        barrier.wait().await;
        f.p.cancel(command).await
    };
    let (complete, cancel) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(complete, cancel)
    })
    .await
    .unwrap();
    cancel.unwrap();
    let state = f.state().await;
    if state["run"] == "succeeded" {
        assert!(complete.is_ok());
        assert_eq!(state["decision"], "pending");
    } else {
        assert_eq!(state["run"], "cancelled");
        assert_eq!(state["decision"], Value::Null);
        assert_eq!(state["proposals"], 0);
    }
}

#[tokio::test]
async fn channel_reply_completion_release_revocation_and_finalization_do_not_deadlock() {
    let s = Intents::new().await;
    let recipient = verified_recipient(&s).await;
    let f = FinalFixture::configured(s, source("data.map"), literal_contract("Done"), false).await;
    sqlx::query("UPDATE workflow_runs SET content_level=5 WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.run.as_uuid())
        .execute(f.p.pool())
        .await
        .unwrap();
    let barrier = tokio::sync::Barrier::new(2);
    let complete = async {
        barrier.wait().await;
        f.p.advance_pure(
            f.scope,
            BatchBudget::new(4, Duration::from_millis(100)).unwrap(),
        )
        .await
    };
    let revoke = async {
        barrier.wait().await;
        let mut tx = f.p.pool().begin().await.unwrap();
        sqlx::query("SELECT id FROM users WHERE id=ANY($1) ORDER BY id FOR UPDATE")
            .bind(vec![recipient, f.s.owner().user_id()])
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SELECT id FROM companies WHERE id=$1 FOR UPDATE")
            .bind(f.scope.company.as_uuid())
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE company_members SET access_level=0 WHERE company_id=$1 AND user_id=$2")
            .bind(f.scope.company.as_uuid())
            .bind(recipient)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    };
    let (complete, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(complete, revoke)
    })
    .await
    .unwrap();
    complete.unwrap();
    let state = f.state().await;
    if state["run"] == "succeeded" {
        assert_eq!(state["decision"], "pending");
    } else {
        assert_eq!(state["run"], "failed");
        assert_eq!(state["decision"], Value::Null);
    }
}
