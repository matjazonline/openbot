//! Token approval parks compete with actual reviewer-account ownership.
use super::*;
use crate::adapters::persistence::user_lifecycle::{finish_account_deletion_on, lock_account_on};
use crate::application::ports::account_deletion::HostAccountDeletion;

pub(super) async fn blocked_by(pool: &sqlx::PgPool, blocker: i32, pattern: &str) -> i32 {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let waiting: Option<i32> = sqlx::query_scalar(
                "SELECT pid FROM pg_stat_activity WHERE datname=current_database() \
                 AND wait_event_type='Lock' AND $1=ANY(pg_blocking_pids(pid)) \
                   AND query LIKE $2 ORDER BY pid LIMIT 1",
            )
            .bind(blocker)
            .bind(pattern)
            .fetch_optional(pool)
            .await
            .unwrap();
            if let Some(pid) = waiting {
                return pid;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the actual token claimant reaches the intended account/run lock")
}

#[tokio::test]
async fn workflow_harness_token_reviewer_account_precedes_company_ownership() {
    let f = configured().await;
    let model = &f.model;
    let p = model.native.fixture.persistence();
    let notices = notices(p);
    let reviewer = f.reviewer.user_id();
    let owner = model.native.fixture.binding.target.actor.user_id();
    assert_ne!(reviewer, owner);
    let mut gate = p.pool().begin().await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(reviewer)
        .execute(&mut *gate)
        .await
        .unwrap();
    let request = model_request(&reservation());
    let park = p.reserve_workflow_model(model.write, request, Some(&notices));
    tokio::pin!(park);
    tokio::select! {
        _=blocked_by(p.pool(),blocker,"%")=>{},
        _=park.as_mut()=>panic!("token park must own its separate synchronous reviewer account"),
    }
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE NOWAIT")
        .bind(model.write.fence.scope.company.as_uuid())
        .execute(&mut *gate)
        .await
        .expect("token reviewer account ownership must precede company ownership");
    gate.rollback().await.unwrap();
    assert!(matches!(
        park.await.unwrap(),
        WorkflowModelAdmission::Parked(_)
    ));
    assert_eq!(accounting(model).await["calls"], 0);
    assert_eq!(accounting(model).await["rows"], json!([]));
}

#[tokio::test]
async fn workflow_harness_token_retirement_first_refuses_without_model_admission_or_notice() {
    let f = configured().await;
    let model = &f.model;
    let p = model.native.fixture.persistence();
    let notices = notices(p);
    let user = f.reviewer.user_id();
    let mut retirement = p.pool().begin().await.unwrap();
    let locks = lock_account_on(&mut retirement, user).await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *retirement)
        .await
        .unwrap();
    let finish = async {
        blocked_by(p.pool(), blocker, "%FROM users WHERE id=ANY%FOR SHARE%").await;
        finish_account_deletion_on(&mut retirement, user, locks)
            .await
            .unwrap();
        retirement.commit().await.unwrap();
    };
    let (result, ()) = tokio::join!(
        p.reserve_workflow_model(model.write, model_request(&reservation()), Some(&notices)),
        finish
    );
    assert!(matches!(
        result.unwrap(),
        WorkflowModelAdmission::OwnershipLost
    ));
    assert_eq!(accounting(model).await["calls"], 0);
    assert_eq!(accounting(model).await["rows"], json!([]));
    let counts: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object('decisions',(SELECT count(*) FROM workflow_decisions), \
        'notices',(SELECT count(*) FROM message_deliveries))",
    )
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(counts, json!({"decisions":0,"notices":0}));
}

#[tokio::test]
async fn workflow_harness_token_park_first_blocks_retirement_before_the_company() {
    let f = configured().await;
    let model = &f.model;
    let p = model.native.fixture.persistence();
    let notices = notices(p);
    let mut gate = p.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(model.write.fence.scope.company.as_uuid())
        .bind(model.write.fence.scope.run.as_uuid())
        .execute(&mut *gate)
        .await
        .unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    let retirement = async {
        let publisher = blocked_by(p.pool(), blocker, "%FROM workflow_runs%").await;
        let deletion = p.delete_user_account(f.reviewer.user_id());
        tokio::pin!(deletion);
        tokio::select! {
            _=deletion.as_mut()=>panic!("retirement must wait for the actual token reviewer's account lock"),
            _=blocked_by(p.pool(),publisher,"%FROM users WHERE id=$1 FOR UPDATE%")=>{},
        }
        gate.rollback().await.unwrap();
        deletion.await.unwrap();
    };
    let (result, ()) = tokio::join!(
        p.reserve_workflow_model(model.write, model_request(&reservation()), Some(&notices)),
        retirement
    );
    assert!(matches!(result.unwrap(), WorkflowModelAdmission::Parked(_)));
    assert_eq!(accounting(model).await["calls"], 0);
    let cancelled: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_decision_reviewers AS reviewer \
        JOIN message_deliveries AS delivery ON delivery.id=reviewer.notice_delivery_id \
        WHERE reviewer.company_id=$1 AND delivery.cancellation_reason='recipient_revoked'",
    )
    .bind(model.write.fence.scope.company.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(cancelled, 1);
}

struct ReceivedTurn {
    write: WorkflowCheckpointWrite,
    turn: crate::services::harness::runs::SavedModelTurn,
}

async fn admitted_turn(f: &TokenFixture) -> ReceivedTurn {
    let model = &f.model;
    let p = model.native.fixture.persistence();
    let notices = notices(p);
    let mut request = reservation();
    request.input_tokens = 50;
    request.output_tokens = 50;
    let WorkflowModelAdmission::New(permit) = p
        .reserve_workflow_model(model.write, model_request(&request), Some(&notices))
        .await
        .unwrap()
    else {
        panic!("within-allowance model reservation is admitted")
    };
    let checkpoint = permit.consume();
    let mut turn = response(&request);
    turn.input_tokens = 100;
    turn.output_tokens = 100;
    ReceivedTurn {
        write: WorkflowCheckpointWrite {
            expected_revision: checkpoint.revision,
            ..model.write
        },
        turn,
    }
}

#[tokio::test]
async fn workflow_harness_token_settlement_retirement_keeps_received_truth_without_review_or_duplicate()
 {
    let f = configured().await;
    let received = admitted_turn(&f).await;
    let model = &f.model;
    let p = model.native.fixture.persistence();
    let notices = notices(p);
    let user = f.reviewer.user_id();
    let mut retirement = p.pool().begin().await.unwrap();
    let locks = lock_account_on(&mut retirement, user).await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *retirement)
        .await
        .unwrap();
    let finish = async {
        blocked_by(p.pool(), blocker, "%FROM users WHERE id=ANY%FOR SHARE%").await;
        finish_account_deletion_on(&mut retirement, user, locks)
            .await
            .unwrap();
        retirement.commit().await.unwrap();
    };
    let (settled, ()) = tokio::join!(
        p.settle_workflow_model(received.write, received.turn.clone(), Some(&notices)),
        finish
    );
    let WorkflowModelSettlement::ReviewUnavailable(checkpoint) = settled.unwrap() else {
        panic!("received truth is saved and cannot authorize further model work")
    };
    assert_eq!(checkpoint.turns.last(), Some(&received.turn));
    let before = accounting(model).await;
    assert_eq!(before["calls"], 1);
    assert_eq!(before["rows"][0]["input_tokens"], 100);
    assert_eq!(before["rows"][0]["output_tokens"], 100);
    assert!(matches!(
        p.settle_workflow_model(received.write, received.turn, Some(&notices))
            .await
            .unwrap(),
        WorkflowModelSettlement::ReviewUnavailable(_)
    ));
    assert_eq!(accounting(model).await, before);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_decisions")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn workflow_harness_token_under_allowance_and_recorded_reservation_ignore_retired_reviewers()
{
    let f = configured().await;
    let model = &f.model;
    let p = model.native.fixture.persistence();
    let notices = notices(p);
    p.delete_user_account(f.reviewer.user_id()).await.unwrap();
    let mut request = reservation();
    request.input_tokens = 50;
    request.output_tokens = 50;
    let prepared = model_request(&request);
    let result = p
        .reserve_workflow_model(model.write, prepared.clone(), Some(&notices))
        .await
        .unwrap();
    assert!(matches!(result, WorkflowModelAdmission::New(_)));
    assert!(matches!(
        p.reserve_workflow_model(model.write, prepared, Some(&notices))
            .await
            .unwrap(),
        WorkflowModelAdmission::PreviouslyAdmitted(_)
    ));
    let usage = accounting(model).await;
    assert_eq!(usage["calls"], 1);
    assert_eq!(usage["rows"].as_array().unwrap().len(), 1);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_decisions")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn workflow_harness_token_under_allowance_reservation_does_not_lock_unused_reviewer() {
    let f = configured().await;
    let model = &f.model;
    let p = model.native.fixture.persistence();
    let notices = notices(p);
    let mut gate = p.pool().begin().await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(f.reviewer.user_id())
        .execute(&mut *gate)
        .await
        .unwrap();
    let mut request = reservation();
    request.input_tokens = 50;
    request.output_tokens = 50;
    let admission = tokio::select! {
        result=p.reserve_workflow_model(model.write, model_request(&request), Some(&notices))=>result.unwrap(),
        _=blocked_by(p.pool(),blocker,"%FROM users WHERE id=ANY%FOR SHARE%")=>{
            panic!("a reservation within allowance must not wait for an unused token reviewer")
        },
    };
    assert!(matches!(admission, WorkflowModelAdmission::New(_)));
    gate.rollback().await.unwrap();
    assert_eq!(accounting(model).await["calls"], 1);
    let decisions: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_decisions")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(decisions, 0);
}

#[tokio::test]
async fn workflow_harness_token_under_allowance_settlement_does_not_lock_unused_reviewer() {
    let f = configured().await;
    let model = &f.model;
    let p = model.native.fixture.persistence();
    let notices = notices(p);
    let mut received = admitted_turn(&f).await;
    received.turn.input_tokens = 40;
    received.turn.output_tokens = 40;
    let mut gate = p.pool().begin().await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(f.reviewer.user_id())
        .execute(&mut *gate)
        .await
        .unwrap();
    let settlement = tokio::select! {
        result=p.settle_workflow_model(received.write, received.turn, Some(&notices))=>result.unwrap(),
        _=blocked_by(p.pool(),blocker,"%FROM users WHERE id=ANY%FOR SHARE%")=>{
            panic!("received usage within allowance must not wait for an unused token reviewer")
        },
    };
    assert!(
        matches!(settlement, WorkflowModelSettlement::Saved(outcome) if matches!(*outcome, WriteOutcome::Applied(_)))
    );
    gate.rollback().await.unwrap();
    let usage = accounting(model).await;
    assert_eq!(usage["calls"], 1);
    assert_eq!(usage["rows"].as_array().unwrap().len(), 1);
    assert_eq!(usage["rows"][0]["input_tokens"], 40);
    assert_eq!(usage["rows"][0]["output_tokens"], 40);
    let decisions: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_decisions")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(decisions, 0);
}

#[tokio::test]
async fn workflow_harness_token_unavailable_notices_settlement_does_not_lock_unused_reviewer() {
    let f = configured().await;
    let model = &f.model;
    let p = model.native.fixture.persistence();
    let received = admitted_turn(&f).await;
    let mut gate = p.pool().begin().await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(f.reviewer.user_id())
        .execute(&mut *gate)
        .await
        .unwrap();
    let settlement = tokio::select! {
        result=p.settle_workflow_model(received.write, received.turn, None)=>result.unwrap(),
        _=blocked_by(p.pool(),blocker,"%FROM users WHERE id=ANY%FOR SHARE%")=>{
            panic!("settlement without a notification composer must retain truth without reviewer locks")
        },
    };
    assert!(matches!(
        settlement,
        WorkflowModelSettlement::ReviewUnavailable(_)
    ));
    gate.rollback().await.unwrap();
    let usage = accounting(model).await;
    assert_eq!(usage["rows"][0]["input_tokens"], 100);
    assert_eq!(usage["rows"][0]["output_tokens"], 100);
    let decisions: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_decisions")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(decisions, 0);
}

#[tokio::test]
async fn workflow_harness_token_nonreply_terminal_company_ownership_precedes_run_ownership() {
    let f = configured().await;
    let model = &f.model;
    let p = model.native.fixture.persistence();
    let scope = model.write.fence.scope;
    sqlx::query("UPDATE background_tasks SET max_retries=1 WHERE id=$1")
        .bind(scope.job.0)
        .execute(p.pool())
        .await
        .unwrap();
    let user = f.reviewer.user_id();
    let mut retirement = p.pool().begin().await.unwrap();
    let locks = lock_account_on(&mut retirement, user).await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *retirement)
        .await
        .unwrap();
    let release = p.release_io(
        model.write.fence,
        model.write.lease_policy,
        crate::application::workflow::lease::LeaseReleaseCause::HandlerFailure,
    );
    tokio::pin!(release);
    tokio::select! {
        _=blocked_by(p.pool(),blocker,"%FROM companies%")=>{},
        result=release.as_mut()=>panic!("actual non-reply terminal release must serialize with company retirement first: {result:?}"),
    }
    sqlx::query("SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE NOWAIT")
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .execute(&mut *retirement)
        .await
        .expect("a terminal owner waiting on company must not already hold the run");
    finish_account_deletion_on(&mut retirement, user, locks)
        .await
        .unwrap();
    retirement.commit().await.unwrap();
    assert!(release.await.unwrap());
    let state: String =
        sqlx::query_scalar("SELECT state FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(scope.company.as_uuid())
            .bind(scope.run.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(state, "failed");
    assert_eq!(accounting(model).await["calls"], 0);
    assert_eq!(accounting(model).await["rows"], json!([]));
}
