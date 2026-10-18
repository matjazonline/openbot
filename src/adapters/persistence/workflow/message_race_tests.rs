//! 04.11-D2 §2.6 row 6: a publication COMMIT stalled past its attempt bound resolves three
//! ways under the original's own locks (applied, absent, unresolved). Deliberate stalls run
//! under `Injected` and stay out of the lease-budget filters (D3 §K).
use super::*;
use crate::adapters::persistence::workflow::persistence_bound::recorder::Injected;
use crate::adapters::persistence::workflow::tests::admission_tests::lease_tests::resolution_gate::{
    Abort, Gate, GatePool, Stall, connect, resolution_behind,
};

impl GatePool for Setup {
    fn gate_pool(&self) -> &sqlx::PgPool {
        self.pool()
    }
}

#[derive(Clone, Copy, Debug)]
enum Resolution {
    Applied,
    Absent,
    Unresolved,
}

#[tokio::test]
async fn workflow_message_race_ambiguous_publication_commit_resolves_three_ways() {
    let _injected = Injected::mark();
    for case in [
        Resolution::Applied,
        Resolution::Absent,
        Resolution::Unresolved,
    ] {
        let setup = Box::pin(send(&[To::User])).await;
        let (mut observer, _) = connect(&setup).await;
        let mut gate = Gate::arm(
            &setup,
            "workflow_message_actions",
            "INSERT",
            "true",
            Stall::Commit,
        )
        .await;
        let (runtime, kind, claim) = (setup.runtime(), setup.kind.clone(), setup.claim.clone());
        let publishing = tokio::spawn(async move { runtime.execute(&kind, &claim).await });
        let stalled = gate
            .stalled(&mut observer, &[&|| !publishing.is_finished()])
            .await;
        let outcome = match case {
            Resolution::Applied | Resolution::Absent => {
                if matches!(case, Resolution::Absent) {
                    gate.abort_on_release(Abort::Backend(stalled)).await;
                }
                resolution_behind(
                    &mut observer,
                    stalled,
                    lease(),
                    &[&|| !publishing.is_finished()],
                )
                .await;
                gate.release().await;
                publishing.await.unwrap()
            }
            Resolution::Unresolved => {
                // The resolution elapses behind the stall; the COMMIT applies only afterwards.
                let outcome = publishing.await.unwrap();
                gate.release().await;
                gate.retake().await;
                gate.release().await;
                outcome
            }
        };
        let run = setup.scope().run;
        let rows = rows(setup.pool(), run).await;
        match case {
            Resolution::Applied => {
                assert!(
                    matches!(outcome, WorkflowHandlerOutcome::Suspended(_)),
                    "{case:?}: {outcome:?}"
                );
                assert_eq!((rows.markers, rows.deliveries, rows.waits), (1, 1, 1));
            }
            Resolution::Absent => {
                let WorkflowHandlerOutcome::Completed(Err(failure)) = outcome else {
                    panic!("{case:?}: {outcome:?}");
                };
                assert_eq!(failure.failure.code().as_str(), "message.unresolved");
                assert_eq!((rows.markers, rows.deliveries, rows.waits), (0, 0, 0));
                // No marker: the shared predicate leaves it safe to retry.
                assert_eq!(retry_safe(setup.pool(), setup.scope()).await, None);
            }
            Resolution::Unresolved => {
                let WorkflowHandlerOutcome::Completed(Err(failure)) = outcome else {
                    panic!("{case:?}: {outcome:?}");
                };
                assert_eq!(failure.failure.code().as_str(), "message.unresolved");
                // The park committed after the resolution gave up: the fence is closed, so the
                // supervisor's cleanup cannot retire it and the park stands.
                assert_eq!((rows.markers, rows.deliveries, rows.waits), (1, 1, 1));
                let released = setup
                    .fixture
                    .persistence
                    .release_io(
                        setup.claim.fence,
                        lease(),
                        LeaseReleaseCause::Classified(failure),
                    )
                    .await
                    .unwrap();
                assert!(!released);
                let state = state(setup.pool(), setup.scope()).await;
                assert_eq!(
                    (
                        state.run.as_str(),
                        state.waiting.as_deref(),
                        state.stop.as_deref()
                    ),
                    ("waiting", Some("effect"), Some("effect_parked"))
                );
            }
        }
    }
}

/// D2 §8.2 F007: a publication COMMIT that only completes after the captured fence expiry, or
/// after the run deadline, rolls back everything: the deferred guards read wall time inside the
/// COMMIT. A deferred trigger named to fire before the dispatch guard sleeps past the bound the
/// guard checks; a sequence (not rolled back) proves the COMMIT was reached.
#[tokio::test]
async fn workflow_message_race_delayed_commit_past_fence_expiry_or_deadline_rolls_back() {
    let _injected = Injected::mark();
    for bound in ["fence expiry", "run deadline"] {
        let setup = Box::pin(send(&[To::User])).await;
        let scope = setup.scope();
        let passed = match bound {
            "fence expiry" => {
                "(SELECT fence_expires_at FROM workflow_message_actions WHERE company_id=NEW.company_id AND invocation_id=NEW.invocation_id)"
            }
            _ => {
                "(SELECT deadline FROM workflow_runs WHERE company_id=NEW.company_id AND id=NEW.run_id)"
            }
        };
        sqlx::raw_sql(&format!(
            "CREATE SEQUENCE fixture_commit_reached; \
             CREATE FUNCTION fixture_delay_commit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
               PERFORM nextval('fixture_commit_reached'); \
               PERFORM pg_sleep(GREATEST(0, extract(epoch FROM ({passed} - clock_timestamp())))::float8 + 0.1); \
               RETURN NULL; END $$; \
             CREATE CONSTRAINT TRIGGER aa_delay_commit AFTER INSERT ON workflow_action_dispatches \
               DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION fixture_delay_commit();"
        ))
        .execute(setup.pool())
        .await
        .unwrap();
        // A window the step's pre-commit work fits in; only the COMMIT outlives it.
        let window = match bound {
            "fence expiry" => sqlx::query(
                "UPDATE background_tasks SET lock_expires_at=clock_timestamp()+interval '1 second' WHERE id=$1",
            )
            .bind(scope.job.0),
            _ => sqlx::query(
                "UPDATE workflow_runs SET deadline=clock_timestamp()+interval '1 second' WHERE id=$1",
            )
            .bind(scope.run.as_uuid()),
        };
        window.execute(setup.pool()).await.unwrap();
        let WorkflowHandlerOutcome::Completed(Err(failure)) =
            setup.runtime().execute(&setup.kind, &setup.claim).await
        else {
            panic!("{bound}: the late COMMIT cannot park");
        };
        assert_eq!(
            failure.failure.code().as_str(),
            "message.unresolved",
            "{bound}"
        );
        let reached: i64 = sqlx::query_scalar("SELECT last_value FROM fixture_commit_reached")
            .fetch_one(setup.pool())
            .await
            .unwrap();
        let called: bool = sqlx::query_scalar("SELECT is_called FROM fixture_commit_reached")
            .fetch_one(setup.pool())
            .await
            .unwrap();
        assert!(
            called && reached == 1,
            "{bound}: the COMMIT was reached once"
        );
        assert_eq!(
            rows(setup.pool(), scope.run).await,
            Rows {
                markers: 0,
                actions: 0,
                destinations: 0,
                deliveries: 0,
                receipts: 0,
                waits: 0,
                messages: 0
            },
            "{bound}"
        );
        assert_eq!(retry_safe(setup.pool(), scope).await, None, "{bound}");
        let after = state(setup.pool(), scope).await;
        assert_eq!(
            (
                after.run.as_str(),
                after.job.as_str(),
                after.attempt.as_str(),
                after.wait
            ),
            ("running", "processing", "processing", None),
            "{bound}"
        );
        let threads: i64 = sqlx::query_scalar("SELECT count(*) FROM threads WHERE channel_id=$1")
            .bind(setup.channel)
            .fetch_one(setup.pool())
            .await
            .unwrap();
        assert_eq!(threads, 0, "{bound}: the user email thread rolled back too");
    }
}
