use super::*;
use crate::application::workflow::{completion::*, supervise::*};
use crate::domain::workflow::{FailureClass, FailureCode, RetrySafety, StepFailure};

async fn command(f: &AdmissionFixture, scope: ActivationRequest, key: &str) -> CancelCommand {
    CancelCommand {
        company_id: scope.company,
        run_id: scope.run,
        actor: f.binding.target.actor,
        command_key: IdempotencyKey::parse(key).unwrap(),
        expected_revision: f
            .persistence()
            .head(scope.company, scope.run)
            .await
            .unwrap()
            .unwrap()
            .revision,
    }
}
fn retry(command: CancelCommand) -> RetryCommand {
    RetryCommand {
        company_id: command.company_id,
        run_id: command.run_id,
        actor: command.actor,
        command_key: command.command_key,
        expected_revision: command.expected_revision,
    }
}
async fn failed(
    f: &AdmissionFixture,
    scope: ActivationRequest,
    safety: RetrySafety,
) -> ClaimedWorkflow {
    let claim = f
        .persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    let cause = LeaseReleaseCause::Classified(WorkflowFailure {
        failure: StepFailure::new(
            FailureClass::Terminal,
            FailureCode::parse("provider.rejected").unwrap(),
            None,
        )
        .unwrap(),
        safety,
    });
    assert!(
        f.persistence()
            .release_io(claim.fence, policy(), cause)
            .await
            .unwrap()
    );
    claim
}
async fn receipts(f: &AdmissionFixture) -> Value {
    sqlx::query_scalar("SELECT COALESCE(jsonb_agg(to_jsonb(receipt) ORDER BY command_key),'[]') FROM workflow_control_commands AS receipt")
        .fetch_one(f.persistence().pool()).await.unwrap()
}

#[tokio::test]
async fn workflow_control_competing_cancel_replay_closes_exact_expired_attempt() {
    let (f, scope) = fixture().await;
    let claim = f
        .persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    expire(&f, scope).await;
    let cmd = command(&f, scope, "cancel").await;
    let barrier = Barrier::new(2);
    let cancel = || async {
        barrier.wait().await;
        f.persistence().cancel(cmd.clone()).await.unwrap()
    };
    let (a, b) = tokio::join!(cancel(), cancel());
    assert_eq!(a, b);
    assert!(matches!(a, CancelResult::Applied { .. }));
    let state = snapshot(&f).await;
    assert_eq!(state["runs"][0]["state"], "cancelled");
    assert_eq!(state["jobs"][0]["retry_count"], 1);
    assert_eq!(state["attempts"][0]["workflow_retirement"], "cancel");
    assert_eq!(receipts(&f).await.as_array().unwrap().len(), 1);
    assert_eq!(f.persistence().cancel(cmd.clone()).await.unwrap(), a);
    assert_eq!(snapshot(&f).await, state);
    assert!(
        !f.persistence()
            .validate_io(claim.fence, policy())
            .await
            .unwrap()
    );
    let mut conflict = cmd.clone();
    conflict.expected_revision.0 += 1;
    assert!(matches!(
        f.persistence().cancel(conflict).await,
        Err(AppError::Conflict(_))
    ));
    assert!(matches!(
        f.persistence().retry(retry(cmd)).await,
        Err(AppError::Conflict(_))
    ));
    assert_eq!(snapshot(&f).await, state);
}

#[tokio::test]
async fn workflow_control_competing_retry_preserves_identity_facts_and_remaining_allowance() {
    let (f, scope) = fixture().await;
    let first = failed(&f, scope, RetrySafety::SafeToRetry).await;
    let before = snapshot(&f).await;
    let cmd = retry(command(&f, scope, "retry").await);
    let barrier = Barrier::new(2);
    let apply = || async {
        barrier.wait().await;
        f.persistence().retry(cmd.clone()).await.unwrap()
    };
    let (a, b) = tokio::join!(apply(), apply());
    assert_eq!(a, b);
    assert!(matches!(a, RetryResult::Applied { .. }));
    let after = snapshot(&f).await;
    assert_eq!(after["executions"], before["executions"]);
    assert_eq!(after["attempts"], before["attempts"]);
    for key in [
        "id",
        "workflow_execution_id",
        "retry_count",
        "max_retries",
        "payload",
    ] {
        assert_eq!(after["jobs"][0][key], before["jobs"][0][key], "{key}");
    }
    for key in [
        "deadline",
        "max_steps",
        "max_context_bytes",
        "bundle",
        "input",
        "params",
        "resources",
    ] {
        assert_eq!(after["runs"][0][key], before["runs"][0][key], "{key}");
    }
    let next = f
        .persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(next.activation, first.activation);
    assert_eq!(next.fence.attempt.0, 2);
    assert_eq!(f.persistence().retry(cmd).await.unwrap(), a);
    assert_eq!(receipts(&f).await.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn workflow_control_retry_refuses_unknown_exhaustion_deadline_completed_and_child() {
    for kind in 0..5 {
        let (f, mut scope) = fixture().await;
        if kind == 4 {
            let parent = ExecutionRef::new(
                scope.company,
                scope.run,
                scope.execution,
                StepId::parse("start").unwrap(),
            );
            let trigger = TriggerRef::new(
                scope.company,
                TriggerId::new(Uuid::new_v4()),
                TriggerSource::Child {
                    parent: ChildCause::Execution(parent),
                },
            )
            .unwrap();
            let child = f.prepare(f.request("child", trigger)).await;
            f.persistence().admit(&child).await.unwrap();
            scope.run = child.proposed_run_id();
            scope.execution = child.first_execution_id();
            scope.job = WorkflowJobId(
                sqlx::query_scalar(
                    "SELECT id FROM background_tasks WHERE workflow_execution_id=$1",
                )
                .bind(scope.execution.as_uuid())
                .fetch_one(f.persistence().pool())
                .await
                .unwrap(),
            );
            assert!(matches!(
                f.persistence()
                    .head(scope.company, scope.run)
                    .await
                    .unwrap()
                    .unwrap()
                    .causality
                    .trigger()
                    .source(),
                TriggerSource::Child { .. }
            ));
        }
        if kind == 1 {
            sqlx::query("UPDATE background_tasks SET max_retries=1 WHERE id=$1")
                .bind(scope.job.0)
                .execute(f.persistence().pool())
                .await
                .unwrap();
        }
        if kind == 3 {
            let claim = f
                .persistence()
                .claim_io(scope, worker(), policy())
                .await
                .unwrap()
                .unwrap();
            f.persistence()
                .complete_io(FencedWorkflowResult {
                    fence: claim.fence,
                    output: json!({"items":[],"token_count":0}),
                })
                .await
                .unwrap()
                .unwrap();
        } else {
            failed(
                &f,
                scope,
                if kind == 0 {
                    RetrySafety::EffectOutcomeUnknown
                } else {
                    RetrySafety::SafeToRetry
                },
            )
            .await;
        }
        if kind == 2 {
            sqlx::query("UPDATE workflow_runs SET created_at=clock_timestamp()-interval '2 minutes',deadline=clock_timestamp()-interval '1 minute' WHERE id=$1").bind(scope.run.as_uuid()).execute(f.persistence().pool()).await.unwrap();
        }
        let before = snapshot(&f).await;
        let events:Value=sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(event) ORDER BY run_id,sequence) FROM workflow_run_events AS event").fetch_one(f.persistence().pool()).await.unwrap();
        let result = f
            .persistence()
            .retry(retry(command(&f, scope, "unsafe").await))
            .await
            .unwrap();
        assert!(
            matches!(result, RetryResult::Unsafe { .. }),
            "{kind}: {result:?}"
        );
        assert_eq!(snapshot(&f).await, before);
        let saved:Value=sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(event) ORDER BY run_id,sequence) FROM workflow_run_events AS event").fetch_one(f.persistence().pool()).await.unwrap();
        assert_eq!(events, saved);
    }
}

#[tokio::test]
async fn workflow_control_revision_covers_claim_and_stale_caller_cannot_cancel() {
    let (f, scope) = fixture().await;
    let stale = command(&f, scope, "stale").await;
    let claim = f
        .persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    let current = command(&f, scope, "current").await;
    assert!(current.expected_revision > stale.expected_revision);
    assert!(matches!(
        f.persistence().cancel(stale).await.unwrap(),
        CancelResult::RevisionConflict { .. }
    ));
    assert!(
        f.persistence()
            .validate_io(claim.fence, policy())
            .await
            .unwrap()
    );
    assert!(matches!(
        f.persistence().cancel(current).await.unwrap(),
        CancelResult::Applied { .. }
    ));
    assert!(
        sqlx::query("UPDATE workflow_runs SET revision=revision-1 WHERE id=$1")
            .bind(scope.run.as_uuid())
            .execute(f.persistence().pool())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn workflow_control_cancel_completion_competitors_preserve_winning_result() {
    for _ in 0..3 {
        let (f, scope) = fixture().await;
        let claim = f
            .persistence()
            .claim_io(scope, worker(), policy())
            .await
            .unwrap()
            .unwrap();
        let cmd = command(&f, scope, "cancel").await;
        let barrier = Barrier::new(2);
        let cancel = async {
            barrier.wait().await;
            f.persistence().cancel(cmd).await.unwrap()
        };
        let complete = async {
            barrier.wait().await;
            f.persistence()
                .complete_io(FencedWorkflowResult {
                    fence: claim.fence,
                    output: json!({"items":[],"token_count":0}),
                })
                .await
                .unwrap()
        };
        let (cancel, complete) = tokio::join!(cancel, complete);
        let state = snapshot(&f).await;
        if matches!(cancel, CancelResult::Applied { .. }) {
            assert!(complete.is_none());
            assert_eq!(state["executions"][0]["completed_at"], Value::Null);
        } else {
            assert!(matches!(cancel, CancelResult::RevisionConflict { .. }));
            assert!(complete.is_some());
            assert_eq!(state["runs"][0]["state"], "succeeded");
            let cmd = command(&f, scope, "terminal").await;
            assert!(matches!(
                f.persistence().cancel(cmd).await.unwrap(),
                CancelResult::AlreadyTerminalOrApplied { .. }
            ));
            assert_eq!(snapshot(&f).await, state);
        }
        assert_eq!(state["attempts"].as_array().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn workflow_control_cancel_actual_handler_future_is_dropped_by_heartbeat() {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Guard(Arc<AtomicBool>);
    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let (f, scope) = fixture().await;
    let claim = f
        .persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    let cmd = command(&f, scope, "cancel").await;
    let dropped = Arc::new(AtomicBool::new(false));
    let started = tokio::sync::Notify::new();
    let handler = async {
        let _guard = Guard(dropped.clone());
        started.notify_one();
        std::future::pending::<WorkflowHandlerResult>().await
    };
    let cancel = async {
        started.notified().await;
        f.persistence().cancel(cmd).await.unwrap()
    };
    let work = supervise_io(
        f.persistence(),
        claim,
        policy(),
        Instant::now() + Duration::from_secs(10),
        handler,
        std::future::pending(),
    );
    let (result, cancel) =
        tokio::time::timeout(Duration::from_secs(5), async { tokio::join!(work, cancel) })
            .await
            .unwrap();
    assert!(matches!(cancel, CancelResult::Applied { .. }));
    assert!(matches!(result, SupervisedResult::Stopped(_)));
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn workflow_control_current_authority_is_checked_even_on_replay() {
    let (f, scope) = fixture().await;
    let cmd = command(&f, scope, "cancel").await;
    f.persistence().cancel(cmd.clone()).await.unwrap();
    let before = snapshot(&f).await;
    let saved = receipts(&f).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR UPDATE")
        .bind(scope.company.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM company_members WHERE company_id=$1 AND user_id=$2")
        .bind(scope.company.as_uuid())
        .bind(cmd.actor.user_id())
        .execute(&mut *tx)
        .await
        .unwrap();
    let revoke = async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        tx.commit().await.unwrap();
    };
    let replay = f.persistence().cancel(cmd);
    let (_, result) = tokio::join!(revoke, replay);
    assert!(matches!(result, Err(AppError::NotFound(_))));
    assert_eq!(snapshot(&f).await, before);
    assert_eq!(receipts(&f).await, saved);
}

#[path = "control_guard_tests.rs"]
mod guard_tests;

#[path = "control_acceptance_tests.rs"]
mod acceptance_tests;
