//! (h) A budget reservation stalled in COMMIT against a second reservation of the same key.
use super::*;
use crate::adapters::persistence::workflow::persistence_bound::recorder::Injected;
use crate::adapters::persistence::workflow::tests::admission_tests::lease_tests::resolution_gate::*;

/// The ambiguous call answers `Replayed` (no reauthorization) when its receipt committed and
/// is not applied when it aborted; either way exactly one debit exists.
#[tokio::test]
async fn workflow_budget_resolution_stalled_reservation_debits_once() {
    let _injected = Injected::mark();
    for applies in [true, false] {
        let (f, scope) = limited().await;
        let fence = f
            .persistence()
            .settled()
            .claim_io(scope, worker(), gated_policy())
            .await
            .unwrap()
            .unwrap()
            .fence;
        let req = request(fence, BudgetResource::ModelCall, "op", 2);
        let (mut observer, _) = connect(&f).await;
        let mut gate = Gate::arm(
            &f,
            "workflow_budget_receipts",
            "INSERT",
            "true",
            Stall::Commit,
        )
        .await;
        let spawn_reserve = || {
            let p = f.persistence().clone();
            let req = req.clone();
            tokio::spawn(async move { p.reserve_budget(req, gated_policy()).await })
        };
        let first = spawn_reserve();
        let reserving = gate
            .stalled(&mut observer, &[&|| !first.is_finished()])
            .await;
        if !applies {
            gate.abort_on_release(Abort::Backend(reserving)).await;
        }
        resolution_behind(
            &mut observer,
            reserving,
            gated_policy(),
            &[&|| !first.is_finished()],
        )
        .await;
        let second = spawn_reserve();
        waiters(
            &mut observer,
            reserving,
            2,
            "reservation resolution and second reservation",
            &[&|| !first.is_finished(), &|| !second.is_finished()],
        )
        .await;
        gate.release().await;
        let first = first.await.unwrap();
        let second = second.await.unwrap().unwrap();
        if applies {
            assert_eq!(
                first.unwrap(),
                BudgetReservationResult::Replayed(BudgetDisposition::Granted)
            );
            assert_eq!(
                second,
                BudgetReservationResult::Replayed(BudgetDisposition::Granted)
            );
        } else {
            assert_eq!(
                second,
                BudgetReservationResult::Recorded(BudgetDisposition::Granted)
            );
            // The scoped receipt is the evidence and carries no writer: a resolution that
            // runs after the second reservation committed answers the conservative replay.
            match first {
                Err(error) => assert!(not_applied(&error, "budget.reserve"), "{error:?}"),
                Ok(result) => assert_eq!(
                    result,
                    BudgetReservationResult::Replayed(BudgetDisposition::Granted)
                ),
            }
        }
        let saved = accounting(&f).await;
        let model_calls = saved["receipts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|receipt| receipt["resource"] == "model_call")
            .count();
        assert_eq!(model_calls, 1);
        assert_eq!(saved["usage"][0]["model_calls"], 2);
    }
}
