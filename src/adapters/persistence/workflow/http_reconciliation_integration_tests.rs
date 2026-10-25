//! Phase10: genuine publication/admission, HTTP effects and reconciliation service ownership.
use super::*;
use crate::application::app_error::AppResult;
use crate::application::workflow::completion::WorkflowCompletion;
use crate::application::workflow::metrics::WorkflowMetricPersistence;
use crate::domain::workflow::{ActionRemoteEntryId, ActionRemoteMarkerId};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[path = "http_reconciliation_integration_support.rs"]
mod integration_support;
use integration_support::*;

#[tokio::test]
async fn workflow_cutover_http_historical_evidence_never_authorizes_an_extra_effect() {
    let provider = Provider::start().await;
    // Real compiler/publication, configured connection, binding and application admission.
    // Box phase seams so this full journey also runs on the stock2MiB test stack.
    let setup = Box::pin(setup(&provider.origin, Entrance::Http)).await;
    let p = &setup.fixture.persistence;
    let request = p
        .http_step_request(&setup.claim, &setup.kind)
        .await
        .unwrap();
    let failure = runtime(&setup)
        .run(&setup.kind, &setup.claim)
        .await
        .completed_result()
        .unwrap_err();
    assert_eq!(provider.calls(), 1);
    assert_eq!(provider.effects(), 0);
    assert_eq!(
        retire(p, &setup.claim, lease(), failure)
            .await
            .waiting_reason
            .as_deref(),
        Some("reconciliation")
    );
    let proof = CapturedProof::capture(&setup, &request, Truth::Rejected).await;
    let metrics = p.workflow_metrics().await.unwrap();
    assert_eq!(metrics.waiting_runs, 1.0);
    assert_eq!(metrics.reconciliation_runs, 1.0);
    assert_eq!(metrics.unknown_actions, 1.0);
    let original = command(&setup, &request, "cutover-rejected").await;
    let scheduled = Box::pin(reconcile(&setup, &original, proof.clone()))
        .await
        .unwrap();
    assert_eq!(
        scheduled.outcome,
        ReconciliationOutcome::Scheduled {
            receipt_only: false
        }
    );
    let before = snapshot(&setup).await;
    let replay = Box::pin(reconcile(&setup, &original, proof.clone()))
        .await
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(
        snapshot(&setup).await,
        before,
        "duplicate command after service restart writes nothing"
    );

    let next = Box::pin(claim_competitors(&setup)).await;
    assert_eq!(next.fence.attempt.0, setup.claim.fence.attempt.0 + 1);
    provider.apply_next();
    let left = runtime(&setup);
    let right = runtime(&setup);
    let (a, b) = tokio::join!(left.run(&setup.kind, &next), right.run(&setup.kind, &next));
    assert!(a.completed_result().is_err());
    let failure = b.completed_result().unwrap_err();
    assert_eq!(
        provider.calls(),
        2,
        "competing dispatchers spend the proof once"
    );
    assert_eq!(
        provider.effects(),
        1,
        "the second request applies despite its lost answer"
    );
    retire(p, &next, lease(), failure).await;
    let state = snapshot(&setup).await;
    assert_eq!(count(&state, "task_attempts"), 2);
    assert_eq!(count(&state, "workflow_action_remote_entries"), 2);
    assert_eq!(count(&state, "workflow_action_evidence_consumptions"), 1);
    assert_eq!(count(&state, "workflow_action_claim_episodes"), 1);
    assert_eq!(
        state["workflow_runs"][0]["waiting_reason"],
        "reconciliation"
    );

    Box::pin(refuse_historical(&setup, &request, &original, proof)).await;
    Box::pin(recover_applied(&setup, &request, &provider)).await;
    let metrics = p.workflow_metrics().await.unwrap();
    assert_eq!(metrics.waiting_runs, 0.0);
    assert_eq!(metrics.reconciliation_runs, 0.0);
    assert_eq!(metrics.unknown_actions, 0.0);
    provider.finish().await;
}

async fn refuse_historical(
    setup: &Setup,
    request: &ActionRequest,
    original: &ReconcileActionCommand,
    proof: Arc<CapturedProof>,
) {
    let before = snapshot(setup).await;
    let replay = Box::pin(reconcile(setup, original, proof.clone()))
        .await
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(
        snapshot(setup).await,
        before,
        "historical success replay cannot reopen newer uncertainty"
    );
    let mut stale = command(setup, request, "cutover-stale-revision").await;
    stale.expected_revision = original.expected_revision;
    assert_eq!(
        Box::pin(reconcile(setup, &stale, proof.clone()))
            .await
            .unwrap()
            .outcome,
        ReconciliationOutcome::RevisionConflict
    );
    let after_stale = snapshot(setup).await;
    assert_eq!(
        count(&after_stale, "workflow_action_evidence"),
        count(&before, "workflow_action_evidence")
    );
    let fresh_key = command(setup, request, "cutover-old-observation").await;
    assert!(
        Box::pin(reconcile(setup, &fresh_key, proof)).await.is_err(),
        "old provider observation predates the new request and cannot become a fresh grant"
    );
    assert_eq!(
        snapshot(setup).await,
        after_stale,
        "rejected stale observation writes nothing"
    );
    let p = &setup.fixture.persistence;
    let ready = tokio::sync::Barrier::new(2);
    let claim = || async {
        ready.wait().await;
        p.claim_io(
            setup.claim.fence.scope,
            WorkflowWorkerId(Uuid::new_v4()),
            lease(),
        )
        .await
        .unwrap()
    };
    let (left, right) = tokio::join!(claim(), claim());
    assert!(left.is_none() && right.is_none());
    assert_eq!(
        snapshot(setup).await,
        after_stale,
        "no third attempt or continuation from old evidence"
    );
}

async fn recover_applied(setup: &Setup, request: &ActionRequest, provider: &Provider) {
    let p = &setup.fixture.persistence;
    let entry: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_action_remote_entries ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .fetch_one(p.pool())
    .await
    .unwrap();
    let proof = CapturedProof::capture(
        setup,
        request,
        Truth::Applied(ActionRemoteEntryId::new(entry)),
    )
    .await;
    let current = command(setup, request, "cutover-recovered-applied").await;
    assert_eq!(
        Box::pin(reconcile(setup, &current, proof))
            .await
            .unwrap()
            .outcome,
        ReconciliationOutcome::Scheduled { receipt_only: true }
    );
    let next = Box::pin(claim_competitors(setup)).await;
    let output = runtime(setup)
        .run(&setup.kind, &next)
        .await
        .completed_result()
        .unwrap();
    assert_eq!(output["body"]["ok"], true);
    assert_eq!(
        provider.calls(),
        2,
        "restart consumes the receipt without a third HTTP request"
    );
    assert_eq!(provider.effects(), 1);
    let committed = p
        .complete_io(FencedWorkflowResult {
            fence: next.fence,
            policy: lease(),
            output,
        })
        .await
        .unwrap();
    assert!(
        committed.is_some(),
        "real successful progression is the positive control"
    );
    let state = snapshot(setup).await;
    assert_eq!(state["workflow_runs"][0]["state"], "succeeded");
    assert_eq!(count(&state, "workflow_executions"), 1);
    assert!(!state["workflow_executions"][0]["completed_at"].is_null());
    assert_eq!(count(&state, "workflow_action_receipts"), 1);
    assert_eq!(count(&state, "workflow_action_evidence_consumptions"), 1);
    assert_eq!(count(&state, "workflow_action_remote_entries"), 2);
    assert_eq!(count(&state, "task_attempts"), 3);
}
