use super::*;

#[tokio::test]
#[ignore = "Requires pinned Playwright Chromium; CI and final gate run explicitly"]
async fn attribution_dashboard_browser_reports_filters_details_and_retry() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let w =
        Commerce::with_catalog(Catalog::with_fixture(Fixture::with_origin(&origin).await).await)
            .await;
    let mut paid = w.paid();
    paid["lines"][0]["product_id"] = json!("<script>window.injected=true</script>");
    assert_eq!(w.event(&paid).await.1["state"], "accepted");
    assert_eq!(w.event(&w.refund()).await.1["state"], "accepted");
    let mut missing = w.refund();
    missing["order_id"] = json!("missing");
    missing["refund_id"] = json!("unresolved");
    let event = w.event(&missing).await.1;
    sqlx::query("UPDATE attribution_merchant_events SET state='unresolved',next_attempt_at=NULL WHERE id=$1").bind(Uuid::parse_str(event["id"].as_str().unwrap()).unwrap()).execute(&w.c.f._database.pool).await.unwrap();
    let foreign = Uuid::new_v4();
    sqlx::query("INSERT INTO attribution_tenants(company_id,version,state,changed_at) VALUES($1,1,'active',clock_timestamp())").bind(foreign).execute(&w.c.f._database.pool).await.unwrap();
    let modes = w.c.f.upstream.mode.clone();
    let app = w.c.f.app.clone().route(
        "/fixture/role/{mode}",
        post(move |Path(mode): Path<u8>| {
            let modes = modes.clone();
            async move {
                modes.store(mode, Ordering::Release);
                StatusCode::NO_CONTENT
            }
        }),
    );
    let shutdown = CancellationToken::new();
    let stop = shutdown.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(stop.cancelled_owned())
            .await
    });
    let output = tokio::time::timeout(
        Duration::from_secs(90),
        tokio::process::Command::new("node")
            .arg("browser/reports.mjs")
            .env("ATTRIBUTION_BROWSER_ORIGIN", &origin)
            .env("ATTRIBUTION_BROWSER_SESSION", &w.c.f.token.0)
            .env(
                "ATTRIBUTION_BROWSER_COMPANY",
                w.c.f.upstream.company.0.to_string(),
            )
            .env("ATTRIBUTION_BROWSER_FOREIGN", foreign.to_string())
            .kill_on_drop(true)
            .output(),
    )
    .await;
    shutdown.cancel();
    server.await.unwrap().unwrap();
    let state: String =
        sqlx::query_scalar("SELECT state FROM attribution_merchant_events WHERE id=$1")
            .bind(Uuid::parse_str(event["id"].as_str().unwrap()).unwrap())
            .fetch_one(&w.c.f._database.pool)
            .await
            .unwrap();
    w.c.f.close().await;
    let output = output.expect("browser deadline").expect("node available");
    println!("{}", String::from_utf8_lossy(&output.stdout));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        state, "pending",
        "retried write survives response loss and navigation"
    );
}
