use super::{
    catalog_tests::{Catalog, change, path, rebuild, wire_proxy},
    *,
};
use crate::adapters::commerce::worker;
use crate::application::commerce::*;
use axum::body::to_bytes;
use serde_json::{Value, json};

#[path = "../analytics/http_tests.rs"]
mod analytics_tests;
#[path = "e2e_tests.rs"]
mod e2e_tests;
#[path = "ingress_tests.rs"]
mod ingress_tests;
#[path = "ledger_tests.rs"]
mod ledger_tests;
#[path = "recovery_tests.rs"]
mod recovery_tests;
#[path = "../retention/restore_tests.rs"]
mod restore_tests;
#[path = "../retention/http_tests.rs"]
mod retention_tests;
#[path = "scope_tests.rs"]
mod scope_tests;

struct Commerce {
    c: Catalog,
    key: String,
    click: Uuid,
    checkout: String,
    created: chrono::DateTime<Utc>,
}
impl Commerce {
    async fn new() -> Self {
        Self::with_catalog(Catalog::new().await).await
    }
    async fn with_catalog(c: Catalog) -> Self {
        let key=change(&c.f,"POST","/v1/credentials",json!({"store_id":c.store["id"],"label":"Commerce","scopes":["checkouts:write","conversions:write"],"expires_at":null})).await["secret"].as_str().unwrap().to_owned();
        let response = c.get().await;
        let url = url::Url::parse(response.headers()["location"].to_str().unwrap()).unwrap();
        let click =
            Uuid::parse_str(&url.query_pairs().find(|(k, _)| k == "ob_click").unwrap().1).unwrap();
        let checkout = format!("checkout-{}", Uuid::new_v4());
        let created = Utc::now();
        let w = Self {
            c,
            key,
            click,
            checkout,
            created,
        };
        let (status,body)=w.merchant("/v1/checkouts",&json!({"checkout_id":w.checkout,"order_created_at":w.created.to_rfc3339(),"consent":true,"candidate_click_ids":[w.click]})).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["click_id"], json!(w.click));
        w
    }
    fn paid(&self) -> Value {
        let paid = Utc::now().to_rfc3339();
        json!({"kind":"paid","event_id":format!("event-{}",Uuid::new_v4()),"order_id":"order-1","occurred_at":paid,"order_created_at":self.created.to_rfc3339(),"paid_at":paid,"checkout_id":self.checkout,"currency":"EUR","lines":[{"line_id":"line-x","product_id":"product-x","quantity":2,"merchandise_amount":2400},{"line_id":"line-y","product_id":"product-y","quantity":1,"merchandise_amount":3500}],"merchandise_amount":5900,"tax_amount":1180,"shipping_amount":500})
    }
    fn refund(&self) -> Value {
        json!({"kind":"refund","event_id":format!("refund-event-{}",Uuid::new_v4()),"order_id":"order-1","refund_id":"refund-1","occurred_at":Utc::now().to_rfc3339(),"currency":"EUR","lines":[{"line_id":"line-x","returned_quantity":1,"merchandise_amount":1200}],"tax_amount":240,"shipping_amount":0})
    }
    async fn merchant(&self, path: &str, body: &Value) -> (StatusCode, Value) {
        response(
            self.c
                .f
                .app
                .clone()
                .oneshot(merchant_request(path, body, &self.key))
                .await
                .unwrap(),
        )
        .await
    }
    async fn event(&self, event: &Value) -> (StatusCode, Value) {
        self.merchant("/v1/conversions", event).await
    }
    async fn count(&self, table: &str) -> i64 {
        sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&self.c.f._database.pool)
            .await
            .unwrap()
    }
    async fn get(&self, path: &str) -> (StatusCode, Value) {
        response(
            self.c
                .f
                .app
                .clone()
                .oneshot(
                    self.c
                        .f
                        .request(path)
                        .header("x-company-id", self.c.f.upstream.company.0.to_string())
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await
    }
    fn worker(&self) -> crate::adapters::commerce::http::CommerceHttp {
        Fixture::commerce_http(&self.c.f.setup)
    }
}
fn merchant_request(path: &str, body: &Value, key: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(path)
        .header("authorization", format!("Bearer {key}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}
async fn response(response: axum::response::Response) -> (StatusCode, Value) {
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 2_000_000).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or_else(|_| json!({"body":"not json"})),
    )
}
#[tokio::test]
async fn attribution_order_paid_business_identity_immutable_lines_and_checkout() {
    let w = Commerce::new().await;
    let paid = w.paid();
    let (status, event) = w.event(&paid).await;
    assert_eq!(status, StatusCode::OK, "{event}");
    assert_eq!(event["state"], "accepted");
    let id = event["order_id"].as_str().unwrap();
    let (status, order) = w.get(&format!("/v1/orders/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(order["click_id"], json!(w.click));
    assert_eq!(order["ad_id"], w.c.ad["id"]);
    assert_eq!(order["attribution_reason"], "attributed");
    assert_eq!(order["merchandise_amount"], 5900);
    assert_eq!(order["lines"].as_array().unwrap().len(), 2);
    let mut second = paid.clone();
    second["event_id"] = json!("second-payment-delivery");
    second["occurred_at"] = json!(Utc::now().to_rfc3339());
    assert_eq!(w.event(&second).await.1["state"], "accepted");
    assert_eq!(w.count("attribution_orders").await, 1);
    let mut bad = second.clone();
    bad["event_id"] = json!("conflicting-payment");
    bad["lines"][0]["merchandise_amount"] = json!(2500);
    bad["merchandise_amount"] = json!(6000);
    let conflict = w.event(&bad).await.1;
    assert_eq!(conflict["state"], "conflict");
    assert_eq!(conflict["reason"], "paid_conflict");
    change(
        &w.c.f,
        "PATCH",
        &path("ads", &w.c.ad),
        json!({"name":"Later name","advertised_product_id":"changed"}),
    )
    .await;
    assert_eq!(w.c.get().await.status(), StatusCode::FOUND);
    assert_eq!(w.get(&format!("/v1/orders/{id}")).await.1, order);
    assert_eq!(w.count("attribution_merchant_events").await, 3);
    assert_eq!(
        w.get("/v1/orders").await.1["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        w.get("/v1/merchant-events?state=conflict").await.1["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    w.c.f.close().await;
}
#[tokio::test]
async fn attribution_merchant_ingress_competing_event_retries_and_conflicting_envelopes() {
    let w = Commerce::new().await;
    let event = w.paid();
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..4 {
        let app = w.c.f.app.clone();
        let request = merchant_request("/v1/conversions", &event, &w.key);
        tasks.spawn(async move { response(app.oneshot(request).await.unwrap()).await });
    }
    let mut expected = None;
    while let Some(result) = tasks.join_next().await {
        let (status, body) = result.unwrap();
        assert_eq!(status, StatusCode::OK, "{body}");
        if let Some(e) = &expected {
            assert_eq!(&body, e);
        } else {
            expected = Some(body);
        }
    }
    assert_eq!(w.count("attribution_merchant_events").await, 1);
    assert_eq!(w.count("attribution_orders").await, 1);
    assert_eq!(w.count("attribution_order_lines").await, 2);
    let mut reordered = event.clone();
    reordered["lines"].as_array_mut().unwrap().reverse();
    assert_eq!(w.event(&reordered).await.1, expected.unwrap());
    let mut bad = event.clone();
    bad["tax_amount"] = json!(1200);
    assert_eq!(w.event(&bad).await.0, StatusCode::CONFLICT);
    assert_eq!(w.count("attribution_merchant_events").await, 1);
    w.c.f.close().await;
}
#[tokio::test]
async fn attribution_refund_independent_business_identity_and_exact_balances() {
    let w = Commerce::new().await;
    let paid = w.event(&w.paid()).await.1;
    let order = paid["order_id"].as_str().unwrap();
    let refund = w.refund();
    let mut duplicate = refund.clone();
    duplicate["event_id"] = json!("second-refund-delivery");
    let mut tasks = tokio::task::JoinSet::new();
    for body in [refund.clone(), duplicate.clone()] {
        let app = w.c.f.app.clone();
        let request = merchant_request("/v1/conversions", &body, &w.key);
        tasks.spawn(async move { response(app.oneshot(request).await.unwrap()).await });
    }
    while let Some(result) = tasks.join_next().await {
        let (status, body) = result.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["state"], "accepted", "{body}");
    }
    assert_eq!(w.count("attribution_refunds").await, 1);
    assert_eq!(w.count("attribution_refund_lines").await, 1);
    let actual = w.get(&format!("/v1/orders/{order}")).await.1;
    assert_eq!(actual["refunded_merchandise_amount"], 1200);
    assert_eq!(actual["returned_units"], 1);
    assert_eq!(actual["refunded_tax_amount"], 240);
    assert_eq!(actual["refunded_shipping_amount"], 0);
    duplicate["event_id"] = json!("changed-refund-delivery");
    duplicate["lines"][0]["merchandise_amount"] = json!(1000);
    assert_eq!(w.event(&duplicate).await.1["reason"], "refund_conflict");
    duplicate["event_id"] = json!("cross-order-refund-delivery");
    duplicate["order_id"] = json!("another-order");
    assert_eq!(w.event(&duplicate).await.1["reason"], "refund_conflict");
    assert_eq!(
        w.get(&format!("/v1/orders/{order}/refunds")).await.1["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(w.get(&format!("/v1/orders/{order}")).await.1, actual);
    w.c.f.close().await;
}
#[tokio::test]
async fn attribution_reconciliation_refund_before_paid_and_cancellation_converge() {
    let w = Commerce::new().await;
    let paid = w.paid();
    let refund = w.refund();
    let (status, pending) = w.event(&refund).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(pending["state"], "pending");
    assert!(pending["order_id"].is_null());
    assert!(
        w.get("/v1/orders").await.1["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let cancel = json!({"kind":"cancelled","event_id":"cancel-first","order_id":"order-1","occurred_at":Utc::now().to_rfc3339()});
    assert_eq!(w.event(&cancel).await.1["state"], "accepted");
    let result = w.event(&paid).await.1;
    assert_eq!(result["state"], "accepted");
    sqlx::query("UPDATE attribution_merchant_events SET next_attempt_at=clock_timestamp() WHERE state='pending'").execute(&w.c.f._database.pool).await.unwrap();
    let claims = worker::claim(&w.c.f.setup.store).await.unwrap();
    assert_eq!(claims.len(), 1);
    assert_eq!(
        worker::process(&w.worker(), claims[0]).await.unwrap(),
        EventState::Accepted
    );
    assert_eq!(w.event(&refund).await.1["state"], "accepted");
    let order = w
        .get(&format!(
            "/v1/orders/{}",
            result["order_id"].as_str().unwrap()
        ))
        .await
        .1;
    assert_eq!(order["merchandise_amount"], 5900);
    assert_eq!(order["refunded_merchandise_amount"], 1200);
    assert_eq!(order["returned_units"], 1);
    assert_eq!(order["click_id"], json!(w.click));
    let mut late = cancel;
    late["event_id"] = json!("cancel-after-paid");
    assert_eq!(w.event(&late).await.1["state"], "accepted");
    assert_eq!(
        w.get(&format!(
            "/v1/orders/{}",
            result["order_id"].as_str().unwrap()
        ))
        .await
        .1,
        order
    );
    w.c.f.close().await;
}
