use super::*;
use crate::adapters::persistence::workflow::persistence_bound::recorder::Injected;
use crate::adapters::persistence::workflow::tests::admission_tests::lease_tests::resolution_gate::{Abort, Gate, GatePool, Stall, connect, resolution_behind};

impl GatePool for Setup {
    fn gate_pool(&self) -> &sqlx::PgPool {
        self.fixture.persistence.pool()
    }
}

#[derive(Clone, Copy, Debug)]
enum Path {
    Local,
    Remote,
}

#[tokio::test]
async fn workflow_action_approval_race_lost_commit_ack_resolves_exact_park() {
    let _injected = Injected::mark();
    for path in [Path::Local, Path::Remote] {
        for applies in [true, false] {
            let setup = Box::pin(protected("http://127.0.0.1:1", Entrance::Http)).await;
            let p = &setup.fixture.persistence;
            let action = p
                .http_step_request(&setup.claim, &setup.kind)
                .await
                .unwrap();
            let intent = ActionService::new(p.clone())
                .prepare_step(action)
                .await
                .unwrap();
            let request = ActionDispatchRequest {
                subject: intent.approval_subject(),
                fence: setup.claim.fence,
                lease: lease(),
            };
            let (mut observer, _) = connect(&setup).await;
            let mut gate = Gate::arm(
                &setup,
                "workflow_decisions",
                "INSERT",
                "true",
                Stall::Commit,
            )
            .await;
            let writer = action_dispatch::PostgresActionDispatch::new(
                p.clone(),
                http::HttpActionAuthority,
                NoLocal,
            )
            .with_decision_notices(notices(p));
            let task = tokio::spawn(async move {
                match path {
                    Path::Local => match writer.local(&request).await? {
                        LocalDispatchResult::ApprovalParked(wait) => Ok(wait),
                        _ => panic!("expected local approval park"),
                    },
                    Path::Remote => match writer.reserve_remote(&request, None).await? {
                        RemoteReservationResult::ApprovalParked(wait) => Ok(wait),
                        _ => panic!("expected remote approval park"),
                    },
                }
            });
            let stalled = gate
                .stalled(&mut observer, &[&|| !task.is_finished()])
                .await;
            if !applies {
                gate.abort_on_release(Abort::Backend(stalled)).await;
            }
            resolution_behind(&mut observer, stalled, lease(), &[&|| !task.is_finished()]).await;
            gate.release().await;
            let outcome: AppResult<crate::domain::workflow::WaitId> = task.await.unwrap();
            if applies {
                let wait = outcome.unwrap();
                assert!(p.confirm_suspension(setup.claim.fence, wait).await.unwrap());
                assert_eq!(view(&setup).await.state, "waiting");
            } else {
                assert!(outcome.is_err(), "{path:?}");
                let count: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_decisions")
                    .fetch_one(p.pool())
                    .await
                    .unwrap();
                assert_eq!(count, 0);
            }
            assert_eq!(
                action_counts(&setup.fixture).await,
                ActionCounts {
                    dispatches: 0,
                    entries: 0,
                    receipts: 0
                }
            );
        }
    }
}
