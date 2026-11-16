use super::*;

#[tokio::test]
async fn attribution_order_competing_business_facts_count_once() {
    let w = Commerce::new().await;
    let first = w.paid();
    let mut second = first.clone();
    second["event_id"] = json!("another-delivery");
    let (a, b) = tokio::join!(w.event(&first), w.event(&second));
    assert_eq!(a.1["state"], "accepted");
    assert_eq!(b.1["state"], "accepted");
    assert_eq!(a.1["order_id"], b.1["order_id"]);
    assert_eq!(w.count("attribution_orders").await, 1);
    assert_eq!(w.count("attribution_order_lines").await, 2);
    assert_eq!(w.count("attribution_merchant_events").await, 2);
    w.c.f.close().await;
}

#[tokio::test]
async fn attribution_refund_competing_balances_money_only_and_units_only() {
    let w = Commerce::new().await;
    let paid = w.event(&w.paid()).await.1;
    let path = format!("/v1/orders/{}", paid["order_id"].as_str().unwrap());
    let mut a = w.refund();
    a["lines"][0]["merchandise_amount"] = json!(2000);
    a["lines"][0]["returned_quantity"] = json!(0);
    let mut b = a.clone();
    b["event_id"] = json!("r2-event");
    b["refund_id"] = json!("refund-2");
    let (a, b) = tokio::join!(w.event(&a), w.event(&b));
    assert_eq!(
        [&a.1, &b.1]
            .iter()
            .filter(|r| r["state"] == "accepted")
            .count(),
        1
    );
    assert_eq!(
        [&a.1, &b.1]
            .iter()
            .filter(|r| r["reason"] == "refund_exceeds_balance")
            .count(),
        1
    );
    let mut units = w.refund();
    units["event_id"] = json!("units");
    units["refund_id"] = json!("units");
    units["lines"][0]["returned_quantity"] = json!(2);
    units["lines"][0]["merchandise_amount"] = json!(0);
    units["tax_amount"] = json!(0);
    assert_eq!(w.event(&units).await.1["state"], "accepted");
    let actual = w.get(&path).await.1;
    assert_eq!(actual["refunded_merchandise_amount"], 2000);
    assert_eq!(actual["returned_units"], 2);
    let mut full = w.refund();
    full["event_id"] = json!("remaining");
    full["refund_id"] = json!("remaining");
    full["lines"] = json!([{"line_id":"line-x","returned_quantity":0,"merchandise_amount":400},{"line_id":"line-y","returned_quantity":1,"merchandise_amount":3500}]);
    full["tax_amount"] = json!(940);
    full["shipping_amount"] = json!(500);
    assert_eq!(w.event(&full).await.1["state"], "accepted");
    let actual = w.get(&path).await.1;
    assert_eq!(actual["merchandise_amount"], 5900);
    assert_eq!(actual["refunded_merchandise_amount"], 5900);
    assert_eq!(actual["tax_amount"], 1180);
    assert_eq!(actual["refunded_tax_amount"], 1180);
    assert_eq!(actual["refunded_shipping_amount"], 500);
    assert_eq!(actual["returned_units"], 3);
    assert_eq!(
        w.get(&format!("{path}/refunds")).await.1["items"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    w.c.f.close().await;
}

#[tokio::test]
async fn attribution_refund_currencies_unknown_line_time_and_atomic_rejection() {
    let w = Commerce::new().await;
    for currency in ["EUR", "USD", "GBP", "JPY"] {
        let mut paid = w.paid();
        paid["checkout_id"] = Value::Null;
        paid["order_id"] = json!(currency);
        paid["currency"] = json!(currency);
        let result = w.event(&paid).await.1;
        assert_eq!(result["state"], "accepted");
        let mut refund = w.refund();
        refund["order_id"] = json!(currency);
        refund["refund_id"] = json!(currency);
        refund["currency"] = json!(currency);
        assert_eq!(w.event(&refund).await.1["state"], "accepted");
        for (index, reason) in [
            "currency_mismatch",
            "unknown_line",
            "refund_before_payment",
            "refund_exceeds_balance",
        ]
        .iter()
        .enumerate()
        {
            let mut bad = refund.clone();
            bad["event_id"] = json!(format!("{currency}-bad-{index}"));
            bad["refund_id"] = bad["event_id"].clone();
            match index {
                0 => bad["currency"] = json!(if currency == "USD" { "EUR" } else { "USD" }),
                1 => bad["lines"].as_array_mut().unwrap().push(
                    json!({"line_id":"unknown","returned_quantity":0,"merchandise_amount":1}),
                ),
                2 => {
                    bad["occurred_at"] =
                        json!((w.created - chrono::Duration::seconds(1)).to_rfc3339())
                }
                _ => bad["shipping_amount"] = json!(501),
            }
            let r = w.event(&bad).await.1;
            assert_eq!(r["state"], "conflict", "{r}");
            assert_eq!(r["reason"], *reason);
        }
        let order = w
            .get(&format!(
                "/v1/orders/{}",
                result["order_id"].as_str().unwrap()
            ))
            .await
            .1;
        assert_eq!(order["refunded_merchandise_amount"], 1200);
        assert_eq!(order["returned_units"], 1);
    }
    w.c.f.close().await;
}

#[tokio::test]
async fn attribution_policy_frozen_checkout_reasons_and_exact_window() {
    let w = Commerce::new().await;
    let time = chrono::DateTime::from_timestamp(Utc::now().timestamp(), 0).unwrap();
    for (index, delta, bot, consent, expected) in [
        (0, -30 * 86_400_000_000_i64, false, true, "attributed"),
        (1, -30 * 86_400_000_000 - 1, false, true, "expired"),
        (2, 1, false, true, "future"),
        (3, -1_000_000, true, true, "bot"),
        (4, -1_000_000, false, false, "no_consent"),
    ] {
        sqlx::query("UPDATE attribution_clicks SET clicked_at=$2,is_bot=$3 WHERE id=$1")
            .bind(w.click)
            .bind(time + chrono::Duration::microseconds(delta))
            .bind(bot)
            .execute(&w.c.f._database.pool)
            .await
            .unwrap();
        let checkout = format!("boundary-{index}");
        let (s,c)=w.merchant("/v1/checkouts",&json!({"checkout_id":checkout,"order_created_at":time.to_rfc3339(),"consent":consent,"candidate_click_ids":[w.click]})).await;
        assert_eq!(s, StatusCode::OK, "{c}");
        let mut paid = w.paid();
        paid["order_id"] = json!(checkout);
        paid["checkout_id"] = json!(checkout);
        paid["order_created_at"] = json!(time.to_rfc3339());
        let event = w.event(&paid).await.1;
        assert_eq!(event["state"], "accepted", "{event}");
        let order = w
            .get(&format!(
                "/v1/orders/{}",
                event["order_id"].as_str().unwrap()
            ))
            .await
            .1;
        assert_eq!(order["attribution_reason"], expected);
        assert_eq!(
            order["click_id"],
            if expected == "attributed" {
                json!(w.click)
            } else {
                Value::Null
            }
        );
    }
    // A nanosecond change in the original creation instant must be a conflict.
    let mut mismatch = w.paid();
    mismatch["order_id"] = json!("nanosecond");
    mismatch["order_created_at"] =
        json!((w.created + chrono::Duration::nanoseconds(1)).to_rfc3339());
    assert_eq!(
        w.event(&mismatch).await.1["reason"],
        "checkout_time_mismatch"
    );
    // Evidence lost before settlement is explicit; loss after settlement cannot reattribute.
    sqlx::query("DELETE FROM attribution_clicks")
        .execute(&w.c.f._database.pool)
        .await
        .unwrap();
    for (id, checkout, reason) in [
        ("pruned", json!(w.checkout), "evidence_unavailable"),
        ("absent", json!("absent"), "evidence_unavailable"),
        ("none", Value::Null, "missing"),
    ] {
        let mut paid = w.paid();
        paid["order_id"] = json!(id);
        paid["checkout_id"] = checkout;
        let event = w.event(&paid).await.1;
        assert_eq!(event["state"], "accepted");
        let order = w
            .get(&format!(
                "/v1/orders/{}",
                event["order_id"].as_str().unwrap()
            ))
            .await
            .1;
        assert_eq!(order["attribution_reason"], reason);
        assert!(order["click_id"].is_null());
    }
    let orders = w.get("/v1/orders").await.1;
    let original = orders["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["external_order_id"] == "boundary-0")
        .unwrap();
    assert_eq!(original["click_id"], json!(w.click));
    assert_eq!(original["attribution_reason"], "attributed");
    w.c.f.close().await;
}
