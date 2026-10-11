//! (g) A remote receipt commit stalled in COMMIT against a completion queued on the run.
use super::*;
use crate::adapters::persistence::workflow::persistence_bound::recorder::Injected;
use crate::adapters::persistence::workflow::tests::admission_tests::lease_tests::resolution_gate::*;
use crate::application::workflow::completion::*;

async fn observations(f: &AdmissionFixture) -> (i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM workflow_action_actual_receipt_observations), (SELECT count(*) FROM workflow_action_receipts)")
        .fetch_one(f.persistence().pool()).await.unwrap()
}

/// The resolution reads this entry's observation row under the run lock: one observation
/// and one receipt when the commit applied, none when it aborted; the provider is invoked
/// once either way. A commit proved aborted gets one retry inside the same deadline; here
/// every transaction through the gate aborts, so the retry fails too (at its own COMMIT, or
/// not applied when the deadline runs out first).
#[tokio::test]
async fn workflow_action_receipt_resolution_stalled_commit_against_completion() {
    let _injected = Injected::mark();
    for applies in [true, false] {
        let (f, mut request, contract) = receipt_fixture(ActionRecovery::SafeRepeat, false).await;
        // The 6 s gate policy (A = 2 s, R = 1 s), with the fence renewed to match it: the
        // completion must queue behind the resolution within R.
        let lease = gated_policy();
        assert!(
            f.persistence()
                .settled()
                .renew_io(request.fence, lease)
                .await
                .unwrap()
                .is_some()
        );
        request.lease = lease;
        let provider = dedup(&f, contract, false);
        let (mut observer, _) = connect(&f).await;
        let mut gate = Gate::arm(
            &f,
            "workflow_action_actual_receipt_observations",
            "INSERT",
            "true",
            Stall::Commit,
        )
        .await;
        let service = ActionService::new(adapter(&f, 0));
        let done = AtomicBool::new(false);
        let cancel = CancellationToken::new();
        let dispatch = async {
            let result = service.dispatch_remote(&request, &provider, &cancel).await;
            done.store(true, Ordering::SeqCst);
            result
        };
        let p = f.persistence().clone();
        let fence = request.fence;
        let drive = async {
            let running = || !done.load(Ordering::SeqCst);
            let committing = gate.stalled(&mut observer, &[&running]).await;
            if !applies {
                gate.abort_on_release(Abort::Every).await;
            }
            resolution_behind(&mut observer, committing, lease, &[&running]).await;
            let completion = tokio::spawn(async move {
                p.complete_io(FencedWorkflowResult {
                    policy: lease,
                    fence,
                    output: json!({"items":[],"token_count":0}),
                })
                .await
            });
            waiters(
                &mut observer,
                committing,
                2,
                "receipt resolution and completion",
                &[&running, &|| !completion.is_finished()],
            )
            .await;
            gate.release().await;
            completion.await.unwrap()
        };
        let (dispatched, completed) = tokio::join!(dispatch, drive);
        // The competing completion runs after the resolution, under the same run lock, and
        // commits the step exactly once whichever way the receipt resolved.
        let step = completed
            .unwrap()
            .expect("the completion owned the live step");
        assert_eq!(step.disposition, CommitDisposition::Committed);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(effect_count(&f).await, 1);
        if applies {
            match dispatched.unwrap() {
                RemoteDispatchObservation::Committed(receipt) => {
                    assert_eq!(receipt.result, json!({"written":true}));
                }
                // The completion queued behind the resolution may already own the step.
                RemoteDispatchObservation::Interrupted => {}
                _ => panic!("the stalled receipt neither committed nor lost ownership"),
            }
            assert_eq!(observations(&f).await, (1, 1));
        } else {
            let Err(error) = dispatched else {
                panic!("the aborted receipt commit reported success")
            };
            // The original COMMIT elapsed client-side, so an injected abort can only be the
            // retry's own COMMIT.
            let retried = matches!(&error, AppError::Database(message) if message.contains("injected commit abort"));
            assert!(
                retried || not_applied(&error, "action.commit_remote"),
                "{error:?}"
            );
            eprintln!("aborted receipt commit: retry reached its COMMIT = {retried}");
            assert_eq!(observations(&f).await, (0, 0));
        }
    }
}

/// Applies the effect, then takes every pooled connection and returns them `hold` after the
/// provider returned: the receipt commit right after the effect finds the pool exhausted.
struct ExhaustAfterEffect<'a> {
    inner: DeduplicatingProvider<'a>,
    pool: sqlx::PgPool,
    hold: Duration,
    held: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}
#[async_trait]
impl RemoteAction for ExhaustAfterEffect<'_> {
    fn replay_contract(&self) -> Option<&ProviderReplayContract> {
        self.inner.replay_contract()
    }
    async fn invoke(
        &self,
        action: &FrozenAction,
        invocation: &ProviderInvocation,
    ) -> AppResult<Value> {
        let result = self.inner.invoke(action, invocation).await?;
        let mut held = Vec::new();
        for _ in 0..self.pool.options().get_max_connections() {
            held.push(self.pool.acquire().await.unwrap());
        }
        let until = tokio::time::Instant::now() + self.hold;
        *self.held.lock().unwrap() = Some(tokio::spawn(async move {
            tokio::time::sleep_until(until).await;
            drop(held);
        }));
        Ok(result)
    }
}

/// The provider applied, then the pool stays exhausted for longer than the pre-effect
/// acquire bound a but less than the attempt bound A. The receipt commit waits for a
/// connection for the rest of its attempt and records the truth, instead of failing fast
/// and leaving the committed marker to provider reconciliation.
#[tokio::test]
async fn workflow_action_receipt_resolution_commit_waits_out_exhausted_pool_after_effect() {
    let _injected = Injected::mark();
    let (f, mut request, contract) = receipt_fixture(ActionRecovery::SafeRepeat, false).await;
    // The 6 s gate policy (a = 1 s, A = 2 s), with the fence renewed to match it.
    let lease = gated_policy();
    assert!(
        f.persistence()
            .settled()
            .renew_io(request.fence, lease)
            .await
            .unwrap()
            .is_some()
    );
    request.lease = lease;
    let hold = lease.acquire_bound() + (lease.attempt_bound() - lease.acquire_bound()) / 2;
    let provider = ExhaustAfterEffect {
        inner: dedup(&f, contract, false),
        pool: f.persistence().pool().clone(),
        hold,
        held: std::sync::Mutex::new(None),
    };
    let dispatched = ActionService::new(adapter(&f, 0))
        .dispatch_remote(&request, &provider, &CancellationToken::new())
        .await
        .expect("the receipt waited out the exhausted pool");
    let held = provider.held.lock().unwrap().take();
    held.expect("the provider applied and exhausted the pool")
        .await
        .unwrap();
    match dispatched {
        RemoteDispatchObservation::Committed(receipt) => {
            assert_eq!(receipt.result, json!({"written":true}));
        }
        // Recorded either way; only the output is suppressed if ownership ended meanwhile.
        RemoteDispatchObservation::Interrupted => {}
        _ => panic!("the applied effect neither committed its receipt nor lost ownership"),
    }
    assert_eq!(provider.inner.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effect_count(&f).await, 1);
    assert_eq!(observations(&f).await, (1, 1));
}
