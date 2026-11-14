//! Actual protected dispatch must acquire every synchronous reviewer before company ownership.
use super::*;
use crate::adapters::persistence::user_lifecycle::{finish_account_deletion_on, lock_account_on};
use crate::application::ports::account_deletion::HostAccountDeletion;

async fn request(setup: &Setup) -> ActionDispatchRequest {
    let action = setup
        .fixture
        .persistence
        .http_step_request(&setup.claim, &setup.kind)
        .await
        .unwrap();
    let intent = ActionService::new(setup.fixture.persistence.clone())
        .prepare_step(action)
        .await
        .unwrap();
    ActionDispatchRequest {
        subject: intent.approval_subject(),
        fence: setup.claim.fence,
        lease: lease(),
    }
}

fn dispatch(
    setup: &Setup,
) -> action_dispatch::PostgresActionDispatch<http::HttpActionAuthority, NoLocal> {
    let p = &setup.fixture.persistence;
    action_dispatch::PostgresActionDispatch::new(p.clone(), http::HttpActionAuthority, NoLocal)
        .with_decision_notices(notices(p))
}

async fn other_reviewer(setup: &Setup) -> Uuid {
    sqlx::query_scalar(
        "SELECT principal.user_id FROM workflow_executions AS execution \
         CROSS JOIN LATERAL jsonb_array_elements(execution.frozen_inputs->'approval'->'reviewers') AS reviewer \
         JOIN principals AS principal ON principal.company_id=execution.company_id AND principal.id=(reviewer->>'id')::uuid \
         WHERE execution.company_id=$1 AND execution.id=$2 AND principal.user_id<>$3 \
         ORDER BY principal.user_id DESC LIMIT 1",
    )
    .bind(setup.claim.fence.scope.company.as_uuid())
    .bind(setup.claim.fence.scope.execution.as_uuid())
    .bind(setup.fixture.target.actor.user_id())
    .fetch_one(setup.fixture.persistence.pool())
    .await
    .unwrap()
}

async fn blocked_by(pool: &sqlx::PgPool, blocker: i32, pattern: &str) -> i32 {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let blocked: Option<i32> = sqlx::query_scalar(
                "SELECT pid FROM pg_stat_activity WHERE datname=current_database() \
                 AND wait_event_type='Lock' AND $1=ANY(pg_blocking_pids(pid)) AND query LIKE $2 \
                 ORDER BY pid LIMIT 1",
            )
            .bind(blocker)
            .bind(pattern)
            .fetch_optional(pool)
            .await
            .unwrap();
            if let Some(pid) = blocked {
                return pid;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the actual production claimant reaches the intended lock")
}

#[tokio::test]
async fn workflow_action_approval_sixteen_reviewer_accounts_precede_company_ownership() {
    let setup = Box::pin(protected_reviewers(
        "http://127.0.0.1:1",
        Entrance::Http,
        16,
    ))
    .await;
    let request = request(&setup).await;
    let p = &setup.fixture.persistence;
    let mut gate = p.pool().begin().await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(other_reviewer(&setup).await)
        .execute(&mut *gate)
        .await
        .unwrap();
    let dispatcher = dispatch(&setup);
    let park = dispatcher.reserve_remote(&request, None);
    tokio::pin!(park);
    tokio::select! {
        _=blocked_by(p.pool(),blocker,"%")=>{},
        _=park.as_mut()=>panic!("protected park must acquire the separate reviewer"),
    }
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE NOWAIT")
        .bind(setup.claim.fence.scope.company.as_uuid())
        .execute(&mut *gate)
        .await
        .expect("all sixteen synchronous reviewer accounts must precede company ownership");
    gate.rollback().await.unwrap();
    assert!(matches!(
        park.await.unwrap(),
        RemoteReservationResult::ApprovalParked(_)
    ));
    assert_eq!(view(&setup).await.reviewers.len(), 16);
    #[derive(sqlx::FromRow)]
    struct Counts {
        notices: i64,
        dispatches: i64,
    }
    let counts: Counts = sqlx::query_as(
        "SELECT (SELECT count(*) FROM message_deliveries) AS notices, \
         (SELECT count(*) FROM workflow_action_dispatches) AS dispatches",
    )
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(counts.notices, 16);
    assert_eq!(
        counts.dispatches, 0,
        "parking never dispatches the protected effect"
    );
}

#[tokio::test]
async fn workflow_action_approval_retirement_first_refuses_without_partial_park_or_deadlock() {
    let setup = Box::pin(protected_reviewers("http://127.0.0.1:1", Entrance::Http, 2)).await;
    let request = request(&setup).await;
    let p = &setup.fixture.persistence;
    let retiring = other_reviewer(&setup).await;
    let mut retirement = p.pool().begin().await.unwrap();
    let locks = lock_account_on(&mut retirement, retiring).await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *retirement)
        .await
        .unwrap();
    let finish = async {
        blocked_by(p.pool(), blocker, "%FROM users WHERE id=ANY%FOR SHARE%").await;
        finish_account_deletion_on(&mut retirement, retiring, locks)
            .await
            .unwrap();
        retirement.commit().await.unwrap();
    };
    let dispatcher = dispatch(&setup);
    let (result, ()) = tokio::join!(dispatcher.reserve_remote(&request, None), finish);
    assert!(matches!(result, Err(AppError::NotFound(_))));
    assert!(matches!(
        dispatcher.reserve_remote(&request, None).await.unwrap(),
        RemoteReservationResult::ApprovalRequired
    ));
    let counts: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object('decisions',(SELECT count(*) FROM workflow_decisions), \
         'notices',(SELECT count(*) FROM message_deliveries))",
    )
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(counts, json!({"decisions":0,"notices":0}));
    assert_eq!(
        action_counts(&setup.fixture).await,
        ActionCounts {
            dispatches: 0,
            entries: 0,
            receipts: 0
        }
    );
}

#[tokio::test]
async fn workflow_action_approval_park_first_serializes_retirement_and_cancels_the_retired_notice()
{
    let setup = Box::pin(protected_reviewers("http://127.0.0.1:1", Entrance::Http, 2)).await;
    let request = request(&setup).await;
    let p = &setup.fixture.persistence;
    let retiring = other_reviewer(&setup).await;
    let principal = setup.fixture.principal(retiring).await;
    let mut gate = p.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(request.scope().company.as_uuid())
        .bind(request.scope().run.as_uuid())
        .execute(&mut *gate)
        .await
        .unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    let retirement = async {
        let publisher = blocked_by(p.pool(), blocker, "%FROM workflow_runs%").await;
        let deletion = p.delete_user_account(retiring);
        tokio::pin!(deletion);
        tokio::select! {
            _=deletion.as_mut()=>panic!("retirement cannot bypass the pending park's reviewer account lock"),
            _=blocked_by(p.pool(),publisher,"%FROM users WHERE id=$1 FOR UPDATE%")=>{},
        }
        gate.rollback().await.unwrap();
        deletion.await.unwrap();
    };
    let dispatcher = dispatch(&setup);
    let (result, ()) = tokio::join!(dispatcher.reserve_remote(&request, None), retirement);
    assert!(matches!(
        result.unwrap(),
        RemoteReservationResult::ApprovalParked(_)
    ));
    let after = view(&setup).await;
    assert_eq!(
        after.reviewers.len(),
        2,
        "retirement preserves the original assignment history"
    );
    let cancelled: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_decision_reviewers AS reviewer \
         JOIN message_deliveries AS delivery ON delivery.id=reviewer.notice_delivery_id \
         WHERE reviewer.company_id=$1 AND reviewer.principal_id=$2 \
           AND delivery.cancellation_reason='recipient_revoked'",
    )
    .bind(request.scope().company.as_uuid())
    .bind(principal)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(cancelled, 1);
    assert_eq!(
        action_counts(&setup.fixture).await,
        ActionCounts {
            dispatches: 0,
            entries: 0,
            receipts: 0
        }
    );
}

#[tokio::test]
async fn workflow_action_approval_recorded_approval_does_not_require_a_retired_reviewer_account() {
    let (origin, server) =
        server("HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\n{\"ok\":true}").await;
    let setup = Box::pin(protected_reviewers(&origin, Entrance::Http, 2)).await;
    let original = request(&setup).await;
    let retiring = other_reviewer(&setup).await;
    let p = &setup.fixture.persistence;
    let dispatcher = dispatch(&setup);
    let wait = match dispatcher.reserve_remote(&original, None).await.unwrap() {
        RemoteReservationResult::ApprovalParked(wait) => wait,
        _ => panic!("the actual protected operation parks"),
    };
    assert!(p.confirm_suspension(original.fence, wait).await.unwrap());
    let before = view(&setup).await;
    let command = submission(&setup, &before, "approve");
    p.submit_decision(command.clone()).await.unwrap();
    p.delete_user_account(retiring).await.unwrap();
    assert_eq!(
        p.submit_decision(command).await.unwrap().disposition,
        DecisionDisposition::Replayed
    );
    let resumed = p
        .claim_io(
            original.fence.scope,
            WorkflowWorkerId(Uuid::new_v4()),
            lease(),
        )
        .await
        .unwrap()
        .unwrap();
    assert!(
        !server.is_finished(),
        "approval and retirement do not invoke the provider"
    );
    let runtime = actions(&setup);
    let result = runtime.run(&setup.kind, &resumed).await.completed_result();
    assert!(
        result.is_ok(),
        "the recorded approval authorizes the actual request: {result:?}"
    );
    assert!(!server.await.unwrap().is_empty());
    let replay = runtime.run(&setup.kind, &resumed).await.completed_result();
    assert!(
        replay.is_ok(),
        "recorded effect replays without a second request: {replay:?}"
    );
    assert_eq!(
        action_counts(&setup.fixture).await,
        ActionCounts {
            dispatches: 1,
            entries: 1,
            receipts: 1
        }
    );
    let notices: i64 = sqlx::query_scalar("SELECT count(*) FROM message_deliveries")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(
        notices, 2,
        "recorded approval neither resolves nor renotifies historical reviewers"
    );
}
