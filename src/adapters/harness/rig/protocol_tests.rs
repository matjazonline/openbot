use super::super::{
    providers::ProviderRegistry,
    test_support::{KEYS, Reply, check_request, check_result, model, response},
};
use super::*;
use crate::{
    entities::harness::HarnessKind,
    services::test_support::{ScriptedExchange, ScriptedResponse, scripted_scenario},
};
use rig::completion::{CompletionModel, CompletionRequest, ToolDefinition};
use serde_json::json;
use uuid::Uuid;

fn request(chat_history: Vec<Message>) -> CompletionRequest {
    CompletionRequest {
        model: None,
        preamble: Some("Fixture".into()),
        chat_history,
        documents: vec![],
        tools: vec![ToolDefinition {
            name: "lookup".into(),
            description: "Lookup fixture".into(),
            parameters: json!({"type":"object", "properties":{"key":{"type":"string"}}, "required":["key"]}),
        }],
        max_tokens: Some(100),
        temperature: None,
        tool_choice: None,
        additional_params: None,
        output_schema: None,
        record_telemetry_content: false,
    }
}

#[tokio::test]
async fn every_supported_provider_replays_call_and_item_ids_from_application_json() {
    let registry = ProviderRegistry::standard().unwrap();
    for provider in KEYS {
        let llm = scripted_scenario(vec![
            ScriptedExchange::new(
                move |request| check_request(request, provider, "tenant-key"),
                ScriptedResponse::json(response(provider, Reply::ToolCall)),
            ),
            ScriptedExchange::new(
                move |request| {
                    check_request(request, provider, "tenant-key")?;
                    check_replayed_result(&request.body, provider)
                },
                ScriptedResponse::json(response(provider, Reply::Text)),
            ),
        ])
        .await;
        let model = model(&registry, provider, "tenant-key", &llm.base_url);
        let response = model
            .completion(request(vec![Message::user("Look up record")]))
            .await
            .unwrap();
        let budget = ModelReservation {
            repair: None,
            request_id: ModelRequestId(Uuid::new_v4()),
            input_tokens: 100,
            output_tokens: 100,
        };
        let turn = capture(provider, &budget, response).unwrap();
        if provider == "xai" {
            assert_eq!(turn.calls[0].item_id.as_deref(), Some("item-proof"));
            assert_eq!(turn.calls[0].call_id, "call-proof");
        }
        let id = turn.calls[0].invocation_id;
        let mut saved = RunCheckpoint::new(
            RunIdentity {
                response_contract: None,
                company_id: Uuid::new_v4(),
                task_id: Uuid::new_v4(),
                agent_id: Uuid::new_v4(),
                harness: HarnessKind::Rig,
                provider: provider.into(),
                model: "fixture".into(),
                capability_fingerprint: "c".repeat(64),
            },
            "Look up record".into(),
            RigExecutionPolicy::default(),
        )
        .unwrap();
        saved = saved.apply(Mutation::Reserve(budget.clone())).unwrap().0;
        saved = saved.apply(Mutation::Model(turn)).unwrap().0;
        saved = saved.apply(Mutation::Prepare(id)).unwrap().0;
        saved = saved
            .apply(Mutation::Result(id, json!({"value":"record-value"})))
            .unwrap()
            .0;
        // Drop every runtime message. Only the application checkpoint crosses the restart seam.
        let restored: RunCheckpoint =
            serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        let response = model.completion(request(history(&restored).unwrap())).await;
        assert_eq!(llm.finish().await, Ok(2), "provider: {provider}");
        let response = response.unwrap();
        assert_eq!(
            capture(provider, &budget, response).unwrap().text,
            "verified"
        );
    }
}

fn check_replayed_result(body: &serde_json::Value, provider: &str) -> Result<(), &'static str> {
    let mut body = body.clone();
    let encoded = json!({"value":"record-value"}).to_string();
    if let Some(messages) = body["messages"].as_array_mut() {
        for message in messages
            .iter_mut()
            .filter(|message| message["role"] == "tool")
        {
            if message["content"] != encoded {
                return Err("persisted JSON result changed");
            }
            message["content"] = json!("record-value");
        }
    }
    if let Some(input) = body["input"].as_array_mut() {
        for item in input
            .iter_mut()
            .filter(|item| item["type"] == "function_call_output")
        {
            if item["output"] != encoded {
                return Err("persisted JSON result changed");
            }
            item["output"] = json!("record-value");
        }
    }
    check_result(&body, provider)
}
