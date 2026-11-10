//! `memory_observation_` checks of the Anthropic adapter against a local fake Messages API on
//! loopback (no network, no paid call): the exact request sent, response parsing, and the D9
//! error mapping.
use super::*;
use crate::domain::generation::provider_schema;
use axum::{Json, Router, extract::State, http::HeaderMap, response::IntoResponse, routing::post};
use std::sync::{Arc, Mutex};

/// One scripted reply: status, optional `request-id`, body.
type Reply = (u16, Option<&'static str>, String);

#[derive(Clone, Default)]
struct Seen {
    requests: Arc<Mutex<Vec<(HeaderMap, Value)>>>,
    reply: Arc<Mutex<Vec<Reply>>>,
}

async fn messages(
    State(seen): State<Seen>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    seen.requests.lock().unwrap().push((headers, body));
    let (status, request_id, body) = seen.reply.lock().unwrap().remove(0);
    let mut response = (axum::http::StatusCode::from_u16(status).unwrap(), body).into_response();
    if let Some(id) = request_id {
        response
            .headers_mut()
            .insert("request-id", id.parse().unwrap());
    }
    response
}

async fn fake(replies: Vec<Reply>) -> (AnthropicProvider, Seen) {
    let seen = Seen::default();
    *seen.reply.lock().unwrap() = replies;
    let app = Router::new()
        .route("/v1/messages", post(messages))
        .with_state(seen.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await });
    let mut settings = AnthropicSettings::new(ApiKey::new("sk-ant-test-key".into()));
    settings.base_url = format!("http://{address}");
    (AnthropicProvider::new(settings).unwrap(), seen)
}

fn request() -> ProviderRequest {
    ProviderRequest {
        system: "system text".into(),
        user: r#"{"events":[]}"#.into(),
        schema: provider_schema(),
        max_output_tokens: 2048,
    }
}

fn error(kind: &str) -> String {
    json!({"type": "error", "error": {"type": kind, "message": "m"}}).to_string()
}

#[tokio::test]
async fn memory_observation_anthropic_adapter_sends_the_d9_request_and_parses_output() {
    let ok = json!({"type": "message", "stop_reason": "end_turn",
                    "content": [{"type": "text", "text": "{\"outcome\":"}, {"type": "text", "text": "\"x\"}"}],
                    "usage": {"input_tokens": 10, "cache_creation_input_tokens": 1,
                              "cache_read_input_tokens": 2, "output_tokens": 3}});
    let (provider, seen) = fake(vec![(200, Some("req_1"), ok.to_string())]).await;
    let response = provider.generate(&request()).await.unwrap();
    assert_eq!(response.stop, StopReason::EndTurn);
    assert_eq!(response.text, "{\"outcome\":\"x\"}");
    assert_eq!(response.usage.total(), 16);
    let (headers, body) = seen.requests.lock().unwrap().remove(0);
    assert_eq!(headers["x-api-key"], "sk-ant-test-key");
    assert_eq!(headers["anthropic-version"], ANTHROPIC_VERSION);
    assert_eq!(
        body,
        json!({"model": "claude-sonnet-5-5", "max_tokens": 2048, "system": "system text",
               "messages": [{"role": "user", "content": "{\"events\":[]}"}],
               "thinking": {"type": "between_tools"},
               "output_config": {"effort": "low",
                                 "format": {"type": "json_schema", "schema": provider_schema()}}})
    );
    for absent in [
        "temperature",
        "top_p",
        "top_k",
        "fallbacks",
        "tools",
        "tool_choice",
    ] {
        assert!(body.get(absent).is_none(), "{absent}");
    }
    assert!(!format!("{:?}", provider.settings).contains("sk-ant-test-key"));
    // `between_tools` is refused above `high`, so such an effort is refused at construction.
    for effort in ["xhigh", "max", ""] {
        let mut settings = AnthropicSettings::new(ApiKey::new("k".into()));
        settings.effort = effort.into();
        assert!(AnthropicProvider::new(settings).is_err(), "{effort}");
    }
    // `between_tools` is specific to claude-sonnet-5-5: any other model is refused
    // (review p6a-code-r2 n4).
    for model in ["claude-opus-5-5", "claude-haiku-4-5", ""] {
        let mut settings = AnthropicSettings::new(ApiKey::new("k".into()));
        settings.model = model.into();
        assert!(AnthropicProvider::new(settings).is_err(), "{model}");
    }
}

#[tokio::test]
async fn memory_observation_anthropic_errors_map_to_retry_terminal_or_unknown() {
    let (provider, _) = fake(vec![
        (529, Some("req_2"), error("overloaded_error")),
        (429, Some("req_3"), error("rate_limit_error")),
        (401, Some("req_4"), error("authentication_error")),
        (400, Some("req_5"), error("invalid_request_error")),
        // An unknown type is bounded to `provider_error` (a closed outcome and label set).
        (400, Some("req_9"), error("Weird-Type/With Spaces")),
        // Without the request id, or without the envelope, the outcome is unknown.
        (529, None, error("overloaded_error")),
        (502, Some("req_6"), "<html>bad gateway</html>".into()),
        (200, Some("req_7"), "not a message".into()),
        (
            200,
            Some("req_8"),
            json!({"stop_reason": "max_tokens", "content": []}).to_string(),
        ),
    ])
    .await;
    let expected = [
        Err(ProviderError::Rejected("overloaded_error".into())),
        Err(ProviderError::Rejected("rate_limit_error".into())),
        Err(ProviderError::Terminal("authentication_error".into())),
        Err(ProviderError::Terminal("invalid_request_error".into())),
        Err(ProviderError::Terminal("provider_error".into())),
        Err(ProviderError::Ambiguous),
        Err(ProviderError::Ambiguous),
        Err(ProviderError::Ambiguous),
    ];
    for want in expected {
        assert_eq!(provider.generate(&request()).await.map(|r| r.stop), want);
    }
    assert_eq!(
        provider.generate(&request()).await.unwrap().stop,
        StopReason::MaxTokens
    );
    // A closed port: nothing was sent.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let closed = listener.local_addr().unwrap();
    drop(listener);
    let mut settings = AnthropicSettings::new(ApiKey::new("k".into()));
    settings.base_url = format!("http://{closed}");
    let offline = AnthropicProvider::new(settings).unwrap();
    assert_eq!(
        offline.generate(&request()).await,
        Err(ProviderError::NotSent)
    );
}
