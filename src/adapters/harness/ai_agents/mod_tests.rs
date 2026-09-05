//! The adapter's own seam: building a runtime from a compiled configuration, and reporting what
//! the run produced.
//!
//! Building an agent needs no provider call, so these stay offline. Only `chat` would go out, and
//! the one test that drives it lives beside the runner with a scripted model on a socket.

use std::sync::{Arc, atomic::AtomicBool};

use serde_json::json;

use super::*;
use crate::entities::{
    harness::{AgentCapabilitySpec, SubAgentScope},
    transport::RecipientRole,
    value_objects::{ModelName, ModelProvider},
};

fn spec() -> AgentCapabilitySpec {
    AgentCapabilitySpec {
        harness: HarnessKind::AiAgents,
        name: "pravnik".to_string(),
        system_prompt: "You are a legal assistant.".to_string(),
        provider: ModelProvider::canonical("openai"),
        model: ModelName::canonical("gpt-4o"),
        skills: Vec::new(),
        granted_tools: Vec::new(),
        sub_agents: SubAgentScope::AllCompanySiblings,
        extra_config: json!({}),
    }
}

fn run_of(spec: AgentCapabilitySpec, prompt: &str) -> AgentRun<'_> {
    AgentRun {
        spec: Box::new(spec),
        api_key: "test-key",
        full_prompt: prompt,
        history_message_count: 0,
        recipient_role: Some(RecipientRole::Cc),
        approvals: None,
        tool_host: None,
        trace: None,
    }
}

/// The harness must answer to the slot it is registered into, or the registry's mismatch check
/// has nothing to compare against.
#[test]
fn the_harness_reports_the_kind_it_implements() {
    assert_eq!(AiAgentsHarness::new().kind(), HarnessKind::AiAgents);
}

/// `build_agent` is split across a sync/async/sync seam so that only the two `auto_configure_*`
/// calls sit in the future. This drives the whole seam on the configuration shape production
/// actually sends, and reads back the delivery context -- which the last of the three stages is
/// what sets, so reading it back proves the whole seam ran in order.
///
/// It does not check which tools ended up registered: `RuntimeAgent` exposes no accessor for
/// that, and this configuration declares none. The ordering constraint behind that is documented
/// on `build_with_tools`.
#[tokio::test]
async fn build_agent_wires_an_agent_from_a_production_shaped_config() -> AppResult<()> {
    let run = run_of(spec(), "Kaksen je odpovedni rok?");
    let compiled = compile(&run.spec, run.api_key, &[])?;
    let executor = Executor {
        compiled: &compiled,
        run: &run,
        suspended: Arc::new(AtomicBool::new(false)),
        callback_failure: Arc::new(std::sync::Mutex::new(None)),
    };

    let agent = executor.build_agent().await?;

    let context = agent.get_context();
    assert_eq!(context["recipient_role"], json!("cc"));
    assert_eq!(context["is_to"], json!(false));
    assert_eq!(context["is_cc"], json!(true));
    Ok(())
}

/// A run addressed directly, and a run whose role was never established: both are `to`, because
/// the runtime's `context:` block declares all three as required and a missing one fails the
/// build rather than defaulting.
#[tokio::test]
async fn a_run_with_no_recipient_role_is_treated_as_addressed_directly() -> AppResult<()> {
    for (role, expected_to) in [
        (Some(RecipientRole::To), true),
        (None, true),
        (Some(RecipientRole::Cc), false),
    ] {
        let mut run = run_of(spec(), "Hello");
        run.recipient_role = role;
        let compiled = compile(&run.spec, run.api_key, &[])?;
        let agent = Executor {
            compiled: &compiled,
            run: &run,
            suspended: Arc::new(AtomicBool::new(false)),
            callback_failure: Arc::new(std::sync::Mutex::new(None)),
        }
        .build_agent()
        .await?;

        let context = agent.get_context();
        assert_eq!(context["is_to"], json!(expected_to), "{role:?}");
        assert_eq!(context["is_cc"], json!(!expected_to), "{role:?}");
    }
    Ok(())
}

#[test]
fn execution_diagnostics_are_recorded_beside_the_providers_own_metadata() {
    let diagnostics = AgentExecutionDiagnostics {
        duration_ms: 123,
        prompt_characters: 1000,
        response_characters: 500,
        history_message_count: 3,
        token_usage_source: "estimated".to_string(),
        tool_call_count: 1,
        tool_names: vec!["search".to_string()],
    };

    let metadata = attach_execution_diagnostics(
        Some(json!({ "reasoning": { "iterations": 1 } })),
        &diagnostics,
    )
    .expect("metadata is produced");

    assert_eq!(metadata["reasoning"]["iterations"], 1);
    assert_eq!(metadata["execution_diagnostics"]["duration_ms"], 123);
    assert_eq!(
        metadata["execution_diagnostics"]["token_usage_source"],
        "estimated"
    );
    assert_eq!(metadata["execution_diagnostics"]["tool_names"][0], "search");
}

#[test]
fn the_observability_report_is_added_without_displacing_the_diagnostics() {
    let metadata = attach_observability_report(
        Some(json!({ "execution_diagnostics": { "duration_ms": 123 } })),
        json!({ "summary": { "total_events": 2, "total_llm_calls": 1 } }),
    )
    .expect("metadata is produced");

    assert_eq!(metadata["execution_diagnostics"]["duration_ms"], 123);
    assert_eq!(metadata["observability"]["summary"]["total_events"], 2);
    assert_eq!(metadata["observability"]["summary"]["total_llm_calls"], 1);
}

/// A provider that answered with something other than an object: its metadata is nested rather
/// than dropped, because a run that behaved oddly is exactly when it is wanted.
#[test]
fn a_non_object_provider_metadata_is_kept_rather_than_discarded() {
    let diagnostics = AgentExecutionDiagnostics {
        duration_ms: 0,
        prompt_characters: 1,
        response_characters: 1,
        history_message_count: 0,
        token_usage_source: "estimated".to_string(),
        tool_call_count: 0,
        tool_names: Vec::new(),
    };

    let metadata = attach_execution_diagnostics(Some(json!("surprise")), &diagnostics)
        .expect("metadata is produced");

    assert_eq!(metadata["ai_agents_metadata"], json!("surprise"));
    assert!(metadata["execution_diagnostics"].is_object());
}

#[test]
fn a_token_count_falls_back_to_an_estimate_only_for_the_side_the_provider_left_out() {
    assert_eq!(estimate_tokens(""), 0);
    assert_eq!(estimate_tokens("   "), 0);
    // 11 characters, rounded up.
    assert_eq!(estimate_tokens("Hello world"), 3);

    let both = count_tokens(
        Some(&json!({ "usage": { "prompt_tokens": 100, "completion_tokens": 50 } })),
        "ignored",
        "ignored",
    );
    assert_eq!(both.prompt_tokens, 100);
    assert_eq!(both.completion_tokens, 50);
    assert_eq!(both.source(), "provider");

    let neither = count_tokens(None::<&serde_json::Value>, "Hello world", "Hi");
    assert_eq!(neither.prompt_tokens, 3);
    assert_eq!(neither.completion_tokens, 1);
    assert_eq!(neither.source(), "estimated");

    // Providers disagree on the field names as well as on where they live.
    let one_side = count_tokens(Some(&json!({ "input_tokens": 42 })), "ignored", "Hi");
    assert_eq!(one_side.prompt_tokens, 42);
    assert_eq!(one_side.completion_tokens, 1);
    assert_eq!(one_side.source(), "mixed");
}

/// A run that reached a token count has one whichever way it got there, and the total is derived
/// rather than reported.
#[test]
fn a_token_usage_totals_its_two_sides() {
    let usage = TokenUsage::new(100, 50);

    assert_eq!(usage.prompt_tokens, 100);
    assert_eq!(usage.completion_tokens, 50);
    assert_eq!(usage.total_tokens, 150);
}
