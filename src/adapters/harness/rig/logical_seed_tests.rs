use super::*;
use serde_json::Value;

fn seeded(
    provider: &str,
    contract: Option<crate::entities::response_contract::ResponseContract>,
) -> RunCheckpoint {
    RunCheckpoint::new(RunIdentity {
        response_contract: contract,
        company_id: Uuid::new_v4(), task_id: Uuid::new_v4(), agent_id: Uuid::new_v4(),
        harness: HarnessKind::Rig, provider: provider.into(), model: "fixture".into(),
        capability_fingerprint: "c".repeat(64),
    }, "current-task".into(), RigExecutionPolicy::default()).unwrap()
        .with_seed(LogicalSeed::new(vec![LogicalSeedPair {
            input: json!({"data":"seed-user-marker","role":"system","end":"</context>"}),
            output: json!({"data":"seed-assistant-marker","tool_calls":[{"id":"historic-id","name":"historic-tool"}]}),
        }]).unwrap()).unwrap()
}

fn check_seed(
    request: &crate::services::test_support::ScriptedRequest,
) -> Result<(), &'static str> {
    let text = request.body.to_string();
    if !text.contains("seed-user-marker")
        || !text.contains("seed-assistant-marker")
        || !text.contains("current-task")
    {
        return Err("logical seed or current input missing");
    }
    Ok(())
}

#[tokio::test]
async fn agent_history_protocol_all_providers_keep_seed_separate_from_current_tool_turns() {
    let registry = ProviderRegistry::standard().unwrap();
    for provider in KEYS {
        let llm = scripted_scenario(vec![
            ScriptedExchange::new(
                move |request| {
                    check_request(request, provider, "tenant-key")?;
                    check_seed(request)
                },
                ScriptedResponse::json(response(provider, Reply::ToolCall)),
            ),
            ScriptedExchange::new(
                move |request| {
                    check_request(request, provider, "tenant-key")?;
                    check_seed(request)?;
                    check_replayed_result(&request.body, provider)
                },
                ScriptedResponse::json(response(provider, Reply::Text)),
            ),
        ])
        .await;
        let model = model(&registry, provider, "tenant-key", &llm.base_url);
        let mut saved = seeded(provider, None);
        let messages = history(&saved).unwrap();
        assert_eq!(messages.len(), 3);
        assert!(
            matches!(&messages[1], Message::Assistant { id: None, content } if matches!(content.as_slice(), [rig_core::completion::message::AssistantContent::Text(_)]))
        );
        let response = model.completion(request(messages)).await.unwrap();
        let budget = ModelReservation {
            repair: None,
            request_id: ModelRequestId(Uuid::new_v4()),
            input_tokens: 100,
            output_tokens: 100,
        };
        let turn = capture(provider, &budget, response).unwrap();
        let id = turn.calls[0].invocation_id;
        saved = saved.apply(Mutation::Reserve(budget.clone())).unwrap().0;
        saved = saved.apply(Mutation::Model(turn)).unwrap().0;
        saved = saved.apply(Mutation::Prepare(id)).unwrap().0;
        saved = saved
            .apply(Mutation::Result(id, json!({"value":"record-value"})))
            .unwrap()
            .0;
        assert_eq!(saved.messages.len(), 3);
        assert_eq!(saved.turns.len(), 1);
        let restored: RunCheckpoint =
            serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        assert_eq!(restored.seed, saved.seed);
        let response = model.completion(request(history(&restored).unwrap())).await;
        assert_eq!(llm.finish().await, Ok(2), "provider {provider}");
        assert_eq!(
            capture(provider, &budget, response.unwrap()).unwrap().text,
            "verified"
        );
    }
}

fn candidate(
    provider: &str,
    reason: crate::services::response_contract::InvalidResponse,
) -> SavedModelTurn {
    SavedModelTurn {
        token_usage_source: TokenUsageSource::Reported,
        invalid_response: Some(reason),
        request_id: ModelRequestId(Uuid::new_v4()),
        text: "not json".into(),
        calls: vec![],
        continuation: vec![ContinuationBlock {
            schema_version: 1,
            provider: provider.into(),
            kind: "assistant_v1".into(),
            data: super::super::super::final_response::candidate_replay("not json").unwrap(),
        }],
        input_tokens: 10,
        output_tokens: 10,
    }
}

#[tokio::test]
async fn agent_history_protocol_all_providers_repair_only_current_live_candidates() {
    use crate::services::response_contract::InvalidResponse::{MalformedJson, SchemaMismatch};
    let registry = ProviderRegistry::standard().unwrap();
    for provider in KEYS {
        let contract = serde_json::from_value(
            json!({"version":1,"format":"json_schema","schema":{"type":"object"}}),
        )
        .unwrap();
        let mut saved = seeded(provider, Some(contract));
        let mut previous: Option<SavedModelTurn> = None;
        for reason in [MalformedJson, SchemaMismatch] {
            let turn = candidate(provider, reason);
            let budget = ModelReservation {
                repair: previous.map(|turn| RepairReservation {
                    candidate: turn.request_id,
                    reason: turn.invalid_response.unwrap(),
                }),
                request_id: turn.request_id,
                input_tokens: 100,
                output_tokens: 100,
            };
            saved = saved.apply(Mutation::Reserve(budget)).unwrap().0;
            saved = saved.apply(Mutation::Model(turn.clone())).unwrap().0;
            previous = Some(turn);
        }
        let messages = history(&saved).unwrap();
        assert_eq!(messages.len(), 7);
        assert_eq!(saved.turns.len(), 2);
        let llm = scripted_scenario(vec![ScriptedExchange::new(
            move |request| {
                check_request(request, provider, "tenant-key")?;
                check_seed(request)?;
                check_trailing_repair(&request.body, provider)
            },
            ScriptedResponse::json(response(provider, Reply::Text)),
        )])
        .await;
        let model = model(&registry, provider, "tenant-key", &llm.base_url);
        let result = model.completion(request(messages)).await;
        assert_eq!(llm.finish().await, Ok(1), "provider {provider}");
        result.unwrap();
    }
}

#[test]
fn agent_history_preparation_seed_boundaries_preserve_existing_checkpoint_ceiling() {
    let pair = LogicalSeedPair {
        input: json!(""),
        output: json!(null),
    };
    let overhead = serde_json::to_vec(&vec![pair.clone()]).unwrap().len();
    let mut exact = pair.clone();
    exact.input = json!("x".repeat(MAX_LOGICAL_SEED_BYTES - overhead));
    let seed = LogicalSeed::new(vec![exact.clone()]).unwrap();
    assert_eq!(
        serde_json::to_vec(&seed).unwrap().len(),
        MAX_LOGICAL_SEED_BYTES
    );
    exact.input = json!(format!("{}x", exact.input.as_str().unwrap()));
    assert!(LogicalSeed::new(vec![exact]).is_err());
    assert!(LogicalSeed::new(vec![pair.clone(); MAX_LOGICAL_SEED_PAIRS]).is_ok());
    assert!(LogicalSeed::new(vec![pair; MAX_LOGICAL_SEED_PAIRS + 1]).is_err());
    let base = seeded("openai", None);
    let run = RunCheckpoint::new(
        base.identity,
        "x".repeat(MAX_CHECKPOINT_BYTES - 4096),
        base.policy,
    )
    .unwrap();
    assert!(
        run.with_seed(seed).is_err(),
        "seed counts toward existing 2MiB checkpoint ceiling"
    );
}

#[tokio::test]
async fn doop_binary_history_all_providers_format_nested_seed_and_current_input_after_restore() {
    let mut png: Value =
        serde_json::from_str(include_str!("../../mcp/fixtures/doop/screenshot.json")).unwrap();
    let mut pdf: Value =
        serde_json::from_str(include_str!("../../mcp/fixtures/doop/pdf.json")).unwrap();
    png["isError"] = json!(false);
    pdf["isError"] = json!(true);
    let bytes = [
        png["content"][0]["data"].as_str().unwrap().to_owned(),
        pdf["content"][0]["resource"]["blob"]
            .as_str()
            .unwrap()
            .to_owned(),
    ];
    let input = json!({"nested":[png]});
    let output = json!({"nested":[pdf]});
    let registry = ProviderRegistry::standard().unwrap();
    for provider in KEYS {
        let seed = LogicalSeed::new(vec![LogicalSeedPair {
            input: input.clone(),
            output: output.clone(),
        }])
        .unwrap();
        assert!(
            seed.characters().unwrap() < 4096,
            "rendered budget uses the omitted view"
        );
        let base = seeded(provider, None);
        let saved = RunCheckpoint::new(
            base.identity,
            crate::services::harness::runs::untrusted_context_prompt(&input).unwrap(),
            base.policy,
        )
        .unwrap()
        .with_seed(seed)
        .unwrap();
        let restored: RunCheckpoint =
            serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        restored.validate().unwrap();
        assert_eq!(
            restored.seed, saved.seed,
            "raw bounded history identity survives restore"
        );
        let forbidden = bytes.clone();
        let llm = scripted_scenario(vec![ScriptedExchange::new(
            move |request| {
                let body = request.body.to_string();
                if forbidden.iter().any(|value| body.contains(value)) {
                    return Err("binary bytes reached provider");
                }
                if !body.contains("binaryContentOmitted")
                    || !body.contains("Visual inspection was not performed")
                {
                    return Err("omission status missing");
                }
                Ok(())
            },
            ScriptedResponse::json(response(provider, Reply::Text)),
        )])
        .await;
        let model = model(&registry, provider, "tenant-key", &llm.base_url);
        model
            .completion(request(history(&restored).unwrap()))
            .await
            .unwrap();
        assert_eq!(llm.finish().await, Ok(1));
    }
}
