//! Existing-job gates and execution sibling overflow through real recovery.
use super::*;

#[derive(Clone, Copy, Debug)]
enum ReopenGate {
    Running,
    Human,
    Failed,
    Cancelled,
    Succeeded,
    Deadline,
    AttemptCap,
}

async fn change_gate(f: &AdmissionFixture, request: &ActionDispatchRequest, gate: ReopenGate) {
    let (state, reason) = match gate {
        ReopenGate::Running => ("running", None),
        ReopenGate::Human => ("waiting", Some("decision")),
        ReopenGate::Failed => ("failed", None),
        ReopenGate::Cancelled => ("cancelled", None),
        ReopenGate::Succeeded => ("succeeded", None),
        _ => ("waiting", Some("reconciliation")),
    };
    sqlx::query("UPDATE workflow_runs SET state=$2,waiting_reason=$3 WHERE id=$1")
        .bind(request.scope().run.as_uuid())
        .bind(state)
        .bind(reason)
        .execute(f.persistence().pool())
        .await
        .unwrap();
    match gate {
        ReopenGate::Deadline => {
            sqlx::query(
                "UPDATE workflow_runs SET deadline=created_at+interval '1 microsecond' WHERE id=$1",
            )
            .bind(request.scope().run.as_uuid())
            .execute(f.persistence().pool())
            .await
            .unwrap();
        }
        ReopenGate::AttemptCap => {
            sqlx::query("UPDATE background_tasks SET max_retries=retry_count WHERE id=$1")
                .bind(request.fence.scope.job.0)
                .execute(f.persistence().pool())
                .await
                .unwrap();
        }
        _ => {}
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_siblings_existing_job_reopen_gates_preserve_history() {
    for gate in [
        ReopenGate::Running,
        ReopenGate::Human,
        ReopenGate::Failed,
        ReopenGate::Cancelled,
        ReopenGate::Succeeded,
        ReopenGate::Deadline,
        ReopenGate::AttemptCap,
    ] {
        let (f, requests, verifier) = seed_siblings(&[SiblingTruth::Final; 2]).await;
        let first = reconcile(
            &f,
            &requests[0],
            scoped_marker(&f, &requests[0]).await,
            verifier.clone(),
            "first-proof",
        )
        .await;
        assert!(matches!(
            first.outcome,
            ReconciliationOutcome::Blocked { .. }
        ));
        change_gate(&f, &requests[0], gate).await;
        let before = all_tables(&f).await;
        let result = reconcile(
            &f,
            &requests[1],
            scoped_marker(&f, &requests[1]).await,
            verifier,
            "last-proof",
        )
        .await;
        assert!(
            !matches!(result.outcome, ReconciliationOutcome::Scheduled { .. }),
            "{gate:?}"
        );
        let after = all_tables(&f).await;
        preserved_history(&before, &after);
        assert_eq!(
            before["background_tasks"], after["background_tasks"],
            "{gate:?}"
        );
        assert_eq!(
            before["workflow_executions"], after["workflow_executions"],
            "{gate:?}"
        );
        for key in ["state", "waiting_reason", "terminal_execution_id"] {
            assert_eq!(
                before["workflow_runs"][0][key], after["workflow_runs"][0][key],
                "{gate:?}:{key}"
            );
        }
        // Authorized truth is retained even when the existing job cannot reopen.
        assert_eq!(
            after["workflow_action_evidence"].as_array().unwrap().len(),
            2
        );
        assert_eq!(entry_consumptions(&f).await, (2, 0));
        let before_revision = before["workflow_runs"][0]["revision"].as_u64().unwrap();
        let after_revision = after["workflow_runs"][0]["revision"].as_u64().unwrap();
        assert!(result.revision.0 > before_revision, "{gate:?}");
        assert_eq!(result.revision.0, after_revision, "{gate:?}");
    }
}

async fn check_rust_safe_rollback(f: &AdmissionFixture, request: &ActionDispatchRequest) {
    let before = all_tables(f).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query("UPDATE background_tasks SET locked_at=clock_timestamp()-interval '10 seconds',lock_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1")
        .bind(request.fence.scope.job.0).execute(&mut *tx).await.unwrap();
    recovery::retire_on(&mut tx, request.fence, lease::Retirement::Expired)
        .await
        .unwrap();
    let safety: String = sqlx::query_scalar(
        "SELECT workflow_retry_safety FROM task_attempts WHERE task_id=$1 AND attempt_number=$2",
    )
    .bind(request.fence.scope.job.0)
    .bind(request.fence.attempt.0)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(safety, "safe");
    tx.rollback().await.unwrap();
    assert_eq!(
        before,
        all_tables(f).await,
        "rollback restores the whole recovery transaction"
    );
}

struct SiblingDispatchDiagnostic {
    phases: [tokio::time::Instant; 4],
    calls: [usize; 2],
    window: crate::application::workflow::lease::LeaseWindow,
}

struct ConservativeWindowDiagnostic {
    lease_offset_ms: f64,
    lease_crossed: bool,
    run_offset_ms: f64,
    run_crossed: bool,
}

impl std::fmt::Debug for ConservativeWindowDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConservativeWindow")
            .field("lease_offset_ms", &self.lease_offset_ms)
            .field("lease_crossed", &self.lease_crossed)
            .field("run_offset_ms", &self.run_offset_ms)
            .field("run_crossed", &self.run_crossed)
            .finish()
    }
}

impl SiblingDispatchDiagnostic {
    fn message(
        &self,
        request: &ActionDispatchRequest,
        number: usize,
        outcome: &RemoteDispatchObservation,
    ) -> String {
        let variant = match outcome {
            RemoteDispatchObservation::ApprovalRequired => "ApprovalRequired",
            RemoteDispatchObservation::PossibleDispatchExists => "PossibleDispatchExists",
            RemoteDispatchObservation::Committed(_) => "Committed",
            RemoteDispatchObservation::Interrupted => "Interrupted",
        };
        // Signed offsets describe only the conservative renewal window, not database expiry.
        let offset_ms = |deadline: tokio::time::Instant, at: tokio::time::Instant| {
            if deadline >= at {
                deadline.duration_since(at).as_secs_f64() * 1000.0
            } else {
                -at.duration_since(deadline).as_secs_f64() * 1000.0
            }
        };
        let windows = self.phases.map(|at| ConservativeWindowDiagnostic {
            lease_offset_ms: offset_ms(self.window.expires, at),
            lease_crossed: at >= self.window.expires,
            run_offset_ms: offset_ms(self.window.run_deadline, at),
            run_crossed: at >= self.window.run_deadline,
        });
        let [started, renewed, ready, done] = self.phases;
        let [before, after] = self.calls;
        format!(
            "accepted_limit test={} iteration={number} variant={variant} scope={:?} worker={:?} generation={:?} renewal_ms={} sibling_ms={} invoke_ms={} calls_before={before} calls_after={after} call_delta={} conservative_window_at_start_renewed_ready_done=(lease_offset_ms,lease_crossed,run_offset_ms,run_crossed):{windows:?}",
            std::thread::current().name().unwrap_or("unnamed"),
            request.fence.scope,
            request.fence.worker,
            request.fence.generation,
            (renewed - started).as_secs_f64() * 1000.0,
            (ready - renewed).as_secs_f64() * 1000.0,
            (done - ready).as_secs_f64() * 1000.0,
            after - before
        )
    }
}

pub(super) async fn accepted_limit() -> (AdmissionFixture, ActionDispatchRequest, LedgerProvider) {
    let (f, request) = setup().await;
    let _verifier = ledger(&f, &request).await;
    let delivery = LedgerProvider::new(
        &f,
        Delivery::Apply {
            result: good(),
            recover: true,
            lose: false,
        },
    );
    assert!(matches!(
        invoke(&f, &request, &delivery).await.unwrap(),
        RemoteDispatchObservation::Committed(_)
    ));
    for number in 1..128 {
        let renewal_started = tokio::time::Instant::now();
        let window = f
            .persistence()
            .renew_io(request.fence, policy())
            .await
            .unwrap();
        let renewal_finished = tokio::time::Instant::now();
        assert!(window.is_some());
        let sibling = sibling(&f, &request, number + 1000).await;
        let sibling_ready = tokio::time::Instant::now();
        let calls_before = delivery.calls.load(Ordering::SeqCst);
        let outcome = invoke(&f, &sibling, &delivery).await.unwrap();
        let invoke_finished = tokio::time::Instant::now();
        let calls_after = delivery.calls.load(Ordering::SeqCst);
        assert!(
            matches!(outcome, RemoteDispatchObservation::Committed(_)),
            "{}",
            SiblingDispatchDiagnostic {
                phases: [
                    renewal_started,
                    renewal_finished,
                    sibling_ready,
                    invoke_finished
                ],
                calls: [calls_before, calls_after],
                window: window.unwrap(),
            }
            .message(&request, number, &outcome)
        );
    }
    assert!(
        retry_safe(&f, &request).await,
        "exactly128 accepted markers are safe"
    );
    Box::pin(check_rust_safe_rollback(&f, &request)).await;
    let c = command(&f, &request, scoped_marker(&f, &request).await).await;
    let reader = PostgresActionReconciliation::new(f.persistence().clone(), Resources);
    assert!(matches!(
        reader.snapshot(&c).await.unwrap(),
        ReconciliationPreparation::Snapshot(_)
    ));
    (f, request, delivery)
}

#[tokio::test]
async fn workflow_action_reconciliation_siblings_128_and_129_snapshot_and_actual_recovery_never_truncate()
 {
    let (f, request, delivery) = Box::pin(accepted_limit()).await;
    let reader = PostgresActionReconciliation::new(f.persistence().clone(), Resources);
    assert!(
        f.persistence()
            .renew_io(request.fence, policy())
            .await
            .unwrap()
            .is_some()
    );
    let overflow = sibling(&f, &request, 1128).await;
    assert!(matches!(
        invoke(&f, &overflow, &delivery).await.unwrap(),
        RemoteDispatchObservation::Committed(_)
    ));
    assert!(
        !retry_safe(&f, &request).await,
        "129 receipts cannot hide the sibling overflow"
    );
    park(&f, &request).await;
    let before = all_tables(&f).await;
    assert_eq!(
        before["workflow_action_receipts"].as_array().unwrap().len(),
        129
    );
    assert_eq!(
        before["workflow_action_dispatches"]
            .as_array()
            .unwrap()
            .len(),
        129
    );
    assert_eq!(
        before["task_attempts"][0]["workflow_retry_safety"],
        json!("unknown")
    );
    let c = command(&f, &request, scoped_marker(&f, &request).await).await;
    let ReconciliationPreparation::Recorded { result, .. } = reader.snapshot(&c).await.unwrap()
    else {
        panic!("bounded refusal");
    };
    assert_eq!(
        result.outcome,
        ReconciliationOutcome::Blocked {
            reason: ReconciliationBlockedReason::BoundExceeded
        }
    );
    assert!(
        f.persistence()
            .claim_io(request.fence.scope, worker(), policy())
            .await
            .unwrap()
            .is_none()
    );
    let mut after = all_tables(&f).await;
    assert_eq!(
        after["workflow_action_evidence_commands"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    after["workflow_action_evidence_commands"] =
        before["workflow_action_evidence_commands"].clone();
    assert_eq!(
        before, after,
        "only the immutable bounded-refusal receipt is recorded"
    );
}
