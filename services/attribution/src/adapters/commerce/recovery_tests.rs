use super::*;

pub(super) async fn due(w: &Commerce) {
    sqlx::query("UPDATE attribution_merchant_events SET next_attempt_at=clock_timestamp() WHERE state='pending'").execute(&w.c.f._database.pool).await.unwrap();
}
fn retry_request(w: &Commerce, id: &str, key: Uuid, time: &str) -> Request<Body> {
    w.c.f
        .request(&format!("/v1/merchant-events/{id}/retry"))
        .method("POST")
        .header("x-company-id", w.c.f.upstream.company.0.to_string())
        .header("origin", "http://localhost:8091")
        .header("x-csrf-token", &w.c.f.csrf)
        .header("idempotency-key", key.to_string())
        .header("idempotency-created-at", time)
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn attribution_reconciliation_competing_claims_crash_recovery_and_stale_token() {
    let w = Commerce::new().await;
    let paid = w.paid();
    let refund = w.refund();
    assert_eq!(w.event(&refund).await.0, StatusCode::ACCEPTED);
    due(&w).await;
    let (a, b) = tokio::join!(
        worker::claim(&w.c.f.setup.store),
        worker::claim(&w.c.f.setup.store)
    );
    let claims: Vec<_> = a.unwrap().into_iter().chain(b.unwrap()).collect();
    assert_eq!(claims.len(), 1);
    let stale = claims[0];
    sqlx::query(
        "UPDATE attribution_merchant_events SET lease_until=clock_timestamp()-interval '1 second'",
    )
    .execute(&w.c.f._database.pool)
    .await
    .unwrap();
    let live = worker::claim(&w.c.f.setup.store).await.unwrap()[0];
    assert_ne!(stale.execution, live.execution);
    let order = w.event(&paid).await.1;
    assert!(matches!(
        worker::process(&w.worker(), stale).await,
        Err(Failure::Conflict)
    ));
    assert_eq!(
        worker::process(&w.worker(), live).await.unwrap(),
        EventState::Accepted
    );
    let actual = w
        .get(&format!(
            "/v1/orders/{}",
            order["order_id"].as_str().unwrap()
        ))
        .await
        .1;
    assert_eq!(actual["refunded_merchandise_amount"], 1200);
    assert_eq!(w.event(&refund).await.1["attempts"], 2);
    assert_eq!(w.count("attribution_refund_lines").await, 1);
    w.c.f.close().await;
}

#[tokio::test]
async fn attribution_reconciliation_deadline_attempts_manual_retry_and_receipt() {
    let w = Commerce::new().await;
    let paid = w.paid();
    let refund = w.refund();
    let pending = w.event(&refund).await.1;
    let id = pending["id"].as_str().unwrap();
    sqlx::query(
        "UPDATE attribution_merchant_events SET attempts=20,next_attempt_at=clock_timestamp()",
    )
    .execute(&w.c.f._database.pool)
    .await
    .unwrap();
    assert!(worker::claim(&w.c.f.setup.store).await.unwrap().is_empty());
    let unresolved = w.event(&refund).await.1;
    assert_eq!(unresolved["state"], "unresolved");
    assert_eq!(unresolved["reason"], "retry_exhausted");
    let key = Uuid::new_v4();
    let time = Utc::now().to_rfc3339();
    let mut bad = retry_request(&w, id, key, &time);
    bad.headers_mut().remove("x-csrf-token");
    assert_eq!(
        w.c.f.app.clone().oneshot(bad).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    w.c.f.upstream.mode.store(3, Ordering::Release);
    assert_eq!(w.get("/v1/merchant-events").await.0, StatusCode::OK);
    assert_eq!(
        w.c.f
            .app
            .clone()
            .oneshot(retry_request(&w, id, key, &time))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    w.c.f.upstream.mode.store(0, Ordering::Release);
    let (a, b) = tokio::join!(
        w.c.f.app.clone().oneshot(retry_request(&w, id, key, &time)),
        w.c.f.app.clone().oneshot(retry_request(&w, id, key, &time))
    );
    let (a, b) = (response(a.unwrap()).await, response(b.unwrap()).await);
    assert_eq!(a.0, StatusCode::OK, "{}", a.1);
    assert_eq!(a, b);
    assert_eq!(a.1["state"], "pending");
    assert_eq!(a.1["attempts"], 0);
    assert_eq!(w.event(&paid).await.1["state"], "accepted");
    let claim = worker::claim(&w.c.f.setup.store).await.unwrap()[0];
    assert_eq!(
        worker::process(&w.worker(), claim).await.unwrap(),
        EventState::Accepted
    );
    let current = w.event(&refund).await.1;
    assert_eq!(current["state"], "accepted");
    assert_eq!(
        response(
            w.c.f
                .app
                .clone()
                .oneshot(retry_request(&w, id, key, &time))
                .await
                .unwrap()
        )
        .await,
        a,
        "management replay returns original response"
    );
    assert_eq!(
        w.event(&refund).await.1,
        current,
        "merchant replay returns current state"
    );
    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM attribution_audit WHERE action='merchant_event.retry'",
    )
    .fetch_one(&w.c.f._database.pool)
    .await
    .unwrap();
    assert_eq!(audits, 1);
    let mut another = refund.clone();
    another["event_id"] = json!("deadline");
    another["refund_id"] = json!("deadline");
    another["order_id"] = json!("missing");
    assert_eq!(w.event(&another).await.0, StatusCode::ACCEPTED);
    sqlx::query("UPDATE attribution_merchant_events SET deadline_at=clock_timestamp()-interval '1 second',next_attempt_at=clock_timestamp() WHERE state='pending'").execute(&w.c.f._database.pool).await.unwrap();
    assert!(worker::claim(&w.c.f.setup.store).await.unwrap().is_empty());
    assert_eq!(w.event(&another).await.1["reason"], "deadline_expired");
    w.c.f.close().await;
}

#[tokio::test]
async fn attribution_reconciliation_final_lease_fence_rolls_back_financial_effect() {
    let w = Commerce::new().await;
    let paid = w.paid();
    let refund = w.refund();
    assert_eq!(w.event(&refund).await.0, StatusCode::ACCEPTED);
    assert_eq!(w.event(&paid).await.1["state"], "accepted");
    due(&w).await;
    let claim = worker::claim(&w.c.f.setup.store).await.unwrap()[0];
    // A trigger expires the lease after the real balance update, before the final fence.
    sqlx::raw_sql("CREATE FUNCTION expire_test_lease() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN UPDATE attribution_merchant_events SET lease_until=clock_timestamp()-interval '1 second' WHERE execution IS NOT NULL; RETURN NEW; END $$; CREATE TRIGGER expire_test_lease AFTER UPDATE OF refunded_merchandise_amount ON attribution_orders FOR EACH ROW EXECUTE FUNCTION expire_test_lease()").execute(&w.c.f._database.pool).await.unwrap();
    assert!(matches!(
        worker::process(&w.worker(), claim).await,
        Err(Failure::Conflict)
    ));
    assert_eq!(w.count("attribution_refund_lines").await, 0);
    let money: i64 =
        sqlx::query_scalar("SELECT refunded_merchandise_amount FROM attribution_orders")
            .fetch_one(&w.c.f._database.pool)
            .await
            .unwrap();
    assert_eq!(money, 0);
    sqlx::query("DROP TRIGGER expire_test_lease ON attribution_orders")
        .execute(&w.c.f._database.pool)
        .await
        .unwrap();
    assert_eq!(
        worker::process(&w.worker(), claim).await.unwrap(),
        EventState::Accepted
    );
    w.c.f.close().await;
}

async fn waiting_on_lock(w: &Commerce) {
    tokio::time::timeout(Duration::from_secs(3),async {
        loop {
            let blocked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock')").fetch_one(&w.c.f._database.pool).await.unwrap();
            if blocked {break;}
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("competing transaction must actually wait for a database lock");
}

#[tokio::test]
async fn attribution_reconciliation_cleanup_and_store_disable_compete_with_settlement() {
    let w = Commerce::new().await;
    let paid = w.paid();
    let refund = w.refund();
    assert_eq!(w.event(&refund).await.0, StatusCode::ACCEPTED);
    assert_eq!(w.event(&paid).await.1["state"], "accepted");
    due(&w).await;
    let claim = worker::claim(&w.c.f.setup.store).await.unwrap()[0];
    let mut cleanup = w.c.f._database.pool.begin().await.unwrap();
    sqlx::query("SELECT company_id FROM attribution_tenants WHERE company_id=$1 FOR UPDATE")
        .bind(w.c.f.upstream.company.0)
        .execute(&mut *cleanup)
        .await
        .unwrap();
    // The future cleanup owner must hold this lock and preserve pending/leased facts.
    let removed=sqlx::query("DELETE FROM attribution_orders o WHERE NOT EXISTS(SELECT 1 FROM attribution_merchant_events e WHERE e.order_id=o.id AND (e.state IN ('pending','unresolved') OR e.execution IS NOT NULL))").execute(&mut *cleanup).await.unwrap();
    assert_eq!(removed.rows_affected(), 0);
    sqlx::query("UPDATE attribution_stores SET enabled=false")
        .execute(&mut *cleanup)
        .await
        .unwrap();
    let state = w.worker();
    let task = tokio::spawn(async move { worker::process(&state, claim).await });
    waiting_on_lock(&w).await;
    assert!(!task.is_finished());
    cleanup.commit().await.unwrap();
    assert!(matches!(task.await.unwrap(), Err(Failure::Forbidden)));
    assert_eq!(w.count("attribution_refund_lines").await, 0);
    sqlx::query("UPDATE attribution_stores SET enabled=true")
        .execute(&w.c.f._database.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE attribution_credentials SET revoked_at=clock_timestamp()")
        .execute(&w.c.f._database.pool)
        .await
        .unwrap();
    assert_eq!(
        worker::process(&w.worker(), claim).await.unwrap(),
        EventState::Accepted,
        "acknowledged fact survives key rotation"
    );
    assert_eq!(w.count("attribution_refund_lines").await, 1);
    w.c.f.close().await;
}

#[tokio::test]
async fn attribution_reconciliation_shutdown_leases_recover_and_backoff() {
    let w = Commerce::new().await;
    assert_eq!(w.event(&w.refund()).await.0, StatusCode::ACCEPTED);
    due(&w).await;
    let mut tx = w.c.f._database.pool.begin().await.unwrap();
    sqlx::query("SELECT company_id FROM attribution_tenants FOR UPDATE")
        .execute(&mut *tx)
        .await
        .unwrap();
    let stop = CancellationToken::new();
    let shutdown = stop.clone();
    let state = w.worker();
    let task = tokio::spawn(async move { worker::iteration(state, shutdown).await });
    waiting_on_lock(&w).await;
    stop.cancel();
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap();
    tx.rollback().await.unwrap();
    let leased: bool = sqlx::query_scalar(
        "SELECT execution IS NOT NULL AND attempts=1 FROM attribution_merchant_events",
    )
    .fetch_one(&w.c.f._database.pool)
    .await
    .unwrap();
    assert!(leased);
    sqlx::query(
        "UPDATE attribution_merchant_events SET lease_until=clock_timestamp()-interval '1 second'",
    )
    .execute(&w.c.f._database.pool)
    .await
    .unwrap();
    let claim = worker::claim(&w.c.f.setup.store).await.unwrap()[0];
    assert_eq!(
        worker::process(&w.worker(), claim).await.unwrap(),
        EventState::Pending
    );
    assert!(
        worker::claim(&w.c.f.setup.store).await.unwrap().is_empty(),
        "missing prerequisite must back off"
    );
    let delayed:bool=sqlx::query_scalar("SELECT next_attempt_at>clock_timestamp() AND execution IS NULL AND attempts=2 FROM attribution_merchant_events").fetch_one(&w.c.f._database.pool).await.unwrap();
    assert!(delayed);
    w.c.f.close().await;
}

#[tokio::test]
async fn attribution_reconciliation_poison_company_does_not_fill_claim_batch() {
    let w = Commerce::new().await;
    for i in 0..10 {
        let mut r = w.refund();
        r["event_id"] = json!(format!("event-{i}"));
        r["refund_id"] = json!(format!("refund-{i}"));
        assert_eq!(w.event(&r).await.0, StatusCode::ACCEPTED);
    }
    for _ in 0..3 {
        let company = Uuid::new_v4();
        let store = Uuid::new_v4();
        let order = Uuid::new_v4();
        sqlx::query("INSERT INTO attribution_tenants(company_id,version,state,changed_at) VALUES($1,1,'active',clock_timestamp())").bind(company).execute(&w.c.f._database.pool).await.unwrap();
        sqlx::query("INSERT INTO attribution_stores(id,company_id,name,timezone) VALUES($1,$2,'Other','UTC')").bind(store).bind(company).execute(&w.c.f._database.pool).await.unwrap();
        sqlx::query("INSERT INTO attribution_orders(id,company_id,store_id,external_order_id) VALUES($1,$2,$3,'order-1')").bind(order).bind(company).bind(store).execute(&w.c.f._database.pool).await.unwrap();
        sqlx::query("INSERT INTO attribution_merchant_events(id,company_id,store_id,order_id,external_event_id,kind,fingerprint,envelope,occurred_at,state,next_attempt_at) SELECT $1,$2,$3,$4,external_event_id,kind,fingerprint,envelope,occurred_at,'pending',clock_timestamp() FROM attribution_merchant_events LIMIT 1")
            .bind(Uuid::new_v4()).bind(company).bind(store).bind(order).execute(&w.c.f._database.pool).await.unwrap();
    }
    due(&w).await;
    let claims = worker::claim(&w.c.f.setup.store).await.unwrap();
    assert_eq!(claims.len(), 4);
    let companies: std::collections::HashSet<_> = claims.iter().map(|c| c.company).collect();
    assert_eq!(companies.len(), 4);
    for claim in claims {
        worker::failed(&w.c.f.setup.store, claim).await.unwrap();
    }
    w.c.f.close().await;
}

#[tokio::test]
async fn attribution_reconciliation_pending_refund_identity_and_ingress_age() {
    let w = Commerce::new().await;
    let old = Utc::now() - chrono::Duration::days(2);
    let mut paid = w.paid();
    paid["checkout_id"] = Value::Null;
    paid["order_created_at"] = json!((old - chrono::Duration::seconds(2)).to_rfc3339());
    paid["paid_at"] = json!((old - chrono::Duration::seconds(1)).to_rfc3339());
    paid["occurred_at"] = json!(old.to_rfc3339());
    let mut a = w.refund();
    a["occurred_at"] = json!(old.to_rfc3339());
    let mut b = a.clone();
    b["event_id"] = json!("duplicate-pending");
    let (ra, rb) = tokio::join!(w.event(&a), w.event(&b));
    assert_eq!(ra.0, StatusCode::ACCEPTED);
    assert_eq!(rb.0, StatusCode::ACCEPTED);
    assert_eq!(w.count("attribution_refunds").await, 1);
    let mut conflict = a.clone();
    conflict["event_id"] = json!("changed-pending");
    conflict["order_id"] = json!("another-order");
    assert_eq!(w.event(&conflict).await.1["reason"], "refund_conflict");
    assert_eq!(w.event(&paid).await.1["state"], "accepted");
    // Previously accepted two-day-old envelopes still settle under a one-day
    // ingress policy. This is deterministic clock/policy advancement, no sleep.
    let mut state = w.worker();
    state.policy = CommercePolicy::new(1, 365).unwrap();
    for _ in 0..2 {
        due(&w).await;
        let claim = worker::claim(&w.c.f.setup.store).await.unwrap()[0];
        assert_eq!(
            worker::process(&state, claim).await.unwrap(),
            EventState::Accepted
        );
    }
    assert_eq!(w.count("attribution_refund_lines").await, 1);
    let amount:i64=sqlx::query_scalar("SELECT refunded_merchandise_amount FROM attribution_orders WHERE external_order_id='order-1'").fetch_one(&w.c.f._database.pool).await.unwrap();
    assert_eq!(amount, 1200);
    w.c.f.close().await;
}

#[tokio::test]
async fn attribution_reconciliation_current_tenant_and_refund_window_rechecked() {
    let w = Commerce::new().await;
    let mut paid = w.paid();
    let old = Utc::now() - chrono::Duration::days(2);
    paid["checkout_id"] = Value::Null;
    paid["order_created_at"] = json!(old.to_rfc3339());
    paid["paid_at"] = json!(old.to_rfc3339());
    let refund = w.refund();
    assert_eq!(w.event(&refund).await.0, StatusCode::ACCEPTED);
    assert_eq!(w.event(&paid).await.1["state"], "accepted");
    due(&w).await;
    let claim = worker::claim(&w.c.f.setup.store).await.unwrap()[0];
    sqlx::query("UPDATE attribution_tenants SET state='suspended',version=2")
        .execute(&w.c.f._database.pool)
        .await
        .unwrap();
    assert!(worker::process(&w.worker(), claim).await.is_err());
    assert_eq!(w.count("attribution_refund_lines").await, 0);
    sqlx::query("UPDATE attribution_tenants SET state='active',version=1")
        .execute(&w.c.f._database.pool)
        .await
        .unwrap();
    let mut state = w.worker();
    state.policy = CommercePolicy::new(7, 1).unwrap();
    assert_eq!(
        worker::process(&state, claim).await.unwrap(),
        EventState::Conflict
    );
    assert_eq!(w.event(&refund).await.1["reason"], "refund_window_expired");
    assert_eq!(w.count("attribution_refund_lines").await, 0);
    w.c.f.close().await;
}
