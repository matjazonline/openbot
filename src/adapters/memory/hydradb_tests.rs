use std::time::Duration;

use reqwest::StatusCode;
use secrecy::SecretString;

use super::*;
use crate::{
    adapters::memory::{
        http::classify_status,
        test_support::{
            concurrent_server, mock_server, raw_response_server, request_body, request_line,
        },
    },
    entities::memory::{
        MAX_MEMORY_PROVIDER_IDENTIFIER_BYTES, MAX_MEMORY_PROVIDER_RESPONSE_BYTES,
        MemoryProviderError, MemoryScope, ResolvedMemoryScope,
    },
    services::runtime_metrics::MemoryProviderActivity,
};

fn test_provider(base_url: String, timeout: Duration) -> HydraDbProvider {
    HydraDbProvider::new(base_url, SecretString::from("test-key"), timeout, timeout).unwrap()
}

fn company_scope() -> ResolvedMemoryScope {
    ResolvedMemoryScope {
        scope: MemoryScope::Company,
        collection: "company".into(),
        weight: 1.0,
    }
}

/// HydraDB ingests one collection per connection, so a concurrent fan-out needs one socket each.
async fn concurrent_ingest_server() -> (String, tokio::sync::mpsc::Receiver<usize>) {
    concurrent_server(
        MAX_MEMORY_TARGET_COLLECTIONS,
        r#"{"success":true,"data":{"results":[{"id":"stable-id","status":"queued"}],"success_count":1,"failed_count":0}}"#,
    )
    .await
}

#[test]
fn provider_errors_are_safe_and_classified() {
    assert_eq!(
        classify_status(StatusCode::UNAUTHORIZED),
        MemoryProviderError::Authentication
    );
    assert_eq!(
        classify_status(StatusCode::TOO_MANY_REQUESTS),
        MemoryProviderError::RateLimited
    );
    assert!(!MemoryProviderError::Authentication.retryable());
    assert!(MemoryProviderError::RateLimited.retryable());
    assert!(
        !MemoryProviderError::Authentication
            .to_string()
            .contains("secret")
    );
}

#[tokio::test]
async fn provision_uses_v2_bearer_auth_and_treats_conflict_as_idempotent() {
    let (base_url, request) = mock_server(409, "{}").await;
    let provider = HydraDbProvider::new(
        base_url,
        SecretString::from("test-key"),
        Duration::from_secs(2),
        Duration::from_secs(3),
    )
    .unwrap();

    provider.provision("company-memory").await.unwrap();
    let request = request.await.unwrap();
    assert!(request.starts_with("POST /databases HTTP/1.1"));
    assert!(request.to_ascii_lowercase().contains("api-version: 2"));
    assert!(
        request
            .to_ascii_lowercase()
            .contains("authorization: bearer test-key")
    );
    assert!(request.contains("company-memory"));
}

#[tokio::test]
async fn activity_wraps_successful_calls_and_bounded_failures() {
    let (base_url, _) = mock_server(409, "{}").await;
    let activity = MemoryProviderActivity::default();
    let provider = HydraDbProvider::new(
        base_url,
        SecretString::from("test-key"),
        Duration::from_secs(2),
        Duration::from_secs(3),
    )
    .unwrap()
    .with_activity(activity.clone());

    provider.provision("company-memory").await.unwrap();
    assert_eq!(
        provider
            .provision(&"x".repeat(MAX_MEMORY_PROVIDER_IDENTIFIER_BYTES + 1))
            .await,
        Err(MemoryProviderError::RequestTooLarge)
    );

    let interval = activity.drain();
    assert_eq!(interval.calls, 2);
    assert_eq!(interval.failures, 1);
}

#[tokio::test]
async fn persist_surfaces_per_item_rejection_without_echoing_the_response() {
    let (base_url, _) = mock_server(200, r#"{"success":true,"data":{"results":[{"id":"stable-id","status":"failed","error":"the provider said something we must not echo"}],"success_count":0,"failed_count":1}}"#).await;
    let provider = HydraDbProvider::new(
        base_url,
        SecretString::from("test-key"),
        Duration::from_secs(2),
        Duration::from_secs(3),
    )
    .unwrap();
    let results = provider
        .persist(
            "company-memory",
            &[MemoryPersistenceTarget {
                scope: crate::entities::memory::MemoryScope::Company,
                collection: "company".into(),
                custom_instructions: None,
            }],
            &MemoryConversation::new("stable-id".into(), "hello", "world"),
        )
        .await;
    assert_eq!(results, vec![Err(MemoryProviderError::RejectedItem)]);
}

#[tokio::test]
async fn persist_sends_scope_instructions_and_accepts_empty_extraction() {
    let (base_url, request) = mock_server(200, r#"{"success":true,"data":{"results":[{"id":"stable-id","status":"queued"}],"success_count":1,"failed_count":0}}"#).await;
    let provider = HydraDbProvider::new(
        base_url,
        SecretString::from("test-key"),
        Duration::from_secs(2),
        Duration::from_secs(3),
    )
    .unwrap();
    let instructions = crate::entities::memory::MemoryScope::User.extraction_instructions();
    let results = provider
        .persist(
            "company-memory",
            &[MemoryPersistenceTarget {
                scope: crate::entities::memory::MemoryScope::User,
                collection: "user_hash".into(),
                custom_instructions: Some(instructions),
            }],
            &MemoryConversation::new("stable-id".into(), "transient request", "no durable fact"),
        )
        .await;

    assert_eq!(results, vec![Ok(())]);
    let request = request.await.unwrap();

    // The v2 ingest contract: the tenant is `database`, the item kind is a form-level `type`, and
    // the payload is `memories`. The pre-v2 spellings (`database_id`, `items`) are not aliases —
    // they are simply unknown to the server — so their absence is the assertion that matters.
    assert!(request.contains("name=\"database\""), "{request}");
    assert!(request.contains("name=\"collection\""), "{request}");
    assert!(request.contains("name=\"type\""), "{request}");
    assert!(request.contains("name=\"memories\""), "{request}");
    assert!(!request.contains("name=\"database_id\""), "{request}");
    assert!(!request.contains("name=\"items\""), "{request}");

    // `custom_instructions` is a field of the memory item in v2, not of the request.
    assert!(
        !request.contains("name=\"custom_instructions\""),
        "{request}"
    );
    assert!(request.contains("user_assistant_pairs"), "{request}");
    assert!(request.contains(instructions), "{request}");
}

#[tokio::test]
async fn recall_requires_expected_collection_attribution() {
    let scope = ResolvedMemoryScope {
        scope: crate::entities::memory::MemoryScope::Company,
        collection: "company".into(),
        weight: 1.0,
    };
    let (base_url, _) = mock_server(
        200,
        r#"{"success":true,"data":{"chunks":[{"chunk_uuid":"one","chunk_content":"policy","collection":"company"}]}}"#,
    )
    .await;
    let provider = HydraDbProvider::new(
        base_url,
        SecretString::from("test-key"),
        Duration::from_secs(2),
        Duration::from_secs(3),
    )
    .unwrap();
    let chunks = provider
        .recall(
            "company-memory",
            &MemoryRecallQuery::new("query"),
            std::slice::from_ref(&scope),
            MemoryRecallMode::Fast,
            5,
            None,
        )
        .await
        .unwrap();
    assert_eq!(chunks[0].source_scope, scope.scope);

    let (base_url, _) = mock_server(
        200,
        r#"{"success":true,"data":{"chunks":[{"chunk_uuid":"one","chunk_content":"policy"}]}}"#,
    )
    .await;
    let provider = HydraDbProvider::new(
        base_url,
        SecretString::from("test-key"),
        Duration::from_secs(2),
        Duration::from_secs(3),
    )
    .unwrap();
    assert_eq!(
        provider
            .recall(
                "company-memory",
                &MemoryRecallQuery::new("query"),
                &[scope],
                MemoryRecallMode::Fast,
                5,
                None,
            )
            .await,
        Err(MemoryProviderError::MalformedResponse)
    );
}

#[test]
fn multipart_request_enforces_exact_byte_boundary_before_allocation() {
    let boundary = "fixed-boundary";
    let empty = HydraDbProvider::multipart_body_with_boundary(&[("memories", "")], boundary)
        .unwrap()
        .len();
    let exact_payload = "x".repeat(MAX_MEMORY_PROVIDER_REQUEST_BODY_BYTES - empty);
    let exact = HydraDbProvider::multipart_body_with_boundary(
        &[("memories", exact_payload.as_str())],
        boundary,
    )
    .unwrap();
    assert_eq!(exact.len(), MAX_MEMORY_PROVIDER_REQUEST_BODY_BYTES);

    let over_payload = "x".repeat(MAX_MEMORY_PROVIDER_REQUEST_BODY_BYTES - empty + 1);
    assert_eq!(
        HydraDbProvider::multipart_body_with_boundary(
            &[("memories", over_payload.as_str())],
            boundary,
        ),
        Err(MemoryProviderError::RequestTooLarge)
    );
}

#[tokio::test]
async fn response_accepts_absent_and_valid_content_length() {
    let absent = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"success\":true,\"data\":{\"infra\":{\"ready_for_ingestion\":true}}}".to_vec();
    let provider = test_provider(
        raw_response_server(absent, None).await,
        Duration::from_secs(2),
    );
    assert!(provider.is_ready("company-memory").await.unwrap());

    let (base_url, _) = mock_server(
        200,
        r#"{"success":true,"data":{"infra":{"ready_for_ingestion":true}}}"#,
    )
    .await;
    assert!(
        test_provider(base_url, Duration::from_secs(2))
            .is_ready("company-memory")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn oversized_content_length_is_rejected_without_reading_body() {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{{}}",
        MAX_MEMORY_PROVIDER_RESPONSE_BYTES + 1
    )
    .into_bytes();
    let provider = test_provider(
        raw_response_server(response, None).await,
        Duration::from_secs(2),
    );
    assert_eq!(
        provider.is_ready("company-memory").await,
        Err(MemoryProviderError::ResponseTooLarge)
    );
}

#[tokio::test]
async fn response_body_at_exact_byte_cap_is_accepted() {
    // A well-formed v2 envelope, padded with the trailing whitespace JSON permits, so this
    // exercises the byte cap rather than tripping over a body `v2_data` would reject anyway.
    let mut body = br#"{"data":{"infra":{"ready_for_ingestion":false}}}"#.to_vec();
    body.resize(MAX_MEMORY_PROVIDER_RESPONSE_BYTES, b' ');
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    response.extend_from_slice(&body);
    let provider = test_provider(
        raw_response_server(response, None).await,
        Duration::from_secs(2),
    );
    assert!(!provider.is_ready("company-memory").await.unwrap());
}

#[tokio::test]
async fn chunked_response_crossing_byte_cap_stops_with_typed_error() {
    let payload = vec![b'x'; MAX_MEMORY_PROVIDER_RESPONSE_BYTES + 1];
    let mut response = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec();
    response.extend_from_slice(format!("{:x}\r\n", payload.len()).as_bytes());
    response.extend_from_slice(&payload);
    response.extend_from_slice(b"\r\n0\r\n\r\n");
    let provider = test_provider(
        raw_response_server(response, None).await,
        Duration::from_secs(2),
    );
    assert_eq!(
        provider.is_ready("company-memory").await,
        Err(MemoryProviderError::ResponseTooLarge)
    );
}

#[tokio::test]
async fn never_ending_response_body_obeys_request_timeout() {
    let response = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n".to_vec();
    let provider = test_provider(
        raw_response_server(response, Some(Duration::from_secs(1))).await,
        Duration::from_millis(50),
    );
    assert_eq!(
        provider.is_ready("company-memory").await,
        Err(MemoryProviderError::Timeout)
    );
}

#[tokio::test]
async fn recall_rejects_excess_rows_and_caps_one_huge_unicode_chunk() {
    let scope = company_scope();
    let rows = (0..2)
        .map(
            |index| json!({"chunk_uuid": index.to_string(), "chunk_content": "x", "collection": "company"}),
        )
        .collect::<Vec<_>>();
    let body = serde_json::to_string(&json!({"success": true, "data": {"chunks": rows}})).unwrap();
    let leaked_body: &'static str = Box::leak(body.into_boxed_str());
    let (base_url, _) = mock_server(200, leaked_body).await;
    assert_eq!(
        test_provider(base_url, Duration::from_secs(2))
            .recall(
                "company-memory",
                &MemoryRecallQuery::new("query"),
                std::slice::from_ref(&scope),
                MemoryRecallMode::Fast,
                1,
                None,
            )
            .await,
        Err(MemoryProviderError::TooManyResults)
    );

    let content = "🦀".repeat(MAX_MEMORY_CHUNK_CHARS + 1);
    let body = serde_json::to_string(&json!({
        "success": true,
        "data": {"chunks": [{"chunk_uuid": "one", "chunk_content": content, "collection": "company"}]}
    }))
    .unwrap();
    let leaked_body: &'static str = Box::leak(body.into_boxed_str());
    let (base_url, _) = mock_server(200, leaked_body).await;
    let chunks = test_provider(base_url, Duration::from_secs(2))
        .recall(
            "company-memory",
            &MemoryRecallQuery::new("query"),
            &[scope],
            MemoryRecallMode::Fast,
            1,
            None,
        )
        .await
        .unwrap();
    assert_eq!(chunks[0].content.chars().count(), MAX_MEMORY_CHUNK_CHARS);
    assert!(chunks[0].truncated);
    assert!(
        chunks[0]
            .content
            .ends_with(crate::entities::memory::MEMORY_TRUNCATION_MARKER)
    );
}

#[tokio::test]
async fn persistence_rejects_more_than_the_aggregate_target_budget() {
    let provider = test_provider("http://127.0.0.1:1".into(), Duration::from_millis(50));
    let targets = (0..=MAX_MEMORY_TARGET_COLLECTIONS)
        .map(|index| MemoryPersistenceTarget {
            scope: crate::entities::memory::MemoryScope::Company,
            collection: format!("collection-{index}"),
            custom_instructions: None,
        })
        .collect::<Vec<_>>();
    let results = provider
        .persist(
            "company-memory",
            &targets,
            &MemoryConversation::new("id".into(), "user", "assistant"),
        )
        .await;
    assert_eq!(results.len(), targets.len());
    assert!(
        results
            .iter()
            .all(|result| *result == Err(MemoryProviderError::TooManyTargets))
    );
}

#[tokio::test]
async fn three_collection_persistence_is_concurrent_and_aggregate_bounded() {
    let (base_url, mut requests) = concurrent_ingest_server().await;
    let provider = test_provider(base_url, Duration::from_secs(2));
    let targets = (0..MAX_MEMORY_TARGET_COLLECTIONS)
        .map(|index| MemoryPersistenceTarget {
            scope: crate::entities::memory::MemoryScope::Company,
            collection: format!("collection-{index}"),
            custom_instructions: None,
        })
        .collect::<Vec<_>>();
    let results = provider
        .persist(
            "company-memory",
            &targets,
            &MemoryConversation::new("id".into(), "user", "assistant"),
        )
        .await;
    assert_eq!(results, vec![Ok(()); MAX_MEMORY_TARGET_COLLECTIONS]);

    let mut aggregate_request_bytes = 0usize;
    for _ in 0..MAX_MEMORY_TARGET_COLLECTIONS {
        aggregate_request_bytes += requests.recv().await.unwrap();
    }
    assert!(
        aggregate_request_bytes
            <= MAX_MEMORY_TARGET_COLLECTIONS
                * crate::entities::memory::MAX_MEMORY_PROVIDER_REQUEST_BYTES
    );
}

/// The v2 contract names the tenant `database` everywhere. `database_id` — what this adapter sent
/// before — is not a deprecated alias in the v2 schema, it is simply absent, so these three routes
/// would have been reaching the server with no tenant at all. Pin the spelling on every one.
#[tokio::test]
async fn every_route_names_the_tenant_the_way_v2_does() {
    let (base_url, request) = mock_server(
        200,
        r#"{"success":true,"data":{"infra":{"ready_for_ingestion":true}}}"#,
    )
    .await;
    assert!(
        test_provider(base_url, Duration::from_secs(2))
            .is_ready("company-memory")
            .await
            .unwrap()
    );
    let status = request.await.unwrap();
    assert!(
        request_line(&status).starts_with("GET /databases/status?database=company-memory "),
        "{status}"
    );

    let (base_url, request) = mock_server(200, r#"{"success":true,"data":{"chunks":[]}}"#).await;
    test_provider(base_url, Duration::from_secs(2))
        .recall(
            "company-memory",
            &MemoryRecallQuery::new("query"),
            &[company_scope()],
            MemoryRecallMode::Fast,
            5,
            None,
        )
        .await
        .unwrap();
    let query = request.await.unwrap();
    let body = request_body(&query);
    assert_eq!(body["database"], "company-memory");
    assert!(body.get("database_id").is_none(), "{body}");

    let (base_url, request) = mock_server(200, r#"{"success":true,"data":{}}"#).await;
    test_provider(base_url, Duration::from_secs(2))
        .delete("company-memory")
        .await
        .unwrap();
    let deleted = request.await.unwrap();
    assert!(
        request_line(&deleted).starts_with("DELETE /databases?database=company-memory "),
        "{deleted}"
    );
}

/// Readiness is a boolean inside `data.infra`, not a top-level status string. Reading the old
/// shape against a v2 server would report every database as never ready, and provisioning would
/// spin until its deadline.
#[tokio::test]
async fn readiness_comes_from_the_v2_infrastructure_block() {
    let (base_url, _) = mock_server(
        200,
        r#"{"success":true,"data":{"infra":{"ready_for_ingestion":false,"graph_status":true}}}"#,
    )
    .await;
    assert!(
        !test_provider(base_url, Duration::from_secs(2))
            .is_ready("company-memory")
            .await
            .unwrap()
    );

    // The pre-v2 body must not read as ready either — it carries no `data` at all.
    let (base_url, _) = mock_server(200, r#"{"status":"ready_for_ingestion"}"#).await;
    assert_eq!(
        test_provider(base_url, Duration::from_secs(2))
            .is_ready("company-memory")
            .await,
        Err(MemoryProviderError::MalformedResponse)
    );
}

/// A 200 that carries `success: false` is a refusal, and nothing from its `error` object may reach
/// the caller — the message can name a database or a credential.
#[tokio::test]
async fn an_unsuccessful_envelope_is_refused_without_echoing_its_error() {
    let (base_url, _) = mock_server(
        200,
        r#"{"success":false,"data":null,"error":{"code":"FORBIDDEN","message":"key sk-live-secret cannot touch acme-db"}}"#,
    )
    .await;
    let error = test_provider(base_url, Duration::from_secs(2))
        .is_ready("company-memory")
        .await
        .expect_err("an unsuccessful envelope");
    assert_eq!(error, MemoryProviderError::RejectedItem);
    let rendered = error.to_string();
    assert!(!rendered.contains("sk-live-secret"), "{rendered}");
    assert!(!rendered.contains("acme-db"), "{rendered}");
}

#[tokio::test]
#[ignore = "requires HYDRA_DB_* and HYDRA_DB_LIVE_DATABASE_ID"]
async fn live_provisioning_smoke_test() {
    let Ok(base_url) = std::env::var("HYDRA_DB_BASE_URL") else {
        return;
    };
    let Ok(api_key) = std::env::var("HYDRA_DB_API_KEY") else {
        return;
    };
    let Ok(database_id) = std::env::var("HYDRA_DB_LIVE_DATABASE_ID") else {
        return;
    };
    let provider = HydraDbProvider::new(
        base_url,
        SecretString::from(api_key),
        Duration::from_secs(10),
        Duration::from_secs(60),
    )
    .unwrap();
    provider.provision(&database_id).await.unwrap();
}
