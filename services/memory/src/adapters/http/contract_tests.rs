//! `memory_contract_` and `memory_mcp_` transport cases. No database: the store double holds
//! one credential, so these test the contract surface, not storage.
use super::{HttpSettings, MCP_BODY_LIMIT, openapi, router};
use crate::{
    adapters::mcp,
    application::{
        operations::{OPERATIONS, Surface, by_id},
        service::App,
        test_store::static_store,
    },
};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

fn settings(max_in_flight: usize, request_deadline: Duration) -> HttpSettings {
    HttpSettings {
        max_in_flight,
        request_deadline,
        mcp_allowed_hosts: vec!["localhost".into()],
        mcp_allowed_origins: vec![],
    }
}

fn service(max_in_flight: usize) -> (axum::Router, String) {
    slow_service(max_in_flight, Duration::from_secs(5), Duration::ZERO)
}

/// A router whose store delays every credential lookup by `delay`.
fn slow_service(
    max_in_flight: usize,
    deadline: Duration,
    delay: Duration,
) -> (axum::Router, String) {
    let (mut store, token) = static_store();
    store.delay = delay;
    let app = App::new(
        Arc::new(store),
        crate::application::test_embedder::fake_pool(),
    );
    let ready = Arc::new(AtomicBool::new(true));
    (
        router(
            app,
            ready,
            &settings(max_in_flight, deadline),
            CancellationToken::new(),
        ),
        token,
    )
}

/// Every contract example, keyed by operation id.
fn examples() -> std::collections::BTreeMap<String, Value> {
    std::fs::read_dir(manifest_dir().join("contract/examples"))
        .unwrap()
        .map(|entry| {
            let example: Value =
                serde_json::from_str(&std::fs::read_to_string(entry.unwrap().path()).unwrap())
                    .unwrap();
            (
                example["operation"].as_str().unwrap().to_owned(),
                example["request"].clone(),
            )
        })
        .collect()
}

async fn call(
    router: &axum::Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<String>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream");
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let response = router
        .clone()
        .oneshot(request.body(Body::from(body.unwrap_or_default())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 8 << 20).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn memory_contract_openapi_is_current() {
    let path = manifest_dir().join("openapi.json");
    let rendered = openapi::render();
    if std::env::var("MEMORY_UPDATE_OPENAPI").as_deref() == Ok("1") {
        std::fs::write(&path, &rendered).unwrap();
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        committed == rendered,
        "openapi.json is stale; regenerate with MEMORY_UPDATE_OPENAPI=1 scripts/focused.sh memory_contract_openapi"
    );
}

/// n1: a write's deadline is part of its idempotency identity (D5); an independent client must
/// learn from the published schema that a retry resends it unchanged.
#[test]
fn memory_contract_write_deadlines_publish_their_retry_identity() {
    let document: Value = serde_json::from_str(&openapi::render()).unwrap();
    let mut writes = 0;
    for spec in OPERATIONS.iter().filter(|s| s.surface == Surface::Client) {
        let root = (spec.request_root)().to_value();
        if root["properties"].get("idempotency_key").is_none() {
            continue;
        }
        writes += 1;
        let reference = document["paths"][spec.path]["post"]["requestBody"]["content"]
            ["application/json"]["schema"]["$ref"]
            .as_str()
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap()
            .to_owned();
        for description in [
            root["properties"]["expires_at"]["description"].as_str(),
            document["components"]["schemas"][&reference]["properties"]["expires_at"]
                ["description"]
                .as_str(),
        ] {
            let description = description.unwrap_or_default();
            assert!(
                description.contains("resend the original value unchanged")
                    && description.contains("`idempotency_conflict`"),
                "{}: {description}",
                spec.id
            );
        }
    }
    assert_eq!(writes, 9, "every write request carries a deadline");
}

#[test]
fn memory_contract_registry_covers_every_operation_on_both_transports() {
    let ids: BTreeSet<_> = OPERATIONS.iter().map(|spec| spec.id).collect();
    let paths: BTreeSet<_> = OPERATIONS.iter().map(|spec| spec.path).collect();
    assert_eq!(ids.len(), OPERATIONS.len());
    assert_eq!(paths.len(), OPERATIONS.len());
    let client: Vec<_> = OPERATIONS
        .iter()
        .filter(|spec| spec.surface == Surface::Client)
        .collect();
    for spec in &client {
        assert!(spec.path.starts_with("/v1/"), "{}", spec.id);
        assert!(spec.tool.is_some(), "{} has no MCP tool", spec.id);
        assert!(
            spec.grant.is_some() || spec.id == "credential.get",
            "{}",
            spec.id
        );
    }
    for spec in OPERATIONS
        .iter()
        .filter(|spec| spec.surface == Surface::Admin)
    {
        assert!(
            spec.path.starts_with("/admin/v1/") && spec.tool.is_none(),
            "{}",
            spec.id
        );
    }
    let tools: BTreeSet<_> = mcp::tools()
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect();
    assert_eq!(tools.len(), client.len());
    // The client operations with use cases in this release (01B, 02B, 03A, 03B, 04B, 06A): all of
    // them. A declared-but-unimplemented operation would answer not_implemented.
    let implemented: BTreeSet<_> = client
        .iter()
        .filter(|spec| spec.implemented)
        .map(|spec| spec.id)
        .collect();
    let expected: BTreeSet<_> = [
        "credential.get",
        "observations.save",
        "observations.search",
        "observations.timeline",
        "observations.fetch",
        "observations.correct",
        "observations.delete",
        "context.pack",
        "context.validate",
        "sources.record",
        "sessions.open",
        "sessions.close",
        "generations.request",
        "generations.status",
        "generations.cancel",
        "namespaces.purge",
    ]
    .into_iter()
    .collect();
    assert_eq!(implemented, expected);
    let document = openapi::document();
    for spec in OPERATIONS {
        let method = if spec.path == "/v1/credential" {
            "get"
        } else {
            "post"
        };
        assert_eq!(document["paths"][spec.path][method]["operationId"], spec.id);
    }
}

fn openapi_validator(component_ref: &Value) -> jsonschema::Validator {
    let mut document = openapi::document();
    document["$ref"] = component_ref["$ref"].clone();
    jsonschema::draft202012::new(&document).expect("OpenAPI component compiles")
}

fn mcp_validator(tool: &str) -> jsonschema::Validator {
    let tool = mcp::tools().into_iter().find(|t| t.name == tool).unwrap();
    jsonschema::draft202012::new(&Value::Object((*tool.input_schema).clone())).unwrap()
}

#[test]
fn memory_contract_examples_validate_against_openapi_and_mcp_schemas() {
    let mut checked = 0;
    let dir = manifest_dir().join("contract/examples");
    let document = openapi::document();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let example: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let id = example["operation"].as_str().unwrap();
        let spec =
            by_id(id).unwrap_or_else(|| panic!("{} names an unknown operation", path.display()));
        let request = &example["request"];
        (spec.parse)(request).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        if spec.path != "/v1/credential" {
            let reference = &document["paths"][spec.path]["post"]["requestBody"]["content"]["application/json"]
                ["schema"];
            let validator = openapi_validator(reference);
            assert!(
                validator.is_valid(request),
                "{} fails the OpenAPI schema",
                path.display()
            );
        }
        if let Some(tool) = spec.tool {
            assert!(
                mcp_validator(tool).is_valid(request),
                "{} fails the MCP schema",
                path.display()
            );
        }
        checked += 1;
    }
    let client: BTreeSet<_> = OPERATIONS
        .iter()
        .filter(|spec| spec.surface == Surface::Client)
        .map(|spec| spec.id.to_owned())
        .collect();
    let covered: BTreeSet<_> = examples().into_keys().collect();
    assert!(
        client.is_subset(&covered),
        "client operations without a contract example: {:?}",
        client.difference(&covered).collect::<Vec<_>>()
    );
    assert!(checked > client.len(), "contract examples are missing");
    // Invalid requests fail every validator: an out-of-range level, an unknown field and a
    // caller-supplied tenant field.
    let search = by_id("observations.search").unwrap();
    let reference = &document["paths"][search.path]["post"]["requestBody"]["content"]["application/json"]
        ["schema"];
    for bad in [
        json!({"namespaces": ["ns-a"], "read_ceiling": 10, "query": "x"}),
        json!({"namespaces": ["ns-a"], "read_ceiling": 3, "query": "x", "tenant": "other"}),
        json!({"namespaces": ["ns-a"], "query": "x"}),
    ] {
        assert!((search.parse)(&bad).is_err(), "{bad}");
        assert!(!openapi_validator(reference).is_valid(&bad), "{bad}");
        assert!(!mcp_validator("memory_search").is_valid(&bad), "{bad}");
    }
}

#[tokio::test]
async fn memory_contract_http_errors_are_explicit_and_unimplemented_is_never_success() {
    let (router, token) = service(64);
    // Health is unauthenticated; readiness checks the store.
    assert_eq!(
        call(&router, "GET", "/healthz", None, None).await.0,
        StatusCode::OK
    );
    assert_eq!(
        call(&router, "GET", "/readyz", None, None).await.0,
        StatusCode::OK
    );
    // Missing and malformed tokens are 401 with the error envelope.
    let (status, body) = call(&router, "GET", "/v1/credential", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "unauthenticated");
    assert_eq!(body["error"]["retryable"], false);
    assert!(body["request_id"].is_string());
    assert_eq!(
        call(&router, "GET", "/v1/credential", Some("msk_cli_x_y"), None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    // A client token cannot reach the admin surface.
    let (status, _) = call(
        &router,
        "POST",
        "/admin/v1/tenants/create",
        Some(&token),
        Some(r#"{"external_id":"x"}"#.into()),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // The implemented credential view.
    let (status, body) = call(&router, "GET", "/v1/credential", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["tenant"], "tenant-a");
    assert_eq!(body["grants"][0]["namespace"], "ns-a");
    assert_eq!(body["grants"][0]["read_max"], 6);
    // Every operation has a use case in this release (06A implemented purge); a declared one
    // without would answer 501 not_implemented, never a success.
    assert!(OPERATIONS.iter().all(|spec| spec.implemented));
    // Purge reaches its use case: a deadline more than 7 days ahead is refused before the store.
    let purge_body =
        r#"{"idempotency_key":"p1","expires_at":"2030-01-01T00:00:00Z","namespace":"ns-a"}"#;
    let (status, body) = call(
        &router,
        "POST",
        "/v1/namespaces/purge",
        Some(&token),
        Some(purge_body.into()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_request");
    let (status, body) = call(
        &router,
        "POST",
        "/v1/observations/search",
        Some(&token),
        Some(r#"{"namespaces":[]}"#.into()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_request");
    let (status, _) = call(
        &router,
        "POST",
        "/v1/observations/search",
        Some(&token),
        Some("not json".into()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // Oversized bodies are refused with the envelope (256 KiB standard, 4 MiB large).
    let big = format!(r#"{{"query":"{}"}}"#, "x".repeat(300 * 1024));
    let (status, body) = call(
        &router,
        "POST",
        "/v1/observations/search",
        Some(&token),
        Some(big.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["error"]["code"], "payload_too_large");
    let (status, _) = call(
        &router,
        "POST",
        "/v1/observations/save",
        Some(&token),
        Some(big),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "300 KiB is within the 4 MiB save limit"
    );
    // Every client operation must have a contract example: a missing one fails here instead of
    // being skipped. A declared-but-unimplemented client operation answers 501 for a valid
    // body on both transports; this release has none (06A implemented purge).
    let examples = examples();
    for spec in OPERATIONS.iter().filter(|s| s.surface == Surface::Client) {
        assert!(
            examples.contains_key(spec.id),
            "{} has no contract example",
            spec.id
        );
    }
    let unimplemented: Vec<_> = OPERATIONS
        .iter()
        .filter(|s| s.surface == Surface::Client && !s.implemented)
        .collect();
    assert!(unimplemented.is_empty());
    for spec in unimplemented {
        let example = examples
            .get(spec.id)
            .unwrap_or_else(|| panic!("{} has no contract example", spec.id));
        let (status, body) = call(
            &router,
            "POST",
            spec.path,
            Some(&token),
            Some(example.to_string()),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED, "{}: {body}", spec.id);
        assert_eq!(body["error"]["code"], "not_implemented");
        assert_eq!(body["error"]["details"]["operation"], spec.id);
        let (_, body) = rpc(
            &router,
            Some(&token),
            "tools/call",
            json!({"name": spec.tool.unwrap(), "arguments": example}),
        )
        .await;
        assert_eq!(body["result"]["isError"], true, "{}: {body}", spec.id);
        assert_eq!(
            body["result"]["structuredContent"]["error"]["code"], "not_implemented",
            "{}",
            spec.id
        );
    }
}

#[tokio::test]
async fn memory_contract_request_deadline_answers_timeout() {
    let (router, token) = slow_service(64, Duration::from_millis(100), Duration::from_secs(5));
    let (status, body) = call(&router, "GET", "/v1/credential", Some(&token), None).await;
    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(body["error"]["code"], "timeout");
    assert_eq!(body["error"]["retryable"], true);
}

#[tokio::test]
async fn memory_contract_capacity_bound_refuses_instead_of_queueing() {
    let (router, token) = service(0);
    let (status, body) = call(&router, "GET", "/v1/credential", Some(&token), None).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"]["code"], "capacity_exceeded");
    assert_eq!(body["error"]["retryable"], true);
    assert_eq!(
        call(&router, "GET", "/healthz", None, None).await.0,
        StatusCode::OK
    );
}

async fn rpc(
    router: &axum::Router,
    token: Option<&str>,
    method: &str,
    params: Value,
) -> (StatusCode, Value) {
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).to_string();
    call(router, "POST", "/mcp", token, Some(body)).await
}

#[tokio::test]
async fn memory_mcp_tools_mirror_http_with_identical_authority() {
    let (router, token) = service(64);
    // MCP authenticates exactly like /v1.
    let (status, body) = rpc(&router, None, "tools/list", json!({})).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "unauthenticated");
    let (status, body) = rpc(&router, Some(&token), "tools/list", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let names: BTreeSet<String> = body["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_owned())
        .collect();
    assert!(names.contains("memory_search") && names.contains("memory_credential"));
    // The same use case answers on both transports.
    let (_, body) = rpc(
        &router,
        Some(&token),
        "tools/call",
        json!({"name": "memory_credential", "arguments": {}}),
    )
    .await;
    assert_eq!(
        body["result"]["structuredContent"]["tenant"], "tenant-a",
        "{body}"
    );
    let args = json!({"idempotency_key": "p1", "expires_at": "2030-01-01T00:00:00Z",
                      "namespace": "ns-a"});
    let (_, body) = rpc(
        &router,
        Some(&token),
        "tools/call",
        json!({"name": "memory_purge_namespace", "arguments": args}),
    )
    .await;
    assert_eq!(body["result"]["isError"], true, "{body}");
    assert_eq!(
        body["result"]["structuredContent"]["error"]["code"], "invalid_request",
        "the purge tool reaches the same use case as HTTP"
    );
    // Generation status is implemented on both transports, with the same grant checks: a
    // ceiling above the grant is refused before the store is reached.
    let (_, body) = rpc(
        &router,
        Some(&token),
        "tools/call",
        json!({"name": "memory_generation_status", "arguments":
               {"namespace": "ns-a", "read_ceiling": 7,
                "operation_id": "0196f3a2-0000-7000-8000-000000000001"}}),
    )
    .await;
    assert_eq!(
        body["result"]["structuredContent"]["error"]["code"], "forbidden",
        "{body}"
    );
    let bad = json!({"namespaces": ["ns-a"], "read_ceiling": 10, "query": "x"});
    let (_, body) = rpc(
        &router,
        Some(&token),
        "tools/call",
        json!({"name": "memory_search", "arguments": bad}),
    )
    .await;
    assert_eq!(
        body["result"]["structuredContent"]["error"]["code"], "invalid_request",
        "{body}"
    );
    // A foreign Host header is refused (DNS-rebinding protection; MEMORY_MCP_ALLOWED_HOSTS).
    let request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::HOST, "evil.example")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(
            json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}).to_string(),
        ))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn memory_mcp_bounds_bodies_like_http() {
    let (router, token) = service(64);
    // Beyond the /mcp bound (the largest tool body plus JSON-RPC framing): 413 with the envelope.
    let huge = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
        "name": "memory_save", "arguments": {"pad": "x".repeat(MCP_BODY_LIMIT)}}})
    .to_string();
    let (status, body) = call(&router, "POST", "/mcp", Some(&token), Some(huge)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["error"]["code"], "payload_too_large");
    // Each tool is held to its own operation's limit, as on /v1: 300 KiB of arguments exceed
    // the 256 KiB search limit but fit the 4 MiB save limit.
    let big = "x".repeat(300 * 1024);
    let (_, body) = rpc(
        &router,
        Some(&token),
        "tools/call",
        json!({"name": "memory_search", "arguments": {"query": big}}),
    )
    .await;
    assert_eq!(body["result"]["isError"], true, "{body}");
    assert_eq!(
        body["result"]["structuredContent"]["error"]["code"],
        "payload_too_large"
    );
    let (_, body) = rpc(
        &router,
        Some(&token),
        "tools/call",
        json!({"name": "memory_save", "arguments": {"query": big}}),
    )
    .await;
    assert_eq!(
        body["result"]["structuredContent"]["error"]["code"], "invalid_request",
        "within the save limit the body is decoded (and refused as malformed)"
    );
}

#[tokio::test]
async fn memory_mcp_requests_and_tool_calls_have_a_deadline() {
    // The whole exchange, including authentication, is bounded like /v1.
    let (router, token) = slow_service(64, Duration::from_millis(100), Duration::from_secs(5));
    let (status, body) = rpc(&router, Some(&token), "tools/list", json!({})).await;
    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(body["error"]["code"], "timeout");
    // rmcp runs tool calls on a task of its own, so each call is also bounded where it runs:
    // an overrunning use case is dropped and answers the timeout envelope as a tool error.
    let request_id = uuid::Uuid::now_v7();
    let result = mcp::bounded(
        Duration::from_millis(20),
        request_id,
        std::future::pending::<Result<Value, crate::domain::error::ServiceError>>(),
    )
    .await;
    assert_eq!(result.is_error, Some(true));
    let content = result.structured_content.unwrap();
    assert_eq!(content["error"]["code"], "timeout");
    assert_eq!(content["request_id"], request_id.to_string());
}

#[test]
fn memory_contract_published_schemas_carry_the_d6_bounds() {
    let document = openapi::document();
    let schemas = &document["components"]["schemas"];
    assert_eq!(
        schemas["SaveRequest"]["properties"]["items"]["maxItems"],
        256
    );
    assert_eq!(schemas["SaveItem"]["properties"]["title"]["maxLength"], 256);
    assert_eq!(
        schemas["SaveItem"]["properties"]["body"]["maxLength"],
        16384
    );
    assert_eq!(
        schemas["SearchRequest"]["properties"]["namespaces"]["maxItems"],
        16
    );
    assert_eq!(
        schemas["SearchRequest"]["properties"]["query"]["maxLength"],
        1024
    );
    assert_eq!(schemas["FetchRequest"]["properties"]["ids"]["maxItems"], 64);
    assert_eq!(
        schemas["GenerationRequest"]["properties"]["events"]["maxItems"],
        512
    );
    assert_eq!(
        schemas["CredentialIssueRequest"]["properties"]["grants"]["maxItems"],
        64
    );
    let path_schema = |id: &str| {
        let spec = by_id(id).unwrap();
        document["paths"][spec.path]["post"]["requestBody"]["content"]["application/json"]["schema"]
            .clone()
    };
    let examples = examples();
    let search = examples["observations.search"].clone();
    let save = examples["observations.save"].clone();
    let mut cases = Vec::new();
    let mut bad = search.clone();
    bad["namespaces"] = json!((0..17).map(|i| format!("ns-{i}")).collect::<Vec<_>>());
    cases.push(("observations.search", bad));
    let mut bad = search.clone();
    bad["query"] = json!("q".repeat(1025));
    cases.push(("observations.search", bad));
    let mut bad = search.clone();
    bad["limit"] = json!(101);
    cases.push(("observations.search", bad));
    let mut bad = search;
    bad["namespaces"] = json!(["not/opaque"]);
    cases.push(("observations.search", bad));
    let mut bad = save.clone();
    let item = bad["items"][0].clone();
    bad["items"] = json!(vec![item; 257]);
    cases.push(("observations.save", bad));
    let mut bad = save.clone();
    bad["items"][0]["title"] = json!("t".repeat(257));
    cases.push(("observations.save", bad));
    let mut bad = save;
    bad["idempotency_key"] = json!("has space");
    cases.push(("observations.save", bad));
    for (id, request) in cases {
        let spec = by_id(id).unwrap();
        assert!(
            !openapi_validator(&path_schema(id)).is_valid(&request),
            "{id} OpenAPI accepts an out-of-bounds request"
        );
        assert!(
            !mcp_validator(spec.tool.unwrap()).is_valid(&request),
            "{id} MCP accepts an out-of-bounds request"
        );
    }
}

/// F2 (review p4-code-r1): a query within the 1,024-character bound but over the embedding
/// model's 500-token limit (1,024 CJK characters) is refused with 400, never truncated, on both
/// transports, before any storage work.
#[tokio::test]
async fn memory_contract_search_refuses_a_query_over_the_model_token_limit() {
    let (router, token) = service(4);
    let query = "記".repeat(1024);
    let body = json!({"namespaces": ["ns-a"], "read_ceiling": 4, "query": query});
    let (status, refused) = call(
        &router,
        "POST",
        "/v1/observations/search",
        Some(&token),
        Some(body.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    assert_eq!(refused["error"]["code"], "invalid_request");
    assert!(
        refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains("500-token limit")
    );
    let (_, by_tool) = rpc(
        &router,
        Some(&token),
        "tools/call",
        json!({"name": "memory_search", "arguments": body}),
    )
    .await;
    assert_eq!(
        by_tool["result"]["structuredContent"]["error"]["code"],
        "invalid_request"
    );
    // The published description tells clients to split such queries.
    let document = openapi::document();
    let described =
        document["components"]["schemas"]["SearchRequest"]["properties"]["query"]["description"]
            .as_str()
            .unwrap();
    assert!(described.contains("1,024 CJK characters") && described.contains("400"));
}
