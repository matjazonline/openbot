//! Original row3c: retired high-entry history with genuine supported replay.
use super::*;
use crate::application::workflow::lease::WorkflowFailure;
use crate::application::workflow::{RetryCommand, RetryResult};
use crate::domain::workflow::{FailureClass, FailureCode, RetrySafety, StepFailure};

#[path = "action_reconciliation_high_bounds_assertions.rs"]
mod assertions;
#[path = "action_reconciliation_high_bounds_native.rs"]
mod native;
#[path = "action_reconciliation_high_bounds_source.rs"]
mod source;
use assertions::*;
use source::*;

struct HighSource {
    fixture: AdmissionFixture,
    request: ActionDispatchRequest,
    provider: RegisteredLedger,
    proof: Uuid,
    historical: Uuid,
    other_invocation: Uuid,
    other_company: Uuid,
    /// When attempt 130's lease was claimed; phase diagnostics measure from here.
    claimed: std::time::Instant,
}

/// Every phase after the claim needs attempt 130 live, so a probe that finds it lost fails
/// as a labelled lease lapse rather than as whichever assertion meets it next.
async fn phase(source: &HighSource, label: &str) -> LeasePhase {
    let state = lease_phase(
        &source.fixture,
        source.request.fence,
        "high_bounds",
        label,
        source.claimed,
    )
    .await;
    assert!(
        state.owner && state.lease_ms > 0.0 && state.run_ms > 0.0,
        "LEASE LAPSE scenario=high_bounds phase={label}: {state:?}"
    );
    state
}

/// Renew attempt 130 at a quiescent boundary before its window can end (DEC-010).
async fn renew(source: &HighSource, label: &str, snapshots: &mut [&mut Value]) {
    renew_phase(
        &source.fixture,
        source.request.fence,
        "high_bounds",
        label,
        source.claimed,
        snapshots,
    )
    .await;
}

#[tokio::test]
async fn workflow_action_reconciliation_high_bounds_native_owners_and_supported_entry() {
    // Box fixture and native phases to retain the stock 2 MiB libtest stack.
    let source = Box::pin(high_source()).await;
    phase(&source, "source_ready").await;
    let catalog = native_catalog(&source.fixture).await;
    phase(&source, "catalog").await;
    let mut before = all_tables(&source.fixture).await;
    phase(&source, "before_snapshot").await;
    renew(&source, "invalid_exclusions", &mut [&mut before]).await;
    Box::pin(native::invalid_exclusions(&source)).await;
    phase(&source, "invalid_exclusions").await;
    for attack in [native::Attack::Entry, native::Attack::Consumption] {
        renew(&source, "reject_candidate", &mut [&mut before]).await;
        Box::pin(native::reject_candidate(&source, attack)).await;
        phase(&source, "reject_candidate").await;
        unchanged(&source, &before, &catalog).await;
    }
    for attack in [
        native::Attack::HistoricalSpoof,
        native::Attack::HistoricalRetrofit,
    ] {
        renew(&source, "reject_historical", &mut [&mut before]).await;
        Box::pin(native::reject_historical(&source, attack)).await;
        phase(&source, "reject_historical").await;
        unchanged(&source, &before, &catalog).await;
    }
    Box::pin(native::supported_positive(&source)).await;
    assert_eq!(native_catalog(&source.fixture).await, catalog);
    source.fixture.persistence().pool().close().await;
}

async fn unchanged(source: &HighSource, before: &Value, catalog: &Value) {
    assert_eq!(
        &all_tables(&source.fixture).await,
        before,
        "every public row rolls back"
    );
    assert_eq!(&native_catalog(&source.fixture).await, catalog);
    assert_eq!(source.provider.inner.calls.load(Ordering::SeqCst), 128);
    assert_eq!(effects(&source.fixture).await, 0);
    phase(source, "unchanged").await;
    assert_subject(&source.fixture, &source.request, 130, "processing").await;
}
