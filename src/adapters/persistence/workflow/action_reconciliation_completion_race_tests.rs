//! Actual late receipt and usable live completion compete under the owning run lock.
use super::*;
use crate::application::workflow::completion::CommitDisposition;

#[path = "action_reconciliation_completion_race_assertions.rs"]
mod assertions;
#[path = "action_reconciliation_completion_race_lock_support.rs"]
mod locks;

#[derive(Clone, Copy, Debug)]
enum Winner {
    Completion,
    Receipt,
}

async fn usable_result(h: &HeldTruth) -> Value {
    // This new entry consumes real final proof. It is not the covered old entry.
    let (entry, result) = Box::pin(entered_result(h)).await;
    let new_id = entry.entry;
    assert_ne!(new_id, h.entry_id);
    let returned = adapter(&h.fixture, 0)
        .finish_remote(entry, result.clone())
        .await
        .unwrap();
    let RemoteDispatchObservation::Committed(receipt) = returned else {
        panic!("genuine new usable result before late old receipt");
    };
    assert_eq!(receipt.result, result);
    assert_eq!(result, h.result);
    let state = live_snapshot(h).await;
    assert_eq!(counts(&h.fixture).await, (1, 1, 0));
    assert_eq!(
        state["workflow_action_receipts"][0]["remote_entry_id"],
        json!(new_id)
    );
    assert_eq!(state["workflow_action_receipts"][0]["result"], result);
    assert_eq!(
        state["workflow_action_evidence_consumptions"][0]["remote_entry_id"],
        json!(new_id)
    );
    assert!(state["workflow_action_receipts"][0]["reconciliation_evidence_id"].is_null());
    assert_eq!(
        state["workflow_action_actual_receipt_observations"][0]["remote_entry_id"],
        json!(new_id)
    );
    state
}

async fn case(first: Winner) {
    let mut h = Box::pin(live_retry()).await;
    let mut gate = h.fixture.persistence().pool().acquire().await.unwrap();
    locks::install_pause(&h, &mut gate).await;
    locks::controls(&h).await;
    // Positive pause-isolation control: holding the gate for OLD cannot prevent
    // genuine NEW receipt persistence/return. No fixture writes supply truth.
    let before = Box::pin(usable_result(&h)).await;
    let result = Box::pin(locks::race(&mut h, first, &mut gate)).await;
    locks::remove_pause(&mut gate).await;
    drop(gate);
    let after = all_tables(&h.fixture).await;
    assertions::truth(&h, &before, &after);
    match first {
        Winner::Receipt => {
            assert!(result.completion.is_none());
            assert!(result.completed_before_receipt.is_none());
            assertions::retired(&h, &before, &after);
        }
        Winner::Completion => {
            let committed = result.completion.unwrap();
            assert_eq!(committed.disposition, CommitDisposition::Committed);
            assert_eq!(committed.output, h.result);
            assert!(committed.successor.is_none());
            let completed = result.completed_before_receipt.unwrap();
            assertions::completed(&h, &before, &completed);
            // The opposite owner subsequently adds authentic conflict audit; all
            // already committed execution/job/attempt/output/route history stays exact.
            unchanged_tables(
                &completed,
                &after,
                &[
                    "workflow_runs",
                    "workflow_action_actual_receipt_observations",
                    "workflow_action_evidence_conflicts",
                ],
            );
            same_table_except(
                &completed["workflow_runs"],
                &after["workflow_runs"],
                &json!(h.request.scope().run.as_uuid()),
                &["revision"],
            );
            assert!(
                after["workflow_runs"][0]["revision"].as_i64().unwrap()
                    > completed["workflow_runs"][0]["revision"].as_i64().unwrap()
            );
        }
    }
    Box::pin(assertions::inert(&h, &after, first)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_completion_race_actual_receipt_first() {
    Box::pin(case(Winner::Receipt)).await;
}

#[tokio::test]
async fn workflow_action_reconciliation_completion_race_completion_first() {
    Box::pin(case(Winner::Completion)).await;
}
