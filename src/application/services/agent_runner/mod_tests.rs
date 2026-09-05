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
use crate::entities::{agent::Agent as AgentEntity, harness::HarnessKind};
use crate::services::harness::HarnessRegistry;
use crate::services::test_support::{
    LlmTurn, SCRIPTED_MODEL, SCRIPTED_PROVIDER, StubHarness, scripted_agent_config, scripted_llm,
};
use params::ResolvedAgentParams;

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

/// A fixture agent whose configuration points the real runtime at `config_json`.
fn agent_with(config_json: serde_json::Value) -> AgentEntity {
    AgentEntity {
        memory_enabled: false,
        id: Uuid::new_v4(),
        company_id: None,
        name: "Scripted".into(),
        slug: "scripted".into(),
        provider: Some(SCRIPTED_PROVIDER.into()),
        model: Some(SCRIPTED_MODEL.into()),
        run_timeout_secs: None,
        system_prompt: Some("Answer briefly.".into()),
        description: None,
        config_json: Some(config_json),
        memory_persistence_mode: Default::default(),
        memory_recall_mode: Default::default(),
        memory_max_results: crate::entities::memory::default_memory_max_results(),
        avatar_url: None,
        created_by: crate::entities::creation::CreationProvenance::system(),
        created_at: chrono::Utc::now(),
    }
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
    let params = ResolvedAgentParams::new(Some(&company), None).expect("params resolve");

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

/// A run that reached a harness and failed there is recorded and reported -- with the company's
/// credential removed from the message on the way out.
#[tokio::test]
async fn a_failed_run_is_reported_without_the_credential_that_caused_it() {
    let company = company();
    let params = ResolvedAgentParams::new(Some(&company), None).expect("params resolve");
    let harness = StubHarness::new(HarnessKind::AiAgents)
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
    let params = ResolvedAgentParams::new(Some(&company), None).expect("params resolve");
    let runner = AgentRunner::new("Hello world", &params);

    assert!(runner.tool_host().is_none());
}

/// The whole seam, once: the runner composes, the adapter compiles, the runtime resolves its own
/// context sources, and the model sees a system prompt with today's date already in it.
#[tokio::test]
async fn agent_execution_sends_resolved_current_date_to_llm() -> anyhow::Result<()> {
    let mut llm = scripted_llm(vec![LlmTurn::text("Today is noted.")]).await;
    let company = company();
    let agent = agent_with(scripted_agent_config(&llm.base_url));
    let params = ResolvedAgentParams::new(Some(&company), Some(&agent))?;

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

    Ok(())
}
