//! What the runner does around a harness: composing, guarding, resolving one, and recording the
//! result.
//!
//! The harness itself is either a stub or the real `ai-agents` adapter pointed at a scripted
//! model over a socket. Reaching for the adapter from a test file is deliberate and allowed --
//! `src/application/transport/dependency_tests.rs` scans production code only -- and it is what
//! makes the end-to-end assertion below worth having: the whole compile-and-run seam runs, and
//! only the model is scripted.

use std::sync::Arc;

use uuid::Uuid;

use super::*;
use crate::adapters::harness::ai_agents::{AiAgentsHarness, AiAgentsTextClassifier};
use crate::entities::{agent::Agent as AgentEntity, harness::HarnessKind, value_objects::ToolId};
use crate::services::harness::HarnessRegistry;
use crate::services::test_support::{
    LlmTurn, SCRIPTED_MODEL, SCRIPTED_PROVIDER, StubHarness, register_scripted_agent_base_url,
    scripted_llm,
};
use params::ResolvedAgentCapabilities;

fn company() -> Company {
    Company {
        channel_defaults: Default::default(),
        id: Uuid::new_v4(),
        user_id: Uuid::new_v4(),
        name: "Test".to_string(),
        slug: "test".into(),
        enable_llm_spam_guardrail: None,
        avatar_url: None,
        memory_provider: None,
        created_at: chrono::Utc::now(),
    }
}

/// A fixture agent whose trusted connection endpoint points at the scripted provider.
fn agent_with(base_url: &str) -> AgentEntity {
    agent_with_provider(base_url, SCRIPTED_PROVIDER, Vec::new())
}

fn agent_with_provider(
    base_url: &str,
    provider: &str,
    granted_tool_ids: Vec<ToolId>,
) -> AgentEntity {
    let agent = AgentEntity {
        response_contract: None,
        memory_enabled: false,
        id: Uuid::new_v4(),
        company_id: None,
        name: "Scripted".into(),
        slug: "scripted".into(),
        provider: Some(provider.into()),
        model: Some(SCRIPTED_MODEL.into()),
        run_timeout_secs: None,
        system_prompt: Some("Answer briefly.".into()),
        description: None,
        harness_kind: HarnessKind::AiAgents,
        granted_tool_ids,
        native_tool_policy: crate::entities::harness::NativeToolPolicy::default(),
        config_json: None,
        memory_persistence_mode: Default::default(),
        memory_recall_mode: Default::default(),
        memory_max_results: crate::entities::memory::default_memory_max_results(),
        avatar_url: None,
        created_by: crate::entities::creation::CreationProvenance::system(),
        created_at: chrono::Utc::now(),
    };
    register_scripted_agent_base_url(agent.id, base_url);
    agent
}

fn registry_of(harness: Arc<dyn AgentHarness>) -> Arc<HarnessRegistry> {
    let kind = harness.kind();
    Arc::new(
        HarnessRegistry::new()
            .register(kind, harness)
            .expect("one harness registers"),
    )
}

fn classifier() -> Arc<dyn TextClassifier> {
    Arc::new(AiAgentsTextClassifier::new())
}

/// The no-silent-downgrade rule, from the caller's side: a deployment that wired no harness must
/// stop the run rather than pick one.
#[tokio::test]
async fn a_run_with_no_configured_harness_fails_instead_of_choosing_one() {
    let company = company();
    let params = ResolvedAgentCapabilities::new(Some(&company), None).expect("params resolve");

    let error = AgentRunner::new("Hello world", &params)
        .execute()
        .await
        .expect_err("a deployment with no harness cannot run an agent");

    assert!(
        error.to_string().contains("No agent harness is configured"),
        "unexpected error: {error}"
    );
    assert!(matches!(error, AppError::BadRequest(_)));
}

#[tokio::test]
async fn an_agent_with_no_registered_harness_fails_the_run() {
    let company = company();
    let params = ResolvedAgentCapabilities::new(Some(&company), None).expect("params resolve");
    let registry = Arc::new(HarnessRegistry::new());

    let error = AgentRunner::new("Hello world", &params)
        .harnesses(registry.clone(), classifier())
        .execute()
        .await
        .expect_err("an empty registry cannot execute the requested kind");

    assert!(error.to_string().contains(HarnessKind::Rig.as_str()));
    assert!(registry.registered().is_empty());
}

/// A run that reached a harness and failed there is recorded and reported -- with the company's
/// credential removed from the message on the way out.
#[tokio::test]
async fn a_failed_run_is_reported_without_the_credential_that_caused_it() {
    let company = company();
    let params = ResolvedAgentCapabilities::new(Some(&company), None).expect("params resolve");
    let harness = StubHarness::new(HarnessKind::Rig)
        .failing("provider rejected key company-api-key with 401");

    let error = AgentRunner::new("Hello world", &params)
        .harnesses(registry_of(Arc::new(harness)), classifier())
        .execute()
        .await
        .expect_err("the harness failed");

    assert!(!error.to_string().contains("company-api-key"), "{error}");
    assert!(error.to_string().contains("[REDACTED]"), "{error}");
    assert!(matches!(error, AppError::Internal(_)));
}

/// A run with no task, no outreach context and no provisioning port offers the harness nothing of
/// ours. Registration is not a grant, and neither is the absence of one -- but a harness must not
/// be handed an empty host to reason about.
#[test]
fn a_run_with_no_tool_context_offers_the_harness_no_native_tools() {
    let company = company();
    let params = ResolvedAgentCapabilities::new(Some(&company), None).expect("params resolve");
    let runner = AgentRunner::new("Hello world", &params);

    assert!(runner.tool_host().is_none());
}

/// The whole seam, once: the runner composes, the adapter compiles, the runtime resolves its own
/// context sources, and the model sees a system prompt with today's date already in it.
#[tokio::test]
async fn agent_execution_sends_resolved_current_date_to_llm() -> anyhow::Result<()> {
    let mut llm = scripted_llm(vec![LlmTurn::text("Today is noted.")]).await;
    let company = company();
    let agent = agent_with(&llm.base_url);
    let params = ResolvedAgentCapabilities::new(Some(&company), Some(&agent))?;

    let output = AgentRunner::new("What date is it today?", &params)
        .harnesses(registry_of(Arc::new(AiAgentsHarness::new())), classifier())
        .execute()
        .await?;
    assert_eq!(output.content, "Today is noted.");

    let requests = llm.observed();
    assert_eq!(requests.len(), 1);

    let messages = requests[0]["messages"]
        .as_array()
        .expect("messages array present");
    let sys_msg = messages
        .iter()
        .find(|m| m["role"] == "system")
        .expect("system message present");
    let content = sys_msg["content"]
        .as_str()
        .map(|s| s.to_string())
        .or_else(|| {
            sys_msg["content"].as_array().and_then(|arr| {
                arr.iter()
                    .find_map(|item| item["text"].as_str().map(|s| s.to_string()))
            })
        })
        .expect("system message text content");

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    assert!(
        content.contains(&format!("Current local date: {today}")),
        "Expected system prompt to contain resolved local date '{today}', got: {content}"
    );
    assert!(
        !content.contains("{{ context.time.date }}"),
        "System prompt must not contain unrendered time template variable: {content}"
    );

    // The clock belongs to the runner, so the harness could not have filled this in.
    let diagnostics = output
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("execution_diagnostics"))
        .expect("the run records its own diagnostics");
    assert!(diagnostics.get("duration_ms").is_some());
    assert_eq!(
        diagnostics["response_characters"].as_u64(),
        Some("Today is noted.".chars().count() as u64)
    );
    assert_eq!(diagnostics["history_message_count"].as_u64(), Some(0));
    // The prompt the harness counted is the fenced one, not the message that went into it.
    assert!(
        diagnostics["prompt_characters"]
            .as_u64()
            .unwrap_or_default()
            > "What date is it today?".chars().count() as u64
    );

    assert_eq!(llm.finish().await, Ok(requests.len()));
    Ok(())
}

/// The first model response asks
/// for a tool, and the second request must preserve the call id and send the result as a `tool`
/// message. A one-turn completion test cannot prove this protocol works.
#[tokio::test]
async fn completes_a_two_turn_tool_loop_through_ai_agents() -> anyhow::Result<()> {
    let mut llm = scripted_llm(vec![
        LlmTurn::tool_call("todo", serde_json::json!({ "operation": "list" })),
        LlmTurn::text("No tasks are pending."),
    ])
    .await;
    let company = company();
    let agent = agent_with_provider(&llm.base_url, SCRIPTED_PROVIDER, vec![ToolId::from("todo")]);
    let params = ResolvedAgentCapabilities::new(Some(&company), Some(&agent))?;

    let output = AgentRunner::new("Check the task list.", &params)
        .harnesses(registry_of(Arc::new(AiAgentsHarness::new())), classifier())
        .execute()
        .await?;
    assert_eq!(output.content, "No tasks are pending.");

    let requests = llm.observed();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[0]["tools"]
            .as_array()
            .is_some_and(|tools| { tools.iter().any(|tool| tool["function"]["name"] == "todo") })
    );

    let second_messages = requests[1]["messages"]
        .as_array()
        .expect("the follow-up carries conversation messages");
    assert!(second_messages.iter().any(|message| {
        message["role"] == "assistant"
            && message["tool_calls"][0]["id"] == "call_0"
            && message["tool_calls"][0]["function"]["name"] == "todo"
    }));
    assert!(
        second_messages
            .iter()
            .any(|message| { message["role"] == "tool" && message["tool_call_id"] == "call_0" })
    );

    assert_eq!(llm.finish().await, Ok(requests.len()));
    Ok(())
}

#[tokio::test]
async fn deployment_wiring_dispatches_the_same_provider_independently_through_both_harnesses() {
    use crate::adapters::{
        harness::deployment_registry,
        mcp::{HttpMcpClient, policy::EndpointPolicy},
        persistence::PostgresPersistence,
    };
    let Some(pool) = crate::adapters::persistence::test_support::test_pool().await else {
        return;
    };
    let persistence = Arc::new(PostgresPersistence::new(pool));
    let mcp = Arc::new(crate::services::mcp_runtime::McpRuntime::new(
        persistence.clone(),
        persistence.clone(),
        Arc::new(HttpMcpClient::new(
            EndpointPolicy::new(std::iter::empty::<String>()).unwrap(),
        )),
    ));
    let guardrail = Arc::new(WiringGuardrail::default());
    for default in HarnessKind::ALL {
        let registry =
            Arc::new(deployment_registry(default, persistence.clone(), mcp.clone()).unwrap());
        assert_eq!(registry.registered().len(), 2);
        for kind in HarnessKind::ALL {
            let llm = checked_todo_loop().await;
            let company = company();
            let mut agent =
                agent_with_provider(&llm.base_url, SCRIPTED_PROVIDER, vec!["todo".into()]);
            agent.harness_kind = kind;
            let params = ResolvedAgentCapabilities::new(Some(&company), Some(&agent)).unwrap();
            let output = AgentRunner::new("Hello", &params)
                .ids(Some(company.id), None, Some(agent.id))
                .company(Some(company.clone()))
                .config(Some(Arc::new(wiring_config())))
                .harnesses(registry.clone(), guardrail.clone())
                .execute()
                .await
                .unwrap();
            assert_eq!(output.content, "Wired response");
            assert!(output.metadata.unwrap()["execution_diagnostics"]["duration_ms"].is_number());
            assert_eq!(llm.finish().await, Ok(2));
        }
    }
    assert_eq!(guardrail.0.load(std::sync::atomic::Ordering::SeqCst), 4);
}

#[derive(Default)]
struct WiringGuardrail(std::sync::atomic::AtomicUsize);
#[async_trait::async_trait]
impl TextClassifier for WiringGuardrail {
    async fn complete(
        &self,
        request: crate::services::harness::ClassificationRequest<'_>,
    ) -> AppResult<String> {
        assert_eq!(request.purpose, "spam_guardrail");
        assert!(request.user_prompt.contains("Hello"));
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(r#"{"is_spam":false}"#.into())
    }
}

fn wiring_config() -> AppConfig {
    AppConfig {
        default_agent_harness: HarnessKind::Rig,
        jwt_secret: "test-secret".into(),
        sendgrid_inbound: None,
        resend_api: crate::infra::config::ResendApiConfig::default(),
        hydradb: None,
        hindsight: None,
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "example.test".into(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".into(),
        smtp_port: 2525,
        smtp_username: String::new(),
        smtp_password: String::new(),
        smtp_from_address: "test@example.test".into(),
        incoming_smtp_enabled: false,
        incoming_smtp_host: "127.0.0.1".into(),
        incoming_smtp_port: 2525,
        max_spam_score: 5.0,
        dnsbl_enabled: false,
        dnsbl_servers: vec![],
        smtp_rate_limit_conns_per_ip: 30,
        reject_self_domain_helo: true,
        enable_heuristic_scanner: false,
        enable_spam_scanner: false,
        spam_scanner_type: "rspamd".into(),
        spam_scanner_url: "http://localhost:11333/checkv2".into(),
        enable_llm_spam_guardrail: true,
        secure_cookies: false,
        gcs: None,
        operator_emails: vec![],
    }
}

fn check_todo_request(
    request: &crate::services::test_support::ScriptedRequest,
) -> Result<(), &'static str> {
    if request.method != "POST"
        || request.path != "/chat/completions"
        || request.body["model"] != SCRIPTED_MODEL
        || !request.header_matches("authorization", "Bearer company-api-key")
    {
        return Err("shared harness request (authentication redacted)");
    }
    Ok(())
}

async fn checked_todo_loop() -> crate::services::test_support::ScriptedLlm {
    use crate::services::test_support::{ScriptedExchange, ScriptedResponse, scripted_scenario};
    use serde_json::{Value, json};
    scripted_scenario(vec![
        ScriptedExchange::new(
            |request| {
                check_todo_request(request)?;
                let tool = request.body["tools"]
                    .as_array()
                    .ok_or("tool declarations")?
                    .iter()
                    .find(|tool| tool["function"]["name"] == "todo")
                    .ok_or("todo grant")?;
                if !tool["function"]["parameters"]
                    .to_string()
                    .contains("operation")
                {
                    return Err("todo operation schema");
                }
                Ok(())
            },
            ScriptedResponse::turn(LlmTurn::tool_call("todo", json!({"operation":"list"})), 0),
        ),
        ScriptedExchange::new(
            |request| {
                check_todo_request(request)?;
                let messages = request.body["messages"].as_array().ok_or("tool history")?;
                let calls: Vec<_> = messages
                    .iter()
                    .filter_map(|message| message["tool_calls"].as_array())
                    .flatten()
                    .collect();
                if calls.len() != 1
                    || calls[0]["id"] != "call_0"
                    || calls[0]["function"]["name"] != "todo"
                {
                    return Err("assistant call correlation");
                }
                let results: Vec<_> = messages
                    .iter()
                    .filter(|message| message["role"] == "tool")
                    .collect();
                if results.len() != 1 || results[0]["tool_call_id"] != "call_0" {
                    return Err("duplicate/orphan/missing tool result");
                }
                let mut value: Value =
                    serde_json::from_str(results[0]["content"].as_str().ok_or("result content")?)
                        .map_err(|_| "result JSON")?;
                // Rig preserves the builtin's JSON string as a JSON value in its durable transcript.
                if let Value::String(text) = value {
                    value = serde_json::from_str(&text).map_err(|_| "builtin result JSON")?;
                }
                if value["operation"] != "list"
                    || value["count"] != 0
                    || value["items"] != json!([])
                {
                    return Err("todo did not return the empty run-local list");
                }
                Ok(())
            },
            ScriptedResponse::turn(LlmTurn::text("Wired response"), 1),
        ),
    ])
    .await
}
