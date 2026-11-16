use super::*;

#[tokio::test]
async fn attribution_merchant_ingress_auth_before_body_and_strict_money() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let w = Commerce::new().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let stop = CancellationToken::new();
    let shutdown = stop.clone();
    let app = w.c.f.app.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
    });
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    socket.write_all(b"POST /v1/conversions HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer invalid\r\nContent-Length: 100\r\nConnection: close\r\n\r\n").await.unwrap();
    let mut bytes = [0u8; 4096];
    let n = tokio::time::timeout(Duration::from_secs(1), socket.read(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    assert!(
        std::str::from_utf8(&bytes[..n])
            .unwrap()
            .starts_with("HTTP/1.1 401")
    );
    drop(socket);
    stop.cancel();
    server.await.unwrap().unwrap();
    for (pointer, value) in [
        ("/currency", json!("CHF")),
        ("/tax_amount", json!(0.1)),
        ("/shipping_amount", json!(-1)),
        ("/merchandise_amount", json!(5901)),
        ("/tax_amount", json!(MAX_AMOUNT)),
        ("/lines/0/quantity", json!(0)),
        ("/lines/0/quantity", json!(1000001)),
        ("/lines/0/line_id", json!("line-y")),
        ("/event_id", json!("bad id")),
        ("/order_id", json!("x".repeat(129))),
        ("/lines", json!([])),
        (
            "/paid_at",
            json!((Utc::now() + chrono::Duration::hours(1)).to_rfc3339()),
        ),
    ] {
        let mut input = w.paid();
        *input.pointer_mut(pointer).unwrap() = value;
        assert_eq!(
            w.event(&input).await.0,
            StatusCode::BAD_REQUEST,
            "{pointer}"
        );
    }
    for field in ["company_id", "store_id", "customer_email", "click_id"] {
        let mut input = w.paid();
        input[field] = json!(Uuid::new_v4());
        assert_eq!(w.event(&input).await.0, StatusCode::BAD_REQUEST, "{field}");
    }
    let input = w.paid();
    let wrong=change(&w.c.f,"POST","/v1/credentials",json!({"store_id":w.c.store["id"],"label":"Wrong scope","scopes":["checkouts:write"],"expires_at":null})).await;
    for key in [
        wrong["secret"].as_str().unwrap(),
        &w.c.f.token.0,
        "public-installation",
    ] {
        let status =
            w.c.f
                .app
                .clone()
                .oneshot(merchant_request("/v1/conversions", &input, key))
                .await
                .unwrap()
                .status();
        assert!(status == StatusCode::FORBIDDEN || status == StatusCode::UNAUTHORIZED);
    }
    let mut req = merchant_request("/v1/conversions", &input, &w.key);
    *req.body_mut() = Body::from(" ".repeat(262145));
    assert_eq!(
        w.c.f.app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(w.count("attribution_merchant_events").await, 0);
    assert_eq!(w.event(&input).await.0, StatusCode::OK);
    sqlx::query("UPDATE attribution_credentials SET revoked_at=clock_timestamp()")
        .execute(&w.c.f._database.pool)
        .await
        .unwrap();
    assert_eq!(w.event(&input).await.0, StatusCode::UNAUTHORIZED);
    w.c.f.close().await;
}

#[tokio::test]
async fn attribution_merchant_ingress_atomic_failure_and_real_commit_ack_loss() {
    let mut w = Commerce::new().await;
    let input = w.paid();
    sqlx::query(
        "ALTER TABLE attribution_order_lines ADD CONSTRAINT injected_failure CHECK(quantity<1)",
    )
    .execute(&w.c.f._database.pool)
    .await
    .unwrap();
    assert_eq!(w.event(&input).await.0, StatusCode::SERVICE_UNAVAILABLE);
    for table in [
        "attribution_orders",
        "attribution_order_lines",
        "attribution_merchant_events",
    ] {
        assert_eq!(w.count(table).await, 0);
    }
    sqlx::query("ALTER TABLE attribution_order_lines DROP CONSTRAINT injected_failure")
        .execute(&w.c.f._database.pool)
        .await
        .unwrap();
    let proxy = wire_proxy::Proxy::new(&w.c.f._database.options).await;
    w.c.f.setup.store.pool = proxy.pool.clone();
    rebuild(&mut w.c.f);
    proxy.mode.store(1, Ordering::Release);
    let app = w.c.f.app.clone();
    let req = merchant_request("/v1/conversions", &input, &w.key);
    let request = tokio::spawn(async move { app.oneshot(req).await.unwrap() });
    proxy.observed().await;
    assert_eq!(w.count("attribution_merchant_events").await, 1);
    assert_eq!(w.count("attribution_order_lines").await, 2);
    let result = tokio::time::timeout(Duration::from_secs(5), request)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(result.headers()["retry-after"], "1");
    assert_eq!(w.event(&input).await.1["state"], "accepted");
    assert_eq!(w.count("attribution_orders").await, 1);
    assert_eq!(w.count("attribution_merchant_events").await, 1);
    proxy.close().await;
    w.c.f.close().await;
}

#[tokio::test]
async fn attribution_merchant_ingress_original_horizon_survives_pruning() {
    let w = Commerce::new().await;
    let mut input = w.paid();
    assert_eq!(w.event(&input).await.0, StatusCode::OK);
    sqlx::query("DELETE FROM attribution_merchant_events")
        .execute(&w.c.f._database.pool)
        .await
        .unwrap();
    let old = (Utc::now() - chrono::Duration::days(8)).to_rfc3339();
    input["order_created_at"] = json!(old);
    input["paid_at"] = json!(old);
    input["occurred_at"] = json!(old);
    assert_eq!(w.event(&input).await.0, StatusCode::GONE);
    assert_eq!(w.count("attribution_merchant_events").await, 0);
    input["occurred_at"] = json!(Utc::now().to_rfc3339());
    assert_eq!(
        w.event(&input).await.0,
        StatusCode::GONE,
        "paid time also has delivery horizon"
    );
    w.c.f.close().await;
}
