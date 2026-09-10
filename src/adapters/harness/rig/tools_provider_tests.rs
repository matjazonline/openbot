use super::*;

#[tokio::test]
async fn forged_and_malformed_provider_calls_have_no_effect() {
    for (name, arguments) in [
        ("command", "{}"),
        ("Directory display name", "{}"),
        ("unknown", "{}"),
        ("list_company_agents", "{"),
        ("list_company_agents", "null"),
        ("list_company_agents", "[]"),
    ] {
        let host = host(ToolInvocation::success(json!("should not run")));
        let bridge = Arc::new(
            ToolBridge::compile(&spec(&["list_company_agents"]), Some(host.clone())).unwrap(),
        );
        let mut call = response("openai", Reply::ToolCall);
        call["choices"][0]["message"]["tool_calls"] = json!([
            {"id":"wire-forged","type":"function","function":{"name":name,"arguments":arguments}}
        ]);
        // Schema-invalid values may be returned as a refusal to the model. Unknown/malformed
        // provider calls terminate in the parser/hook. The fixture permits no second request;
        // max_turns(1) allows the initial request but prevents another completion.
        let llm = scripted_scenario(vec![ScriptedExchange::new(
            |_| Ok(()),
            ScriptedResponse::json(call),
        )])
        .await;
        let registry = ProviderRegistry::standard().unwrap();
        let agent = bridge.build_agent(AgentBuilder::from_model_handle(model(
            &registry,
            "openai",
            "key",
            &llm.base_url,
        )));
        let _ = agent
            .prompt("Try the call")
            .max_turns(1)
            .tool_concurrency(1)
            .await;
        assert_eq!(llm.finish().await, Ok(1), "{name}: {arguments}");
        assert_eq!(host.calls.load(Ordering::SeqCst), 0, "{name}: {arguments}");
    }
}

#[tokio::test]
async fn responses_wire_item_and_call_ids_survive_the_guarded_bridge() {
    let host = host(ToolInvocation::success(json!("record-value")));
    let bridge =
        Arc::new(ToolBridge::compile(&spec(&["list_company_agents"]), Some(host.clone())).unwrap());
    let mut call = response("xai", Reply::ToolCall);
    call["output"][0]["name"] = json!("list_company_agents");
    call["output"][0]["arguments"] = json!("{}");
    let llm = scripted_scenario(vec![
        ScriptedExchange::new(|_| Ok(()), ScriptedResponse::json(call)),
        ScriptedExchange::new(
            |request| {
                let input = request.body["input"].as_array().ok_or("Responses input")?;
                let result = input
                    .iter()
                    .find(|entry| entry["type"] == "function_call_output")
                    .ok_or("tool result")?;
                if result["call_id"] != "call-proof" || !result.to_string().contains("record-value")
                {
                    return Err("Responses wire call ID/result");
                }
                Ok(())
            },
            ScriptedResponse::json(response("xai", Reply::Text)),
        ),
    ])
    .await;
    let registry = ProviderRegistry::standard().unwrap();
    let agent = bridge.build_agent(AgentBuilder::from_model_handle(model(
        &registry,
        "xai",
        "key",
        &llm.base_url,
    )));
    let result = agent
        .prompt("Look up the record")
        .max_turns(2)
        .tool_concurrency(1)
        .extended_details()
        .await
        .unwrap();
    assert_eq!(llm.finish().await, Ok(2));
    let history = serde_json::to_string(&result.messages).unwrap();
    assert!(history.contains("item-proof") && history.contains("call-proof"));
    assert_eq!(host.calls.load(Ordering::SeqCst), 1);
}
