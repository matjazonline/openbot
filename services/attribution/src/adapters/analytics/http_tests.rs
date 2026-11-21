use super::*;
use crate::application::analytics::midnight;

pub(super) fn query(extra: &[(&str, &str)]) -> String {
    let mut q = url::form_urlencoded::Serializer::new(String::new());
    q.append_pair(
        "start",
        &(Utc::now() - chrono::Duration::days(1)).to_rfc3339(),
    )
    .append_pair(
        "end",
        &(Utc::now() + chrono::Duration::days(1)).to_rfc3339(),
    )
    .append_pair("timezone", "UTC");
    for (k, v) in extra {
        q.append_pair(k, v);
    }
    format!("/v1/analytics?{}", q.finish())
}
#[tokio::test]
async fn attribution_analytics_ledger_independent_counts_money_and_product_bases() {
    let w = Commerce::new().await;
    assert_eq!(w.event(&w.paid()).await.1["state"], "accepted");
    assert_eq!(w.event(&w.refund()).await.1["state"], "accepted");
    let mut paid = w.paid();
    paid["order_id"] = json!("second");
    assert_eq!(w.event(&paid).await.1["state"], "accepted");
    paid["event_id"] = json!("third");
    paid["order_id"] = json!("third");
    paid["currency"] = json!("USD");
    paid["checkout_id"] = Value::Null;
    assert_eq!(w.event(&paid).await.1["state"], "accepted");
    let (status, report) = w.get(&query(&[])).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["groups"].as_array().unwrap().len(), 1);
    let group = &report["groups"][0];
    assert_eq!(group["raw_clicks"], 1);
    assert_eq!(group["eligible_clicks"], 1);
    assert_eq!(group["attributed_paid_orders"], 2);
    assert_eq!(group["unattributed_paid_orders"], 1);
    assert_eq!(group["unattributed_reasons"]["missing"], 1);
    assert_eq!(group["period_conversion_rate"], 2.0);
    let eur = group["currencies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["currency"] == "EUR")
        .unwrap();
    assert_eq!(eur["paid_orders"], 2);
    assert_eq!(eur["sold_units"], 6);
    assert_eq!(eur["merchandise_amount"], 11800);
    assert_eq!(eur["refunded_merchandise_amount"], 1200);
    assert_eq!(eur["net_merchandise_amount"], 10600);
    assert_eq!(eur["tax_amount"], 2360);
    assert_eq!(eur["refunded_tax_amount"], 240);
    assert_eq!(eur["refunds"], 1);
    assert_eq!(eur["returned_units"], 1);
    let usd = group["currencies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["currency"] == "USD")
        .unwrap();
    assert_eq!(usd["paid_orders"], 1);
    assert_eq!(usd["sold_units"], 3);
    assert_eq!(usd["merchandise_amount"], 5900);
    assert_eq!(usd["refunded_merchandise_amount"], 0);
    assert_eq!(usd["refunds"], 0);
    let (status, filtered) = w
        .get(&query(&[
            ("sold_product_id", "product-x"),
            ("currency", "EUR"),
        ]))
        .await;
    assert_eq!(status, StatusCode::OK, "{filtered}");
    let f = &filtered["groups"][0];
    assert!(f["period_conversion_rate"].is_null());
    assert_eq!(f["rate_denominator"], "omitted_sold_product_filter");
    assert_eq!(f["raw_clicks"], 1);
    assert_eq!(f["currencies"][0]["paid_orders"], 2);
    assert_eq!(f["currencies"][0]["sold_units"], 4);
    assert_eq!(f["currencies"][0]["merchandise_amount"], 4800);
    assert_eq!(f["currencies"][0]["net_merchandise_amount"], 3600);
    let (status, by_product) = w.get(&query(&[("group_by", "sold_product")])).await;
    assert_eq!(status, StatusCode::OK, "{by_product}");
    assert_eq!(by_product["groups"].as_array().unwrap().len(), 2);
    for g in by_product["groups"].as_array().unwrap() {
        assert_eq!(g["raw_clicks"], 1);
        assert!(g["period_conversion_rate"].is_null());
    }
    w.c.f.close().await;
}

async fn merchant_get(w: &Commerce, key: &str, path: &str) -> (StatusCode, Value) {
    response(
        w.c.f
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("authorization", format!("Bearer {key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await
}
#[tokio::test]
async fn attribution_analytics_scope_roles_keys_and_foreign_filters() {
    let w = Commerce::new().await;
    assert_eq!(w.event(&w.paid()).await.1["state"], "accepted");
    let page = query(&[]);
    assert_eq!(
        merchant_get(&w, &w.key, &page).await.0,
        StatusCode::FORBIDDEN
    );
    let credential=change(&w.c.f,"POST","/v1/credentials",json!({"store_id":w.c.store["id"],"label":"Report reader","scopes":["analytics:read"],"expires_at":null})).await;
    let key = credential["secret"].as_str().unwrap();
    let report = merchant_get(&w, key, &page).await;
    assert_eq!(report.0, StatusCode::OK, "{}", report.1);
    assert_eq!(report.1["groups"][0]["attributed_paid_orders"], 1);
    let store = change(
        &w.c.f,
        "POST",
        "/v1/stores",
        json!({"name":"Other store","timezone":"UTC"}),
    )
    .await;
    assert_eq!(
        merchant_get(
            &w,
            key,
            &query(&[("store_id", store["id"].as_str().unwrap())])
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let other_key = change(&w.c.f, "POST", "/v1/credentials", json!({"store_id":store["id"],"label":"Other merchant","scopes":["conversions:write"],"expires_at":null})).await;
    let mut other_paid = w.paid();
    other_paid["checkout_id"] = Value::Null;
    let result = response(
        w.c.f
            .app
            .clone()
            .oneshot(merchant_request(
                "/v1/conversions",
                &other_paid,
                other_key["secret"].as_str().unwrap(),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(result.1["state"], "accepted");
    assert_eq!(
        w.get(&page).await.1["groups"][0]["currencies"][0]["paid_orders"],
        2
    );
    assert_eq!(
        merchant_get(&w, key, &page).await.1["groups"][0]["currencies"][0]["paid_orders"],
        1,
        "store key must exclude a populated sibling store"
    );
    let company = Uuid::new_v4();
    let foreign_store = Uuid::new_v4();
    let campaign = Uuid::new_v4();
    let ad = Uuid::new_v4();
    sqlx::query("INSERT INTO attribution_tenants(company_id,version,state,changed_at) VALUES($1,1,'active',clock_timestamp())").bind(company).execute(&w.c.f._database.pool).await.unwrap();
    sqlx::query(
        "INSERT INTO attribution_stores(id,company_id,name,timezone) VALUES($1,$2,'Foreign','UTC')",
    )
    .bind(foreign_store)
    .bind(company)
    .execute(&w.c.f._database.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO attribution_campaigns(id,company_id,store_id,name) VALUES($1,$2,$3,'Foreign')",
    )
    .bind(campaign)
    .bind(company)
    .bind(foreign_store)
    .execute(&w.c.f._database.pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO attribution_ads(id,company_id,store_id,campaign_id,name) VALUES($1,$2,$3,$4,'Foreign')").bind(ad).bind(company).bind(foreign_store).bind(campaign).execute(&w.c.f._database.pool).await.unwrap();
    w.c.f.upstream.mode.store(7, Ordering::Release);
    for (filter, id) in [
        ("store_id", foreign_store),
        ("ad_id", ad),
        ("campaign_id", campaign),
    ] {
        assert_eq!(
            w.get(&query(&[(filter, &id.to_string())])).await.0,
            StatusCode::NOT_FOUND
        );
    }
    let req =
        w.c.f
            .request(&query(&[("store_id", &foreign_store.to_string())]))
            .header("x-company-id", company.to_string())
            .body(Body::empty())
            .unwrap();
    let other = response(w.c.f.app.clone().oneshot(req).await.unwrap()).await;
    assert_eq!(other.0, StatusCode::OK);
    assert_eq!(other.1["groups"][0]["attributed_paid_orders"], 0);
    assert_eq!(
        w.get(&page).await.1["groups"][0]["attributed_paid_orders"],
        1
    );
    w.c.f.upstream.mode.store(3, Ordering::Release);
    assert_eq!(w.get(&page).await.0, StatusCode::OK);
    w.c.f.upstream.mode.store(4, Ordering::Release);
    assert_eq!(w.get(&page).await.0, StatusCode::NOT_FOUND);
    w.c.f.upstream.mode.store(0, Ordering::Release);
    sqlx::query("UPDATE attribution_credentials SET revoked_at=clock_timestamp() WHERE id=$1")
        .bind(Uuid::parse_str(credential["credential"]["id"].as_str().unwrap()).unwrap())
        .execute(&w.c.f._database.pool)
        .await
        .unwrap();
    assert_eq!(
        merchant_get(&w, key, &page).await.0,
        StatusCode::UNAUTHORIZED
    );
    w.c.f.close().await;
}

#[tokio::test]
async fn attribution_analytics_bounds_unicode_keysets_filters_and_invalid_queries() {
    let w = Commerce::new().await;
    let names = ["a".into(), "🙂".repeat(128), "🙃".repeat(128)];
    for (i, name) in names.iter().enumerate() {
        let mut paid = w.paid();
        paid["order_id"] = json!(format!("unicode-{i}"));
        paid["lines"][0]["product_id"] = json!(name);
        assert_eq!(w.event(&paid).await.1["state"], "accepted");
    }
    let base = query(&[("group_by", "sold_product"), ("limit", "1")]);
    let mut path = base.clone();
    let mut seen = Vec::new();
    let mut longest = 0;
    loop {
        let (status, page) = w.get(&path).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert_eq!(page["groups"].as_array().unwrap().len(), 1);
        seen.push(page["groups"][0]["group"].as_str().unwrap().to_owned());
        let Some(cursor) = page["next_cursor"].as_str() else {
            break;
        };
        assert!(cursor.len() <= 1024);
        longest = longest.max(cursor.len());
        let appended = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("cursor", cursor)
            .finish();
        path = format!("{base}&{appended}");
        assert_eq!(
            w.get(&format!("{path}&sold_product_id=a")).await.0,
            StatusCode::BAD_REQUEST,
            "cursor binds filters"
        );
    }
    assert_eq!(
        seen,
        vec![
            "a".to_owned(),
            "product-y".to_owned(),
            names[1].clone(),
            names[2].clone()
        ]
    );
    assert!(
        longest > 512,
        "exercise the Unicode case requiring the bounded1024 cursor"
    );
    for extra in [
        "limit=0",
        "limit=101",
        "timezone=unknown",
        "group_by=unknown",
        "unexpected=1",
    ] {
        assert_eq!(
            w.get(&format!("{}&{extra}", query(&[]))).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        w.get(&query(&[("cursor", &"x".repeat(1025))])).await.0,
        StatusCode::BAD_REQUEST
    );
    let end = Utc::now();
    let start = end - chrono::Duration::days(94);
    let q = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("start", &start.to_rfc3339())
        .append_pair("end", &end.to_rfc3339())
        .append_pair("timezone", "UTC")
        .finish();
    assert_eq!(
        w.get(&format!("/v1/analytics?{q}")).await.0,
        StatusCode::BAD_REQUEST
    );
    w.c.f.close().await;
}

fn historical(w: &mut Commerce) {
    let mut commerce = w.worker();
    commerce.policy = CommercePolicy::new(3650, 3650).unwrap();
    w.c.f.app = router(
        HttpState::new(
            w.c.f._database.pool.clone(),
            Arc::new(std::sync::atomic::AtomicBool::new(true)),
            false,
        )
        .with_identity(w.c.f.setup.identity.clone())
        .with_setup(w.c.f.setup.clone())
        .with_commerce(commerce),
    );
}
fn period(start: &str, end: &str, zone: &str) -> String {
    let q = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("start", start)
        .append_pair("end", end)
        .append_pair("timezone", zone)
        .append_pair("group_by", "day")
        .finish();
    format!("/v1/analytics?{q}")
}
#[tokio::test]
async fn attribution_analytics_clocks_half_open_dst_and_refund_only_negative_net() {
    let mut w = Commerce::new().await;
    historical(&mut w);
    for (id, time) in [
        ("spring-first", "2026-03-28T23:00:00Z"),
        ("spring-next", "2026-03-29T22:00:00Z"),
        ("fall-first", "2026-10-25T00:30:00Z"),
        ("fall-second", "2026-10-25T01:30:00Z"),
    ] {
        let mut paid = w.paid();
        paid["checkout_id"] = Value::Null;
        paid["order_id"] = json!(id);
        for k in ["order_created_at", "paid_at", "occurred_at"] {
            paid[k] = json!(time);
        }
        assert_eq!(w.event(&paid).await.1["state"], "accepted");
    }
    let spring_start = midnight("2026-03-29", "Europe/Berlin").unwrap();
    let spring_end = midnight("2026-03-30", "Europe/Berlin").unwrap();
    assert_eq!((spring_end - spring_start).num_hours(), 23);
    let spring = w
        .get(&period(
            &spring_start.to_rfc3339(),
            &spring_end.to_rfc3339(),
            "Europe/Berlin",
        ))
        .await;
    assert_eq!(spring.0, StatusCode::OK, "{}", spring.1);
    assert_eq!(spring.1["groups"].as_array().unwrap().len(), 1);
    assert_eq!(spring.1["groups"][0]["group"], "2026-03-29");
    assert_eq!(spring.1["groups"][0]["unattributed_paid_orders"], 1);
    let fall_start = midnight("2026-10-25", "Europe/Berlin").unwrap();
    let fall_end = midnight("2026-10-26", "Europe/Berlin").unwrap();
    assert_eq!((fall_end - fall_start).num_hours(), 25);
    let fall = w
        .get(&period(
            &fall_start.to_rfc3339(),
            &fall_end.to_rfc3339(),
            "Europe/Berlin",
        ))
        .await
        .1;
    assert_eq!(fall["groups"][0]["unattributed_paid_orders"], 2);
    assert_eq!(
        fall["groups"][0]["currencies"][0]["merchandise_amount"],
        11800
    );
    assert!(fall["groups"][0]["period_conversion_rate"].is_null());
    assert_eq!(fall["groups"][0]["eligible_clicks"], 0);
    let mut refund = w.refund();
    refund["order_id"] = json!("fall-first");
    refund["occurred_at"] = json!("2026-10-28T12:00:00Z");
    assert_eq!(w.event(&refund).await.1["state"], "accepted");
    let report = w
        .get(&period(
            "2026-10-28T00:00:00Z",
            "2026-10-29T00:00:00Z",
            "UTC",
        ))
        .await
        .1;
    assert_eq!(report["groups"][0]["currencies"][0]["paid_orders"], 0);
    assert_eq!(report["groups"][0]["currencies"][0]["refunds"], 1);
    assert_eq!(
        report["groups"][0]["currencies"][0]["net_merchandise_amount"],
        -1200
    );
    assert!(
        midnight("2011-12-30", "Pacific/Apia").is_err(),
        "skipped local date must not silently move"
    );
    w.c.f.close().await;
}

async fn document(w: &Commerce, path: &str) -> (StatusCode, String) {
    let r =
        w.c.f
            .app
            .clone()
            .oneshot(w.c.f.request(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
    let status = r.status();
    let bytes = to_bytes(r.into_body(), 4_000_000).await.unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}
#[tokio::test]
async fn attribution_dashboard_pages_details_escaping_roles_and_exports() {
    let w = Commerce::new().await;
    let paid = w.event(&w.paid()).await.1;
    assert_eq!(w.event(&w.refund()).await.1["state"], "accepted");
    let company = w.c.f.upstream.company.0;
    let (status, html) = document(&w, &format!("/reports?company_id={company}")).await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(
        html.contains("59.00")
            && html.contains("47.00")
            && html.contains("Financial totals by currency")
    );
    assert!(html.contains("Company &lt;script&gt;"));
    assert!(!html.contains("Company <script>"));
    assert!(!html.contains(&w.key));
    assert!(html.contains("undefined") && html.contains("not a click-cohort"));
    let order = paid["order_id"].as_str().unwrap();
    let (status, html) =
        document(&w, &format!("/reports/orders/{order}?company_id={company}")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        html.contains("product-x") && html.contains("1200") && html.contains("Applied refunds")
    );
    let mut missing = w.refund();
    missing["refund_id"] = json!("unresolved");
    missing["event_id"] = json!("<script>");
    missing["order_id"] = json!("missing");
    let id = w.event(&missing).await.1["id"].as_str().unwrap().to_owned();
    sqlx::query("UPDATE attribution_merchant_events SET state='unresolved',next_attempt_at=NULL WHERE id=$1").bind(Uuid::parse_str(&id).unwrap()).execute(&w.c.f._database.pool).await.unwrap();
    let page = format!("/reports/events?company_id={company}&state=unresolved");
    let (_, html) = document(&w, &page).await;
    assert!(html.contains("event-retry"));
    assert!(html.contains("&lt;script&gt;"));
    assert!(!html.contains("Retry <script>"));
    w.c.f.upstream.mode.store(3, Ordering::Release);
    let (status, html) = document(&w, &page).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!html.contains("class=\"event-retry\""));
    let (status, csv) = document(&w, &format!("/reports/export?company_id={company}")).await;
    assert_eq!(status, StatusCode::OK, "{csv}");
    assert!(csv.starts_with("group,start_utc"));
    assert!(csv.contains("5900,1200,4700"));
    let (status, html) = document(
        &w,
        &format!("/reports?company_id={company}&timezone=invalid"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(html.contains("role=\"alert\""));
    w.c.f.close().await;
}

#[path = "bounds_tests.rs"]
mod bounds_tests;
#[path = "browser_tests.rs"]
mod browser_tests;
