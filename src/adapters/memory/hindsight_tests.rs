use std::time::Duration;

use secrecy::SecretString;
use serde_json::json;
use uuid::Uuid;

use super::*;
use crate::{
    adapters::memory::test_support::{
        concurrent_server, mock_server, request_body, request_line, scripted_server, uniform_server,
    },
    entities::memory::{
        MemoryPersistenceMode, MemoryScope, remote_memory_database_id, resolve_scopes,
    },
    services::memory_provider::MemoryRecallQuery,
};

const DATABASE_ID: &str = "mail-agents-company-0193abcd0193abcd0193abcd0193abcd";

fn test_provider(base_url: String, timeout: Duration) -> HindsightProvider {
    HindsightProvider::new(base_url, SecretString::from("test-key"), timeout, timeout).unwrap()
}

fn scope(collection: &str, weight: f32, scope: MemoryScope) -> ResolvedMemoryScope {
    ResolvedMemoryScope {
        scope,
        collection: collection.into(),
        weight,
    }
}

fn conversation() -> MemoryConversation {
    MemoryConversation::new(
        "memory_abc123".into(),
        "what is our refund window?",
        "30 days.",
    )
}

fn target(
    scope: MemoryScope,
    collection: &str,
    mode: MemoryPersistenceMode,
) -> MemoryPersistenceTarget {
    MemoryPersistenceTarget {
        scope,
        collection: collection.into(),
        custom_instructions: match mode {
            MemoryPersistenceMode::AudienceOnly => None,
            MemoryPersistenceMode::ScopeSpecificFacts => Some(scope.extraction_instructions()),
        },
    }
}

fn recall_body(rows: &[(&str, f64)]) -> String {
    json!({
        "results": rows
            .iter()
            .map(|(text, score)| json!({"id": *text, "text": *text, "scores": {"final": *score}}))
            .collect::<Vec<_>>()
    })
    .to_string()
}

#[test]
fn bank_ids_compose_the_company_namespace_with_the_scope() {
    assert_eq!(
        HindsightProvider::bank_id(DATABASE_ID, "company").unwrap(),
        format!("{DATABASE_ID}--company")
    );
    assert_eq!(
        HindsightProvider::namespace_prefix(DATABASE_ID).unwrap(),
        format!("{DATABASE_ID}--")
    );
}

#[test]
fn the_widest_bank_id_we_generate_stays_inside_the_bound() {
    // The user scope is the worst case: a sha256 collection under a company namespace. Hindsight
    // publishes no limit, so this pins the headroom we are actually relying on.
    let scopes = resolve_scopes(
        false,
        false,
        true,
        None,
        Some("a-very-long-sender-address@an-equally-long-domain.example.com"),
    );
    let widest = HindsightProvider::bank_id(
        &remote_memory_database_id(Uuid::new_v4()),
        &scopes.resolved[0].collection,
    )
    .unwrap();
    assert_eq!(widest.len(), 123);
    assert!(widest.len() <= MAX_HINDSIGHT_BANK_ID_BYTES);
}

#[test]
fn a_bank_id_over_the_bound_or_outside_the_path_charset_is_refused() {
    assert_eq!(
        HindsightProvider::bank_id(DATABASE_ID, &"c".repeat(MAX_HINDSIGHT_BANK_ID_BYTES)),
        Err(MemoryProviderError::RequestTooLarge)
    );
    // A collection that could escape into the URL path must never reach a request.
    assert_eq!(
        HindsightProvider::bank_id(DATABASE_ID, "../another-company--company"),
        Err(MemoryProviderError::InvalidIdentifier)
    );
    assert_eq!(
        HindsightProvider::bank_id("../elsewhere", "company"),
        Err(MemoryProviderError::InvalidIdentifier)
    );
}

#[tokio::test]
async fn provision_creates_the_company_bank_with_bearer_auth() {
    let (base_url, request) = mock_server(200, r#"{"bank_id":"x","name":"x"}"#).await;
    test_provider(base_url, Duration::from_secs(2))
        .provision(DATABASE_ID)
        .await
        .unwrap();

    let request = request.await.unwrap();
    assert_eq!(
        request_line(&request),
        format!("PUT /banks/{DATABASE_ID}--company HTTP/1.1")
    );
    assert!(
        request
            .to_ascii_lowercase()
            .contains("authorization: bearer test-key")
    );
}

#[tokio::test]
async fn readiness_follows_the_company_bank_existing() {
    let (base_url, request) = mock_server(200, r#"{"bank_id":"x"}"#).await;
    assert!(
        test_provider(base_url, Duration::from_secs(2))
            .is_ready(DATABASE_ID)
            .await
            .unwrap()
    );
    assert_eq!(
        request_line(&request.await.unwrap()),
        format!("GET /banks/{DATABASE_ID}--company/config HTTP/1.1")
    );

    let (base_url, _) = mock_server(404, r#"{"detail":"not found"}"#).await;
    assert!(
        !test_provider(base_url, Duration::from_secs(2))
            .is_ready(DATABASE_ID)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn recall_calls_each_scope_bank_and_ranks_across_them() {
    let scopes = [
        scope("company", 1.0, MemoryScope::Company),
        scope("user_9f", 3.0, MemoryScope::User),
    ];
    // The company row wins on raw score and loses on scope weight.
    let (base_url, mut requests) = scripted_server(vec![
        (200, recall_body(&[("company-fact", 0.9)])),
        (200, recall_body(&[("user-fact", 0.5)])),
    ])
    .await;

    let chunks = test_provider(base_url, Duration::from_secs(2))
        .recall(
            DATABASE_ID,
            &MemoryRecallQuery::new("refund window"),
            &scopes,
            MemoryRecallMode::Fast,
            5,
            None,
        )
        .await
        .unwrap();

    assert_eq!(
        chunks
            .iter()
            .map(|chunk| (chunk.content.as_str(), chunk.source_scope))
            .collect::<Vec<_>>(),
        [
            ("user-fact", MemoryScope::User),
            ("company-fact", MemoryScope::Company),
        ]
    );

    let first = requests.recv().await.unwrap();
    assert_eq!(
        request_line(&first),
        format!("POST /banks/{DATABASE_ID}--company/memories/recall HTTP/1.1")
    );
    assert_eq!(request_body(&first)["budget"], "low");
    assert_eq!(request_body(&first)["query"], "refund window");
    let second = requests.recv().await.unwrap();
    assert_eq!(
        request_line(&second),
        format!("POST /banks/{DATABASE_ID}--user_9f/memories/recall HTTP/1.1")
    );
}

#[tokio::test]
async fn thinking_recall_asks_for_the_larger_budget_and_carries_extra_context() {
    let (base_url, request) = mock_server(200, r#"{"results":[]}"#).await;
    test_provider(base_url, Duration::from_secs(2))
        .recall(
            DATABASE_ID,
            &MemoryRecallQuery::new("refund window"),
            &[scope("company", 1.0, MemoryScope::Company)],
            MemoryRecallMode::Thinking,
            5,
            Some(&MemoryAdditionalContext::new("thread is about EU orders")),
        )
        .await
        .unwrap();

    let body = request_body(&request.await.unwrap());
    assert_eq!(body["budget"], "high");
    // Recall has no context field of its own, so it rides on the query.
    assert_eq!(body["query"], "refund window\n\nthread is about EU orders");
}

#[tokio::test]
async fn a_scope_bank_that_does_not_exist_yet_contributes_nothing() {
    let (base_url, _) = scripted_server(vec![
        (200, recall_body(&[("company-fact", 0.9)])),
        (404, r#"{"detail":"bank not found"}"#.into()),
    ])
    .await;

    let chunks = test_provider(base_url, Duration::from_secs(2))
        .recall(
            DATABASE_ID,
            &MemoryRecallQuery::new("refund window"),
            &[
                scope("company", 1.0, MemoryScope::Company),
                scope("user_9f", 3.0, MemoryScope::User),
            ],
            MemoryRecallMode::Fast,
            5,
            None,
        )
        .await
        .unwrap();

    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].source_scope, MemoryScope::Company);
}

#[tokio::test]
async fn recall_rejects_a_bank_returning_more_rows_than_the_row_bound() {
    let rows = (0..=MAX_MEMORY_RETURNED_ROWS)
        .map(|index| (format!("row-{index}"), 0.5))
        .collect::<Vec<_>>();
    let body = recall_body(
        &rows
            .iter()
            .map(|(text, score)| (text.as_str(), *score))
            .collect::<Vec<_>>(),
    );
    let (base_url, _) = scripted_server(vec![(200, body)]).await;

    assert_eq!(
        test_provider(base_url, Duration::from_secs(2))
            .recall(
                DATABASE_ID,
                &MemoryRecallQuery::new("q"),
                &[scope("company", 1.0, MemoryScope::Company)],
                MemoryRecallMode::Fast,
                5,
                None,
            )
            .await,
        Err(MemoryProviderError::TooManyResults)
    );
}

#[tokio::test]
async fn recall_refuses_more_scopes_than_the_target_bound_without_calling_out() {
    let scopes = (0..=MAX_MEMORY_TARGET_COLLECTIONS)
        .map(|index| scope(&format!("agent_{index}"), 1.0, MemoryScope::Company))
        .collect::<Vec<_>>();
    // No server: exceeding the bound must be decided before any request is made.
    assert_eq!(
        test_provider("http://127.0.0.1:1".into(), Duration::from_millis(50))
            .recall(
                DATABASE_ID,
                &MemoryRecallQuery::new("q"),
                &scopes,
                MemoryRecallMode::Fast,
                5,
                None,
            )
            .await,
        Err(MemoryProviderError::TooManyTargets)
    );
}

#[tokio::test]
async fn persist_retains_asynchronously_under_a_stable_document_id() {
    let (base_url, request) = mock_server(200, r#"{"success":true,"items_count":1}"#).await;
    let results = test_provider(base_url, Duration::from_secs(2))
        .persist(
            DATABASE_ID,
            &[target(
                MemoryScope::Company,
                "company",
                MemoryPersistenceMode::AudienceOnly,
            )],
            &conversation(),
        )
        .await;
    assert_eq!(results, vec![Ok(())]);

    let request = request.await.unwrap();
    assert_eq!(
        request_line(&request),
        format!("POST /banks/{DATABASE_ID}--company/memories HTTP/1.1")
    );
    let body = request_body(&request);
    assert_eq!(body["async"], true);
    assert_eq!(body["operation_id"], "memory_abc123");
    assert_eq!(body["items"][0]["document_id"], "memory_abc123");
    assert_eq!(
        body["items"][0]["content"],
        "what is our refund window?\n\n30 days."
    );
    assert_eq!(body["items"][0]["tags"], json!(["Company"]));
    // Audience-only persistence constrains nothing about extraction.
    assert!(body["items"][0].get("context").is_none());
}

#[tokio::test]
async fn scope_specific_persistence_sends_the_scope_instructions_as_extraction_context() {
    let (base_url, request) = mock_server(200, r#"{"success":true,"items_count":1}"#).await;
    test_provider(base_url, Duration::from_secs(2))
        .persist(
            DATABASE_ID,
            &[target(
                MemoryScope::User,
                "user_9f",
                MemoryPersistenceMode::ScopeSpecificFacts,
            )],
            &conversation(),
        )
        .await;

    let body = request_body(&request.await.unwrap());
    assert_eq!(
        body["items"][0]["context"],
        MemoryScope::User.extraction_instructions()
    );
}

#[tokio::test]
async fn persist_creates_a_missing_bank_and_retries_exactly_once() {
    let (base_url, mut requests) = scripted_server(vec![
        (404, r#"{"detail":"bank not found"}"#.into()),
        (200, r#"{"bank_id":"x","name":"x"}"#.into()),
        (200, r#"{"success":true,"items_count":1}"#.into()),
    ])
    .await;

    let results = test_provider(base_url, Duration::from_secs(2))
        .persist(
            DATABASE_ID,
            &[target(
                MemoryScope::User,
                "user_9f",
                MemoryPersistenceMode::AudienceOnly,
            )],
            &conversation(),
        )
        .await;
    assert_eq!(results, vec![Ok(())]);

    let bank = format!("{DATABASE_ID}--user_9f");
    let lines = [
        requests.recv().await.unwrap(),
        requests.recv().await.unwrap(),
        requests.recv().await.unwrap(),
    ];
    assert_eq!(
        lines
            .iter()
            .map(|request| request_line(request))
            .collect::<Vec<_>>(),
        [
            format!("POST /banks/{bank}/memories HTTP/1.1"),
            format!("PUT /banks/{bank} HTTP/1.1"),
            format!("POST /banks/{bank}/memories HTTP/1.1"),
        ]
    );
    // Exactly one retry: a fourth request would mean the 404 path can loop.
    assert!(requests.try_recv().is_err());
}

#[tokio::test]
async fn persist_reports_a_refused_item_without_echoing_the_response() {
    let (base_url, _) = mock_server(
        200,
        r#"{"success":false,"items_count":0,"detail":"quota exhausted for key sk-live-secret"}"#,
    )
    .await;
    let results = test_provider(base_url, Duration::from_secs(2))
        .persist(
            DATABASE_ID,
            &[target(
                MemoryScope::Company,
                "company",
                MemoryPersistenceMode::AudienceOnly,
            )],
            &conversation(),
        )
        .await;

    assert_eq!(results, vec![Err(MemoryProviderError::RejectedItem)]);
    assert!(
        !MemoryProviderError::RejectedItem
            .to_string()
            .contains("sk-live-secret")
    );
}

#[tokio::test]
async fn persistence_refuses_more_than_the_aggregate_target_budget() {
    let targets = (0..=MAX_MEMORY_TARGET_COLLECTIONS)
        .map(|index| {
            target(
                MemoryScope::Company,
                &format!("company_{index}"),
                MemoryPersistenceMode::AudienceOnly,
            )
        })
        .collect::<Vec<_>>();
    let results = test_provider("http://127.0.0.1:1".into(), Duration::from_millis(50))
        .persist(DATABASE_ID, &targets, &conversation())
        .await;

    assert_eq!(results.len(), targets.len());
    assert!(
        results
            .iter()
            .all(|result| result == &Err(MemoryProviderError::TooManyTargets))
    );
}

#[tokio::test]
async fn scope_persistence_is_concurrent_rather_than_serialized() {
    let (base_url, mut sizes) = concurrent_server(
        MAX_MEMORY_TARGET_COLLECTIONS,
        r#"{"success":true,"items_count":1}"#,
    )
    .await;
    let targets = [
        target(
            MemoryScope::Company,
            "company",
            MemoryPersistenceMode::AudienceOnly,
        ),
        target(
            MemoryScope::Agent(Uuid::nil()),
            "agent_x",
            MemoryPersistenceMode::AudienceOnly,
        ),
        target(
            MemoryScope::User,
            "user_9f",
            MemoryPersistenceMode::AudienceOnly,
        ),
    ];

    // The server holds every connection until all three have arrived, so this only completes if
    // the three calls were in flight together.
    let results = test_provider(base_url, Duration::from_secs(5))
        .persist(DATABASE_ID, &targets, &conversation())
        .await;
    assert!(results.iter().all(Result::is_ok));
    for _ in 0..MAX_MEMORY_TARGET_COLLECTIONS {
        assert!(sizes.recv().await.unwrap() > 0);
    }
}

#[tokio::test]
async fn delete_removes_only_the_banks_of_the_company_it_was_asked_about() {
    let other = "mail-agents-company-ffffffffffffffffffffffffffffffff";
    let listing = json!({
        "banks": [
            {"bank_id": format!("{DATABASE_ID}--company")},
            {"bank_id": format!("{DATABASE_ID}--user_9f")},
            // `q` is a substring filter, so another company's bank can legitimately come back
            // here. The prefix check, not the server, is what keeps it safe.
            {"bank_id": format!("{other}--company-{DATABASE_ID}--company")},
        ],
        "total": 3,
        "limit": 100,
        "offset": 0
    })
    .to_string();
    let (base_url, mut requests) = scripted_server(vec![
        (200, listing),
        (200, r#"{"success":true,"deleted_count":4}"#.into()),
        (200, r#"{"success":true,"deleted_count":2}"#.into()),
        (200, r#"{"success":true,"deleted_count":0}"#.into()),
    ])
    .await;

    test_provider(base_url, Duration::from_secs(2))
        .delete(DATABASE_ID)
        .await
        .unwrap();

    let mut seen = Vec::new();
    while let Ok(request) = requests.try_recv() {
        seen.push(request_line(&request).to_owned());
    }
    assert!(seen[0].starts_with("GET /banks?q="));
    assert_eq!(
        &seen[1..],
        [
            format!("DELETE /banks/{DATABASE_ID}--company HTTP/1.1"),
            format!("DELETE /banks/{DATABASE_ID}--user_9f HTTP/1.1"),
            // The anchor bank again, unconditionally, in case the listing missed it.
            format!("DELETE /banks/{DATABASE_ID}--company HTTP/1.1"),
        ]
    );
    assert!(!seen.iter().any(|line| line.contains(other)));
}

#[tokio::test]
async fn delete_walks_every_page_of_a_company_listing() {
    let page = |ids: Vec<String>, total: usize| {
        json!({
            "banks": ids.iter().map(|id| json!({"bank_id": id})).collect::<Vec<_>>(),
            "total": total,
            "limit": 100,
            "offset": 0
        })
        .to_string()
    };
    let first: Vec<String> = (0..100)
        .map(|index| format!("{DATABASE_ID}--agent_{index:032x}"))
        .collect();
    let second = vec![format!("{DATABASE_ID}--company")];
    let mut responses = vec![(200, page(first, 101)), (200, page(second, 101))];
    responses.extend((0..102).map(|_| (200, r#"{"success":true}"#.to_string())));
    let (base_url, mut requests) = scripted_server(responses).await;

    test_provider(base_url, Duration::from_secs(5))
        .delete(DATABASE_ID)
        .await
        .unwrap();

    let mut listings = 0;
    let mut deletes = 0;
    while let Ok(request) = requests.try_recv() {
        if request_line(&request).starts_with("GET /banks?") {
            listings += 1;
        } else {
            deletes += 1;
        }
    }
    assert_eq!(listings, 2);
    assert_eq!(
        deletes, 102,
        "101 listed banks plus the unconditional anchor"
    );
}

#[tokio::test]
async fn a_provider_whose_configuration_cannot_be_honoured_is_unavailable_not_a_panic() {
    assert_eq!(
        HindsightProvider::new(
            "https://api.hindsight.vectorize.io/v1/default",
            SecretString::from(String::new()),
            Duration::from_secs(2),
            Duration::from_secs(2),
        )
        .err(),
        Some(MemoryProviderError::Unavailable)
    );
    assert_eq!(
        HindsightProvider::new(
            "https://api.hindsight.vectorize.io/v1/default",
            SecretString::from("k"),
            Duration::from_secs(30),
            Duration::from_secs(5),
        )
        .err(),
        Some(MemoryProviderError::Unavailable)
    );
}

#[tokio::test]
async fn every_call_is_reported_into_the_shared_activity_tally() {
    let activity = MemoryProviderActivity::default();
    let (base_url, _) = uniform_server(2, 500, r#"{"detail":"boom"}"#).await;
    let provider = test_provider(base_url, Duration::from_secs(2)).with_activity(activity.clone());

    assert!(provider.provision(DATABASE_ID).await.is_err());
    assert!(provider.is_ready(DATABASE_ID).await.is_err());

    let interval = activity.drain();
    assert_eq!(interval.calls, 2);
    assert_eq!(interval.failures, 2);
}
