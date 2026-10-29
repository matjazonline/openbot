//! 04.11 R006 F016: the D4 §S F012 variants (breach, then every destination final), the D3 §K
//! SQL-only reopen negatives and wrong-association actor, and the D4 §L seam's delivery reopen.
use super::*;
use crate::application::workflow::message::MessageObservation;
use crate::application::workflow::waits::WaitFailureReason;

async fn uncertainty_codes(setup: &Setup) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT failure_code FROM workflow_action_reconciliations WHERE run_id=$1 ORDER BY failure_code",
    )
    .bind(setup.scope().run.as_uuid())
    .fetch_all(setup.pool())
    .await
    .unwrap()
}

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct Execution {
    executions: i64,
    completed: bool,
    output: bool,
    successor: bool,
    wait_code: Option<String>,
}
async fn execution(setup: &Setup) -> Execution {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM workflow_executions WHERE run_id=$1) AS executions, \
                execution.completed_at IS NOT NULL AS completed, execution.committed_output IS NOT NULL AS output, \
                execution.successor_execution_id IS NOT NULL AS successor, \
                (SELECT wait.failure_code FROM workflow_waits AS wait WHERE wait.execution_id=execution.id) AS wait_code \
           FROM workflow_executions AS execution WHERE execution.id=$2",
    )
    .bind(setup.scope().run.as_uuid())
    .bind(setup.scope().execution.as_uuid())
    .fetch_one(setup.pool())
    .await
    .unwrap()
}

/// D4 §S F012: a breach (row 20), then every destination final, then the consumer writes the
/// receipt and the poll resumes. The veto wins over the receipt, accepted or failed: no
/// successor or output, wait `failed/evidence_conflict`, job `failed`, run
/// `waiting/reconciliation`; the receipt and the first-uncertainty observation stay recorded.
#[tokio::test]
async fn workflow_message_reconcile_finality_breach_then_every_destination_final_vetoes_the_receipt()
 {
    for (sibling_outcome, receipt_status) in [("delivered", "accepted"), ("dead", "failed")] {
        let setup = Box::pin(send(&[To::Owner, To::User])).await;
        parked(&setup).await;
        let (p, scope, run) = (&setup.fixture.persistence, setup.scope(), setup.scope().run);
        let (sibling, subject) = (
            delivery(setup.pool(), run, 0).await,
            delivery(setup.pool(), run, 1).await,
        );
        let accepted_at = Utc::now();
        deliver(setup.pool(), sibling, sibling_outcome).await;
        send_unknown(&setup, subject).await;
        assert_eq!(p.consume_message_receipts(scope, lease()).await.unwrap(), 1);
        assert_eq!(action_state(setup.pool(), run).await, "uncertain");
        ledger(&setup, subject, "not_applied", None, 60.0, None).await;
        let proof = command(&setup, subject, "breached-proof", verified()).await;
        assert_eq!(
            reconcile(&service(&setup, true), &proof)
                .await
                .unwrap()
                .outcome,
            ReconciliationOutcome::DeliveryReopened,
            "{sibling_outcome}"
        );
        send_unknown(&setup, subject).await;
        ledger(&setup, subject, "applied", Some(accepted_at), 60.0, None).await;
        let truth = command(&setup, subject, "breaching-truth", verified()).await;
        assert_eq!(
            reconcile(&service(&setup, true), &truth)
                .await
                .unwrap()
                .outcome,
            ReconciliationOutcome::EvidenceConflict,
            "{sibling_outcome}"
        );
        let sibling_before = delivery_row(setup.pool(), sibling).await;
        // Every destination is final: the consumer derives the receipt the breach leaves intact.
        assert_eq!(p.consume_message_receipts(scope, lease()).await.unwrap(), 1);
        assert_eq!(action_state(setup.pool(), run).await, receipt_status);
        let receipt: Value =
            sqlx::query_scalar("SELECT result FROM workflow_action_receipts WHERE run_id=$1")
                .bind(run.as_uuid())
                .fetch_one(setup.pool())
                .await
                .unwrap();
        assert_eq!(receipt["status"], receipt_status);
        assert_eq!(candidates(&setup).await, vec![PollWork::Wait]);
        assert!(
            matches!(
                p.resume_wait(scope).await.unwrap(),
                WaitProgress::Failed {
                    reason: WaitFailureReason::EvidenceConflict,
                    ..
                }
            ),
            "{sibling_outcome}"
        );
        let after = state(setup.pool(), scope).await;
        assert_eq!(
            (
                after.run.as_str(),
                after.waiting.as_deref(),
                after.job.as_str(),
                after.wait.as_deref()
            ),
            ("waiting", Some("reconciliation"), "failed", Some("failed")),
            "{sibling_outcome}"
        );
        assert_eq!(
            execution(&setup).await,
            Execution {
                executions: 1,
                completed: false,
                output: false,
                successor: false,
                wait_code: Some("evidence_conflict".into()),
            },
            "{sibling_outcome}"
        );
        // The receipt is retained truth. The marker's uncertainty is its first observation: the
        // accepted reconciliation guard refuses an observation for a receipted marker, so the
        // park's `record(action.evidence_conflict)` adds none (TASK R006 amendment A1).
        assert_eq!(rows(setup.pool(), run).await.receipts, 1);
        assert_eq!(
            uncertainty_codes(&setup).await,
            vec!["message.outcome_unknown".to_string()]
        );
        let invocation = invocations(setup.pool(), run).await[0];
        assert_eq!(
            effect_state(setup.pool(), invocation).await,
            ("committed".into(), true)
        );
        let reason: String = sqlx::query_scalar(
            "SELECT reason FROM workflow_action_evidence_conflicts WHERE run_id=$1",
        )
        .bind(run.as_uuid())
        .fetch_one(setup.pool())
        .await
        .unwrap();
        assert_eq!(reason, "finality_breach");
        // Nothing was unsent to supersede; the final sibling is untouched.
        assert_eq!(delivery_row(setup.pool(), sibling).await, sibling_before);
        assert!(candidates(&setup).await.is_empty());
        assert!(candidates(&setup).await.is_empty());
        let reopen_safe: bool =
            sqlx::query_scalar("SELECT workflow_action_reconciliation_reopen_safe($1,$2,$3)")
                .bind(scope.company.as_uuid())
                .bind(run.as_uuid())
                .bind(scope.job.0)
                .fetch_one(setup.pool())
                .await
                .unwrap();
        assert!(!reopen_safe, "{sibling_outcome}");
    }
}

/// A reopen insert for `delivery` from `evidence`, copying every other column from the facts.
async fn reopen_from(setup: &Setup, evidence: Uuid, delivery: Uuid) -> Option<String> {
    sqlx::query(
        "INSERT INTO workflow_message_delivery_reopens(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,id,evidence_id,ordinal,delivery_id,attempt_count,valid_until) \
         SELECT evidence.company_id,evidence.run_id,evidence.execution_id,evidence.invocation_id,evidence.argument_digest,evidence.dispatch_id,gen_random_uuid(),evidence.id,target.ordinal,target.delivery_id,delivery.attempt_count,evidence.valid_until \
           FROM workflow_action_evidence AS evidence \
           JOIN workflow_message_destinations AS target ON target.company_id=evidence.company_id AND target.invocation_id=evidence.invocation_id AND target.delivery_id=$2 \
           JOIN message_deliveries AS delivery ON delivery.id=target.delivery_id WHERE evidence.id=$1",
    )
    .bind(evidence)
    .bind(delivery)
    .execute(setup.pool())
    .await
    .err()
    .map(|error| {
        let database = error.as_database_error().expect("a database refusal");
        assert!(
            database.code().as_deref() != Some("23514")
                || database
                    .message()
                    .contains("requires one unexpired final proof"),
            "{error}"
        );
        database.code().unwrap().into_owned()
    })
}

/// D3 §K SQL-only negatives: the reopen guard refuses expired, non-final and foreign evidence on
/// its own. A control insert that differs only in the refused fact passes the guard and is
/// stopped by one-use consumption (23505), so each 23514 is that fact alone.
#[tokio::test]
async fn workflow_message_reconcile_sql_reopen_refuses_expired_non_final_and_foreign_evidence() {
    let setup = Box::pin(send(&[To::Owner, To::User])).await;
    parked(&setup).await;
    let run = setup.scope().run;
    let (sibling, subject) = (
        delivery(setup.pool(), run, 0).await,
        delivery(setup.pool(), run, 1).await,
    );
    send_unknown(&setup, sibling).await;
    send_unknown(&setup, subject).await;
    ledger(&setup, subject, "not_applied", None, 3.0, None).await;
    let proof = reconcile(
        &service(&setup, true),
        &command(&setup, subject, "sql-proof", verified()).await,
    )
    .await
    .unwrap();
    assert_eq!(proof.outcome, ReconciliationOutcome::DeliveryReopened);
    let proof = proof.evidence.unwrap().as_uuid();
    // The resend is ambiguous again: the subject is unknown at its new attempt count.
    send_unknown(&setup, subject).await;
    // Control: the proof's own delivery passes the guard and hits one-use consumption.
    assert_eq!(
        reopen_from(&setup, proof, subject).await.as_deref(),
        Some("23505")
    );
    // Foreign: the same live final proof names another destination's delivery.
    assert_eq!(
        reopen_from(&setup, proof, sibling).await.as_deref(),
        Some("23514")
    );
    // Non-final: an unknown note for the sibling.
    let note = reconcile(
        &service(&setup, false),
        &command(
            &setup,
            sibling,
            "sql-note",
            EvidenceInput::UnknownNote {
                note: "no ledger".into(),
                claimed: ClaimedDisposition::NotApplied,
            },
        )
        .await,
    )
    .await
    .unwrap();
    assert_eq!(note.outcome, ReconciliationOutcome::UnknownRecorded);
    assert_eq!(
        reopen_from(&setup, note.evidence.unwrap().as_uuid(), sibling)
            .await
            .as_deref(),
        Some("23514")
    );
    // Expired: once its validity passes, the control insert is refused by the guard itself.
    let remaining: f64 = sqlx::query_scalar(
        "SELECT GREATEST(0, extract(epoch FROM (valid_until-clock_timestamp())))::float8 FROM workflow_action_evidence WHERE id=$1",
    )
    .bind(proof)
    .fetch_one(setup.pool())
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_secs_f64(remaining + 0.05)).await;
    assert_eq!(
        reopen_from(&setup, proof, subject).await.as_deref(),
        Some("23514")
    );
    let reopens: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_message_delivery_reopens")
        .fetch_one(setup.pool())
        .await
        .unwrap();
    assert_eq!(reopens, 1);
    for delivery in [sibling, subject] {
        assert_eq!(
            delivery_row(setup.pool(), delivery).await.status,
            "outcome_unknown"
        );
    }
}

/// D3 §K: an admin without access to the run's association (an allowlisted run channel with no
/// view grant) is refused before any write; the same admin with the grant is not.
#[tokio::test]
async fn workflow_message_reconcile_wrong_association_actor_is_refused() {
    let (setup, _, unknown) = Box::pin(uncertain()).await;
    let company = setup.scope().company.as_uuid();
    sqlx::query("UPDATE channels SET access_mode='allowlist' WHERE id=$1")
        .bind(setup.channel)
        .execute(setup.pool())
        .await
        .unwrap();
    let admin = member(&setup.fixture, "admin").await;
    // Isolate channel-grant authorization from the separate current-email proof guard.
    sqlx::query("UPDATE users SET email_verified_at = CURRENT_TIMESTAMP WHERE id = $1")
        .bind(admin)
        .execute(setup.pool())
        .await
        .unwrap();
    let actor = WorkflowActor::authenticated(admin).unwrap();
    let note = || EvidenceInput::UnknownNote {
        note: "checked elsewhere".into(),
        claimed: ClaimedDisposition::Unknown,
    };
    let recorded = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM workflow_action_evidence_commands")
            .fetch_one(setup.pool())
            .await
            .unwrap()
    };
    let refused = command_as(&setup, unknown, "outside-association", note(), actor).await;
    assert!(
        reconcile(&service(&setup, false), &refused).await.is_err(),
        "no view grant on the run's channel"
    );
    assert_eq!(recorded().await, 0);
    sqlx::query("INSERT INTO channel_principal_grants(company_id,channel_id,principal_id,capability,provenance) SELECT $1,$2,id,'view','configured_allowlist' FROM principals WHERE company_id=$1 AND user_id=$3")
        .bind(company).bind(setup.channel).bind(admin).execute(setup.pool()).await.unwrap();
    let granted = command_as(&setup, unknown, "inside-association", note(), actor).await;
    assert_eq!(
        reconcile(&service(&setup, false), &granted)
            .await
            .unwrap()
            .outcome,
        ReconciliationOutcome::UnknownRecorded
    );
    assert_eq!(recorded().await, 1);
    assert_eq!(
        delivery_row(setup.pool(), unknown).await.status,
        "outcome_unknown"
    );
}

/// D4 §L port seam: a `MessageDelivery` reconciliation of a saved model-tool call's unknown
/// delivery reopens only the delivery. The agent job stays parked on its own effect wait
/// until a settled receipt continues it (D4 delivery continuation).
#[tokio::test]
async fn workflow_message_reconcile_model_tool_delivery_reopen_never_reopens_the_job() {
    let setup = Box::pin(send(&[To::Channel])).await;
    let (p, scope, run) = (&setup.fixture.persistence, setup.scope(), setup.scope().run);
    let observed = Box::pin(tool(
        &setup,
        "agent-call",
        send_arguments(&[("user", setup.user)], "From the agent"),
    ))
    .await
    .unwrap();
    assert!(matches!(observed, MessageObservation::DeliveryParked(_)));
    let parked_state = |state: State| {
        (
            state.run,
            state.waiting,
            state.job,
            state.attempt,
            state.wait,
        )
    };
    let before = parked_state(state(setup.pool(), scope).await);
    assert_eq!(
        (
            before.0.as_str(),
            before.1.as_deref(),
            before.2.as_str(),
            before.4.as_deref()
        ),
        ("waiting", Some("effect"), "completed", Some("waiting"))
    );
    let unknown = delivery(setup.pool(), run, 0).await;
    send_unknown(&setup, unknown).await;
    assert_eq!(p.consume_message_receipts(scope, lease()).await.unwrap(), 1);
    assert_eq!(action_state(setup.pool(), run).await, "uncertain");
    let job_before: (i32, String) =
        sqlx::query_as("SELECT retry_count, status FROM background_tasks WHERE id=$1")
            .bind(scope.job.0)
            .fetch_one(setup.pool())
            .await
            .unwrap();
    ledger(&setup, unknown, "not_applied", None, 60.0, None).await;
    let command = command(&setup, unknown, "model-tool-reopen", verified()).await;
    assert_eq!(
        reconcile(&service(&setup, true), &command)
            .await
            .unwrap()
            .outcome,
        ReconciliationOutcome::DeliveryReopened
    );
    let reopened = delivery_row(setup.pool(), unknown).await;
    assert_eq!(
        (reopened.status.as_str(), reopened.part.as_str()),
        ("retryable", "retryable")
    );
    // Only the delivery: the job, its attempts, the run and the execution are as parked.
    assert_eq!(parked_state(state(setup.pool(), scope).await), before);
    let job_after: (i32, String) =
        sqlx::query_as("SELECT retry_count, status FROM background_tasks WHERE id=$1")
            .bind(scope.job.0)
            .fetch_one(setup.pool())
            .await
            .unwrap();
    assert_eq!(job_after, job_before);
    let scheduled: Option<Uuid> = sqlx::query_scalar(
        "SELECT scheduled_job_id FROM workflow_action_evidence_commands WHERE command_key='model-tool-reopen'",
    )
    .fetch_one(setup.pool())
    .await
    .unwrap();
    assert!(scheduled.is_none());
    let executions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_executions WHERE run_id=$1")
            .bind(run.as_uuid())
            .fetch_one(setup.pool())
            .await
            .unwrap();
    assert_eq!(executions, 1);
    // The resend's receipt is truth only; the park still stands.
    assert_eq!(p.consume_message_receipts(scope, lease()).await.unwrap(), 1);
    deliver(setup.pool(), unknown, "delivered").await;
    assert_eq!(p.consume_message_receipts(scope, lease()).await.unwrap(), 1);
    assert_eq!(action_state(setup.pool(), run).await, "accepted");
    assert_eq!(parked_state(state(setup.pool(), scope).await), before);
    // The settled receipt now continues the parked agent job, exactly once.
    assert!(matches!(
        p.resume_wait(scope).await.unwrap(),
        crate::application::workflow::waits::WaitProgress::Continued
    ));
    let job: (i32, i16, String) = sqlx::query_as(
        "SELECT retry_count, continuation_count, status FROM background_tasks WHERE id=$1",
    )
    .bind(scope.job.0)
    .fetch_one(setup.pool())
    .await
    .unwrap();
    assert_eq!(job, (job_before.0, 1, "pending".to_string()));
}
