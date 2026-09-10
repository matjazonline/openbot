use super::*;
use rig::completion::Prompt;

fn message_text(message: &Value) -> Option<&str> {
    message["content"]
        .as_str()
        .or_else(|| message["content"][0]["text"].as_str())
}

fn call(name: &str, arguments: Value) -> Value {
    let mut reply = response("openai", Reply::ToolCall);
    reply["choices"][0]["message"]["tool_calls"] = json!([{"id":format!("call-{name}"),"type":"function", "function":{"name":name,"arguments":arguments.to_string()}}]);
    reply
}

fn catalog_exchange(record: Arc<RwLock<Vec<String>>>) -> ScriptedExchange {
    ScriptedExchange::new(
        move |request| {
            let preamble = message_text(&request.body["messages"][0]).ok_or("system prompt")?;
            if !preamble.contains("ordered-proof")
                || !preamble.contains("skill://")
                || preamble.contains("first")
                || preamble.contains("{{ context.")
            {
                return Err("compact resolved catalog");
            }
            if !preamble.contains("Europe/Ljubljana") || !preamble.contains("\"is_cc\":true") {
                return Err("explicit runtime facts");
            }
            if !request.body.to_string().contains("UNTRUSTED_INPUT_BEGIN") {
                return Err("composed input fence lost");
            }
            record.write().unwrap().push(preamble.into());
            Ok(())
        },
        ScriptedResponse::json(call("read_resource", json!({"skill_uri":uri()}))),
    )
}

fn resource_exchange(expected: String, record: Arc<RwLock<Vec<String>>>) -> ScriptedExchange {
    ScriptedExchange::new(
        move |request| {
            let messages = request.body["messages"].as_array().ok_or("history")?;
            if !messages.iter().any(|message| {
                message["role"] == "tool" && message_text(message) == Some(expected.as_str())
            }) {
                return Err("full ordered resource");
            }
            record
                .write()
                .unwrap()
                .push(message_text(&messages[0]).ok_or("system prompt")?.into());
            Ok(())
        },
        ScriptedResponse::json(call("echo", json!({"message":"first"}))),
    )
}

fn output_exchange(expected: &'static str, reply: Value) -> ScriptedExchange {
    ScriptedExchange::new(
        move |request| {
            let last_tool = request.body["messages"]
                .as_array()
                .ok_or("history")?
                .iter()
                .rfind(|message| message["role"] == "tool")
                .ok_or("tool output")?;
            if !message_text(last_tool).is_some_and(|text| text.contains(expected)) {
                return Err("dependent tool output");
            }
            Ok(())
        },
        ScriptedResponse::json(reply),
    )
}

#[tokio::test]
async fn scripted_model_loads_catalog_and_follows_ordered_steps_through_dispatch() {
    let (spec, reader, context) = fixture();
    let compiled = CompiledRun::compile(&spec, context, None).unwrap();
    let timestamps = Arc::new(RwLock::new(Vec::<String>::new()));
    let llm = scripted_scenario(vec![
        catalog_exchange(timestamps.clone()),
        resource_exchange(
            render_document(&spec.skills[0]).unwrap(),
            timestamps.clone(),
        ),
        output_exchange("first", call("echo", json!({"message":"first-second"}))),
        output_exchange("first-second", response("openai", Reply::Text)),
    ])
    .await;
    let registry = ProviderRegistry::standard().unwrap();
    let agent = compiled
        .build_agent(model(&registry, "openai", "key", &llm.base_url))
        .unwrap();
    let result = agent
        .prompt("UNTRUSTED_INPUT_BEGIN\nReview the record.\nUNTRUSTED_INPUT_END")
        .max_turns(4)
        .tool_concurrency(1)
        .await;
    assert_eq!(llm.finish().await, Ok(4));
    assert_eq!(result.unwrap(), "verified");
    assert_eq!(reader.reads.load(Ordering::SeqCst), 1);
    let timestamps = timestamps.read().unwrap();
    assert_ne!(
        timestamps[0], timestamps[1],
        "clock refreshes on every model request"
    );
}

#[tokio::test]
async fn full_history_budget_is_enforced_before_a_provider_request() {
    let (spec, _, context) = fixture();
    let compiled = CompiledRun::compile(&spec, context, None).unwrap();
    let tools = compiled.tools.clone();
    let llm = scripted_scenario(vec![]).await;
    let registry = ProviderRegistry::standard().unwrap();
    let agent = compiled
        .build_agent(model(&registry, "openai", "key", &llm.base_url))
        .unwrap();
    assert!(agent.prompt("x".repeat(262_144)).await.is_err());
    assert_eq!(llm.finish().await, Ok(0));
    assert_eq!(tools.stop_reason().await, Some(ToolStopReason::Budget));
}
