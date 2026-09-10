use super::{
    providers::ProviderRegistry,
    test_support::{Reply, check_request, check_tools, lookup, model, response},
};
use crate::services::test_support::{ScriptedExchange, ScriptedResponse, scripted_scenario};
use rig::{agent::AgentBuilder, completion::Prompt};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[tokio::test]
async fn parallel_calls_preserve_explicit_ids_and_exactly_one_result_per_call() {
    let mut calls = response("openai", Reply::ToolCall);
    let message = &mut calls["choices"][0]["message"];
    message["content"] = json!("Checking both records.");
    let mut second = message["tool_calls"][0].clone();
    second["id"] = json!("call-second");
    message["tool_calls"].as_array_mut().unwrap().push(second);
    let llm = scripted_scenario(vec![
        ScriptedExchange::new(
            |request| {
                check_request(request, "openai", "key")?;
                check_tools(&request.body, "openai")
            },
            ScriptedResponse::json(calls),
        ),
        ScriptedExchange::new(
            |request| {
                check_request(request, "openai", "key")?;
                let messages = request.body["messages"].as_array().ok_or("batch history")?;
                let mut ids = Vec::new();
                for message in messages.iter().filter(|message| message["role"] == "tool") {
                    if message["content"] != "record-value" {
                        return Err("batch result content");
                    }
                    ids.push(message["tool_call_id"].as_str().ok_or("batch result id")?);
                }
                ids.sort_unstable();
                if ids != ["call-proof", "call-second"] {
                    return Err("duplicate/orphan/missing batch result");
                }
                let text_preserved = messages.iter().any(|message| {
                    message["role"] == "assistant"
                        && message["content"]
                            .to_string()
                            .contains("Checking both records.")
                });
                let call_count: usize = messages
                    .iter()
                    .filter(|message| message["role"] == "assistant")
                    .filter_map(|message| message["tool_calls"].as_array())
                    .map(Vec::len)
                    .sum();
                if !text_preserved || call_count != 2 {
                    return Err("mixed text/call history");
                }
                Ok(())
            },
            ScriptedResponse::json(response("openai", Reply::Text)),
        ),
    ])
    .await;
    let count = Arc::new(AtomicUsize::new(0));
    let registry = ProviderRegistry::standard().unwrap();
    let agent = AgentBuilder::from_model_handle(model(&registry, "openai", "key", &llm.base_url))
        .dynamic_tool(lookup(count.clone()))
        .build();
    let result = agent.prompt("Look up both records").max_turns(2).await;
    assert_eq!(llm.finish().await, Ok(2));
    assert_eq!(result.unwrap(), "verified");
    assert_eq!(count.load(Ordering::SeqCst), 2);
}
