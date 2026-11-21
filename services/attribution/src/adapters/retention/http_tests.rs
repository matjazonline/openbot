use super::*;
use crate::adapters::retention;
use sqlx::Row;

fn policy(w: &Commerce) -> crate::application::retention::RetentionPolicy {
    w.c.f._database.config().retention
}
async fn clean(w: &Commerce) -> retention::Progress {
    let p = retention::batch(&w.c.f._database.pool, w.c.f.upstream.company.0, policy(w))
        .await
        .unwrap();
    assert!(p.rows <= 100);
    p
}
async fn age(w: &Commerce) {
    sqlx::raw_sql("UPDATE attribution_clicks SET clicked_at=clock_timestamp()-interval '40 days'; UPDATE attribution_checkouts SET received_at=clock_timestamp()-interval '40 days'; UPDATE attribution_merchant_events SET received_at=clock_timestamp()-interval '500 days'; UPDATE attribution_orders SET received_at=clock_timestamp()-interval '500 days',paid_at=CASE WHEN paid_at IS NOT NULL THEN clock_timestamp()-interval '500 days' END;")
        .execute(&w.c.f._database.pool).await.unwrap();
}
#[tokio::test]
async fn attribution_retention_raw_prune_preserves_frozen_money_and_rejects_old_delivery() {
    let w = Commerce::new().await;
    let paid = w.paid();
    let result = w.event(&paid).await.1;
    assert_eq!(w.event(&w.refund()).await.1["state"], "accepted");
    let path = format!("/v1/orders/{}", result["order_id"].as_str().unwrap());
    let before = w.get(&path).await.1;
    sqlx::raw_sql("UPDATE attribution_clicks SET clicked_at=clock_timestamp()-interval '40 days'; UPDATE attribution_checkouts SET received_at=clock_timestamp()-interval '40 days'; UPDATE attribution_merchant_events SET received_at=clock_timestamp()-interval '9 days'; UPDATE attribution_management_receipts SET created_at=clock_timestamp()-interval '3 days'; UPDATE attribution_audit SET created_at=clock_timestamp()-interval '91 days';")
        .execute(&w.c.f._database.pool).await.unwrap();
    clean(&w).await;
    for table in [
        "attribution_clicks",
        "attribution_checkouts",
        "attribution_merchant_events",
        "attribution_management_receipts",
        "attribution_audit",
    ] {
        assert_eq!(w.count(table).await, 0, "{table}");
    }
    assert_eq!(w.get(&path).await.1, before);
    // A new envelope of the same business fact cannot increment retained money.
    assert_eq!(w.event(&paid).await.1["state"], "accepted");
    assert_eq!(w.get(&path).await.1, before);
    let mut old = paid;
    let original_time = (Utc::now() - chrono::Duration::days(9)).to_rfc3339();
    for key in ["occurred_at", "paid_at", "order_created_at"] {
        old[key] = json!(original_time);
    }
    assert_eq!(w.event(&old).await.0, StatusCode::GONE);
    assert_eq!(w.count("attribution_orders").await, 1);
    let report = w.get(&analytics_tests::query(&[])).await.1;
    assert_eq!(report["groups"][0]["raw_clicks"], 0);
    assert_eq!(
        report["groups"][0]["currencies"][0]["net_merchandise_amount"],
        4700
    );
    w.c.f.close().await;
}
#[tokio::test]
async fn attribution_retention_retirement_hides_all_financial_surfaces_before_chunks() {
    let w = Commerce::new().await;
    let event = w.event(&w.paid()).await.1;
    let id = event["order_id"].as_str().unwrap();
    assert_eq!(w.event(&w.refund()).await.1["state"], "accepted");
    sqlx::query("INSERT INTO attribution_order_lines(company_id,store_id,order_id,line_id,product_id,quantity,merchandise_amount) SELECT company_id,store_id,id,'extra-'||n,'extra',1,1 FROM attribution_orders CROSS JOIN generate_series(1,250) n")
        .execute(&w.c.f._database.pool).await.unwrap();
    age(&w).await;
    // Keep paid_at in the report period while expiring by a deliberately separate
    // fixture transaction below only after first independently observing the ledger.
    let report_path = analytics_tests::query(&[]);
    // Reports of refund time still count these old paid orders before retirement.
    let before = w.get(&report_path).await.1;
    assert_eq!(before["groups"][0]["currencies"][0]["refunds"], 1);
    let mut held = w.c.f._database.pool.begin().await.unwrap();
    sqlx::query("SELECT company_id FROM attribution_tenants WHERE company_id=$1 FOR UPDATE")
        .bind(w.c.f.upstream.company.0)
        .execute(&mut *held)
        .await
        .unwrap();
    let pool = w.c.f._database.pool.clone();
    let company = w.c.f.upstream.company.0;
    let retention = policy(&w);
    let cleanup = tokio::spawn(async move { retention::batch(&pool, company, retention).await });
    let app = w.c.f.app.clone();
    let request =
        w.c.f
            .request(&report_path)
            .header("x-company-id", company.to_string())
            .body(Body::empty())
            .unwrap();
    let reading = tokio::spawn(async move { response(app.oneshot(request).await.unwrap()).await });
    let mut admin = w.c.f._database.admin().await;
    tokio::time::timeout(Duration::from_secs(2),async {loop {
        let waits:i64=sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock'").fetch_one(&mut admin).await.unwrap();
        if waits>=2 {break;}tokio::task::yield_now().await;
    }}).await.unwrap();
    held.commit().await.unwrap();
    let p = cleanup.await.unwrap().unwrap();
    assert_eq!(p.rows, 100);
    let (status, concurrent) = reading.await.unwrap();
    assert_eq!(status, StatusCode::OK);
    let currencies = &concurrent["groups"][0]["currencies"];
    assert!(
        currencies == &json!([]) || currencies == &before["groups"][0]["currencies"],
        "a real report competing with retirement sees the whole fact or its absence"
    );
    assert!(w.count("attribution_order_lines").await > 0);
    assert_eq!(
        w.get(&format!("/v1/orders/{id}")).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        w.get(&format!("/v1/orders/{id}/refunds")).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(w.get("/v1/orders").await.1["items"], json!([]));
    assert_eq!(
        w.get(&report_path).await.1["groups"][0]["currencies"],
        json!([])
    );
    assert_eq!(w.event(&w.paid()).await.0, StatusCode::NOT_FOUND);
    while clean(&w).await.rows > 0 {}
    for t in [
        "attribution_orders",
        "attribution_order_lines",
        "attribution_refunds",
        "attribution_refund_lines",
    ] {
        assert_eq!(w.count(t).await, 0, "{t}");
    }
    w.c.f.close().await;
}
#[tokio::test]
async fn attribution_retention_pending_unresolved_leased_evidence_survives_competing_settlement() {
    let w = Commerce::new().await;
    assert_eq!(w.event(&w.refund()).await.0, StatusCode::ACCEPTED);
    let paid = w.paid();
    assert_eq!(w.event(&paid).await.1["state"], "accepted");
    recovery_tests::due(&w).await;
    let claim = worker::claim(&w.c.f.setup.store).await.unwrap()[0];
    age(&w).await;
    clean(&w).await;
    assert_eq!(w.count("attribution_orders").await, 1);
    let state = w.worker();
    let (cleanup, settlement) = tokio::join!(
        retention::batch(&w.c.f._database.pool, w.c.f.upstream.company.0, policy(&w)),
        worker::process(&state, claim)
    );
    cleanup.unwrap();
    assert_eq!(
        settlement.unwrap(),
        EventState::Conflict,
        "refund remains outside its occurrence horizon after historical fixture aging"
    );
    assert_eq!(
        w.count("attribution_refund_lines").await,
        0,
        "expired refund cannot affect money"
    );
    let w2 = Commerce::new().await;
    let mut paid = w2.paid();
    paid["order_id"] = json!("unresolved");
    let event = w2.event(&paid).await.1;
    // An operator-visible unresolved envelope still protects its checkout/click.
    sqlx::query("UPDATE attribution_merchant_events SET state='unresolved',reason='retry_exhausted',next_attempt_at=NULL WHERE id=$1")
        .bind(Uuid::parse_str(event["id"].as_str().unwrap()).unwrap()).execute(&w2.c.f._database.pool).await.unwrap();
    age(&w2).await;
    clean(&w2).await;
    for t in [
        "attribution_orders",
        "attribution_merchant_events",
        "attribution_checkouts",
        "attribution_clicks",
    ] {
        assert_eq!(w2.count(t).await, 1, "{t}");
    }
    w2.c.f.close().await;
    w.c.f.close().await;
}
#[tokio::test]
async fn attribution_retention_deleted_tenant_chunks_keep_terminal_fence() {
    let w = Commerce::new().await;
    assert_eq!(w.event(&w.paid()).await.1["state"], "accepted");
    assert_eq!(w.event(&w.refund()).await.1["state"], "accepted");
    sqlx::query("INSERT INTO attribution_clicks(id,company_id,store_id,link_id,dimensions,is_bot,is_prefetch,classifier_version) SELECT gen_random_uuid(),company_id,store_id,link_id,dimensions,false,false,1 FROM attribution_clicks CROSS JOIN generate_series(1,240)").execute(&w.c.f._database.pool).await.unwrap();
    let original = w.c.f.upstream.status();
    w.c.f.upstream.mode.store(5, Ordering::Release);
    w.c.f
        .store
        .observe(&w.c.f.upstream.status(), None, None, None)
        .await
        .unwrap();
    let mut calls = 0;
    loop {
        calls += 1;
        let p = clean(&w).await;
        if p.purged {
            break;
        }
        assert!(calls < 20);
    }
    assert!(calls >= 3);
    let tables:Vec<String>=sqlx::query_scalar("SELECT table_name FROM information_schema.columns WHERE table_schema='public' AND column_name='company_id' AND table_name<>'attribution_tenants'").fetch_all(&w.c.f._database.pool).await.unwrap();
    for table in tables {
        assert_eq!(w.count(&table).await, 0, "{table}");
    }
    assert!(matches!(
        w.c.f
            .store
            .observe(&original, Some("delayed"), None, None)
            .await,
        Err(Failure::Forbidden)
    ));
    assert_eq!(w.c.get().await.status(), StatusCode::NOT_FOUND);
    assert_eq!(w.event(&w.paid()).await.0, StatusCode::UNAUTHORIZED);
    let row = sqlx::query(
        "SELECT state,display_name,purged_at IS NOT NULL AS purged FROM attribution_tenants",
    )
    .fetch_one(&w.c.f._database.pool)
    .await
    .unwrap();
    assert_eq!(row.get::<String, _>("state"), "deleted");
    assert!(row.get::<Option<String>, _>("display_name").is_none());
    assert!(row.get::<bool, _>("purged"));
    assert_eq!(
        w.count("attribution_sessions").await,
        1,
        "unrelated user session remains"
    );
    w.c.f.close().await;
}
#[tokio::test]
async fn attribution_recovery_retention_competing_claims_poison_and_cancellation() {
    let w = Commerce::new().await;
    let pool = &w.c.f._database.pool;
    let good = Uuid::new_v4();
    sqlx::query("INSERT INTO attribution_tenants(company_id,version,state,changed_at) VALUES($1,1,'active',clock_timestamp())").bind(good).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO attribution_audit(id,company_id,actor_kind,actor_id,action,resource_id,created_at) VALUES(gen_random_uuid(),$1,'user',gen_random_uuid(),'fixture',gen_random_uuid(),clock_timestamp()-interval '91 days')").bind(good).execute(pool).await.unwrap();
    let (a, b) = tokio::join!(retention::claim(pool), retention::claim(pool));
    let claims: Vec<_> = a.unwrap().into_iter().chain(b.unwrap()).collect();
    assert_eq!(claims.len(), 2);
    assert_ne!(claims[0], claims[1]);
    sqlx::raw_sql("CREATE FUNCTION poison_retention() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF OLD.action<>'fixture' THEN RAISE EXCEPTION 'fixture unavailable'; END IF; RETURN OLD; END $$; CREATE TRIGGER poison_retention BEFORE DELETE ON attribution_audit FOR EACH ROW EXECUTE FUNCTION poison_retention(); UPDATE attribution_audit SET created_at=clock_timestamp()-interval '91 days'; UPDATE attribution_tenants SET maintenance_due_at=clock_timestamp();").execute(pool).await.unwrap();
    retention::iteration(
        pool.clone(),
        policy(&w),
        w.c.f.service.clone(),
        CancellationToken::new(),
    )
    .await;
    let left: i64 =
        sqlx::query_scalar("SELECT count(*) FROM attribution_audit WHERE company_id=$1")
            .bind(good)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(left, 0);
    assert!(
        sqlx::query_scalar::<_, bool>(
            "SELECT maintenance_failure_at IS NOT NULL FROM attribution_tenants WHERE company_id=$1"
        )
        .bind(w.c.f.upstream.company.0)
        .fetch_one(pool)
        .await
        .unwrap()
    );
    sqlx::raw_sql("DROP TRIGGER poison_retention ON attribution_audit; UPDATE attribution_tenants SET maintenance_due_at=clock_timestamp();").execute(pool).await.unwrap();
    let mut held = pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE attribution_audit IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *held)
        .await
        .unwrap();
    let cancel = CancellationToken::new();
    let job = tokio::spawn(retention::iteration(
        pool.clone(),
        policy(&w),
        w.c.f.service.clone(),
        cancel.clone(),
    ));
    let mut admin = w.c.f._database.admin().await;
    tokio::time::timeout(Duration::from_secs(2),async {loop {let waits:i64=sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock'").fetch_one(&mut admin).await.unwrap();if waits>0 {break;}tokio::task::yield_now().await;}}).await.unwrap();
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(2), job)
        .await
        .unwrap()
        .unwrap();
    held.rollback().await.unwrap();
    assert!(
        w.count("attribution_audit").await > 0,
        "cancel rolls back deletion"
    );
    assert!(sqlx::query_scalar::<_,bool>("SELECT maintenance_due_at>clock_timestamp() FROM attribution_tenants WHERE company_id=$1").bind(w.c.f.upstream.company.0).fetch_one(pool).await.unwrap(),"claim survives cancellation");
    clean(&w).await;
    assert_eq!(w.count("attribution_audit").await, 0);
    w.c.f.close().await;
}

#[test]
fn attribution_limits_operational_signals_never_capture_request_secrets() {
    // Tracing callsite interest is process-global. A separate test process makes
    // capture independent of other tests registering callsites with no subscriber.
    if std::env::var_os("ATTRIBUTION_LOG_FIXTURE_CHILD").is_some() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(operational_signals());
    } else {
        let module = module_path!().split_once("::").unwrap().1;
        let test = format!(
            "{module}::attribution_limits_operational_signals_never_capture_request_secrets"
        );
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &test, "--nocapture"])
            .env("ATTRIBUTION_LOG_FIXTURE_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    }
}
async fn operational_signals() {
    let w = Commerce::new().await;
    let output = super::super::boundary_tests::LogOutput::default();
    let writer = output.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_writer(move || writer.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    assert_eq!(w.c.get().await.status(), StatusCode::FOUND);
    let mut paid = w.paid();
    paid["order_id"] = json!("private-customer-marker");
    assert_eq!(w.event(&paid).await.1["state"], "accepted");
    assert_eq!(w.event(&paid).await.1["state"], "accepted");
    let mut conflict = paid.clone();
    conflict["order_id"] = json!("other-private-marker");
    assert_eq!(w.event(&conflict).await.0, StatusCode::CONFLICT);
    assert_eq!(
        w.merchant("/v1/conversions", &json!({})).await.0,
        StatusCode::BAD_REQUEST
    );
    let mut missing = w.paid();
    missing["order_id"] = json!("missing-private-marker");
    missing["checkout_id"] = Value::Null;
    assert_eq!(w.event(&missing).await.1["state"], "accepted");
    let mut limited = false;
    for _ in 0..20 {
        let response =
            w.c.f
                .app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/auth/login")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
        if response.status() == StatusCode::TOO_MANY_REQUESTS {
            limited = true;
            break;
        }
    }
    assert!(limited, "exercise actual identity admission");
    let mut pending = w.refund();
    pending["order_id"] = json!("pending-private-marker");
    assert_eq!(w.event(&pending).await.0, StatusCode::ACCEPTED);
    worker::probe(&w.c.f.setup.store).await.unwrap();
    assert_eq!(w.get(&analytics_tests::query(&[])).await.0, StatusCode::OK);
    sqlx::raw_sql("CREATE FUNCTION fail_click_fixture() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'secret-storage-marker'; END $$; CREATE TRIGGER fail_click_fixture BEFORE INSERT ON attribution_clicks FOR EACH ROW EXECUTE FUNCTION fail_click_fixture();").execute(&w.c.f._database.pool).await.unwrap();
    assert_eq!(w.c.get().await.status(), StatusCode::SERVICE_UNAVAILABLE);
    let response =
        w.c.f
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/events?email=private-email-marker")
                    .method("POST")
                    .header("authorization", "Bearer secret-header-marker")
                    .header("x-forwarded-for", "198.51.100.73")
                    .header("user-agent", "private-useragent-marker")
                    .header("content-type", "application/json")
                    .body(Body::from("secret-body-marker"))
                    .unwrap(),
            )
            .await
            .unwrap();
    assert!(response.status().is_client_error());
    w.c.f.upstream.mode.store(1, Ordering::Release);
    assert_eq!(w.get("/v1/session").await.0, StatusCode::UNAUTHORIZED);
    crate::adapters::identity::worker::probe(&w.c.f.service)
        .await
        .unwrap();
    let log = String::from_utf8(output.0.lock().unwrap().clone()).unwrap();
    for field in [
        "redirect_storage",
        "committed",
        "insert_failed",
        "merchant_ingestion",
        "merchant_delivery",
        "duplicate",
        "conflicting_identity",
        "lag_seconds=",
        "attribution_reason=",
        "merchant_backlog",
        "pending=1",
        "oldest_seconds=",
        "operation=\"report\"",
        "operation=\"identity\"",
        "status=400",
        "status=429",
        "attribution_reason=\"missing\"",
        "status=401",
        "status=503",
        "request_id=",
        "elapsed_ms=",
        "attribution_company_status_health",
    ] {
        assert!(log.contains(field), "missing safe signal {field}: {log}");
    }
    for secret in [
        w.key.as_str(),
        w.c.f.token.0.as_str(),
        w.c.f.csrf.as_str(),
        "private-customer-marker",
        "other-private-marker",
        "pending-private-marker",
        "missing-private-marker",
        "private-email-marker",
        "secret-storage-marker",
        "secret-header-marker",
        "198.51.100.73",
        "private-useragent-marker",
        "secret-body-marker",
        w.checkout.as_str(),
        "https://",
    ] {
        assert!(!log.contains(secret), "sensitive data in operational logs");
    }
    w.c.f.close().await;
}
