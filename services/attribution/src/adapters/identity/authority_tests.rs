use super::{
    authority::{HttpAuthority, MAX_RESPONSE},
    crypto,
};
use crate::{application::identity::*, infra::identity::IdentityConfig};
use axum::{Router, extract::State, routing::get};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use std::sync::{Arc, RwLock};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

fn maximum_page(name_character: &str) -> (Vec<u8>, u16) {
    let contract: Value = serde_json::from_str(include_str!(
        "../../../contract/openbot-identity.openapi.json"
    ))
    .unwrap();
    let schemas = &contract["components"]["schemas"];
    let count = schemas["CompanyPage"]["properties"]["items"]["maxItems"]
        .as_u64()
        .unwrap();
    let name_length = schemas["CompanyAccess"]["properties"]["name"]["maxLength"]
        .as_u64()
        .unwrap();
    let cursor_length =
        schemas["CompanyPage"]["properties"]["next_cursor"]["anyOf"][0]["maxLength"]
            .as_u64()
            .unwrap();
    let items: Vec<_> = (0..count).map(|_| {
        let id = Uuid::new_v4();
        json!({"company_id":id,"name":name_character.repeat(name_length as usize),
            "permissions":["read","links:manage","integrations:manage"],
            "lifecycle":{"company_id":id,"version":i64::MAX,"state":"suspended","changed_at":"2026-11-16T12:34:56.123456789+01:00"}})
    }).collect();
    let bytes = serde_json::to_vec(
        &json!({"items":items,"next_cursor":"a".repeat(cursor_length as usize)}),
    )
    .unwrap();
    // CI must fail before another schema/encoding increase consumes the safety margin.
    assert!(
        bytes.len() <= MAX_RESPONSE * 4 / 5,
        "maximum schema page uses {} of {} response bytes; reconcile the wire contract before raising the cap",
        bytes.len(),
        MAX_RESPONSE
    );
    (bytes, count.try_into().unwrap())
}

#[tokio::test]
async fn attribution_company_access_maximum_unicode_page_and_hard_response_budget() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let payload = Arc::new(RwLock::new(Vec::<u8>::new()));
    let app = Router::new()
        .route(
            "/api/attribution/v1/companies",
            get(|State(body): State<Arc<RwLock<Vec<u8>>>>| async move {
                (
                    [("content-type", "application/json")],
                    body.read().unwrap().clone(),
                )
            }),
        )
        .with_state(payload.clone());
    let shutdown = CancellationToken::new();
    let stop = shutdown.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(stop.cancelled_owned())
            .await
    });
    let config = IdentityConfig::from_lookup(
        &url::Url::parse("https://attribution.example.test").unwrap(),
        |name| match name {
            "ATTRIBUTION_OPENBOT_ORIGIN" => Some(origin.clone()),
            "ATTRIBUTION_CLIENT_SECRET" => Some(URL_SAFE_NO_PAD.encode([1u8; 32])),
            "ATTRIBUTION_LIFECYCLE_SECRET" => Some(URL_SAFE_NO_PAD.encode([2u8; 32])),
            "ATTRIBUTION_SESSION_KEYS" => Some(
                json!({"active":1,"keys":[{"version":1,"key":URL_SAFE_NO_PAD.encode([3u8;32])}]})
                    .to_string(),
            ),
            _ => None,
        },
    )
    .unwrap();
    let authority = HttpAuthority::new(Arc::new(config)).unwrap();
    let grant = IdentityGrant(crypto::random());
    for character in ["🚀", "\u{0001}"] {
        let (bytes, limit) = maximum_page(character);
        *payload.write().unwrap() = bytes;
        let page = authority.companies(&grant, limit, None).await.unwrap();
        assert_eq!(page.items.len(), limit as usize);
        assert!(
            page.items
                .iter()
                .all(|company| company.name.starts_with(character))
        );
    }
    let (mut bytes, limit) = maximum_page("🚀");
    // Whitespace preserves valid JSON: removing the byte guard would make this pass.
    bytes.resize(MAX_RESPONSE + 1, b' ');
    *payload.write().unwrap() = bytes;
    assert!(matches!(
        authority.companies(&grant, limit, None).await,
        Err(Failure::Unavailable)
    ));
    shutdown.cancel();
    server.await.unwrap().unwrap();
}
