//! Expiry revokes failure/backoff ownership as well as successful completion ownership.
use super::*;

async fn expire(p: &PostgresPersistence, event: Uuid) {
    sqlx::query("UPDATE notification_events SET locked_at=clock_timestamp()-INTERVAL '2 seconds',lock_expires_at=clock_timestamp()-INTERVAL '1 second' WHERE id=$1")
        .bind(event).execute(p.pool()).await.unwrap();
}

async fn snapshot(p: &PostgresPersistence, event: Uuid) -> serde_json::Value {
    sqlx::query_scalar("SELECT to_jsonb(event) FROM notification_events AS event WHERE id=$1")
        .bind(event)
        .fetch_one(p.pool())
        .await
        .unwrap()
}

fn failure(claim: &ClaimedNotificationEvent) -> NotificationProjectionFailure<'_> {
    NotificationProjectionFailure {
        fence: &claim.lease,
        class: "database",
        detail: "The projector could not complete its transaction",
    }
}

#[tokio::test]
async fn notification_failure_expired_execution_cannot_reschedule_before_reaping() {
    let database = own_database().await.unwrap();
    let p = PostgresPersistence::new(database.pool.clone());
    let fixture = make_fixture(&p).await;
    let event = insert_events(&p, fixture.company_id, 1, 0).await[0];
    let claims = claim_batch(&p).await;
    assert_eq!(claims.len(), 1);
    expire(&p, event).await;
    let expired = snapshot(&p, event).await;
    assert!(
        !p.fail_notification_projection(failure(&claims[0]))
            .await
            .unwrap(),
        "expiry alone revokes ownership; a reaper need not have replaced the execution yet"
    );
    assert_eq!(snapshot(&p, event).await, expired);
    assert_eq!(p.reap_expired_notification_events().await.unwrap(), 1);
    let reaped = snapshot(&p, event).await;
    assert_eq!(reaped["status"], "pending");
    assert_eq!(reaped["attempt_count"], 1);
    assert_eq!(reaped["last_error_class"], "lease_expired");
    assert!(reaped["execution_id"].is_null());
    assert!(reaped["owner_worker_id"].is_null());
}

#[tokio::test]
async fn notification_failure_replacement_owner_wins_over_expired_execution() {
    let database = own_database().await.unwrap();
    let p = PostgresPersistence::new(database.pool.clone());
    let fixture = make_fixture(&p).await;
    let event = insert_events(&p, fixture.company_id, 1, 0).await[0];
    let first = claim_batch(&p).await;
    expire(&p, event).await;
    assert_eq!(p.reap_expired_notification_events().await.unwrap(), 1);
    sqlx::query("UPDATE notification_events SET available_at=clock_timestamp() WHERE id=$1")
        .bind(event)
        .execute(p.pool())
        .await
        .unwrap();
    let second = claim_batch(&p).await;
    assert_eq!(second.len(), 1);
    assert_ne!(first[0].lease.execution, second[0].lease.execution);
    let (stale, current) = tokio::join!(
        p.fail_notification_projection(failure(&first[0])),
        p.fail_notification_projection(failure(&second[0]))
    );
    assert!(
        !stale.unwrap(),
        "the expired owner cannot fail the replacement"
    );
    assert!(current.unwrap(), "the live replacement records one failure");
    let saved = snapshot(&p, event).await;
    assert_eq!(saved["status"], "pending");
    assert_eq!(saved["attempt_count"], 2);
    assert_eq!(saved["last_error_class"], "database");
    assert!(saved["execution_id"].is_null());
    assert!(saved["owner_worker_id"].is_null());
    assert!(claim_batch(&p).await.is_empty());
    assert!(
        !p.fail_notification_projection(failure(&second[0]))
            .await
            .unwrap()
    );
    assert_eq!(snapshot(&p, event).await, saved, "failure replay is inert");
}

#[tokio::test]
async fn notification_failure_two_expired_attempts_exhaust_without_stale_backoff_writes() {
    let database = own_database().await.unwrap();
    let p = PostgresPersistence::new(database.pool.clone());
    let fixture = make_fixture(&p).await;
    let event = insert_events(&p, fixture.company_id, 1, 8).await[0];
    let mut expired_claims = Vec::new();
    for attempt in [9, 10] {
        let claims = claim_batch(&p).await;
        assert_eq!(claims.len(), 1);
        expire(&p, event).await;
        let expired = snapshot(&p, event).await;
        assert!(
            !p.fail_notification_projection(failure(&claims[0]))
                .await
                .unwrap()
        );
        assert_eq!(snapshot(&p, event).await, expired);
        assert_eq!(p.reap_expired_notification_events().await.unwrap(), 1);
        assert_eq!(snapshot(&p, event).await["attempt_count"], attempt);
        expired_claims.extend(claims);
    }
    let terminal = snapshot(&p, event).await;
    assert_eq!(terminal["status"], "dead_letter");
    assert_eq!(terminal["last_error_class"], "lease_expired");
    assert!(claim_batch(&p).await.is_empty());
    for expired in &expired_claims {
        assert!(
            !p.fail_notification_projection(failure(expired))
                .await
                .unwrap()
        );
    }
    assert_eq!(snapshot(&p, event).await, terminal);
}

#[tokio::test]
async fn notification_stale_release_expired_execution_preserves_reaper_ownership() {
    let database = own_database().await.unwrap();
    let p = PostgresPersistence::new(database.pool.clone());
    let fixture = make_fixture(&p).await;
    let event = insert_events(&p, fixture.company_id, 1, 0).await[0];
    let claims = claim_batch(&p).await;
    expire(&p, event).await;
    let expired = snapshot(&p, event).await;
    let mut tx = p.pool().begin().await.unwrap();
    assert!(
        !release_stale_projection(&mut tx, &claims[0].lease)
            .await
            .unwrap()
    );
    tx.commit().await.unwrap();
    assert_eq!(snapshot(&p, event).await, expired);
    assert_eq!(p.reap_expired_notification_events().await.unwrap(), 1);
}

#[tokio::test]
async fn notification_failure_rechecks_expiry_after_waiting_for_the_event_row() {
    let database = own_database().await.unwrap();
    let p = PostgresPersistence::new(database.pool.clone());
    let fixture = make_fixture(&p).await;
    let event = insert_events(&p, fixture.company_id, 1, 0).await[0];
    let claims = claim_batch(&p).await;
    sqlx::query("UPDATE notification_events SET lock_expires_at=clock_timestamp()+INTERVAL '2 seconds' WHERE id=$1")
        .bind(event).execute(p.pool()).await.unwrap();
    let before = snapshot(&p, event).await;
    let mut blocker = p.pool().begin().await.unwrap();
    let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM notification_events WHERE id=$1 FOR UPDATE")
        .bind(event)
        .execute(&mut *blocker)
        .await
        .unwrap();
    let pending = p.fail_notification_projection(failure(&claims[0]));
    tokio::pin!(pending);
    let observe_expiry = async {
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND $1=ANY(pg_blocking_pids(pid)))")
                    .bind(blocker_pid).fetch_one(p.pool()).await.unwrap();
                if blocked { break; }
                tokio::task::yield_now().await;
            }
        }).await.expect("the real failure writer must reach the row lock");
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let expired: bool = sqlx::query_scalar("SELECT lock_expires_at<=clock_timestamp() FROM notification_events WHERE id=$1")
                    .bind(event).fetch_one(p.pool()).await.unwrap();
                if expired { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("the actual database lease must expire while the writer waits");
    };
    tokio::select! {
        result = &mut pending => panic!("the failure writer passed an owned row lock: {result:?}"),
        () = observe_expiry => {}
    }
    // This transaction only held a lock. With no row-version change to force
    // EvalPlanQual, the failure writer must check the clock after its lock wait.
    blocker.rollback().await.unwrap();
    assert!(
        !tokio::time::timeout(Duration::from_secs(2), pending)
            .await
            .unwrap()
            .unwrap()
    );
    assert_eq!(snapshot(&p, event).await, before);
    assert_eq!(p.reap_expired_notification_events().await.unwrap(), 1);
}
