use super::super::{
    providers::ProviderRegistry,
    test_support::{Reply, model, response},
};
use super::*;
use crate::{
    entities::{
        harness::{HarnessConfig, HarnessKind, SubAgentScope},
        tool_catalogue::ALLOWED_BUILTIN_TOOL_IDS,
    },
    services::{
        harness::NativeToolSafety,
        test_support::{ScriptedExchange, ScriptedResponse, scripted_scenario},
    },
};
use async_trait::async_trait;
use rig::{agent::AgentBuilder, completion::Prompt};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

#[path = "tools_capability_tests.rs"]
mod capabilities;
#[path = "tools_provider_tests.rs"]
mod provider;

fn spec(ids: &[&str]) -> AgentCapabilitySpec {
    AgentCapabilitySpec {
        response_contract: None,
        harness: HarnessKind::Rig,
        name: "tools".into(),
        system_prompt: "test".into(),
        provider: "openai".into(),
        model: "model".into(),
        provider_base_url: None,
        skills: vec![],
        granted_tools: ids.iter().copied().map(ToolId::from).collect(),
        sub_agents: SubAgentScope::AllCompanySiblings,
        harness_config: HarnessConfig::empty(HarnessKind::Rig),
    }
}

fn correlation() -> ToolCorrelationId {
    ToolCorrelationId::parse("rig-call").unwrap()
}

struct Host {
    declarations: Vec<NativeToolDeclaration>,
    calls: AtomicUsize,
    result: ToolInvocation,
    delay: Duration,
}

fn host(result: ToolInvocation) -> Arc<Host> {
    Arc::new(Host {
        declarations: vec![NativeToolDeclaration {
            id: "list_company_agents".into(),
            name: "Directory display name",
            description: "Read the directory",
            input_schema: json!({"type":"object","properties":{}}),
            safety: NativeToolSafety {
                read_only: true,
                concurrency_safe: true,
                has_external_effect: false,
                requires_network: false,
                destructive: false,
                open_world: false,
                requires_approval_by_default: false,
                max_output_chars: 128,
                max_result_chars: 256,
            },
        }],
        calls: AtomicUsize::new(0),
        result,
        delay: Duration::ZERO,
    })
}

#[async_trait]
impl HarnessToolHost for Host {
    fn available(&self) -> &[NativeToolDeclaration] {
        &self.declarations
    }
    async fn invoke(
        &self,
        id: &ToolId,
        call_id: &str,
        _: Value,
        _: Option<crate::services::harness::runs::InvocationRef>,
    ) -> AppResult<ToolInvocation> {
        assert_eq!(id, &self.declarations[0].id);
        assert!(!call_id.is_empty());
        assert_ne!(
            call_id, "call-proof",
            "host receives a correlator, not a wire ID"
        );
        self.calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(self.delay).await;
        Ok(self.result.clone())
    }
}

#[tokio::test]
async fn all_allowlisted_builtins_preserve_schemas_and_execute_real_behavior() {
    let bridge = ToolBridge::compile(&spec(&ALLOWED_BUILTIN_TOOL_IDS), None).unwrap();
    assert_eq!(bridge.entries.len(), 10);
    for id in ALLOWED_BUILTIN_TOOL_IDS {
        assert_eq!(
            bridge.entries[&ToolId::from(id)].schema,
            builtins::create(&id.into()).unwrap().input_schema()
        );
    }
    let cases = [
        ("calculator", json!({"expression":"2 + 3"}), "5"),
        ("datetime", json!({"operation":"now"}), "20"),
        ("echo", json!({"message":"hello"}), "hello"),
        (
            "json",
            json!({"operation":"parse","data":"{\"answer\":42}"}),
            "42",
        ),
        ("math", json!({"operation":"mean","values":[1,2,3]}), "2"),
        ("random", json!({"operation":"string","length":8}), "length"),
        (
            "template",
            json!({"operation":"render","template":"Hello {{ name }}!","data":{"name":"World"}}),
            "Hello World!",
        ),
        (
            "text",
            json!({"operation":"uppercase","text":"hello"}),
            "HELLO",
        ),
        ("todo", json!({"operation":"list"}), "count"),
    ];
    for (id, args, expected) in cases {
        let result = bridge
            .invoke(&id.into(), &correlation(), args)
            .await
            .unwrap();
        assert!(result.success, "{id}: {}", result.render());
        assert!(
            result.render().contains(expected),
            "{id}: {}",
            result.render()
        );
    }
    for url in [
        "file:///etc/passwd",
        "http://127.0.0.1/",
        "http://169.254.169.254/latest/meta-data",
        "http://[::1]/",
    ] {
        let result = bridge
            .invoke(&"web_fetch".into(), &correlation(), json!({"url":url}))
            .await
            .unwrap();
        assert!(!result.success, "{url}");
    }
}

#[tokio::test]
async fn unknown_ungranted_display_names_and_malformed_arguments_never_reach_host() {
    let host = host(ToolInvocation::success(json!("directory")));
    let bridge = ToolBridge::compile(&spec(&["list_company_agents"]), Some(host.clone())).unwrap();
    for id in ["echo", "command", "Directory display name", "unknown"] {
        assert!(
            !bridge
                .invoke(&id.into(), &correlation(), json!({}))
                .await
                .unwrap()
                .success
        );
    }
    for args in [
        Value::Null,
        json!([]),
        json!({"huge":"x".repeat(MAX_TOOL_ARGUMENT_BYTES)}),
    ] {
        assert!(
            !bridge
                .invoke(&"list_company_agents".into(), &correlation(), args)
                .await
                .unwrap()
                .success
        );
    }
    assert_eq!(host.calls.load(Ordering::SeqCst), 0);
    assert!(ToolBridge::compile(&spec(&["command"]), None).is_err());
    assert!(ToolCorrelationId::parse("\nwire").is_err());
    assert!(ToolCorrelationId::parse("").is_err());
    let missing = ToolBridge::compile(&spec(&["list_company_agents"]), None).unwrap();
    assert_eq!(
        missing.diagnostics.missing_context,
        vec![ToolId::from("list_company_agents")]
    );
}

#[tokio::test]
async fn mutable_builtin_state_is_isolated_between_concurrent_runs() {
    let left = ToolBridge::compile(&spec(&["todo"]), None).unwrap();
    let right = ToolBridge::compile(&spec(&["todo"]), None).unwrap();
    let set = json!({"operation":"set","items":[{"id":"private","content":"tenant one"}]});
    let id = ToolId::from("todo");
    let correlation = correlation();
    let (left_result, right_result) = tokio::join!(
        left.invoke(&id, &correlation, set),
        right.invoke(&id, &correlation, json!({"operation":"list"})),
    );
    assert!(left_result.unwrap().render().contains("tenant one"));
    assert!(!right_result.unwrap().render().contains("tenant one"));
}

#[tokio::test]
async fn limits_fail_early_and_unicode_output_is_bounded() {
    let bridge =
        ToolBridge::compile(&spec(&["echo", "math", "random", "template", "json"]), None).unwrap();
    for (id, args) in [
        (
            "math",
            json!({"operation":"range","max":1e100,"step":1e-100}),
        ),
        ("random", json!({"operation":"string","length":u64::MAX})),
        (
            "template",
            json!({"operation":"render_file","path":"/etc/passwd","data":{}}),
        ),
        (
            "json",
            json!({"operation":"set","data":[],"path":"999999999","value":1}),
        ),
    ] {
        assert!(
            !bridge
                .invoke(&id.into(), &correlation(), args)
                .await
                .unwrap()
                .success
        );
    }
    let result = bridge
        .invoke(
            &"echo".into(),
            &correlation(),
            json!({"message":"é".repeat(20_000)}),
        )
        .await
        .unwrap();
    assert!(result.render().chars().count() <= MAX_TOOL_OUTPUT_CHARS);
    assert!(result.render().ends_with("[truncated]"));
    for _ in 5..MAX_TOOL_INVOCATIONS {
        bridge
            .invoke(&"echo".into(), &correlation(), json!({"message":"ok"}))
            .await
            .unwrap();
    }
    assert!(
        bridge
            .invoke(&"echo".into(), &correlation(), json!({}))
            .await
            .is_err()
    );
    assert_eq!(bridge.stop_reason().await, Some(ToolStopReason::Budget));
}

#[tokio::test]
async fn provider_batch_uses_real_bridge_preserves_wire_ids_and_checks_results() {
    let host = host(ToolInvocation::success(json!("directory-value")));
    let bridge =
        Arc::new(ToolBridge::compile(&spec(&["list_company_agents"]), Some(host.clone())).unwrap());
    let mut call = response("openai", Reply::ToolCall);
    call["choices"][0]["message"]["tool_calls"] = json!([
        {"id":"call-proof","type":"function","function":{"name":"list_company_agents","arguments":"{}"}},
        {"id":"second-wire","type":"function","function":{"name":"list_company_agents","arguments":"{}"}}
    ]);
    let llm = scripted_scenario(vec![
        ScriptedExchange::new(
            |request| {
                let definition = &request.body["tools"][0]["function"];
                if definition["name"] != "list_company_agents"
                    || definition["parameters"]["type"] != "object"
                {
                    return Err("canonical schema declaration");
                }
                Ok(())
            },
            ScriptedResponse::json(call),
        ),
        ScriptedExchange::new(
            |request| {
                let results: Vec<_> = request.body["messages"]
                    .as_array()
                    .ok_or("messages")?
                    .iter()
                    .filter(|m| m["role"] == "tool")
                    .collect();
                if results.len() != 2
                    || results[0]["tool_call_id"] != "call-proof"
                    || results[1]["tool_call_id"] != "second-wire"
                    || results.iter().any(|m| m["content"] != "directory-value")
                {
                    return Err("actual results and wire identities");
                }
                Ok(())
            },
            ScriptedResponse::json(response("openai", Reply::Text)),
        ),
    ])
    .await;
    let registry = ProviderRegistry::standard().unwrap();
    let agent = bridge.build_agent(AgentBuilder::from_model_handle(model(
        &registry,
        "openai",
        "key",
        &llm.base_url,
    )));
    assert_eq!(
        agent
            .prompt("Read twice")
            .tool_concurrency(1)
            .max_turns(2)
            .await
            .unwrap(),
        "verified"
    );
    assert_eq!(llm.finish().await, Ok(2));
    assert_eq!(host.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn suspension_stops_the_second_side_effect_and_next_model_request() {
    let host = host(ToolInvocation::suspended(json!({"status":"waiting"})));
    let bridge =
        Arc::new(ToolBridge::compile(&spec(&["list_company_agents"]), Some(host.clone())).unwrap());
    let mut call = response("openai", Reply::ToolCall);
    call["choices"][0]["message"]["tool_calls"] = json!([
        {"id":"call-proof","type":"function","function":{"name":"list_company_agents","arguments":"{}"}},
        {"id":"second-wire","type":"function","function":{"name":"list_company_agents","arguments":"{}"}}
    ]);
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
    assert!(
        agent
            .prompt("Wait then read")
            .tool_concurrency(1)
            .max_turns(2)
            .await
            .is_err()
    );
    assert_eq!(llm.finish().await, Ok(1));
    assert_eq!(host.calls.load(Ordering::SeqCst), 1);
    assert_eq!(bridge.stop_reason().await, Some(ToolStopReason::Suspended));
}

#[tokio::test]
async fn deadline_and_cancellation_stop_future_effects() {
    let mut slow = host(ToolInvocation::success(json!("late")));
    Arc::get_mut(&mut slow).unwrap().delay = Duration::from_secs(60);
    let bridge = ToolBridge::compile(&spec(&["list_company_agents"]), Some(slow.clone()))
        .unwrap()
        .with_deadline(tokio::time::Instant::now() + Duration::from_millis(10));
    assert!(
        bridge
            .invoke(&"list_company_agents".into(), &correlation(), json!({}))
            .await
            .is_err()
    );
    assert_eq!(bridge.stop_reason().await, Some(ToolStopReason::Timeout));
    assert_eq!(slow.calls.load(Ordering::SeqCst), 1);
    assert!(
        bridge
            .invoke(&"list_company_agents".into(), &correlation(), json!({}))
            .await
            .is_err()
    );
    assert_eq!(slow.calls.load(Ordering::SeqCst), 1);
    let bridge = ToolBridge::compile(&spec(&["list_company_agents"]), Some(slow.clone())).unwrap();
    assert!(
        tokio::time::timeout(
            Duration::from_millis(10),
            bridge.invoke(&"list_company_agents".into(), &correlation(), json!({}))
        )
        .await
        .is_err()
    );
    assert_eq!(
        bridge.stop_reason().await,
        Some(ToolStopReason::HostFailure)
    );
    assert_eq!(slow.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn competing_dispatchers_cannot_execute_after_suspension() {
    let mut host = host(ToolInvocation::suspended(Value::Null));
    Arc::get_mut(&mut host).unwrap().delay = Duration::from_millis(10);
    let bridge = ToolBridge::compile(&spec(&["list_company_agents"]), Some(host.clone())).unwrap();
    let id = ToolId::from("list_company_agents");
    let correlation = correlation();
    let (first, second) = tokio::join!(
        bridge.invoke(&id, &correlation, json!({})),
        bridge.invoke(&id, &correlation, json!({}))
    );
    assert!(first.unwrap().suspends_run());
    assert!(second.is_err());
    assert_eq!(host.calls.load(Ordering::SeqCst), 1);
}

#[path = "tools_trace_tests.rs"]
mod tracing_tests;
