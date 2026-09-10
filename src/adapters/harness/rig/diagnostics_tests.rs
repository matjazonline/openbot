use super::{protocol, providers, test_support::*};
use crate::services::{
    harness::runs::*,
    test_support::{ScriptedExchange, ScriptedResponse, scripted_scenario},
};
use rig::completion::{CompletionRequest, Message};
use serde_json::json;
use std::sync::{Arc, Mutex};
use tracing::instrument::WithSubscriber;

#[derive(Clone, Default)]
struct Logs(Arc<Mutex<Vec<u8>>>);
impl std::io::Write for Logs {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn request() -> CompletionRequest {
    CompletionRequest {
        model: None,
        preamble: Some("private-system-sentinel".into()),
        chat_history: vec![Message::user("private-prompt-sentinel")],
        documents: vec![],
        tools: vec![],
        temperature: None,
        max_tokens: Some(100),
        tool_choice: None,
        additional_params: None,
        output_schema: None,
        // Even callers opting in cannot bypass the provider boundary.
        record_telemetry_content: true,
    }
}

#[tokio::test]
async fn every_provider_omits_wire_content_and_errors_even_at_trace_level() {
    let registry = providers::ProviderRegistry::standard().unwrap();
    for provider in KEYS {
        let logs = Logs::default();
        let writer = logs.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_writer(move || writer.clone())
            .without_time()
            .finish();
        let mut reply = response(provider, Reply::Text);
        // Response metadata is also untrusted; the provider's own telemetry must not expose it.
        reply["model"] = json!("private-model-sentinel");
        reply["modelVersion"] = json!("private-model-sentinel");
        let mut error = ScriptedResponse::json(
            json!({"error":{"message":"private-error-sentinel private-key-sentinel","type":"api_error"}}),
        );
        error.status = 400;
        let llm = scripted_scenario(vec![
            ScriptedExchange::new(|_| Ok(()), ScriptedResponse::json(reply)),
            ScriptedExchange::new(|_| Ok(()), error),
        ])
        .await;
        let model = model(&registry, provider, "private-key-sentinel", &llm.base_url);
        async {
            providers::complete(&model, request()).await.unwrap();
            let error = providers::complete(&model, request()).await.unwrap_err();
            assert!(error.to_string().contains("Rig provider request failed"));
            assert!(!error.to_string().contains("private-"));
            tracing::info!("application-trace-survives");
        }
        .with_subscriber(subscriber)
        .await;
        assert_eq!(llm.finish().await, Ok(2));
        let captured = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        assert!(captured.contains("application-trace-survives"));
        assert!(!captured.contains("private-"), "{provider}: {captured}");
    }
}

#[tokio::test]
async fn missing_and_partial_provider_usage_is_persisted_as_estimated_or_mixed() {
    let registry = providers::ProviderRegistry::standard().unwrap();
    for provider in KEYS {
        for partial in [false, true] {
            let mut reply = response(provider, Reply::Text);
            match provider {
                "google" => {
                    reply["usageMetadata"] = json!({"promptTokenCount":if partial {2} else {0},"candidatesTokenCount":0,"totalTokenCount":if partial {2} else {0}})
                }
                "openai" | "groq" => {
                    reply["usage"] = json!({"prompt_tokens":if partial {2} else {0},"completion_tokens":0,"total_tokens":if partial {2} else {0}})
                }
                _ => {
                    reply["usage"] = json!({"input_tokens":if partial {2} else {0},"output_tokens":0,"total_tokens":if partial {2} else {0}})
                }
            }
            let llm = scripted_scenario(vec![ScriptedExchange::new(
                |_| Ok(()),
                ScriptedResponse::json(reply),
            )])
            .await;
            let model = model(&registry, provider, "key", &llm.base_url);
            let response = providers::complete(&model, request()).await.unwrap();
            let reservation = ModelReservation {
                repair: None,
                request_id: ModelRequestId(uuid::Uuid::new_v4()),
                input_tokens: 100,
                output_tokens: 100,
            };
            let turn = protocol::capture(provider, &reservation, response).unwrap();
            let restored: SavedModelTurn =
                serde_json::from_value(serde_json::to_value(&turn).unwrap()).unwrap();
            assert_eq!(
                restored.token_usage_source,
                if partial {
                    TokenUsageSource::Mixed
                } else {
                    TokenUsageSource::Estimated
                }
            );
            assert_eq!(restored.input_tokens, if partial { 2 } else { 100 });
            assert_eq!(restored.output_tokens, 100);
            assert_eq!(llm.finish().await, Ok(1));
        }
    }
}
