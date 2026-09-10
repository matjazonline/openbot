use super::super::{
    providers::ProviderRegistry,
    test_support::{KEYS, MODEL, Reply, check_request, model, response},
};
use super::*;
use crate::{
    entities::harness::{AgentCapabilitySpec, HarnessConfig, SubAgentScope},
    services::test_support::{ScriptedExchange, ScriptedResponse, scripted_scenario},
    use_cases::skill::{AgentCapabilityReader, StoredAgentCapabilities},
};
use serde_json::{Value, json};
use uuid::Uuid;

struct Reader;
#[async_trait::async_trait]
impl AgentCapabilityReader for Reader {
    async fn load_for_execution(
        &self,
        _: Uuid,
        _: Uuid,
    ) -> AppResult<Option<StoredAgentCapabilities>> {
        Ok(None)
    }
}
fn candidate(provider: &str, text: &str) -> Value {
    let mut value = response(provider, Reply::Text);
    match provider {
        "google" => value["candidates"][0]["content"]["parts"][0]["text"] = json!(text),
        "anthropic" => value["content"][0]["text"] = json!(text),
        "xai" => value["output"][0]["content"][0]["text"] = json!(text),
        _ => value["choices"][0]["message"]["content"] = json!(text),
    }
    value
}
fn compiled(provider: &str) -> CompiledRun {
    let spec = AgentCapabilitySpec {
        harness: HarnessKind::Rig,
        name: "Structured fixture".into(),
        system_prompt: "Answer the question".into(),
        provider: provider.into(),
        model: MODEL.into(),
        provider_base_url: None,
        skills: vec![],
        granted_tools: vec!["echo".into()],
        sub_agents: SubAgentScope::Restricted(vec![]),
        harness_config: HarnessConfig::empty(HarnessKind::Rig),
        response_contract: None,
    };
    CompiledRun::compile(
        &spec,
        CompileContext {
            facts: RuntimeContext {
                company_id: Uuid::new_v4(),
                agent_id: Uuid::new_v4(),
                recipient_role: RecipientRole::To,
                timezone: chrono_tz::UTC,
            },
            clock: Arc::new(SystemClock),
            capabilities: Arc::new(Reader),
            token_budget: 1_048_576,
            approvals: None,
            mcp: None,
        },
        None,
    )
    .unwrap()
}
async fn scenario(
    provider: &'static str,
    candidates: Vec<Value>,
    model_calls: u16,
) -> AppResult<AgentExecutionOutput> {
    let count = candidates.len();
    let exchanges = candidates
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            ScriptedExchange::new(
                move |request| {
                    check_request(request, provider, "test-secret")?;
                    let wire = request.body.to_string();
                    if !wire.contains("Draft 2020-12") || !wire.contains("final_status") {
                        return Err("missing schema instructions");
                    }
                    if index > 0 {
                        if request.body.get("tools").is_some_and(|tools| {
                            tools.as_array().is_none_or(|tools| !tools.is_empty())
                        }) {
                            return Err("repair tools must be absent");
                        }
                        if !wire.contains("complete JSON answer")
                            || !wire.contains("Validation reason")
                        {
                            return Err("missing safe repair feedback");
                        }
                        if wire.contains("test-secret") {
                            return Err("candidate credential leaked");
                        }
                    }
                    Ok(())
                },
                ScriptedResponse::json(value),
            )
        })
        .collect();
    let llm = scripted_scenario(exchanges).await;
    let registry = ProviderRegistry::standard().unwrap();
    let contract=serde_json::from_value(json!({"version":1,"format":"json_schema","schema":{"type":"object","properties":{"final_status":{"const":"done"}},"required":["final_status"],"additionalProperties":false}})).unwrap();
    let checkpoint = RunCheckpoint::new(
        RunIdentity {
            company_id: Uuid::new_v4(),
            task_id: Uuid::nil(),
            agent_id: Uuid::new_v4(),
            harness: HarnessKind::Rig,
            provider: provider.into(),
            model: MODEL.into(),
            capability_fingerprint: "a".repeat(64),
            response_contract: Some(contract),
        },
        "Finish the task".into(),
        RigExecutionPolicy {
            model_calls,
            ..Default::default()
        },
    )
    .unwrap();
    let result = Direct {
        inputs: Default::default(),
        compiled: compiled(provider),
        checkpoint,
        model: model(&registry, provider, "test-secret", &llm.base_url),
        secret: "test-secret",
        deadline: tokio::time::Instant::now() + std::time::Duration::from_secs(10),
    }
    .drive()
    .await;
    assert_eq!(
        llm.finish().await,
        Ok(count),
        "provider {provider}: {result:?}"
    );
    result
}
#[tokio::test]
async fn every_provider_validates_initial_and_both_repair_positions() {
    for provider in KEYS {
        for failures in 0..=2 {
            let mut candidates = (0..failures)
                .map(|_| candidate(provider, "invalid test-secret"))
                .collect::<Vec<_>>();
            candidates.push(candidate(provider, r#"{"final_status":"done"}"#));
            let output = scenario(provider, candidates, 8).await.unwrap();
            assert_eq!(output.content, r#"{"final_status":"done"}"#);
            assert!(output.structured.is_some());
            let diagnostics = &output.metadata.as_ref().unwrap()["execution_diagnostics"];
            assert_eq!(diagnostics["harness"], "rig");
            assert_eq!(diagnostics["provider"], provider);
            assert_eq!(diagnostics["model_calls"], failures + 1);
            assert_eq!(diagnostics["repair_call_count"], failures);
            assert_eq!(diagnostics["structured_validation"], "validated");
            assert_eq!(diagnostics["token_usage_source"], "reported");
            assert!(
                !output
                    .metadata
                    .as_ref()
                    .unwrap()
                    .to_string()
                    .contains("test-secret")
            );
            assert_eq!(output.token_usage.prompt_tokens, 2 * (failures + 1));
            assert_eq!(output.token_usage.completion_tokens, 3 * (failures + 1));
        }
    }
}
#[tokio::test]
async fn every_provider_exhausts_repairs_without_completing_or_executing_tools() {
    for provider in KEYS {
        let invalid = candidate(provider, "{}");
        let output = scenario(
            provider,
            vec![
                invalid.clone(),
                response(provider, Reply::ToolCall),
                invalid,
            ],
            8,
        )
        .await;
        assert!(matches!(
            output,
            Err(AppError::Execution(
                crate::app_error::ExecutionFailure::InvalidOutput
            ))
        ));
    }
}
#[tokio::test]
async fn repair_uses_the_original_model_call_budget() {
    let output = scenario("openai", vec![candidate("openai", "{}")], 1).await;
    assert!(matches!(
        output,
        Err(AppError::Execution(
            crate::app_error::ExecutionFailure::Budget
        ))
    ));
}

#[tokio::test]
async fn oversized_answers_are_replaced_without_truncating_a_completion() {
    let output = scenario(
        "openai",
        vec![
            candidate("openai", &"x".repeat(65_537)),
            candidate("openai", r#"{"final_status":"done"}"#),
        ],
        8,
    )
    .await
    .unwrap();
    assert_eq!(output.content, r#"{"final_status":"done"}"#);
    assert_eq!(output.token_usage.total_tokens, 10);
}

#[tokio::test]
async fn text_without_a_contract_remains_text_even_when_it_looks_like_json() {
    let llm = scripted_scenario(vec![ScriptedExchange::new(
        |_| Ok(()),
        ScriptedResponse::json(candidate("openai", r#"{"ordinary":"text"}"#)),
    )])
    .await;
    let registry = ProviderRegistry::standard().unwrap();
    let checkpoint = RunCheckpoint::new(
        RunIdentity {
            company_id: Uuid::new_v4(),
            task_id: Uuid::nil(),
            agent_id: Uuid::new_v4(),
            harness: HarnessKind::Rig,
            provider: "openai".into(),
            model: MODEL.into(),
            capability_fingerprint: "a".repeat(64),
            response_contract: None,
        },
        "Finish".into(),
        RigExecutionPolicy::default(),
    )
    .unwrap();
    let output = Direct {
        inputs: Default::default(),
        compiled: compiled("openai"),
        checkpoint,
        model: model(&registry, "openai", "test-secret", &llm.base_url),
        secret: "test-secret",
        deadline: tokio::time::Instant::now() + std::time::Duration::from_secs(10),
    }
    .drive()
    .await
    .unwrap();
    assert_eq!(llm.finish().await, Ok(1));
    assert!(output.structured.is_none());
    assert_eq!(output.content, r#"{"ordinary":"text"}"#);
}

#[tokio::test]
async fn diagnostics_are_included_in_the_candidate_size_gate_before_completion() {
    let provider = "openai";
    let oversized = json!("x".repeat(32_400)).to_string();
    let contract = serde_json::from_value(
        json!({"version":1,"format":"json_schema","schema":{"type":"string"}}),
    )
    .unwrap();
    let structured =
        StructuredResponse::validate(&contract, &oversized, None, &JsonResponseValidator)
            .unwrap()
            .unwrap();
    let without_diagnostics = AgentExecutionOutput {
        content: structured.body().into(),
        structured: Some(structured),
        disposition: AgentExecutionDisposition::Completed,
        token_usage: crate::entities::task::TokenUsage::new(2, 3),
        metadata: None,
    };
    assert!(serde_json::to_vec(&without_diagnostics).unwrap().len() < MAX_RESULT_BYTES);
    let llm = scripted_scenario(vec![
        ScriptedExchange::new(
            |_| Ok(()),
            ScriptedResponse::json(candidate(provider, &oversized)),
        ),
        ScriptedExchange::new(
            |request| {
                if !request.body.to_string().contains("output_limit") {
                    return Err("missing bounded size repair reason");
                }
                Ok(())
            },
            ScriptedResponse::json(candidate(provider, r#""done""#)),
        ),
    ])
    .await;
    let registry = ProviderRegistry::standard().unwrap();
    let checkpoint = RunCheckpoint::new(
        RunIdentity {
            company_id: Uuid::new_v4(),
            task_id: Uuid::nil(),
            agent_id: Uuid::new_v4(),
            harness: HarnessKind::Rig,
            provider: provider.into(),
            model: MODEL.into(),
            capability_fingerprint: "a".repeat(64),
            response_contract: Some(contract),
        },
        "Finish".into(),
        RigExecutionPolicy::default(),
    )
    .unwrap();
    let result = Direct {
        inputs: Default::default(),
        compiled: compiled(provider),
        checkpoint,
        model: model(&registry, provider, "test-secret", &llm.base_url),
        secret: "test-secret",
        deadline: tokio::time::Instant::now() + std::time::Duration::from_secs(10),
    }
    .drive()
    .await
    .unwrap();
    assert_eq!(llm.finish().await, Ok(2));
    assert_eq!(result.content, r#""done""#);
    assert_eq!(
        result.metadata.unwrap()["execution_diagnostics"]["repair_call_count"],
        1
    );
}
