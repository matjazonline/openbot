use super::*;
use crate::application::workflow::{completion::*, polling::*, waits::*};

async fn overdue(f: &AdmissionFixture, scope: ActivationRequest) {
    sqlx::query("UPDATE workflow_runs SET created_at=clock_timestamp()-interval '2 minutes',deadline=clock_timestamp()-interval '1 minute' WHERE id=$1")
        .bind(scope.run.as_uuid()).execute(f.persistence().pool()).await.unwrap();
}
async fn claimed(f: &AdmissionFixture, scope: ActivationRequest) -> ClaimedWorkflow {
    f.persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap()
}
async fn evidence(f: &AdmissionFixture) -> Value {
    let mut saved = snapshot(f).await;
    saved["audit"] = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(event) ORDER BY sequence) FROM workflow_run_events AS event",
    )
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    saved
}
async fn assert_expired(f: &AdmissionFixture, scope: ActivationRequest, debit: i64) {
    let saved = evidence(f).await;
    assert_eq!(saved["runs"][0]["state"], "failed");
    assert_eq!(saved["runs"][0]["waiting_reason"], Value::Null);
    assert_eq!(saved["jobs"][0]["status"], "failed");
    assert_eq!(saved["jobs"][0]["retry_count"], debit);
    assert_eq!(saved["executions"].as_array().unwrap().len(), 1);
    assert_eq!(
        saved["executions"][0]["successor_execution_id"],
        Value::Null
    );
    assert!(!f.persistence().expire_run(scope).await.unwrap());
    assert_eq!(evidence(f).await, saved);
    assert!(
        f.persistence()
            .poll_work(None, 128)
            .await
            .unwrap()
            .candidates
            .is_empty()
    );
}

#[tokio::test]
async fn workflow_maintenance_discovers_all_active_states_and_future_jobs() {
    for reason in [
        None,
        Some("running"),
        Some("decision"),
        Some("event"),
        Some("timer"),
        Some("child_run"),
        Some("effect"),
        Some("reconciliation"),
    ] {
        let (f, scope) = fixture().await;
        if let Some(reason) = reason {
            let state = if reason == "running" {
                "running"
            } else {
                "waiting"
            };
            let waiting = (state == "waiting").then_some(reason);
            sqlx::query("UPDATE workflow_runs SET state=$2,waiting_reason=$3 WHERE id=$1")
                .bind(scope.run.as_uuid())
                .bind(state)
                .bind(waiting)
                .execute(f.persistence().pool())
                .await
                .unwrap();
        }
        sqlx::query(
            "UPDATE background_tasks SET run_at=clock_timestamp()+interval '1 day' WHERE id=$1",
        )
        .bind(scope.job.0)
        .execute(f.persistence().pool())
        .await
        .unwrap();
        overdue(&f, scope).await;
        let before = snapshot(&f).await;
        let page = f.persistence().poll_work(None, 1).await.unwrap();
        assert_eq!(
            page.candidates,
            vec![PollCandidate {
                scope,
                work: PollWork::ExpiredRun
            }]
        );
        assert!(f.persistence().expire_run(scope).await.unwrap());
        assert_eq!(snapshot(&f).await["executions"], before["executions"]);
        assert_expired(&f, scope, 0).await;
    }
}

#[tokio::test]
async fn workflow_maintenance_live_expired_and_spent_attempts_debit_once() {
    for ownership in ["live", "expired", "spent"] {
        let (f, scope) = fixture().await;
        if ownership == "spent" {
            sqlx::query("UPDATE background_tasks SET max_retries=1 WHERE id=$1")
                .bind(scope.job.0)
                .execute(f.persistence().pool())
                .await
                .unwrap();
        }
        let claim = claimed(&f, scope).await;
        if ownership == "expired" {
            expire(&f, scope).await;
        }
        overdue(&f, scope).await;
        let before = snapshot(&f).await;
        assert!(f.persistence().expire_run(scope).await.unwrap());
        let after = snapshot(&f).await;
        assert_eq!(after["executions"], before["executions"]);
        assert_eq!(
            after["attempts"][0]["execution_generation"],
            before["attempts"][0]["execution_generation"]
        );
        assert_eq!(after["attempts"][0]["workflow_retry_safety"], "unknown");
        assert_eq!(after["attempts"][0]["workflow_retirement"], "deadline");
        assert!(
            !f.persistence()
                .validate_io(claim.fence, policy())
                .await
                .unwrap()
        );
        assert!(
            f.persistence()
                .complete_io(FencedWorkflowResult {
                    fence: claim.fence,
                    output: json!({"items":[],"token_count":0})
                })
                .await
                .unwrap()
                .is_none()
        );
        assert_expired(&f, scope, 1).await;
    }
}

#[tokio::test]
async fn workflow_maintenance_scope_and_predeadline_refuse_without_writes() {
    let (f, scope) = fixture().await;
    let before = evidence(&f).await;
    assert!(!f.persistence().expire_run(scope).await.unwrap());
    assert_eq!(evidence(&f).await, before);
    overdue(&f, scope).await;
    for wrong in [
        ActivationRequest {
            company: CompanyId::new(Uuid::new_v4()),
            ..scope
        },
        ActivationRequest {
            job: WorkflowJobId(Uuid::new_v4()),
            ..scope
        },
    ] {
        let before = evidence(&f).await;
        assert!(!f.persistence().expire_run(wrong).await.unwrap());
        assert_eq!(evidence(&f).await, before);
    }
}

#[tokio::test]
async fn workflow_maintenance_competes_with_expiry_completion_and_heartbeat() {
    let (f, scope) = fixture().await;
    let claim = claimed(&f, scope).await;
    overdue(&f, scope).await;
    let barrier = Barrier::new(4);
    let expiry = async {
        barrier.wait().await;
        f.persistence().expire_run(scope).await.unwrap()
    };
    let other = async {
        barrier.wait().await;
        f.persistence().expire_run(scope).await.unwrap()
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
    let renew = async {
        barrier.wait().await;
        f.persistence()
            .renew_io(claim.fence, policy())
            .await
            .unwrap()
    };
    let (first, second, completed, renewed) = tokio::join!(expiry, other, complete, renew);
    assert_ne!(first, second);
    assert!(completed.is_none());
    assert!(renewed.is_none());
    assert_expired(&f, scope, 1).await;
}

#[tokio::test]
async fn workflow_maintenance_commit_fault_rolls_back_every_fact() {
    let (f, scope) = fixture().await;
    claimed(&f, scope).await;
    overdue(&f, scope).await;
    sqlx::raw_sql("CREATE FUNCTION reject_deadline() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected deadline commit'; END $$; CREATE CONSTRAINT TRIGGER reject_deadline AFTER UPDATE ON workflow_runs DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION reject_deadline()")
        .execute(f.persistence().pool()).await.unwrap();
    let before = evidence(&f).await;
    assert!(f.persistence().expire_run(scope).await.is_err());
    assert_eq!(evidence(&f).await, before);
}

#[tokio::test]
async fn workflow_maintenance_deadline_retirement_survives_lease_crossing_at_commit() {
    let (f, scope) = fixture().await;
    claimed(&f, scope).await;
    overdue(&f, scope).await;
    sqlx::query("UPDATE background_tasks SET lock_expires_at=clock_timestamp()+interval '150 milliseconds' WHERE id=$1")
        .bind(scope.job.0).execute(f.persistence().pool()).await.unwrap();
    let mut tx = f.persistence().pool().begin().await.unwrap();
    assert!(maintenance::expire_on(&mut tx, scope).await.unwrap());
    tokio::time::sleep(Duration::from_millis(200)).await;
    tx.commit().await.unwrap();
    assert_expired(&f, scope, 1).await;
}

#[tokio::test]
async fn workflow_maintenance_full_pages_drain_without_time_advance() {
    let (f, scope) = fixture().await;
    for index in 0..4 {
        let command = f
            .prepare(f.request(&format!("deadline-{index}"), f.manual()))
            .await;
        f.persistence().admit(&command).await.unwrap();
    }
    sqlx::query("UPDATE workflow_runs SET created_at=clock_timestamp()-interval '2 minutes',deadline=clock_timestamp()-interval '1 minute' WHERE company_id=$1")
        .bind(scope.company.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    let mut total = 0;
    loop {
        let page = f.persistence().poll_work(None, 2).await.unwrap();
        if page.candidates.is_empty() {
            break;
        }
        assert!(total < 5, "unchanged deadline page was reclaimed");
        for candidate in page.candidates {
            assert_eq!(candidate.work, PollWork::ExpiredRun);
            assert!(f.persistence().expire_run(candidate.scope).await.unwrap());
            total += 1;
        }
    }
    assert_eq!(total, 5);
    assert!(
        f.persistence()
            .poll_work(None, 2)
            .await
            .unwrap()
            .candidates
            .is_empty()
    );
}

#[tokio::test]
async fn workflow_maintenance_wait_sweeper_competes_and_preserves_notification() {
    for kind in ["wait.timer", "wait.event"] {
        let mut source: Value =
            serde_json::from_str(&registry::example(kind).unwrap().source).unwrap();
        source["steps"]["start"]["with"]["deadline"] =
            json!({"literal":(chrono::Utc::now()+chrono::Duration::minutes(30)).to_rfc3339()});
        let (f, scope) = fixture_source(source).await;
        f.persistence().park_wait(scope).await.unwrap().unwrap();
        let before = snapshot(&f).await;
        let notification: Value = sqlx::query_scalar("SELECT to_jsonb(wait) - 'state' - 'settled_at' FROM workflow_waits AS wait WHERE run_id=$1")
            .bind(scope.run.as_uuid()).fetch_one(f.persistence().pool()).await.unwrap();
        overdue(&f, scope).await;
        let barrier = Barrier::new(2);
        let expire = async {
            barrier.wait().await;
            f.persistence().expire_run(scope).await.unwrap()
        };
        let sweep = async {
            barrier.wait().await;
            f.persistence().sweep_waits(8).await.unwrap()
        };
        tokio::join!(expire, sweep);
        let after: Value =
            sqlx::query_scalar("SELECT to_jsonb(wait) FROM workflow_waits AS wait WHERE run_id=$1")
                .bind(scope.run.as_uuid())
                .fetch_one(f.persistence().pool())
                .await
                .unwrap();
        assert_eq!(after["state"], "expired");
        let mut immutable = after;
        immutable.as_object_mut().unwrap().remove("state");
        immutable.as_object_mut().unwrap().remove("settled_at");
        assert_eq!(immutable, notification);
        assert_eq!(snapshot(&f).await["executions"], before["executions"]);
        assert_eq!(
            f.persistence().resume_wait(scope).await.unwrap(),
            WaitProgress::Expired(CommitDisposition::Replayed)
        );
        assert!(!f.persistence().expire_run(scope).await.unwrap());
        assert!(
            f.persistence()
                .poll_work(None, 8)
                .await
                .unwrap()
                .candidates
                .is_empty()
        );
    }
}

#[tokio::test]
async fn workflow_maintenance_preserves_existing_ambiguous_attempt_truth() {
    use crate::domain::workflow::{FailureClass, FailureCode, RetrySafety, StepFailure};
    let (f, scope) = fixture().await;
    let claim = claimed(&f, scope).await;
    let failure = WorkflowFailure {
        failure: StepFailure::new(
            FailureClass::Retryable,
            FailureCode::parse("effect.unknown").unwrap(),
            None,
        )
        .unwrap(),
        safety: RetrySafety::EffectOutcomeUnknown,
    };
    assert!(
        f.persistence()
            .release_io(
                claim.fence,
                policy(),
                LeaseReleaseCause::Classified(failure)
            )
            .await
            .unwrap()
    );
    let before = snapshot(&f).await;
    assert_eq!(before["runs"][0]["waiting_reason"], "reconciliation");
    assert_eq!(before["jobs"][0]["status"], "failed");
    overdue(&f, scope).await;
    assert_eq!(
        f.persistence().poll_work(None, 1).await.unwrap().candidates[0].work,
        PollWork::ExpiredRun
    );
    assert!(f.persistence().expire_run(scope).await.unwrap());
    let after = snapshot(&f).await;
    assert_eq!(after["attempts"], before["attempts"]);
    assert_eq!(after["executions"], before["executions"]);
    assert_eq!(after["jobs"], before["jobs"]);
    assert_expired(&f, scope, 1).await;
}

#[path = "maintenance_guard_tests.rs"]
mod guard_tests;
