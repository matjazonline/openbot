use super::*;

#[tokio::test]
async fn attribution_e2e_two_ads_business_retries_refunds_and_product_reports() {
    let mut w = Commerce::new().await;
    let first_click = w.click;
    let ad = change(&w.c.f, "POST", "/v1/ads", json!({"store_id":w.c.store["id"],"campaign_id":w.c.campaign["id"],"name":"Ad B","advertised_product_id":"product-x"})).await;
    let link = change(&w.c.f, "POST", "/v1/links", json!({"store_id":w.c.store["id"],"ad_id":ad["id"],"destination":"https://shop.merchant.com/product","placement":"B"})).await;
    let redirect =
        w.c.f
            .app
            .clone()
            .oneshot(catalog_tests::public(link["code"].as_str().unwrap(), "GET"))
            .await
            .unwrap();
    assert_eq!(redirect.status(), StatusCode::FOUND);
    let location = url::Url::parse(redirect.headers()["location"].to_str().unwrap()).unwrap();
    w.click = Uuid::parse_str(
        &location
            .query_pairs()
            .find(|(key, _)| key == "ob_click")
            .unwrap()
            .1,
    )
    .unwrap();
    w.created = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&w.c.f._database.pool)
        .await
        .unwrap();
    w.checkout = "two-ad-checkout".into();
    let (status, selected) = w.merchant("/v1/checkouts", &json!({"checkout_id":w.checkout,"order_created_at":w.created.to_rfc3339(),"consent":true,"candidate_click_ids":[first_click,w.click]})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(selected["click_id"], json!(w.click));
    let paid = w.paid();
    let mut alternate = paid.clone();
    alternate["event_id"] = json!("same-business-payment");
    let mut tasks = tokio::task::JoinSet::new();
    for body in [paid.clone(), paid.clone(), alternate] {
        let app = w.c.f.app.clone();
        let request = merchant_request("/v1/conversions", &body, &w.key);
        tasks.spawn(async move { response(app.oneshot(request).await.unwrap()).await });
    }
    let mut order_id = None;
    while let Some(result) = tasks.join_next().await {
        let (status, result) = result.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(result["state"], "accepted", "{result}");
        if let Some(id) = &order_id {
            assert_eq!(&result["order_id"], id);
        } else {
            order_id = Some(result["order_id"].clone());
        }
    }
    let id = order_id.unwrap();
    let detail = format!("/v1/orders/{}", id.as_str().unwrap());
    let frozen = w.get(&detail).await.1;
    assert_eq!(frozen["ad_id"], ad["id"]);
    assert_eq!(frozen["click_id"], json!(w.click));
    assert_eq!(frozen["lines"].as_array().unwrap().len(), 2);
    assert_eq!(w.count("attribution_orders").await, 1);
    assert_eq!(w.c.get().await.status(), StatusCode::FOUND);
    assert_eq!(
        w.get(&detail).await.1,
        frozen,
        "a later Ad A click cannot reassign payment"
    );
    let mut conflict = paid.clone();
    conflict["tax_amount"] = json!(999);
    assert_eq!(w.event(&conflict).await.0, StatusCode::CONFLICT);
    let partial = w.refund();
    let mut duplicate = partial.clone();
    duplicate["event_id"] = json!("same-business-refund");
    let (one, two) = tokio::join!(w.event(&partial), w.event(&duplicate));
    assert_eq!(one.1["state"], "accepted");
    assert_eq!(two.1["state"], "accepted");
    let partial_report = w
        .get(&analytics_tests::query(&[(
            "ad_id",
            ad["id"].as_str().unwrap(),
        )]))
        .await;
    assert_eq!(partial_report.0, StatusCode::OK);
    let eur = &partial_report.1["groups"][0]["currencies"][0];
    assert_eq!(eur["paid_orders"], 1);
    assert_eq!(eur["refunds"], 1);
    assert_eq!(eur["net_merchandise_amount"], 4700);
    let remainder = json!({"kind":"refund","event_id":"remaining-refund","order_id":"order-1","refund_id":"refund-2","occurred_at":Utc::now().to_rfc3339(),"currency":"EUR","lines":[{"line_id":"line-x","returned_quantity":1,"merchandise_amount":1200},{"line_id":"line-y","returned_quantity":1,"merchandise_amount":3500}],"tax_amount":940,"shipping_amount":500});
    assert_eq!(w.event(&remainder).await.1["state"], "accepted");
    assert_eq!(w.event(&remainder).await.1["state"], "accepted");
    let (status, report) = w
        .get(&analytics_tests::query(&[
            ("group_by", "sold_product"),
            ("ad_id", ad["id"].as_str().unwrap()),
        ]))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(report["groups"].as_array().unwrap().len(), 2);
    for group in report["groups"].as_array().unwrap() {
        assert_eq!(group["attributed_paid_orders"], 1);
        assert!(group["period_conversion_rate"].is_null());
        assert_eq!(group["currencies"][0]["paid_orders"], 1);
        assert_eq!(group["currencies"][0]["net_merchandise_amount"], 0);
    }
    let order = w.get(&detail).await.1;
    assert_eq!(order["ad_id"], ad["id"]);
    assert_eq!(order["refunded_merchandise_amount"], 5900);
    assert_eq!(order["refunded_tax_amount"], 1180);
    assert_eq!(order["refunded_shipping_amount"], 500);
    assert_eq!(order["returned_units"], 3);
    assert_eq!(w.count("attribution_orders").await, 1);
    assert_eq!(w.count("attribution_refunds").await, 2);
    w.c.f.close().await;
}
