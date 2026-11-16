//! Real HTTP authority/session, production routes, DNS and fresh PostgreSQL fixtures.
use super::*;
use crate::adapters::setup::tests::DnsFixture;
use serde_json::{Value, json};
use sqlx::Row;

pub(super) struct Catalog {
    pub(super) f: Fixture,
    pub(super) _dns: DnsFixture,
    pub(super) store: Value,
    pub(super) domain: Value,
    pub(super) campaign: Value,
    pub(super) ad: Value,
    pub(super) link: Value,
}
pub(super) fn write(
    f: &Fixture,
    method: &str,
    path: &str,
    body: &Value,
    key: Uuid,
    time: &str,
) -> Request<Body> {
    f.request(path)
        .method(method)
        .header("x-company-id", f.upstream.company.0.to_string())
        .header(
            "origin",
            f.setup
                .identity
                .config
                .public_origin
                .origin()
                .ascii_serialization(),
        )
        .header("x-csrf-token", &f.csrf)
        .header("idempotency-key", key.to_string())
        .header("idempotency-created-at", time)
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap()
}
pub(super) async fn response(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let r = app.clone().oneshot(request).await.unwrap();
    let code = r.status();
    let bytes = axum::body::to_bytes(r.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (code, serde_json::from_slice(&bytes).unwrap())
}
pub(super) async fn change(f: &Fixture, method: &str, path: &str, body: Value) -> Value {
    let (code, result) = response(
        &f.app,
        write(
            f,
            method,
            path,
            &body,
            Uuid::new_v4(),
            &Utc::now().to_rfc3339(),
        ),
    )
    .await;
    assert!(code.is_success(), "{path}: {code} {result}");
    result
}
pub(super) fn public(code: &str, method: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(format!("/r/{code}"))
        .body(Body::empty())
        .unwrap()
}
impl Catalog {
    pub(super) async fn new() -> Self {
        Self::with_fixture(Fixture::new().await).await
    }
    pub(super) async fn with_fixture(mut f: Fixture) -> Self {
        let dns = DnsFixture::new().await;
        f.setup.dns = dns.dns.clone();
        rebuild(&mut f);
        let store = change(
            &f,
            "POST",
            "/v1/stores",
            json!({"name":"Shop","timezone":"UTC"}),
        )
        .await;
        let domain = change(
            &f,
            "POST",
            "/v1/domains",
            json!({"store_id":store["id"],"host":"shop.merchant.com"}),
        )
        .await;
        dns.proof(
            "shop.merchant.com",
            domain["record_value"].as_str().unwrap(),
        );
        let req = write(
            &f,
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
            response(&f.app, Request::from_parts(parts, Body::empty()))
                .await
                .0,
            StatusCode::OK
        );
        let campaign = change(
            &f,
            "POST",
            "/v1/campaigns",
            json!({"store_id":store["id"],"name":"Campaign before"}),
        )
        .await;
        let ad = change(&f,"POST","/v1/ads",json!({"store_id":store["id"],"campaign_id":campaign["id"],"name":"Ad before","advertised_product_id":"P1"})).await;
        let link = change(&f,"POST","/v1/links",json!({"store_id":store["id"],"ad_id":ad["id"],"destination":"https://SHOP.merchant.com/item?a=1&a=2&x=hello%20world#details","placement":"social"})).await;
        Self {
            f,
            _dns: dns,
            store,
            domain,
            campaign,
            ad,
            link,
        }
    }
    pub(super) fn code(&self) -> &str {
        self.link["code"].as_str().unwrap()
    }
    pub(super) async fn count(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM attribution_clicks")
            .fetch_one(&self.f._database.pool)
            .await
            .unwrap()
    }
    pub(super) async fn get(&self) -> axum::response::Response {
        self.f
            .app
            .clone()
            .oneshot(public(self.code(), "GET"))
            .await
            .unwrap()
    }
}
pub(super) fn rebuild(f: &mut Fixture) {
    f.app = router(
        HttpState::new(
            f._database.pool.clone(),
            Arc::new(std::sync::atomic::AtomicBool::new(true)),
            false,
        )
        .with_identity(f.setup.identity.clone())
        .with_setup(f.setup.clone())
        .with_commerce(Fixture::commerce_http(&f.setup)),
    );
}
pub(super) fn path(resource: &str, value: &Value) -> String {
    format!("/v1/{resource}/{}", value["id"].as_str().unwrap())
}

#[tokio::test]
async fn attribution_links_http_roles_scope_retry_and_competing_claimants() {
    let c = Catalog::new().await;
    let body = json!({"store_id":c.store["id"],"ad_id":c.ad["id"],"destination":"https://shop.merchant.com/new","placement":"email"});
    let key = Uuid::new_v4();
    let time = Utc::now().to_rfc3339();
    let (a, b) = tokio::join!(
        response(
            &c.f.app,
            write(&c.f, "POST", "/v1/links", &body, key, &time)
        ),
        response(
            &c.f.app,
            write(&c.f, "POST", "/v1/links", &body, key, &time)
        )
    );
    assert!(matches!(
        (a.0, b.0),
        (StatusCode::CREATED, StatusCode::OK) | (StatusCode::OK, StatusCode::CREATED)
    ));
    assert_eq!(a.1, b.1);
    let effects: (i64,i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM attribution_links),(SELECT count(*) FROM attribution_audit WHERE resource_id=$1),(SELECT count(*) FROM attribution_management_receipts WHERE key=$2)").bind(Uuid::parse_str(a.1["id"].as_str().unwrap()).unwrap()).bind(key).fetch_one(&c.f._database.pool).await.unwrap();
    assert_eq!(effects, (2, 1, 1));
    let mut changed = body.clone();
    changed["placement"] = json!("other");
    assert_eq!(
        response(
            &c.f.app,
            write(&c.f, "POST", "/v1/links", &changed, key, &time)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        response(
            &c.f.app,
            write(
                &c.f,
                "POST",
                "/v1/links",
                &body,
                key,
                &(Utc::now() + chrono::Duration::seconds(1)).to_rfc3339()
            )
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    c.f.upstream.mode.store(3, Ordering::Release);
    assert_eq!(
        response(
            &c.f.app,
            write(&c.f, "POST", "/v1/links", &body, key, &time)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let read =
        c.f.request("/v1/links")
            .header("x-company-id", c.f.upstream.company.0.to_string())
            .body(Body::empty())
            .unwrap();
    assert_eq!(
        response(&c.f.app, read).await.1["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    c.f.upstream.mode.store(0, Ordering::Release);
    for field in ["store_id", "ad_id"] {
        let mut foreign = body.clone();
        foreign[field] = json!(Uuid::new_v4());
        assert_eq!(
            response(
                &c.f.app,
                write(&c.f, "POST", "/v1/links", &foreign, Uuid::new_v4(), &time)
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
    }
    let query = format!(
        "/v1/links?limit=1&store_id={}",
        c.store["id"].as_str().unwrap()
    );
    let page = response(
        &c.f.app,
        c.f.request(&query)
            .header("x-company-id", c.f.upstream.company.0.to_string())
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .1;
    let cursor = page["next_cursor"].as_str().unwrap();
    let next = format!("{query}&cursor={cursor}");
    let page2 = response(
        &c.f.app,
        c.f.request(&next)
            .header("x-company-id", c.f.upstream.company.0.to_string())
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .1;
    assert_ne!(page["items"][0]["id"], page2["items"][0]["id"]);
    for query in [
        format!("/v1/ads?cursor={cursor}"),
        format!("/v1/links?cursor={cursor}"),
        "/v1/links?limit=101".into(),
    ] {
        assert_eq!(
            response(
                &c.f.app,
                c.f.request(&query)
                    .header("x-company-id", c.f.upstream.company.0.to_string())
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    let foreign = Uuid::new_v4();
    let error=sqlx::query("INSERT INTO attribution_links(id,company_id,store_id,ad_id,code,destination,placement) VALUES($1,$2,$3,$4,'123456789012345678901234','https://shop.merchant.com/','bad')")
        .bind(Uuid::new_v4()).bind(c.f.upstream.company.0).bind(foreign).bind(Uuid::parse_str(c.ad["id"].as_str().unwrap()).unwrap()).execute(&c.f._database.pool).await.unwrap_err();
    assert!(
        error
            .as_database_error()
            .unwrap()
            .is_foreign_key_violation()
    );
    c.f.close().await;
}

#[tokio::test]
async fn attribution_redirect_headers_head_classification_and_immutable_history() {
    let c = Catalog::new().await;
    let head =
        c.f.app
            .clone()
            .oneshot(public(c.code(), "HEAD"))
            .await
            .unwrap();
    assert_eq!(head.status(), StatusCode::NO_CONTENT);
    assert!(!head.headers().contains_key("location"));
    for code in ["bad", "123456789012345678901234"] {
        let unknown = c.f.app.clone().oneshot(public(code, "GET")).await.unwrap();
        assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
        assert!(!unknown.headers().contains_key("location"));
    }
    assert_eq!(c.count().await, 0);
    let first = c.get().await;
    assert_eq!(first.status(), StatusCode::FOUND);
    for (name, value) in [
        ("cache-control", "no-store"),
        ("referrer-policy", "no-referrer"),
        ("x-content-type-options", "nosniff"),
    ] {
        assert_eq!(first.headers()[name], value);
    }
    let location = url::Url::parse(first.headers()["location"].to_str().unwrap()).unwrap();
    let pairs: Vec<_> = location
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    assert_eq!(
        &pairs[..3],
        &[
            ("a".into(), "1".into()),
            ("a".into(), "2".into()),
            ("x".into(), "hello world".into())
        ]
    );
    assert_eq!(location.fragment(), Some("details"));
    assert_eq!(pairs[3].0, "ob_click");
    let click = Uuid::parse_str(&pairs[3].1).unwrap();
    let old: Value = sqlx::query_scalar("SELECT dimensions FROM attribution_clicks WHERE id=$1")
        .bind(click)
        .fetch_one(&c.f._database.pool)
        .await
        .unwrap();
    assert_eq!(old["advertised_product_id"], "P1");
    assert_eq!(old["link_revision"], 1);
    change(
        &c.f,
        "PATCH",
        &path("campaigns", &c.campaign),
        json!({"name":"Campaign after"}),
    )
    .await;
    change(
        &c.f,
        "PATCH",
        &path("ads", &c.ad),
        json!({"name":"Ad after","advertised_product_id":"P2"}),
    )
    .await;
    change(
        &c.f,
        "PATCH",
        &path("links", &c.link),
        json!({"destination":"https://shop.merchant.com/after?keep=yes#new","placement":"after"}),
    )
    .await;
    let mut request = public(c.code(), "GET");
    request
        .headers_mut()
        .insert("user-agent", "ScannerBot preview".parse().unwrap());
    request
        .headers_mut()
        .insert("sec-purpose", "prefetch;prerender".parse().unwrap());
    request.headers_mut().insert(
        "x-forwarded-for",
        "private-ip-not-retained".parse().unwrap(),
    );
    let second = c.f.app.clone().oneshot(request).await.unwrap();
    assert_eq!(second.status(), StatusCode::FOUND);
    let rows=sqlx::query("SELECT id,dimensions,is_bot,is_prefetch,classifier_version,clicked_at FROM attribution_clicks ORDER BY clicked_at").fetch_all(&c.f._database.pool).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get::<Value, _>("dimensions"), old);
    assert!(!rows[0].get::<bool, _>("is_bot"));
    assert!(!rows[0].get::<bool, _>("is_prefetch"));
    assert!(rows[1].get::<bool, _>("is_bot"));
    assert!(rows[1].get::<bool, _>("is_prefetch"));
    assert_eq!(rows[1].get::<i16, _>("classifier_version"), 1);
    assert_ne!(rows[1].get::<Uuid, _>("id"), click);
    let new = rows[1].get::<Value, _>("dimensions");
    assert_eq!(new["campaign_name"], "Campaign after");
    assert_eq!(new["ad_name"], "Ad after");
    assert_eq!(new["advertised_product_id"], "P2");
    assert_eq!(new["link_revision"], 2);
    change(
        &c.f,
        "PATCH",
        &path("ads", &c.ad),
        json!({"advertised_product_id":null}),
    )
    .await;
    assert_eq!(c.get().await.status(), StatusCode::FOUND);
    assert!(
        sqlx::query_scalar::<_, Value>(
            "SELECT dimensions FROM attribution_clicks ORDER BY clicked_at DESC LIMIT 1"
        )
        .fetch_one(&c.f._database.pool)
        .await
        .unwrap()["advertised_product_id"]
            .is_null()
    );
    change(
        &c.f,
        "PATCH",
        &path("links", &c.link),
        json!({"enabled":false}),
    )
    .await;
    let disabled = c.get().await;
    assert_eq!(disabled.status(), StatusCode::GONE);
    assert!(!disabled.headers().contains_key("location"));
    assert_eq!(c.count().await, 3);
    c.f.close().await;
}

#[tokio::test]
async fn attribution_links_destination_and_parent_eligibility() {
    let c = Catalog::new().await;
    let time = Utc::now().to_rfc3339();
    for (destination, expected) in [
        (
            "https://shop.merchant.com/?%6fb_click=bad",
            StatusCode::BAD_REQUEST,
        ),
        ("https://other.merchant.com/", StatusCode::NOT_FOUND),
        ("http://shop.merchant.com/", StatusCode::BAD_REQUEST),
    ] {
        assert_eq!(
            response(
                &c.f.app,
                write(
                    &c.f,
                    "PATCH",
                    &path("links", &c.link),
                    &json!({"destination":destination}),
                    Uuid::new_v4(),
                    &time
                )
            )
            .await
            .0,
            expected
        );
    }
    for (kind, object) in [
        ("campaigns", &c.campaign),
        ("ads", &c.ad),
        ("stores", &c.store),
    ] {
        change(&c.f, "PATCH", &path(kind, object), json!({"enabled":false})).await;
        let r = c.get().await;
        assert!(!r.status().is_redirection());
        assert!(!r.headers().contains_key("location"));
        change(&c.f, "PATCH", &path(kind, object), json!({"enabled":true})).await;
        assert_eq!(c.get().await.status(), StatusCode::FOUND);
    }
    let req = write(
        &c.f,
        "POST",
        &format!(
            "/v1/domains/{}/revoke",
            c.domain["domain"]["id"].as_str().unwrap()
        ),
        &Value::Null,
        Uuid::new_v4(),
        &time,
    );
    let (parts, _) = req.into_parts();
    assert_eq!(
        response(&c.f.app, Request::from_parts(parts, Body::empty()))
            .await
            .0,
        StatusCode::OK
    );
    assert!(!c.get().await.status().is_redirection());
    change(
        &c.f,
        "PATCH",
        &path("links", &c.link),
        json!({"enabled":false}),
    )
    .await;
    assert_eq!(c.count().await, 3);
    c.f.close().await;
}

#[tokio::test]
async fn attribution_redirect_storage_rejection_and_freshness_fail_closed() {
    let c = Catalog::new().await;
    sqlx::query("ALTER TABLE attribution_clicks ADD CONSTRAINT reject_fixture CHECK (false)")
        .execute(&c.f._database.pool)
        .await
        .unwrap();
    let r = c.get().await;
    assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(r.headers()["retry-after"], "1");
    assert!(!r.headers().contains_key("location"));
    assert_eq!(c.count().await, 0);
    sqlx::query("ALTER TABLE attribution_clicks DROP CONSTRAINT reject_fixture")
        .execute(&c.f._database.pool)
        .await
        .unwrap();
    c.f.upstream.mode.store(2, Ordering::Release);
    sqlx::query(
        "UPDATE attribution_tenants SET status_valid_until=clock_timestamp()-interval '1 minute'",
    )
    .execute(&c.f._database.pool)
    .await
    .unwrap();
    assert_eq!(c.get().await.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(c.count().await, 0);
    c.f.upstream.mode.store(5, Ordering::Release);
    c.f.service
        .refresh(c.f.upstream.company, None)
        .await
        .unwrap();
    assert_eq!(c.get().await.status(), StatusCode::FORBIDDEN);
    assert_eq!(c.count().await, 0);
    c.f.close().await;
}

#[tokio::test]
async fn attribution_redirect_concurrent_distinct_ids_and_latency_budget() {
    let c = Catalog::new().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = format!("http://{}/r/{}", listener.local_addr().unwrap(), c.code());
    let stop = CancellationToken::new();
    let shutdown = stop.clone();
    let app = c.f.app.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
    });
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    assert_eq!(
        client.get(&target).send().await.unwrap().status(),
        StatusCode::FOUND
    );
    let mut elapsed = Vec::new();
    for batch in 0..13 {
        let mut requests = tokio::task::JoinSet::new();
        for _ in 0..if batch == 12 { 4 } else { 8 } {
            let client = client.clone();
            let target = target.clone();
            requests.spawn(async move {
                let start = std::time::Instant::now();
                let r = client.get(target).send().await.unwrap();
                assert_eq!(r.status(), StatusCode::FOUND);
                (start.elapsed().as_millis(), r.headers()["location"].clone())
            });
        }
        while let Some(result) = requests.join_next().await {
            elapsed.push(result.unwrap().0);
        }
    }
    elapsed.sort();
    assert_eq!(elapsed.len(), 100);
    assert!(elapsed[94] < 1000, "fixture p95 {}ms", elapsed[94]);
    println!(
        "redirect fixture warm concurrency8 n100: p50={}ms p95={}ms p99={}ms (implementation budget, not production SLO)",
        elapsed[49], elapsed[94], elapsed[98]
    );
    let counts: (i64, i64, i64) = sqlx::query_as(
        "SELECT count(*),count(DISTINCT id),count(DISTINCT company_id) FROM attribution_clicks",
    )
    .fetch_one(&c.f._database.pool)
    .await
    .unwrap();
    assert_eq!(counts, (101, 101, 1));
    stop.cancel();
    server.await.unwrap().unwrap();
    c.f.close().await;
}

#[path = "wire_proxy.rs"]
pub(super) mod wire_proxy;

#[tokio::test]
async fn attribution_redirect_unknown_commit_outcome_retains_row_without_location() {
    let mut c = Catalog::new().await;
    let proxy = wire_proxy::Proxy::new(&c.f._database.options).await;
    c.f.setup.store.pool = proxy.pool.clone();
    rebuild(&mut c.f);
    proxy.mode.store(1, Ordering::Release);
    let app = c.f.app.clone();
    let req = public(c.code(), "GET");
    let request = tokio::spawn(async move { app.oneshot(req).await.unwrap() });
    proxy.observed().await;
    assert_eq!(
        c.count().await,
        1,
        "real database COMMIT succeeded before acknowledgement was lost"
    );
    let response = tokio::time::timeout(Duration::from_secs(5), request)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(!response.headers().contains_key("location"));
    assert_eq!(response.headers()["retry-after"], "1");
    assert_eq!(c.get().await.status(), StatusCode::FOUND);
    assert_eq!(
        c.count().await,
        2,
        "separate browser retry is another raw click"
    );
    proxy.close().await;
    c.f.close().await;
}

#[tokio::test]
async fn attribution_redirect_client_disconnect_after_real_commit_is_durable() {
    use tokio::io::AsyncWriteExt;
    let mut c = Catalog::new().await;
    let proxy = wire_proxy::Proxy::new(&c.f._database.options).await;
    c.f.setup.store.pool = proxy.pool.clone();
    rebuild(&mut c.f);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = c.f.app.clone();
    let stop = CancellationToken::new();
    let shutdown = stop.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
    });
    proxy.mode.store(2, Ordering::Release);
    let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
    client
        .write_all(
            format!(
                "GET /r/{} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
                c.code()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    proxy.observed().await;
    assert_eq!(c.count().await, 1);
    client.shutdown().await.unwrap();
    drop(client);
    proxy.release.notify_one();
    assert_eq!(c.get().await.status(), StatusCode::FOUND);
    assert_eq!(c.count().await, 2);
    stop.cancel();
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    proxy.close().await;
    c.f.close().await;
}

pub(super) fn key_request(
    method: &str,
    path: &str,
    secret: &str,
    body: Body,
    key: Uuid,
    time: &str,
) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {secret}"))
        .header("idempotency-key", key.to_string())
        .header("idempotency-created-at", time)
        .body(body)
        .unwrap()
}
#[tokio::test]
async fn attribution_links_merchant_scope_principal_collision_and_revoke_before_replay() {
    let c = Catalog::new().await;
    let issued=change(&c.f,"POST","/v1/credentials",json!({"store_id":c.store["id"],"label":"Links","scopes":["links:read","links:write"],"expires_at":null})).await;
    let secret = issued["secret"].as_str().unwrap();
    // Force the normally improbable cross-class UUID equality, preserving actual
    // production credential authentication and both kinds of receipt consumers.
    sqlx::query("UPDATE attribution_credentials SET id=$1 WHERE id=$2")
        .bind(c.f.upstream.user.0)
        .bind(Uuid::parse_str(issued["credential"]["id"].as_str().unwrap()).unwrap())
        .execute(&c.f._database.pool)
        .await
        .unwrap();
    let payload = json!({"store_id":c.store["id"],"ad_id":c.ad["id"],"destination":"https://shop.merchant.com/","placement":"merchant"});
    let key = Uuid::new_v4();
    let time = Utc::now().to_rfc3339();
    let (status, user) = response(
        &c.f.app,
        write(&c.f, "POST", "/v1/links", &payload, key, &time),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let bytes = serde_json::to_vec(&payload).unwrap();
    let (status, merchant) = response(
        &c.f.app,
        key_request(
            "POST",
            "/v1/links",
            secret,
            Body::from(bytes.clone()),
            key,
            &time,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_ne!(user["id"], merchant["id"]);
    let (status, replay) = response(
        &c.f.app,
        key_request(
            "POST",
            "/v1/links",
            secret,
            Body::from(bytes.clone()),
            key,
            &time,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(merchant, replay);
    let kinds:i64=sqlx::query_scalar("SELECT count(DISTINCT principal_kind) FROM attribution_management_receipts WHERE principal_id=$1 AND key=$2").bind(c.f.upstream.user.0).bind(key).fetch_one(&c.f._database.pool).await.unwrap();
    assert_eq!(kinds, 2);
    let mut request = key_request("GET", "/v1/links", secret, Body::empty(), key, &time);
    request
        .headers_mut()
        .insert("x-company-id", Uuid::new_v4().to_string().parse().unwrap());
    assert_eq!(response(&c.f.app, request).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        response(
            &c.f.app,
            key_request(
                "POST",
                "/v1/campaigns",
                secret,
                Body::from("{}"),
                key,
                &time
            )
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let second = change(
        &c.f,
        "POST",
        "/v1/stores",
        json!({"name":"Other","timezone":"UTC"}),
    )
    .await;
    let other = change(
        &c.f,
        "POST",
        "/v1/campaigns",
        json!({"store_id":second["id"],"name":"Other"}),
    )
    .await;
    let other_ad=change(&c.f,"POST","/v1/ads",json!({"store_id":second["id"],"campaign_id":other["id"],"name":"Other","advertised_product_id":null})).await;
    let other_link=change(&c.f,"POST","/v1/credentials",json!({"store_id":second["id"],"label":"Other","scopes":["conversions:write"],"expires_at":null})).await;
    let mut foreign = payload.clone();
    foreign["ad_id"] = other_ad["id"].clone();
    assert_eq!(
        response(
            &c.f.app,
            key_request(
                "POST",
                "/v1/links",
                secret,
                Body::from(foreign.to_string()),
                Uuid::new_v4(),
                &time
            )
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    foreign["store_id"] = second["id"].clone();
    assert_eq!(
        response(
            &c.f.app,
            key_request(
                "POST",
                "/v1/links",
                secret,
                Body::from(foreign.to_string()),
                Uuid::new_v4(),
                &time
            )
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        response(
            &c.f.app,
            key_request(
                "GET",
                "/v1/links",
                other_link["secret"].as_str().unwrap(),
                Body::empty(),
                key,
                &time
            )
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    // An unbounded body must never be polled for an invalid/revoked credential.
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = c.f.app.clone();
    let stop = CancellationToken::new();
    let shutdown = stop.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
    });
    let mut socket = tokio::net::TcpStream::connect(addr).await.unwrap();
    socket.write_all(b"POST /v1/links HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer bad\r\nContent-Length: 100\r\nConnection: close\r\n\r\n").await.unwrap();
    let mut response_bytes = [0u8; 4096];
    let n = tokio::time::timeout(Duration::from_secs(1), socket.read(&mut response_bytes))
        .await
        .unwrap()
        .unwrap();
    assert!(
        std::str::from_utf8(&response_bytes[..n])
            .unwrap()
            .starts_with("HTTP/1.1 401")
    );
    drop(socket);
    stop.cancel();
    server.await.unwrap().unwrap();
    let revoke = write(
        &c.f,
        "POST",
        &format!("/v1/credentials/{}/revoke", c.f.upstream.user.0),
        &Value::Null,
        Uuid::new_v4(),
        &time,
    );
    let (parts, _) = revoke.into_parts();
    assert_eq!(
        response(&c.f.app, Request::from_parts(parts, Body::empty()))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        response(
            &c.f.app,
            key_request("POST", "/v1/links", secret, Body::from(bytes), key, &time)
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    c.f.close().await;
}

#[tokio::test]
async fn attribution_redirect_public_admission_competing_requests_and_recovery() {
    let c = Catalog::new().await;
    let mut holds = Vec::new();
    for _ in 0..3 {
        holds.push(c.f._database.pool.acquire().await.unwrap());
    }
    let mut requests = tokio::task::JoinSet::new();
    for _ in 0..24 {
        let app = c.f.app.clone();
        let req = public(c.code(), "GET");
        requests.spawn(async move { app.oneshot(req).await.unwrap().status() });
    }
    for _ in 0..8 {
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), requests.join_next())
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
            StatusCode::TOO_MANY_REQUESTS
        );
    }
    assert_eq!(
        c.f.app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    drop(holds);
    let mut accepted = 0;
    while let Some(result) = requests.join_next().await {
        match result.unwrap() {
            StatusCode::FOUND => accepted += 1,
            StatusCode::TOO_MANY_REQUESTS => {}
            other => panic!("unexpected admission outcome {other}"),
        }
    }
    assert!(accepted > 0);
    assert_eq!(c.count().await, accepted);
    assert_eq!(c.get().await.status(), StatusCode::FOUND);
    assert_eq!(c.count().await, accepted + 1);
    c.f.close().await;
}

pub(super) async fn wait_blocked(admin: &mut sqlx::PgConnection, holder: i32) -> i32 {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if let Some(pid)=sqlx::query_scalar::<_,i32>("SELECT pid FROM pg_stat_activity WHERE datname=current_database() AND $1=ANY(pg_blocking_pids(pid)) LIMIT 1").bind(holder).fetch_optional(&mut *admin).await.unwrap() {return pid;}
            tokio::task::yield_now().await;
        }
    }).await.expect("production transaction actually waits for competing lock")
}
#[tokio::test]
async fn attribution_redirect_competing_disable_serializes_both_orders() {
    use crate::{
        adapters::{catalog::postgres, management::Authorized},
        application::{
            catalog::{LinkUpdate, Mutation},
            setup::Retry,
        },
    };
    for redirect_first in [false, true] {
        let c = Catalog::new().await;
        let p = Authorized::User(
            c.f.service
                .company(&c.f.token, c.f.upstream.company, Permission::ManageLinks)
                .await
                .unwrap(),
        );
        let command = Mutation::UpdateLink(
            serde_json::from_value(c.link["id"].clone()).unwrap(),
            LinkUpdate {
                destination: None,
                placement: None,
                enabled: Some(false),
            },
        );
        let store = c.f.setup.store.clone();
        let disable = async move {
            postgres::mutate(
                &store,
                &p,
                command,
                Retry {
                    key: Uuid::new_v4(),
                    created_at: Utc::now(),
                },
            )
            .await
            .unwrap()
        };
        let mut lock = c.f._database.pool.begin().await.unwrap();
        let holder: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *lock)
            .await
            .unwrap();
        sqlx::query("SELECT company_id FROM attribution_tenants WHERE company_id=$1 FOR UPDATE")
            .bind(c.f.upstream.company.0)
            .execute(&mut *lock)
            .await
            .unwrap();
        let mut admin = c.f._database.admin().await;
        let app = c.f.app.clone();
        let req = public(c.code(), "GET");
        let redirect = async move { app.oneshot(req).await.unwrap() };
        let (click, disable) = if redirect_first {
            let click = tokio::spawn(redirect);
            let first = wait_blocked(&mut admin, holder).await;
            let disable = tokio::spawn(disable);
            wait_blocked(&mut admin, first).await;
            (click, disable)
        } else {
            let disable = tokio::spawn(disable);
            let first = wait_blocked(&mut admin, holder).await;
            let click = tokio::spawn(redirect);
            wait_blocked(&mut admin, first).await;
            (click, disable)
        };
        lock.commit().await.unwrap();
        let click = click.await.unwrap();
        disable.await.unwrap();
        assert_eq!(
            click.status(),
            if redirect_first {
                StatusCode::FOUND
            } else {
                StatusCode::GONE
            }
        );
        assert_eq!(c.count().await, if redirect_first { 1 } else { 0 });
        assert_eq!(c.get().await.status(), StatusCode::GONE);
        c.f.close().await;
    }
}

#[tokio::test]
async fn attribution_links_existing_foreign_dimensions_never_cross_scope() {
    let c = Catalog::new().await;
    let company = Uuid::new_v4();
    let store = Uuid::new_v4();
    let campaign = Uuid::new_v4();
    let ad = Uuid::new_v4();
    let link = Uuid::new_v4();
    let mut tx = c.f._database.pool.begin().await.unwrap();
    sqlx::query("INSERT INTO attribution_tenants(company_id,version,state,changed_at) VALUES($1,1,'active',clock_timestamp())").bind(company).execute(&mut *tx).await.unwrap();
    sqlx::query(
        "INSERT INTO attribution_stores(id,company_id,name,timezone) VALUES($1,$2,'Foreign','UTC')",
    )
    .bind(store)
    .bind(company)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO attribution_campaigns(id,company_id,store_id,name) VALUES($1,$2,$3,'Foreign')",
    )
    .bind(campaign)
    .bind(company)
    .bind(store)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("INSERT INTO attribution_ads(id,company_id,store_id,campaign_id,name) VALUES($1,$2,$3,$4,'Foreign')").bind(ad).bind(company).bind(store).bind(campaign).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO attribution_links(id,company_id,store_id,ad_id,code,destination,placement) VALUES($1,$2,$3,$4,'123456789012345678901234','https://shop.merchant.com/','foreign')").bind(link).bind(company).bind(store).bind(ad).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    for (kind, id) in [("campaigns", campaign), ("ads", ad), ("links", link)] {
        let req =
            c.f.request(&format!("/v1/{kind}/{id}"))
                .header("x-company-id", c.f.upstream.company.0.to_string())
                .body(Body::empty())
                .unwrap();
        let (status, body) = response(&c.f.app, req).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(!body.to_string().contains("Foreign"));
    }
    let time = Utc::now().to_rfc3339();
    for payload in [
        json!({"store_id":c.store["id"],"ad_id":ad,"destination":"https://shop.merchant.com/","placement":"foreign"}),
        json!({"store_id":store,"ad_id":ad,"destination":"https://shop.merchant.com/","placement":"foreign"}),
    ] {
        assert_eq!(
            response(
                &c.f.app,
                write(&c.f, "POST", "/v1/links", &payload, Uuid::new_v4(), &time)
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
    }
    let query = format!("/v1/links?store_id={store}");
    assert_eq!(
        response(
            &c.f.app,
            c.f.request(&query)
                .header("x-company-id", c.f.upstream.company.0.to_string())
                .body(Body::empty())
                .unwrap()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(c.get().await.status(), StatusCode::FOUND);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM attribution_clicks WHERE company_id=$1")
            .bind(company)
            .fetch_one(&c.f._database.pool)
            .await
            .unwrap(),
        0
    );
    c.f.close().await;
}

#[tokio::test]
async fn attribution_redirect_tenant_admission_competing_requests_and_recovery() {
    let mut c = Catalog::new().await;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(16)
        .acquire_timeout(Duration::from_secs(2))
        .connect_with(c.f._database.options.clone())
        .await
        .unwrap();
    c.f.setup.store.pool = pool.clone();
    rebuild(&mut c.f);
    let mut lock = c.f._database.pool.begin().await.unwrap();
    sqlx::query("SELECT company_id FROM attribution_tenants WHERE company_id=$1 FOR UPDATE")
        .bind(c.f.upstream.company.0)
        .execute(&mut *lock)
        .await
        .unwrap();
    let mut admin = c.f._database.admin().await;
    let mut requests = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let app = c.f.app.clone();
        let req = public(c.code(), "GET");
        requests.spawn(async move { app.oneshot(req).await.unwrap().status() });
    }
    tokio::time::timeout(Duration::from_secs(1),async {
        loop {
            let waiting:i64=sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock'").fetch_one(&mut admin).await.unwrap();
            if waiting==8 {break;}tokio::task::yield_now().await;
        }
    }).await.expect("all eight real requests must wait on tenant transaction");
    let r = c.get().await;
    assert_eq!(r.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(r.headers()["retry-after"], "1");
    lock.commit().await.unwrap();
    while let Some(r) = requests.join_next().await {
        assert_eq!(r.unwrap(), StatusCode::FOUND);
    }
    assert_eq!(c.count().await, 8);
    assert_eq!(c.get().await.status(), StatusCode::FOUND);
    assert_eq!(c.count().await, 9);
    pool.close().await;
    c.f.close().await;
}
