use super::*;

#[tokio::test]
async fn attribution_order_and_event_lists_scope_all_objects_for_multi_company_member() {
    let w = Commerce::new().await;
    let paid = w.event(&w.paid()).await.1;
    let local_order = paid["order_id"].as_str().unwrap();
    let mut p = w.paid();
    p["event_id"] = json!("second");
    p["order_id"] = json!("second");
    assert_eq!(w.event(&p).await.0, StatusCode::OK);
    let company = Uuid::new_v4();
    let store = Uuid::new_v4();
    let order = Uuid::new_v4();
    let event = Uuid::new_v4();
    sqlx::query("INSERT INTO attribution_tenants(company_id,version,state,changed_at) VALUES($1,1,'active',clock_timestamp())").bind(company).execute(&w.c.f._database.pool).await.unwrap();
    sqlx::query(
        "INSERT INTO attribution_stores(id,company_id,name,timezone) VALUES($1,$2,'Other','UTC')",
    )
    .bind(store)
    .bind(company)
    .execute(&w.c.f._database.pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO attribution_orders(id,company_id,store_id,external_order_id,paid_fingerprint,order_created_at,paid_original,paid_at,currency,merchandise_amount,tax_amount,shipping_amount,attribution_reason,policy) SELECT $1,$2,$3,'foreign-order',paid_fingerprint,order_created_at,paid_original,paid_at,currency,merchandise_amount,tax_amount,shipping_amount,'missing',policy FROM attribution_orders LIMIT 1")
        .bind(order).bind(company).bind(store).execute(&w.c.f._database.pool).await.unwrap();
    sqlx::query("INSERT INTO attribution_order_lines(company_id,store_id,order_id,line_id,product_id,quantity,merchandise_amount) SELECT $1,$2,$3,line_id,product_id,quantity,merchandise_amount FROM attribution_order_lines WHERE order_id=$4")
        .bind(company).bind(store).bind(order).bind(Uuid::parse_str(local_order).unwrap()).execute(&w.c.f._database.pool).await.unwrap();
    sqlx::query("INSERT INTO attribution_merchant_events(id,company_id,store_id,order_id,external_event_id,kind,fingerprint,envelope,occurred_at,state,next_attempt_at) SELECT $1,$2,$3,$4,'foreign-event',kind,fingerprint,jsonb_set(envelope,'{event_id}','\"foreign-event\"'),occurred_at,'pending',clock_timestamp() FROM attribution_merchant_events LIMIT 1")
        .bind(event).bind(company).bind(store).bind(order).execute(&w.c.f._database.pool).await.unwrap();
    w.c.f.upstream.mode.store(7, Ordering::Release);
    // The same session really has live access to the second company.
    let req =
        w.c.f
            .request(&format!("/v1/orders/{order}"))
            .header("x-company-id", company.to_string())
            .body(Body::empty())
            .unwrap();
    let own = response(w.c.f.app.clone().oneshot(req).await.unwrap()).await;
    assert_eq!(own.0, StatusCode::OK);
    assert_eq!(own.1["external_order_id"], "foreign-order");
    for path in [
        format!("/v1/orders/{order}"),
        format!("/v1/orders/{order}/refunds"),
        format!("/v1/orders?store_id={store}"),
        format!("/v1/merchant-events?store_id={store}"),
    ] {
        assert_eq!(w.get(&path).await.0, StatusCode::NOT_FOUND, "{path}");
    }
    let req =
        w.c.f
            .request(&format!("/v1/merchant-events/{event}/retry"))
            .method("POST")
            .header("x-company-id", w.c.f.upstream.company.0.to_string())
            .header("origin", "http://localhost:8091")
            .header("x-csrf-token", &w.c.f.csrf)
            .header("idempotency-key", Uuid::new_v4().to_string())
            .header("idempotency-created-at", Utc::now().to_rfc3339())
            .body(Body::empty())
            .unwrap();
    assert_eq!(
        w.c.f.app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::NOT_FOUND
    );
    let page = w.get("/v1/orders?limit=1").await.1;
    let cursor = page["next_cursor"].as_str().unwrap();
    let next = w
        .get(&format!("/v1/orders?limit=1&cursor={cursor}"))
        .await
        .1;
    assert_ne!(page["items"][0]["id"], next["items"][0]["id"]);
    assert!(next["next_cursor"].is_null());
    for path in [
        format!("/v1/merchant-events?cursor={cursor}"),
        format!(
            "/v1/orders?cursor={cursor}&store_id={}",
            w.c.store["id"].as_str().unwrap()
        ),
        format!("/v1/orders/{local_order}/refunds?cursor={cursor}"),
    ] {
        assert_eq!(w.get(&path).await.0, StatusCode::BAD_REQUEST);
    }
    let req =
        w.c.f
            .request(&format!("/v1/orders?cursor={cursor}"))
            .header("x-company-id", company.to_string())
            .body(Body::empty())
            .unwrap();
    assert_eq!(
        w.c.f.app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    let diagnostics = w.get("/v1/merchant-events").await.1;
    assert_eq!(diagnostics["items"].as_array().unwrap().len(), 2);
    let serialized = diagnostics.to_string();
    for forbidden in [
        "foreign-event",
        "envelope",
        "fingerprint",
        &w.key,
        "product-x",
        "line-x",
    ] {
        assert!(!serialized.contains(forbidden));
    }
    w.c.f.close().await;
}
