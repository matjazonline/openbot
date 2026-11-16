use super::{
    catalog_tests::{Catalog, change, path, rebuild, wire_proxy},
    *,
};
use serde_json::{Value, json};
struct Website {
    c: Catalog,
    installation: Value,
    key: String,
    click: Uuid,
}
impl Website {
    async fn new() -> Self {
        let c = Catalog::new().await;
        let installation = change(
            &c.f,
            "POST",
            "/v1/installations",
            json!({"store_id":c.store["id"],"domain_id":c.domain["domain"]["id"],"name":"Website"}),
        )
        .await;
        let key=change(&c.f,"POST","/v1/credentials",json!({"store_id":c.store["id"],"label":"Checkout","scopes":["checkouts:write"],"expires_at":null})).await["secret"].as_str().unwrap().to_owned();
        let clicked = c.get().await;
        let url = url::Url::parse(clicked.headers()["location"].to_str().unwrap()).unwrap();
        let click =
            Uuid::parse_str(&url.query_pairs().find(|(k, _)| k == "ob_click").unwrap().1).unwrap();
        Self {
            c,
            installation,
            key,
            click,
        }
    }
    fn event(&self) -> Value {
        json!({"installation_id":self.installation["id"],"event_id":Uuid::new_v4(),"kind":"landing_page_load","click_id":self.click,"link_id":null,"observed_at":Utc::now().to_rfc3339(),"consent":true})
    }
    async fn count(&self, table: &str) -> i64 {
        sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&self.c.f._database.pool)
            .await
            .unwrap()
    }
    fn checkout(&self, ids: Vec<Uuid>) -> Value {
        json!({"checkout_id":format!("checkout-{}",Uuid::new_v4()),"order_created_at":Utc::now().to_rfc3339(),"consent":true,"candidate_click_ids":ids})
    }
}
fn event_request(value: &Value, origin: Option<&str>) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri("/v1/events")
        .header("content-type", "application/json");
    if let Some(origin) = origin {
        req = req.header("origin", origin);
    }
    req.body(Body::from(value.to_string())).unwrap()
}
async fn send(app: &Router, req: Request<Body>) -> axum::response::Response {
    app.clone().oneshot(req).await.unwrap()
}
fn checkout_request(value: &Value, key: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/checkouts")
        .header("authorization", format!("Bearer {key}"))
        .body(Body::from(value.to_string()))
        .unwrap()
}
async fn checkout_response(w: &Website, value: &Value) -> (StatusCode, Value) {
    let r = send(&w.c.f.app, checkout_request(value, &w.key)).await;
    let status = r.status();
    let bytes = axum::body::to_bytes(r.into_body(), 65536).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}
fn pixel(w: &Website, extra: &str, method: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(format!(
            "/pixel.gif?installation_id={}&link_id={}&consent=granted{extra}",
            w.installation["id"].as_str().unwrap(),
            w.c.link["id"].as_str().unwrap()
        ))
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn attribution_website_event_strict_types_scope_origins_and_cors() {
    let w = Website::new().await;
    let original = w.event();
    for field in ["kind", "consent", "click_id", "email", "link_id"] {
        let mut event = original.clone();
        event[field] = match field {
            "kind" => json!("paid"),
            "consent" => json!(false),
            "click_id" => json!(Uuid::new_v4()),
            "link_id" => json!(Uuid::new_v4()),
            _ => json!("customer@private.invalid"),
        };
        let r = send(
            &w.c.f.app,
            event_request(&event, Some("https://shop.merchant.com")),
        )
        .await;
        assert_eq!(
            r.status(),
            if matches!(field, "click_id" | "link_id") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::BAD_REQUEST
            }
        );
        assert_eq!(
            r.headers()["access-control-allow-origin"],
            "https://shop.merchant.com"
        );
    }
    for origin in [
        "https://evil.merchant.com",
        "null",
        "http://shop.merchant.com",
        "https://shop.merchant.com:8443",
        "https://shop.merchant.com/path",
    ] {
        let r = send(&w.c.f.app, event_request(&original, Some(origin))).await;
        assert_eq!(r.status(), StatusCode::FORBIDDEN);
        assert!(!r.headers().contains_key("access-control-allow-origin"));
    }
    let preflight = Request::builder()
        .method("OPTIONS")
        .uri("/v1/events")
        .header("origin", "https://shop.merchant.com")
        .header("access-control-request-method", "POST")
        .header("access-control-request-headers", "content-type")
        .body(Body::empty())
        .unwrap();
    let r = send(&w.c.f.app, preflight).await;
    assert_eq!(r.status(), StatusCode::NO_CONTENT);
    assert_eq!(r.headers()["access-control-allow-methods"], "POST");
    assert!(!r.headers().contains_key("access-control-allow-credentials"));
    assert_eq!(w.count("attribution_observations").await, 0);
    let r = send(
        &w.c.f.app,
        event_request(&original, Some("https://shop.merchant.com")),
    )
    .await;
    assert_eq!(r.status(), StatusCode::NO_CONTENT);
    assert_eq!(r.headers()["cache-control"], "no-store");
    assert_eq!(r.headers()["vary"], "Origin");
    assert_eq!(r.headers()["access-control-expose-headers"], "Retry-After");
    assert_eq!(
        send(&w.c.f.app, event_request(&w.event(), None))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    let mut none = w.event();
    none["click_id"] = Value::Null;
    assert_eq!(
        send(&w.c.f.app, event_request(&none, None)).await.status(),
        StatusCode::BAD_REQUEST
    );
    let raw = original
        .to_string()
        .replacen("{", "{\"kind\":\"image_load\",", 1);
    let req = Request::builder()
        .method("POST")
        .uri("/v1/events")
        .body(Body::from(raw))
        .unwrap();
    assert_eq!(
        send(&w.c.f.app, req).await.status(),
        StatusCode::BAD_REQUEST
    );
    let req = Request::builder()
        .method("POST")
        .uri("/v1/events")
        .body(Body::from(" ".repeat(8193)))
        .unwrap();
    assert_eq!(
        send(&w.c.f.app, req).await.status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(w.count("attribution_observations").await, 2);
    assert_eq!(w.count("attribution_checkouts").await, 0);
    let schema:Vec<String>=sqlx::query_scalar("SELECT column_name::text FROM information_schema.columns WHERE table_name='attribution_observations'").fetch_all(&w.c.f._database.pool).await.unwrap();
    for forbidden in ["ip", "user_agent", "referrer", "email", "visitor_id"] {
        assert!(!schema.iter().any(|s| s == forbidden));
    }
    w.c.f.close().await;
}

#[tokio::test]
async fn attribution_website_event_competing_retries_conflict_and_pruned_horizon() {
    let w = Website::new().await;
    let event = w.event();
    let (a, b) = tokio::join!(
        send(&w.c.f.app, event_request(&event, None)),
        send(&w.c.f.app, event_request(&event, None))
    );
    assert_eq!(a.status(), StatusCode::NO_CONTENT);
    assert_eq!(b.status(), StatusCode::NO_CONTENT);
    assert_eq!(w.count("attribution_observations").await, 1);
    let mut conflict = event.clone();
    conflict["kind"] = json!("image_load");
    assert_eq!(
        send(&w.c.f.app, event_request(&conflict, None))
            .await
            .status(),
        StatusCode::CONFLICT
    );
    conflict["event_id"] = json!(Uuid::new_v4());
    conflict["observed_at"] = json!((Utc::now() + chrono::Duration::minutes(1)).to_rfc3339());
    assert_eq!(
        send(&w.c.f.app, event_request(&conflict, None))
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    conflict["observed_at"] = json!((Utc::now() - chrono::Duration::hours(25)).to_rfc3339());
    sqlx::query("DELETE FROM attribution_observations")
        .execute(&w.c.f._database.pool)
        .await
        .unwrap();
    assert_eq!(
        send(&w.c.f.app, event_request(&conflict, None))
            .await
            .status(),
        StatusCode::GONE
    );
    assert_eq!(w.count("attribution_observations").await, 0);
    let mut visible = w.event();
    visible["kind"] = json!("visible_placement");
    assert_eq!(
        send(&w.c.f.app, event_request(&visible, None))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    let flags:(String,i16,i32,i16)=sqlx::query_as("SELECT kind,visibility_percent,visibility_millis,observation_version FROM attribution_observations").fetch_one(&w.c.f._database.pool).await.unwrap();
    assert_eq!(flags, ("visible_placement".into(), 50, 1000, 1));
    assert!(
        sqlx::query("UPDATE attribution_observations SET visibility_percent=NULL")
            .execute(&w.c.f._database.pool)
            .await
            .is_err()
    );
    w.c.f.close().await;
}

#[tokio::test]
async fn attribution_website_event_pixel_durable_gif_head_and_retry_timestamp() {
    let w = Website::new().await;
    assert_eq!(
        send(&w.c.f.app, pixel(&w, "", "HEAD")).await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(w.count("attribution_observations").await, 0);
    for _ in 0..2 {
        let r = send(&w.c.f.app, pixel(&w, "", "GET")).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(r.headers()["content-type"], "image/gif");
        assert_eq!(r.headers()["cache-control"], "no-store");
        let bytes = axum::body::to_bytes(r.into_body(), 128).await.unwrap();
        assert_eq!(&bytes[..6], b"GIF89a");
        assert_eq!(&bytes[6..10], &[1, 0, 1, 0]);
        assert_eq!(&bytes[19..27], &[0x21, 0xf9, 4, 1, 0, 0, 0, 0]);
    }
    assert_eq!(w.count("attribution_observations").await, 2);
    let id = Uuid::new_v4();
    let original = Utc::now().to_rfc3339();
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("event_id", &id.to_string())
        .append_pair("observed_at", &original)
        .finish();
    for _ in 0..2 {
        assert_eq!(
            send(&w.c.f.app, pixel(&w, &format!("&{query}"), "GET"))
                .await
                .status(),
            StatusCode::OK
        );
    }
    assert_eq!(w.count("attribution_observations").await, 3);
    assert_eq!(
        send(&w.c.f.app, pixel(&w, &format!("&event_id={id}"), "GET"))
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    sqlx::query("ALTER TABLE attribution_observations ADD CONSTRAINT storage_failure CHECK (false) NOT VALID").execute(&w.c.f._database.pool).await.unwrap();
    let r = send(&w.c.f.app, pixel(&w, "", "GET")).await;
    assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_ne!(r.headers()["content-type"], "image/gif");
    assert_eq!(r.headers()["retry-after"], "1");
    assert_eq!(w.count("attribution_observations").await, 3);
    w.c.f.close().await;
}

#[tokio::test]
async fn attribution_checkout_handoff_latest_eligible_immutable_concurrent_and_scope() {
    let w = Website::new().await;
    let mut request = catalog_tests::public(w.c.code(), "GET");
    request
        .headers_mut()
        .insert("user-agent", "PreviewBot".parse().unwrap());
    assert_eq!(send(&w.c.f.app, request).await.status(), StatusCode::FOUND);
    let bot: Uuid = sqlx::query_scalar("SELECT id FROM attribution_clicks WHERE is_bot")
        .fetch_one(&w.c.f._database.pool)
        .await
        .unwrap();
    let input = w.checkout(vec![w.click, bot, Uuid::new_v4()]);
    let (a, b) = tokio::join!(checkout_response(&w, &input), checkout_response(&w, &input));
    assert_eq!(a.0, StatusCode::OK);
    assert_eq!(a, b);
    assert_eq!(a.1["click_id"], json!(w.click));
    assert_eq!(a.1["reason"], "attributed");
    assert_eq!(w.count("attribution_checkouts").await, 1);
    let evidence: Value = sqlx::query_scalar("SELECT evidence FROM attribution_checkouts")
        .fetch_one(&w.c.f._database.pool)
        .await
        .unwrap();
    assert_eq!(evidence["dimensions"]["advertised_product_id"], "P1");
    change(
        &w.c.f,
        "PATCH",
        &path("ads", &w.c.ad),
        json!({"advertised_product_id":"P2"}),
    )
    .await;
    assert_eq!(w.c.get().await.status(), StatusCode::FOUND);
    let mut reordered = input.clone();
    reordered["candidate_click_ids"]
        .as_array_mut()
        .unwrap()
        .reverse();
    assert_eq!(checkout_response(&w, &reordered).await, a);
    let retained: Value = sqlx::query_scalar("SELECT evidence FROM attribution_checkouts")
        .fetch_one(&w.c.f._database.pool)
        .await
        .unwrap();
    assert_eq!(retained, evidence);
    let mut changed = input.clone();
    changed["consent"] = json!(false);
    assert_eq!(
        checkout_response(&w, &changed).await.0,
        StatusCode::CONFLICT
    );
    let mut no_consent = w.checkout(vec![w.click]);
    no_consent["consent"] = json!(false);
    assert_eq!(
        checkout_response(&w, &no_consent).await.1["reason"],
        "no_consent"
    );
    let duplicate = w.checkout(vec![w.click, w.click]);
    assert_eq!(
        checkout_response(&w, &duplicate).await.0,
        StatusCode::BAD_REQUEST
    );
    let mut forged = w.checkout(vec![w.click]);
    forged["store_id"] = w.c.store["id"].clone();
    assert_eq!(
        checkout_response(&w, &forged).await.0,
        StatusCode::BAD_REQUEST
    );
    for secret in [w.installation["id"].as_str().unwrap(), "invalid"] {
        assert_eq!(
            send(&w.c.f.app, checkout_request(&input, secret))
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    sqlx::query("UPDATE attribution_credentials SET revoked_at=clock_timestamp()")
        .execute(&w.c.f._database.pool)
        .await
        .unwrap();
    assert_eq!(
        checkout_response(&w, &input).await.0,
        StatusCode::UNAUTHORIZED
    );
    w.c.f.close().await;
}

#[tokio::test]
async fn attribution_website_event_and_checkout_unknown_commit_outcomes_replay_once() {
    for is_checkout in [false, true] {
        let mut w = Website::new().await;
        let proxy = wire_proxy::Proxy::new(&w.c.f._database.options).await;
        w.c.f.setup.store.pool = proxy.pool.clone();
        rebuild(&mut w.c.f);
        let input = w.checkout(vec![w.click]);
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("event_id", &Uuid::new_v4().to_string())
            .append_pair("observed_at", &Utc::now().to_rfc3339())
            .finish();
        let extra = format!("&{query}");
        let req = if is_checkout {
            checkout_request(&input, &w.key)
        } else {
            pixel(&w, &extra, "GET")
        };
        let app = w.c.f.app.clone();
        proxy.mode.store(1, Ordering::Release);
        let response = tokio::spawn(async move { app.oneshot(req).await.unwrap() });
        proxy.observed().await;
        let table = if is_checkout {
            "attribution_checkouts"
        } else {
            "attribution_observations"
        };
        assert_eq!(
            w.count(table).await,
            1,
            "real PG commit precedes lost acknowledgement"
        );
        let r = tokio::time::timeout(Duration::from_secs(5), response)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(r.headers()["retry-after"], "1");
        assert_ne!(r.headers()["content-type"], "image/gif");
        let retry = if is_checkout {
            checkout_request(&input, &w.key)
        } else {
            pixel(&w, &extra, "GET")
        };
        assert_eq!(send(&w.c.f.app, retry).await.status(), StatusCode::OK);
        assert_eq!(w.count(table).await, 1);
        proxy.close().await;
        w.c.f.close().await;
    }
}

async fn candidate(w: &Website, time: chrono::DateTime<Utc>, bot: bool, prefetch: bool) -> Uuid {
    let mut request = catalog_tests::public(w.c.code(), "GET");
    if bot {
        request
            .headers_mut()
            .insert("user-agent", "CrawlerBot".parse().unwrap());
    }
    if prefetch {
        request
            .headers_mut()
            .insert("purpose", "prefetch".parse().unwrap());
    }
    let r = send(&w.c.f.app, request).await;
    assert_eq!(r.status(), StatusCode::FOUND);
    let target = url::Url::parse(r.headers()["location"].to_str().unwrap()).unwrap();
    let id = Uuid::parse_str(
        &target
            .query_pairs()
            .find(|(k, _)| k == "ob_click")
            .unwrap()
            .1,
    )
    .unwrap();
    // Controlled historical clock fixture; IDs/dimensions/classification come
    // from the production redirect, not an invented paid/checkout substitute.
    sqlx::query("UPDATE attribution_clicks SET clicked_at=$2 WHERE id=$1")
        .bind(id)
        .bind(time)
        .execute(&w.c.f._database.pool)
        .await
        .unwrap();
    id
}
#[tokio::test]
async fn attribution_checkout_handoff_exact_window_precision_reasons_and_ties() {
    let w = Website::new().await;
    let now = chrono::DateTime::from_timestamp(Utc::now().timestamp(), 0).unwrap();
    let boundary = candidate(&w, now - chrono::Duration::days(30), false, false).await;
    let expired = candidate(
        &w,
        now - chrono::Duration::days(30) - chrono::Duration::microseconds(1),
        false,
        false,
    )
    .await;
    let future = candidate(&w, now + chrono::Duration::microseconds(1), false, false).await;
    let bot = candidate(&w, now - chrono::Duration::seconds(1), true, false).await;
    let prefetch = candidate(&w, now - chrono::Duration::seconds(1), false, true).await;
    let tie1 = candidate(&w, now - chrono::Duration::seconds(2), false, false).await;
    let tie2 = candidate(&w, now - chrono::Duration::seconds(2), false, false).await;
    for (ids, reason, chosen) in [
        (vec![boundary], "attributed", Some(boundary)),
        (vec![expired], "expired", None),
        (vec![future], "future", None),
        (vec![bot], "bot", None),
        (vec![prefetch], "bot", None),
        (vec![Uuid::new_v4()], "foreign_or_missing", None),
        (vec![], "missing", None),
        (
            vec![tie1, tie2, expired, future, bot, prefetch],
            "attributed",
            Some(tie1.max(tie2)),
        ),
    ] {
        let mut input = w.checkout(ids);
        input["order_created_at"] = json!(now.to_rfc3339());
        let (status, result) = checkout_response(&w, &input).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(result["reason"], reason);
        assert_eq!(result["click_id"], json!(chosen));
    }
    let precise = now + chrono::Duration::nanoseconds(1);
    let mut input = w.checkout(vec![boundary]);
    input["order_created_at"] = json!(precise.to_rfc3339());
    let r = checkout_response(&w, &input).await;
    assert_eq!(r.0, StatusCode::OK);
    assert_eq!(
        r.1["reason"], "expired",
        "rounding original creation to PG microseconds would incorrectly attribute this boundary"
    );
    assert_eq!(r.1["order_created_at"], json!(precise));
    assert_eq!(checkout_response(&w, &input).await, r);
    input["order_created_at"] = json!((precise + chrono::Duration::nanoseconds(1)).to_rfc3339());
    assert_eq!(checkout_response(&w, &input).await.0, StatusCode::CONFLICT);
    let mut old = w.checkout(vec![w.click]);
    old["order_created_at"] = json!((Utc::now() - chrono::Duration::hours(25)).to_rfc3339());
    sqlx::query("DELETE FROM attribution_checkouts")
        .execute(&w.c.f._database.pool)
        .await
        .unwrap();
    assert_eq!(checkout_response(&w, &old).await.0, StatusCode::GONE);
    assert_eq!(w.count("attribution_checkouts").await, 0);
    w.c.f.close().await;
}

#[tokio::test]
async fn attribution_website_event_current_installation_domain_and_company_before_replay() {
    let w = Website::new().await;
    let event = w.event();
    assert_eq!(
        send(&w.c.f.app, event_request(&event, None)).await.status(),
        StatusCode::NO_CONTENT
    );
    change(
        &w.c.f,
        "PATCH",
        &path("installations", &w.installation),
        json!({"enabled":false}),
    )
    .await;
    assert_eq!(
        send(&w.c.f.app, event_request(&event, None)).await.status(),
        StatusCode::GONE
    );
    change(
        &w.c.f,
        "PATCH",
        &path("installations", &w.installation),
        json!({"enabled":true}),
    )
    .await;
    // Link disablement stops new clicks, but a committed historical landing
    // remains observable through an otherwise active installation.
    change(
        &w.c.f,
        "PATCH",
        &path("links", &w.c.link),
        json!({"enabled":false}),
    )
    .await;
    assert_eq!(
        send(&w.c.f.app, event_request(&w.event(), None))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    sqlx::query(
        "UPDATE attribution_domains SET verified_until=clock_timestamp()-interval '1 second'",
    )
    .execute(&w.c.f._database.pool)
    .await
    .unwrap();
    assert!(
        !send(&w.c.f.app, event_request(&event, None))
            .await
            .status()
            .is_success()
    );
    sqlx::query("UPDATE attribution_domains SET verified_until=clock_timestamp()+interval '1 day'")
        .execute(&w.c.f._database.pool)
        .await
        .unwrap();
    w.c.f.upstream.mode.store(5, Ordering::Release);
    w.c.f
        .service
        .refresh(w.c.f.upstream.company, None)
        .await
        .unwrap();
    assert_eq!(
        send(&w.c.f.app, event_request(&event, None)).await.status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(w.count("attribution_observations").await, 2);
    w.c.f.close().await;
}

#[tokio::test]
async fn attribution_website_event_public_admission_competitors_release_and_health() {
    let w = Website::new().await;
    let mut holds = Vec::new();
    for _ in 0..3 {
        holds.push(w.c.f._database.pool.acquire().await.unwrap());
    }
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..24 {
        let app = w.c.f.app.clone();
        let req = event_request(&w.event(), None);
        tasks.spawn(async move { app.oneshot(req).await.unwrap().status() });
    }
    for _ in 0..8 {
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), tasks.join_next())
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
            StatusCode::TOO_MANY_REQUESTS
        );
    }
    assert_eq!(
        send(
            &w.c.f.app,
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap()
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    drop(holds);
    let mut accepted = 0;
    while let Some(result) = tasks.join_next().await {
        match result.unwrap() {
            StatusCode::NO_CONTENT => accepted += 1,
            StatusCode::TOO_MANY_REQUESTS => {}
            status => panic!("unexpected admission status {status}"),
        }
    }
    assert!(accepted > 0);
    assert_eq!(w.count("attribution_observations").await, accepted);
    assert_eq!(
        send(&w.c.f.app, event_request(&w.event(), None))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    w.c.f.close().await;
}

#[tokio::test]
#[ignore = "Requires pinned Playwright Chromium; CI and final gate run explicitly"]
async fn attribution_checkout_handoff_browser_sdk_cart_and_freeze() {
    use sha2::Digest;
    let integrity = format!(
        "sha384-{}",
        base64::engine::general_purpose::STANDARD.encode(sha2::Sha384::digest(include_bytes!(
            "../../../assets/sdk-1.0.0.js"
        )))
    );
    let w = Website::new().await;
    let commerce_key=change(&w.c.f,"POST","/v1/credentials",json!({"store_id":w.c.store["id"],"label":"Merchant fixture","scopes":["checkouts:write","conversions:write"],"expires_at":null})).await["secret"].as_str().unwrap().to_owned();
    let ad=change(&w.c.f,"POST","/v1/ads",json!({"store_id":w.c.store["id"],"campaign_id":w.c.campaign["id"],"name":"Ad B","advertised_product_id":"P2"})).await;
    let link=change(&w.c.f,"POST","/v1/links",json!({"store_id":w.c.store["id"],"ad_id":ad["id"],"destination":"https://shop.merchant.com/product?keep=1&keep=2#detail","placement":"B"})).await;
    let foreign = other_store_click(&w).await;
    let expired = candidate(&w, Utc::now() - chrono::Duration::days(31), false, false).await;
    let future = candidate(&w, Utc::now() + chrono::Duration::days(1), false, false).await;
    let pool = w.c.f._database.pool.clone();
    let app=w.c.f.app.clone().route("/fixture/website-state",get(move||{
        let pool=pool.clone();async move {
            let (observations,visible):(i64,i64)=sqlx::query_as("SELECT count(*),count(*) FILTER(WHERE kind='visible_placement') FROM attribution_observations").fetch_one(&pool).await.unwrap();
            Json(json!({"observations":observations,"visible":visible}))
        }
    }));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let stop = CancellationToken::new();
    let shutdown = stop.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
    });
    let output = tokio::time::timeout(
        Duration::from_secs(120),
        tokio::process::Command::new("node")
            .arg("browser/website.mjs")
            .env("ATTRIBUTION_BROWSER_SDK_INTEGRITY", integrity)
            .env("ATTRIBUTION_BROWSER_ORIGIN", origin)
            .env(
                "ATTRIBUTION_BROWSER_INSTALLATION",
                w.installation["id"].as_str().unwrap(),
            )
            .env("ATTRIBUTION_BROWSER_MERCHANT_KEY", &commerce_key)
            .env("ATTRIBUTION_BROWSER_LINK_A", w.c.code())
            .env("ATTRIBUTION_BROWSER_LINK_B", link["code"].as_str().unwrap())
            .env(
                "ATTRIBUTION_BROWSER_LINK_ID_B",
                link["id"].as_str().unwrap(),
            )
            .env("ATTRIBUTION_BROWSER_FOREIGN_CLICK", foreign.to_string())
            .env("ATTRIBUTION_BROWSER_EXPIRED_CLICK", expired.to_string())
            .env("ATTRIBUTION_BROWSER_FUTURE_CLICK", future.to_string())
            .kill_on_drop(true)
            .output(),
    )
    .await;
    stop.cancel();
    server.await.unwrap().unwrap();
    let checkouts = w.count("attribution_checkouts").await;
    let ledger:(i64,i64,i64,i64,i64,i64,Uuid)=sqlx::query_as("SELECT merchandise_amount,tax_amount,shipping_amount,refunded_merchandise_amount,refunded_tax_amount,refunded_shipping_amount,ad_id FROM attribution_orders WHERE paid_at IS NOT NULL").fetch_one(&w.c.f._database.pool).await.expect("one real paid browser order");
    assert_eq!(
        ledger,
        (
            5900,
            1180,
            500,
            5900,
            1180,
            500,
            Uuid::parse_str(ad["id"].as_str().unwrap()).unwrap()
        )
    );
    assert_eq!(w.count("attribution_orders").await, 1);
    assert_eq!(w.count("attribution_refunds").await, 2);
    assert_eq!(w.count("attribution_order_lines").await, 2);
    w.c.f.close().await;
    let output = output.expect("browser deadline").expect("node installed");
    println!("{}", String::from_utf8_lossy(&output.stdout));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(checkouts >= 4);
}

async fn other_store_click(w: &Website) -> Uuid {
    let f = &w.c.f;
    let store = change(
        f,
        "POST",
        "/v1/stores",
        json!({"name":"Other store","timezone":"UTC"}),
    )
    .await;
    let domain = change(
        f,
        "POST",
        "/v1/domains",
        json!({"store_id":store["id"],"host":"other.merchant.com"}),
    )
    .await;
    w.c._dns.proof(
        "other.merchant.com",
        domain["record_value"].as_str().unwrap(),
    );
    let req = catalog_tests::write(
        f,
        "POST",
        &format!(
            "/v1/domains/{}/verify",
            domain["domain"]["id"].as_str().unwrap()
        ),
        &Value::Null,
        Uuid::new_v4(),
        &Utc::now().to_rfc3339(),
    );
    let (parts, _) = req.into_parts();
    assert_eq!(
        send(&f.app, Request::from_parts(parts, Body::empty()))
            .await
            .status(),
        StatusCode::OK
    );
    let campaign = change(
        f,
        "POST",
        "/v1/campaigns",
        json!({"store_id":store["id"],"name":"Other"}),
    )
    .await;
    let ad=change(f,"POST","/v1/ads",json!({"store_id":store["id"],"campaign_id":campaign["id"],"name":"Other","advertised_product_id":null})).await;
    let link=change(f,"POST","/v1/links",json!({"store_id":store["id"],"ad_id":ad["id"],"destination":"https://other.merchant.com/product","placement":"other"})).await;
    let clicked = send(
        &f.app,
        catalog_tests::public(link["code"].as_str().unwrap(), "GET"),
    )
    .await;
    assert_eq!(clicked.status(), StatusCode::FOUND);
    let location = url::Url::parse(clicked.headers()["location"].to_str().unwrap()).unwrap();
    Uuid::parse_str(
        &location
            .query_pairs()
            .find(|(k, _)| k == "ob_click")
            .unwrap()
            .1,
    )
    .unwrap()
}
#[tokio::test]
async fn attribution_checkout_handoff_and_observation_existing_cross_store_evidence_rejected() {
    let w = Website::new().await;
    let foreign = other_store_click(&w).await;
    let mut event = w.event();
    event["click_id"] = json!(foreign);
    assert_eq!(
        send(&w.c.f.app, event_request(&event, None)).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(w.count("attribution_observations").await, 0);
    let input = w.checkout(vec![foreign]);
    let (status, result) = checkout_response(&w, &input).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["reason"], "foreign_or_missing");
    assert!(result["click_id"].is_null());
    let mixed = w.checkout(vec![w.click, foreign]);
    assert_eq!(
        checkout_response(&w, &mixed).await.1["click_id"],
        json!(w.click)
    );
    w.c.f.close().await;
}

#[tokio::test]
async fn attribution_checkout_handoff_headers_authenticate_before_body_and_limits() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let w = Website::new().await;
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
    socket.write_all(b"POST /v1/checkouts HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer invalid\r\nContent-Length: 100\r\nConnection: close\r\n\r\n").await.unwrap();
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
    let mut oversized = w.checkout((0..33).map(|_| Uuid::new_v4()).collect());
    assert_eq!(
        checkout_response(&w, &oversized).await.0,
        StatusCode::BAD_REQUEST
    );
    oversized["candidate_click_ids"] = json!([]);
    oversized["checkout_id"] = json!("x".repeat(129));
    assert_eq!(
        checkout_response(&w, &oversized).await.0,
        StatusCode::BAD_REQUEST
    );
    let req = Request::builder()
        .method("POST")
        .uri("/v1/checkouts")
        .header("authorization", format!("Bearer {}", w.key))
        .body(Body::from(" ".repeat(16385)))
        .unwrap();
    assert_eq!(
        send(&w.c.f.app, req).await.status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(w.count("attribution_checkouts").await, 0);
    let last: Option<chrono::DateTime<Utc>> =
        sqlx::query_scalar("SELECT last_used_at FROM attribution_credentials")
            .fetch_one(&w.c.f._database.pool)
            .await
            .unwrap();
    assert!(
        last.is_none(),
        "pre-body header check rolls back last use until an effect commits"
    );
    w.c.f.close().await;
}

#[tokio::test]
async fn attribution_website_event_competing_installation_disable_serializes_both_orders() {
    use crate::{
        adapters::website::postgres,
        application::setup::{EnabledUpdate, Mutation, Retry},
    };
    for observe_first in [false, true] {
        let w = Website::new().await;
        let p =
            w.c.f
                .service
                .company(
                    &w.c.f.token,
                    w.c.f.upstream.company,
                    Permission::ManageIntegrations,
                )
                .await
                .unwrap();
        let ticket = w.c.f.service.tenant(w.c.f.upstream.company).await.unwrap();
        let event = serde_json::from_value(w.event()).unwrap();
        let company = w.c.f.upstream.company;
        let store = w.c.f.setup.store.clone();
        let observe =
            async move { postgres::observe(&store, company, &ticket, &event, None, false).await };
        let id = serde_json::from_value(w.installation["id"].clone()).unwrap();
        let setup = w.c.f.setup.clone();
        let disable = async move {
            setup
                .store
                .mutate(
                    &p,
                    Mutation::UpdateInstallation(id, EnabledUpdate { enabled: false }),
                    Retry {
                        key: Uuid::new_v4(),
                        created_at: Utc::now(),
                    },
                    &setup.dns,
                )
                .await
                .unwrap()
        };
        let mut lock = w.c.f._database.pool.begin().await.unwrap();
        let holder: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *lock)
            .await
            .unwrap();
        sqlx::query("SELECT company_id FROM attribution_tenants WHERE company_id=$1 FOR UPDATE")
            .bind(company.0)
            .execute(&mut *lock)
            .await
            .unwrap();
        let mut admin = w.c.f._database.admin().await;
        let (observation, disabled) = if observe_first {
            let a = tokio::spawn(observe);
            let pid = catalog_tests::wait_blocked(&mut admin, holder).await;
            let b = tokio::spawn(disable);
            catalog_tests::wait_blocked(&mut admin, pid).await;
            (a, b)
        } else {
            let b = tokio::spawn(disable);
            let pid = catalog_tests::wait_blocked(&mut admin, holder).await;
            let a = tokio::spawn(observe);
            catalog_tests::wait_blocked(&mut admin, pid).await;
            (a, b)
        };
        lock.commit().await.unwrap();
        disabled.await.unwrap();
        let observed = observation.await.unwrap();
        if observe_first {
            observed.unwrap();
        } else {
            assert!(matches!(observed, Err(Failure::Gone)));
        }
        assert_eq!(
            w.count("attribution_observations").await,
            if observe_first { 1 } else { 0 }
        );
        w.c.f.close().await;
    }
}

#[tokio::test]
async fn attribution_website_event_competing_browser_overload_exposes_cors_and_retry_after() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let w = Website::new().await;
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
    // Competing production requests occupy all public slots while waiting for
    // their bodies, after origin validation. The pool remains available for CORS.
    let mut sockets = Vec::new();
    for _ in 0..16 {
        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        socket.write_all(b"POST /v1/events HTTP/1.1\r\nHost: localhost\r\nOrigin: https://shop.merchant.com\r\nContent-Type: application/json\r\nContent-Length: 100\r\nExpect: 100-continue\r\nConnection: close\r\n\r\n").await.unwrap();
        let mut ready = [0u8; 25];
        tokio::time::timeout(Duration::from_secs(1), socket.read_exact(&mut ready))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            &ready, b"HTTP/1.1 100 Continue\r\n\r\n",
            "body polling proves this request owns a public slot"
        );
        sockets.push(socket);
    }
    let mut request = pixel(&w, "", "HEAD");
    request
        .headers_mut()
        .insert("origin", "https://shop.merchant.com".parse().unwrap());
    let response = send(&w.c.f.app, request).await;
    let readable = |response: &axum::response::Response| {
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            response.headers()["access-control-allow-origin"],
            "https://shop.merchant.com"
        );
        assert_eq!(response.headers()["vary"], "Origin");
        assert_eq!(
            response.headers()["access-control-expose-headers"],
            "Retry-After"
        );
        assert_eq!(response.headers()["retry-after"], "1");
        assert!(
            !response
                .headers()
                .contains_key("access-control-allow-credentials")
        );
    };
    readable(&response);
    readable(
        &send(
            &w.c.f.app,
            event_request(&w.event(), Some("https://shop.merchant.com")),
        )
        .await,
    );
    let preflight = || {
        Request::builder()
            .method("OPTIONS")
            .uri("/v1/events")
            .header("origin", "https://shop.merchant.com")
            .header("access-control-request-method", "POST")
            .header("access-control-request-headers", "Content-Type")
            .body(Body::empty())
            .unwrap()
    };
    readable(&send(&w.c.f.app, preflight()).await);
    let foreign = send(
        &w.c.f.app,
        event_request(&w.event(), Some("https://foreign.merchant.com")),
    )
    .await;
    assert_eq!(foreign.status(), StatusCode::FORBIDDEN);
    assert!(
        !foreign
            .headers()
            .contains_key("access-control-allow-origin")
    );
    assert_eq!(w.count("attribution_observations").await, 0);
    drop(sockets);
    stop.cancel();
    server.await.unwrap().unwrap();
    let response = send(
        &w.c.f.app,
        event_request(&w.event(), Some("https://shop.merchant.com")),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        response.headers()["access-control-allow-origin"],
        "https://shop.merchant.com"
    );
    assert_eq!(w.count("attribution_observations").await, 1);
    let mut bad = preflight();
    bad.headers_mut()
        .insert("access-control-request-method", "PUT".parse().unwrap());
    let rejected = send(&w.c.f.app, bad).await;
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        rejected.headers()["access-control-allow-origin"],
        "https://shop.merchant.com"
    );
    w.c.f.close().await;
}
