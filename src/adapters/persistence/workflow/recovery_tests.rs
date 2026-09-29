use super::*;
use crate::application::workflow::{completion::*, polling::*, supervise::*};
use crate::domain::workflow::{FailureClass, FailureCode, RetrySafety, StepFailure};

fn failure(class: FailureClass, safety: RetrySafety) -> LeaseReleaseCause {
    LeaseReleaseCause::Classified(WorkflowFailure {
        failure: StepFailure::new(
            class,
            FailureCode::parse("provider.unavailable").unwrap(),
            Some("private diagnostic".into()),
        )
        .unwrap(),
        safety,
    })
}
async fn claim(f: &AdmissionFixture, scope: ActivationRequest) -> ClaimedWorkflow {
    f.persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap()
}
fn source(target: Option<&str>) -> Value {
    let mut source: Value =
        serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    if let Some(target) = target {
        source["steps"]["start"]["routes"]["error"] = json!(target);
    }
    if target == Some("recover") {
        source["steps"]["recover"] = json!({"type":"data.map","with":{"value":{"literal":42},"output_schema":{"literal":{"type":"integer"}}},"routes":{"success":"$end"}});
    }
    source
}

#[tokio::test]
async fn workflow_failure_retry_debits_once_preserves_input_and_poison_backoff() {
    let (f, scope) = fixture().await;
    let first = claim(&f, scope).await;
    let barrier = Barrier::new(2);
    let fail = || async {
        barrier.wait().await;
        f.persistence()
            .release_io(
                first.fence,
                policy(),
                failure(FailureClass::Retryable, RetrySafety::SafeToRetry),
            )
            .await
            .unwrap()
    };
    let (a, b) = tokio::join!(fail(), fail());
    assert_ne!(a, b);
    let saved = snapshot(&f).await;
    assert_eq!(saved["jobs"][0]["retry_count"], 1);
    assert_eq!(
        saved["attempts"][0]["workflow_failure_code"],
        "provider.unavailable"
    );
    assert!(!saved.to_string().contains("private diagnostic"));
    for _ in 0..2 {
        assert!(
            f.persistence()
                .poll_work(None, 10)
                .await
                .unwrap()
                .candidates
                .is_empty()
        );
    }
    due(&f, scope).await;
    let second = claim(&f, scope).await;
    assert_eq!(first.activation, second.activation);
    assert_eq!(second.fence.attempt.0, 2);
}

#[tokio::test]
async fn workflow_failure_terminal_routes_or_fails_without_reexecuting() {
    for target in [None, Some("$end"), Some("recover")] {
        let (f, scope) = fixture_source(source(target)).await;
        let owned = claim(&f, scope).await;
        assert!(
            f.persistence()
                .release_io(
                    owned.fence,
                    policy(),
                    failure(FailureClass::Terminal, RetrySafety::SafeToRetry)
                )
                .await
                .unwrap()
        );
        let saved = snapshot(&f).await;
        assert_eq!(
            saved["runs"][0]["state"],
            if target == Some("recover") {
                "running"
            } else {
                "failed"
            }
        );
        assert_eq!(
            saved["executions"].as_array().unwrap().len(),
            if target == Some("recover") { 2 } else { 1 }
        );
        let execution = saved["executions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["id"] == json!(scope.execution.as_uuid()))
            .unwrap();
        assert_eq!(
            execution["committed_route"],
            if target.is_some() {
                json!("final_error")
            } else {
                Value::Null
            }
        );
        assert!(
            !f.persistence()
                .release_io(
                    owned.fence,
                    policy(),
                    failure(FailureClass::Terminal, RetrySafety::SafeToRetry)
                )
                .await
                .unwrap()
        );
        assert_eq!(snapshot(&f).await, saved);
    }
}

#[tokio::test]
async fn workflow_failure_unknown_effect_suspends_even_with_final_error() {
    let (f, scope) = fixture_source(source(Some("recover"))).await;
    let owned = claim(&f, scope).await;
    let report = WorkflowFailure {
        failure: StepFailure::new(
            FailureClass::Retryable,
            FailureCode::parse("effect.unknown").unwrap(),
            None,
        )
        .unwrap(),
        safety: RetrySafety::EffectOutcomeUnknown,
    };
    let result = supervise_io(
        f.persistence(),
        owned,
        policy(),
        Instant::now() + Duration::from_secs(2),
        async { Err(report) },
        std::future::pending(),
    )
    .await;
    assert!(matches!(
        result,
        SupervisedResult::Stopped(StopReason::HandlerFailed)
    ));
    let saved = snapshot(&f).await;
    assert_eq!(saved["runs"][0]["waiting_reason"], "reconciliation");
    assert_eq!(saved["jobs"][0]["status"], "failed");
    assert_eq!(saved["executions"].as_array().unwrap().len(), 1);
    assert!(
        f.persistence()
            .poll_work(None, 10)
            .await
            .unwrap()
            .candidates
            .is_empty()
    );
}

#[tokio::test]
async fn workflow_failure_competes_with_completion_without_duplicate_successor() {
    for _ in 0..3 {
        let (f, scope) = fixture_source(source(Some("recover"))).await;
        let owned = claim(&f, scope).await;
        let barrier = Barrier::new(2);
        let failed = async {
            barrier.wait().await;
            f.persistence()
                .release_io(
                    owned.fence,
                    policy(),
                    failure(FailureClass::Terminal, RetrySafety::SafeToRetry),
                )
                .await
                .unwrap()
        };
        let complete = async {
            barrier.wait().await;
            f.persistence()
                .complete_io(FencedWorkflowResult {
                    fence: owned.fence,
                    output: json!({"items":[],"token_count":0}),
                })
                .await
                .unwrap()
        };
        let (failed, completed) = tokio::join!(failed, complete);
        assert_ne!(failed, completed.is_some());
        let saved = snapshot(&f).await;
        assert_eq!(saved["attempts"].as_array().unwrap().len(), 1);
        assert_eq!(
            saved["executions"].as_array().unwrap().len(),
            if failed { 2 } else { 1 }
        );
        assert_eq!(
            saved["attempts"][0]["status"],
            if failed { "failed" } else { "completed" }
        );
    }
}

#[tokio::test]
async fn workflow_failure_commit_failure_rolls_back_all_retirement_writes() {
    let (f, scope) = fixture_source(source(Some("recover"))).await;
    let owned = claim(&f, scope).await;
    sqlx::raw_sql("CREATE FUNCTION reject_retirement() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected retirement commit'; END $$; CREATE CONSTRAINT TRIGGER reject_retirement AFTER UPDATE ON background_tasks DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION reject_retirement()").execute(f.persistence().pool()).await.unwrap();
    let before = snapshot(&f).await;
    assert!(
        f.persistence()
            .release_io(
                owned.fence,
                policy(),
                failure(FailureClass::Terminal, RetrySafety::SafeToRetry)
            )
            .await
            .is_err()
    );
    assert_eq!(snapshot(&f).await, before);
}

#[tokio::test]
async fn workflow_failure_interrupted_and_expired_effects_cannot_replay() {
    for expired in [false, true] {
        let source: Value =
            serde_json::from_str(&registry::example("memory.save").unwrap().source).unwrap();
        let (f, scope) = fixture_source(source).await;
        let owned = claim(&f, scope).await;
        if expired {
            expire(&f, scope).await;
            assert!(
                f.persistence()
                    .retire_expired_io(scope, policy())
                    .await
                    .unwrap()
            );
        } else {
            assert!(
                f.persistence()
                    .release_io(owned.fence, policy(), LeaseReleaseCause::LocalInterruption)
                    .await
                    .unwrap()
            );
        }
        let saved = snapshot(&f).await;
        assert_eq!(saved["runs"][0]["waiting_reason"], "reconciliation");
        assert_eq!(saved["attempts"][0]["workflow_retry_safety"], "unknown");
        assert_eq!(saved["jobs"][0]["retry_count"], 1);
        assert!(
            f.persistence()
                .claim_io(scope, worker(), policy())
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            f.persistence()
                .poll_work(None, 10)
                .await
                .unwrap()
                .candidates
                .is_empty()
        );
        assert_eq!(snapshot(&f).await, saved);
    }
}

async fn delay_retirement(f: &AdmissionFixture) {
    sqlx::raw_sql("CREATE FUNCTION delay_retirement() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_sleep(0.3); RETURN NULL; END $$; CREATE CONSTRAINT TRIGGER aa_delay_retirement AFTER UPDATE ON background_tasks DEFERRABLE INITIALLY DEFERRED FOR EACH ROW WHEN (OLD.status='processing' AND NEW.status<>'processing') EXECUTE FUNCTION delay_retirement()").execute(f.persistence().pool()).await.unwrap();
}

#[tokio::test]
async fn workflow_failure_live_retirement_cannot_commit_after_lease_expiry() {
    let (f, scope) = fixture().await;
    let owned = claim(&f, scope).await;
    delay_retirement(&f).await;
    sqlx::query("UPDATE background_tasks SET lock_expires_at=clock_timestamp()+interval '0.15 seconds' WHERE id=$1").bind(scope.job.0).execute(f.persistence().pool()).await.unwrap();
    let before = snapshot(&f).await;
    assert!(
        f.persistence()
            .release_io(
                owned.fence,
                policy(),
                failure(FailureClass::Retryable, RetrySafety::SafeToRetry)
            )
            .await
            .is_err()
    );
    assert_eq!(snapshot(&f).await, before);
}

#[tokio::test]
async fn workflow_failure_exhausted_expiry_routes_and_deadline_delay_rolls_back() {
    for delay in [false, true] {
        let (f, scope) = fixture_source(source(Some("recover"))).await;
        sqlx::query("UPDATE background_tasks SET max_retries=1 WHERE id=$1")
            .bind(scope.job.0)
            .execute(f.persistence().pool())
            .await
            .unwrap();
        claim(&f, scope).await;
        expire(&f, scope).await;
        if delay {
            delay_retirement(&f).await;
            sqlx::query("UPDATE workflow_runs SET deadline=clock_timestamp()+interval '0.15 seconds' WHERE id=$1").bind(scope.run.as_uuid()).execute(f.persistence().pool()).await.unwrap();
        }
        let before = snapshot(&f).await;
        let result = f.persistence().retire_expired_io(scope, policy()).await;
        if delay {
            assert!(result.is_err());
            assert_eq!(snapshot(&f).await, before);
        } else {
            assert!(result.unwrap());
            let saved = snapshot(&f).await;
            assert_eq!(saved["executions"].as_array().unwrap().len(), 2);
            assert_eq!(
                saved["jobs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|j| j["id"] == json!(scope.job.0))
                    .unwrap()["retry_count"],
                1
            );
        }
    }
}
