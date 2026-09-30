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
}

#[tokio::test]
async fn workflow_action_reconciliation_high_bounds_native_owners_and_supported_entry() {
    // Box fixture and native phases to retain the stock 2 MiB libtest stack.
    let source = Box::pin(high_source()).await;
    let catalog = native_catalog(&source.fixture).await;
    let before = all_tables(&source.fixture).await;
    Box::pin(native::invalid_exclusions(&source)).await;
    for attack in [native::Attack::Entry, native::Attack::Consumption] {
        Box::pin(native::reject_candidate(&source, attack)).await;
        unchanged(&source, &before, &catalog).await;
    }
    for attack in [
        native::Attack::HistoricalSpoof,
        native::Attack::HistoricalRetrofit,
    ] {
        Box::pin(native::reject_historical(&source, attack)).await;
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
    assert_subject(&source.fixture, &source.request, 130, "processing").await;
}
