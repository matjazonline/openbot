use super::providers::*;
use super::test_support::{
    KEYS, MODEL, Reply, check_request, check_result, check_tools, lookup, model, response,
};
use crate::services::test_support::{ScriptedExchange, ScriptedResponse, scripted_scenario};
use rig::agent::{
    AgentBuilder,
    hook::{AgentHook, HookContext, ToolCall, ToolCallAction},
};
use rig::completion::Prompt;
use secrecy::SecretString;
use serde_json::json;
use std::future::IntoFuture;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[tokio::test]
async fn every_factory_executes_a_request_checked_native_tool_loop() {
    let registry = ProviderRegistry::standard().unwrap();
    for provider in KEYS {
        let llm = scripted_scenario(vec![
            ScriptedExchange::new(
                move |request| {
                    check_request(request, provider, "tenant-key")?;
                    if !request.body.to_string().contains("Look up the record") {
                        return Err("user prompt");
                    }
                    check_tools(&request.body, provider)
                },
                ScriptedResponse::json(response(provider, Reply::ToolCall)),
            ),
            ScriptedExchange::new(
                move |request| {
                    check_request(request, provider, "tenant-key")?;
                    check_result(&request.body, provider)
                },
                ScriptedResponse::json(response(provider, Reply::Text)),
            ),
        ])
        .await;
        let calls = Arc::new(AtomicUsize::new(0));
        let agent = AgentBuilder::from_model_handle(model(
            &registry,
            provider,
            "tenant-key",
            &llm.base_url,
        ))
        .max_tokens(100)
        .dynamic_tool(lookup(calls.clone()))
        .build();
        let result = agent
            .prompt("Look up the record")
            .max_turns(2)
            .extended_details()
            .await;
        let completion = llm.finish().await;
        assert_eq!(completion, Ok(2), "{provider}");
        let result = result.unwrap();
        assert_eq!(result.output, "verified", "{provider}");
        if provider == "xai" {
            let history = serde_json::to_string(&result.messages).unwrap();
            assert!(
                history.contains("item-proof") && history.contains("call-proof"),
                "xAI transcript retains both identities"
            );
        }
        assert_eq!(result.usage.input_tokens, 4, "{provider}");
        assert_eq!(result.usage.output_tokens, 6, "{provider}");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn construction_is_fallible_without_global_keys_or_network() {
    let registry = ProviderRegistry::standard().unwrap();
    for provider in KEYS {
        assert!(
            registry
                .model(&ResolvedModelRequest {
                    provider: &provider.into(),
                    model: &MODEL.into(),
                    secret: &SecretString::from("supplied-key"),
                    endpoint: None
                })
                .is_ok()
        );
    }
    let unknown = registry
        .model(&ResolvedModelRequest {
            provider: &"unknown".into(),
            model: &MODEL.into(),
            secret: &SecretString::from("never-print-this"),
            endpoint: None,
        })
        .err();
    assert_eq!(unknown, Some(ProviderError::UnsupportedProvider));
    let error = registry
        .model(&ResolvedModelRequest {
            provider: &"openai".into(),
            model: &MODEL.into(),
            secret: &SecretString::from(""),
            endpoint: None,
        })
        .err();
    assert_eq!(error, Some(ProviderError::InvalidCredentials));
    let duplicate = registry
        .register("openai".into(), |_, _| Err(ProviderError::Configuration))
        .err();
    assert_eq!(duplicate, Some(ProviderError::DuplicateProvider));
}

struct Park;
impl AgentHook for Park {
    async fn on_tool_call(&self, _: &HookContext, event: ToolCall<'_>) -> ToolCallAction {
        assert_eq!(event.tool_call_id, Some("call-proof"));
        ToolCallAction::Stop("parked by application policy".into())
    }
}

#[tokio::test]
async fn lifecycle_hook_stops_before_tool_execution_and_second_provider_call() {
    let registry = ProviderRegistry::standard().unwrap();
    let llm = scripted_scenario(vec![ScriptedExchange::new(
        |request| check_request(request, "openai", "key"),
        ScriptedResponse::json(response("openai", Reply::ToolCall)),
    )])
    .await;
    let calls = Arc::new(AtomicUsize::new(0));
    let agent = AgentBuilder::from_model_handle(model(&registry, "openai", "key", &llm.base_url))
        .dynamic_tool(lookup(calls.clone()))
        .add_hook(Park)
        .build();
    assert!(
        agent
            .prompt("Look up the record")
            .max_turns(2)
            .await
            .is_err()
    );
    assert_eq!(llm.finish().await, Ok(1));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn concurrent_tenants_and_rotation_use_only_supplied_credentials() {
    let registry = ProviderRegistry::standard().unwrap();
    let registry = &registry;
    let run = |key: &'static str| async move {
        let llm = scripted_scenario(vec![ScriptedExchange::new(
            move |request| check_request(request, "openai", key),
            ScriptedResponse::json(response("openai", Reply::Text)),
        )])
        .await;
        let agent =
            AgentBuilder::from_model_handle(model(registry, "openai", key, &llm.base_url)).build();
        let result = agent.prompt("Hello").max_turns(1).await;
        assert_eq!(llm.finish().await, Ok(1));
        assert_eq!(result.unwrap(), "verified");
    };
    tokio::join!(run("tenant-one"), run("tenant-two"));
    let llm = scripted_scenario(
        ["tenant-one", "rotated-key"]
            .into_iter()
            .map(|key| {
                ScriptedExchange::new(
                    move |request| check_request(request, "openai", key),
                    ScriptedResponse::json(response("openai", Reply::Text)),
                )
            })
            .collect(),
    )
    .await;
    for key in ["tenant-one", "rotated-key"] {
        let agent =
            AgentBuilder::from_model_handle(model(registry, "openai", key, &llm.base_url)).build();
        assert_eq!(
            agent.prompt("Hello").max_turns(1).await.unwrap(),
            "verified"
        );
    }
    assert_eq!(llm.finish().await, Ok(2));
}

#[tokio::test]
async fn dropping_the_run_cancels_an_inflight_completion_before_tools_execute() {
    let registry = ProviderRegistry::standard().unwrap();
    let (arrived, arrival) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let mut reply = ScriptedResponse::json(response("openai", Reply::ToolCall));
    reply.barrier = Some((arrived, released));
    reply.disconnect = true;
    let llm = scripted_scenario(vec![ScriptedExchange::new(
        |request| check_request(request, "openai", "key"),
        reply,
    )])
    .await;
    let calls = Arc::new(AtomicUsize::new(0));
    let agent = AgentBuilder::from_model_handle(model(&registry, "openai", "key", &llm.base_url))
        .dynamic_tool(lookup(calls.clone()))
        .build();
    let mut run = Box::pin(
        agent
            .prompt("Look up the record")
            .max_turns(2)
            .into_future(),
    );
    tokio::select! {
        result = &mut run => panic!("run finished before barrier: {}", result.is_ok()),
        result = tokio::time::timeout(std::time::Duration::from_secs(5), arrival) => { result.unwrap().unwrap(); }
    }
    drop(run);
    release.send(()).unwrap();
    assert_eq!(llm.finish().await, Ok(1));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn provider_error_bodies_and_credentials_do_not_escape_the_transport() {
    let registry = ProviderRegistry::standard().unwrap();
    for provider in KEYS {
        let mut reply =
            ScriptedResponse::json(json!({"error":{"message":"tenant-secret echoed by provider"}}));
        reply.status = 401;
        let llm = scripted_scenario(vec![ScriptedExchange::new(
            move |request| check_request(request, provider, "tenant-secret"),
            reply,
        )])
        .await;
        let agent = AgentBuilder::from_model_handle(model(
            &registry,
            provider,
            "tenant-secret",
            &llm.base_url,
        ))
        .max_tokens(100)
        .build();
        use tracing::instrument::WithSubscriber;
        let logs = LogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(logs.clone())
            .with_max_level(tracing::Level::TRACE)
            .without_time()
            .finish();
        let error = agent
            .prompt("Hello")
            .max_turns(1)
            .into_future()
            .with_subscriber(subscriber)
            .await
            .unwrap_err();
        assert!(!String::from_utf8_lossy(&logs.0.lock().unwrap()).contains("tenant-secret"));
        assert!(!format!("{error:?} {error}").contains("tenant-secret"));
        assert_eq!(llm.finish().await, Ok(1));
    }
}

#[test]
fn invalid_models_endpoints_and_credentials_are_safe_configuration_errors() {
    let registry = ProviderRegistry::standard().unwrap();
    let provider = "openai".into();
    let secret = SecretString::from("synthetic-secret");
    let model = MODEL.into();
    let mut request = ResolvedModelRequest {
        provider: &provider,
        model: &model,
        secret: &secret,
        endpoint: None,
    };
    for endpoint in [
        "file:///tmp/provider",
        "https://synthetic-secret@example.com",
        "https://example.com/?key=synthetic-secret",
        "http://example.com",
        "not a URL",
    ] {
        request.endpoint = Some(endpoint);
        assert_eq!(
            registry.model(&request).err(),
            Some(ProviderError::InvalidEndpoint)
        );
    }
    request.endpoint = None;
    let empty_model = " ".into();
    request.model = &empty_model;
    assert_eq!(
        registry.model(&request).err(),
        Some(ProviderError::MissingModel)
    );
    request.model = &model;
    let invalid_key = SecretString::from("synthetic-secret\r\nforged: header");
    request.secret = &invalid_key;
    assert_eq!(
        registry.model(&request).err(),
        Some(ProviderError::InvalidCredentials)
    );
}

#[derive(Clone, Default)]
struct LogBuffer(Arc<std::sync::Mutex<Vec<u8>>>);
impl std::io::Write for LogBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogBuffer {
    type Writer = Self;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[test]
fn startup_rejects_incomplete_provider_factory_coverage() {
    assert_eq!(
        ProviderRegistry::new().unwrap().validate_coverage(),
        Err(ProviderError::IncompleteCoverage)
    );
    ProviderRegistry::standard()
        .unwrap()
        .validate_coverage()
        .unwrap();
}
