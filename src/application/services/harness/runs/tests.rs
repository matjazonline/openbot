use super::*;
use serde_json::json;

fn run() -> RunCheckpoint {
    RunCheckpoint::new(
        RunIdentity {
            response_contract: None,
            company_id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            agent_id: Uuid::new_v4(),
            harness: HarnessKind::Rig,
            provider: "openai".into(),
            model: "fixture".into(),
            capability_fingerprint: "a".repeat(64),
        },
        "fixture prompt".into(),
        RigExecutionPolicy::default(),
    )
    .unwrap()
}
fn reservation() -> ModelReservation {
    ModelReservation {
        repair: None,
        request_id: ModelRequestId(Uuid::new_v4()),
        input_tokens: 100,
        output_tokens: 100,
    }
}
fn call(name: &str) -> SavedToolCall {
    SavedToolCall {
        invocation_id: InvocationId(Uuid::new_v4()),
        call_id: format!("call_{name}"),
        item_id: Some(format!("item_{name}")),
        tool_id: "read_resource".into(),
        arguments: json!({"skill_uri":name}),
    }
}
fn turn(reservation: &ModelReservation, calls: Vec<SavedToolCall>) -> SavedModelTurn {
    SavedModelTurn {
        token_usage_source: crate::services::harness::runs::TokenUsageSource::Reported,
        invalid_response: None,
        request_id: reservation.request_id,
        text: String::new(),
        calls,
        continuation: vec![],
        input_tokens: 70,
        output_tokens: 50,
    }
}
#[test]
fn a_partial_batch_round_trips_without_another_model_request() {
    let initial = run();
    let budget = reservation();
    let (reserved, _) = initial.apply(Mutation::Reserve(budget.clone())).unwrap();
    let first = call("one");
    let second = call("two");
    let (saved, _) = reserved
        .apply(Mutation::Model(turn(
            &budget,
            vec![first.clone(), second.clone()],
        )))
        .unwrap();
    assert!(
        saved
            .apply(Mutation::Prepare(second.invocation_id))
            .is_err()
    );
    let (prepared, _) = saved.apply(Mutation::Prepare(first.invocation_id)).unwrap();
    let (completed, _) = prepared
        .apply(Mutation::Result(first.invocation_id, json!({"saved":1})))
        .unwrap();
    let restored: RunCheckpoint =
        serde_json::from_value(serde_json::to_value(&completed).unwrap()).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored, completed);
    assert_eq!(restored.pending_call(), Some(&second));
    assert!(restored.apply(Mutation::Reserve(reservation())).is_err());
    let (replay, changed) = restored
        .apply(Mutation::Result(first.invocation_id, json!({"saved":1})))
        .unwrap();
    assert!(!changed);
    assert_eq!(replay.revision, restored.revision);
    assert!(
        restored
            .apply(Mutation::Result(first.invocation_id, json!({"changed":1})))
            .is_err()
    );
    let ConversationMessage::Tool {
        call_id, item_id, ..
    } = restored.messages.last().unwrap()
    else {
        panic!("tool result");
    };
    assert_eq!(call_id, &first.call_id);
    assert_eq!(item_id, &first.item_id);
}
#[test]
fn unknown_usage_and_retries_do_not_restore_allowance() {
    let mut saved = run();
    for _ in 0..saved.policy.model_calls {
        saved = saved.apply(Mutation::Reserve(reservation())).unwrap().0;
        saved = serde_json::from_value(serde_json::to_value(saved).unwrap()).unwrap();
    }
    assert!(saved.apply(Mutation::Reserve(reservation())).is_err());
    let mut exhausted = run();
    exhausted.policy.total_output_tokens = 100;
    exhausted = exhausted.apply(Mutation::Reserve(reservation())).unwrap().0;
    assert!(exhausted.apply(Mutation::Reserve(reservation())).is_err());
}
#[test]
fn malformed_batches_fail_before_any_invocation_is_prepared() {
    let budget = reservation();
    let saved = run().apply(Mutation::Reserve(budget.clone())).unwrap().0;
    let first = call("one");
    assert!(
        saved
            .apply(Mutation::Model(turn(&budget, vec![first.clone(), first])))
            .is_err()
    );
    let mut oversized = call("oversized");
    oversized.arguments = json!({"body":"x".repeat(MAX_ARGUMENT_BYTES)});
    assert!(
        saved
            .apply(Mutation::Model(turn(&budget, vec![oversized])))
            .is_err()
    );
    assert!(saved.invocations.is_empty());
    assert!(saved.turns.is_empty());
}
#[test]
fn checkpoint_schema_and_server_ceilings_are_enforced() {
    let mut saved = run();
    saved.schema_version = 2;
    assert!(saved.validate().is_err());
    saved.schema_version = 1;
    saved.policy.model_calls = 17;
    assert!(saved.validate().is_err());
    saved.policy = RigExecutionPolicy::default();
    saved.messages = vec![ConversationMessage::User {
        text: "x".repeat(MAX_CHECKPOINT_BYTES),
    }];
    assert!(saved.validate().is_err());
}

#[test]
fn execution_allowances_and_replay_counts_survive_restart_and_cannot_reset() {
    let mut saved = run();
    for _ in 0..(MAX_ACTIVE_EXECUTION_MS / MAX_CLAIM_MS) {
        let reservation = ExecutionReservation {
            generation: Uuid::new_v4(),
            allowance_ms: MAX_CLAIM_MS,
            started_at: chrono::Utc::now(),
            replayed_invocations: 0,
        };
        saved = saved
            .apply(Mutation::Execution(reservation.clone()))
            .unwrap()
            .0;
        let (same, changed) = saved.apply(Mutation::Execution(reservation)).unwrap();
        assert!(!changed);
        saved = serde_json::from_value(serde_json::to_value(same).unwrap()).unwrap();
    }
    assert!(matches!(
        saved.apply(Mutation::Execution(ExecutionReservation {
            generation: Uuid::new_v4(),
            allowance_ms: 1,
            started_at: chrono::Utc::now(),
            replayed_invocations: 0
        })),
        Err(AppError::Execution(
            crate::app_error::ExecutionFailure::Budget
        ))
    ));
    let diagnostics = serde_json::to_string(&RunDiagnostics::from(&saved)).unwrap();
    assert!(!diagnostics.contains("fixture prompt"));
    assert!(!diagnostics.contains("arguments"));
}

#[test]
fn maximum_turn_configuration_has_room_for_sixteen_output_reservations() {
    let mut saved = run();
    saved.policy.model_calls = 16;
    for _ in 0..16 {
        saved = saved
            .apply(Mutation::Reserve(ModelReservation {
                repair: None,
                request_id: ModelRequestId(Uuid::new_v4()),
                input_tokens: 100,
                output_tokens: saved.policy.request_output_tokens,
            }))
            .unwrap()
            .0;
    }
    assert!(saved.apply(Mutation::Reserve(reservation())).is_err());
    assert_eq!(
        saved
            .reservations
            .iter()
            .map(|r| r.output_tokens)
            .sum::<u64>(),
        saved.policy.total_output_tokens
    );
}

#[test]
fn repair_reservations_are_atomic_bounded_and_survive_unknown_responses() {
    use crate::services::response_contract::{InvalidResponse, invalid_output};
    let mut saved = run();
    saved.identity.response_contract = Some(
        serde_json::from_value(
            json!({"version":1,"format":"json_schema","schema":{"type":"object"}}),
        )
        .unwrap(),
    );
    saved.contract_fingerprint = saved
        .identity
        .response_contract
        .as_ref()
        .map(|contract| contract.fingerprint());
    let first = reservation();
    saved = saved.apply(Mutation::Reserve(first.clone())).unwrap().0;
    let mut candidate = turn(&first, vec![]);
    candidate.invalid_response = Some(InvalidResponse::MalformedJson);
    candidate.text = "invalid".into();
    saved = saved.apply(Mutation::Model(candidate)).unwrap().0;
    assert!(saved.apply(Mutation::Reserve(reservation())).is_err());
    for _ in 0..2 {
        let mut repair = reservation();
        repair.repair = Some(RepairReservation {
            candidate: first.request_id,
            reason: InvalidResponse::MalformedJson,
        });
        saved = saved.apply(Mutation::Reserve(repair.clone())).unwrap().0;
        let revision = saved.revision;
        let (replay, changed) = saved.apply(Mutation::Reserve(repair)).unwrap();
        assert!(!changed);
        assert_eq!(replay.revision, revision);
        saved = serde_json::from_value(serde_json::to_value(&replay).unwrap()).unwrap();
        saved.validate().unwrap();
    }
    let mut third = reservation();
    third.repair = Some(RepairReservation {
        candidate: first.request_id,
        reason: InvalidResponse::MalformedJson,
    });
    assert_eq!(
        saved
            .apply(Mutation::Reserve(third))
            .unwrap_err()
            .to_string(),
        invalid_output().to_string()
    );
    assert_eq!(saved.reservations.len(), 3);
    assert_eq!(saved.repair_count(), 2);
    let terminal = saved.apply(Mutation::InvalidOutput).unwrap().0;
    assert_eq!(terminal.state, RunState::InvalidOutput);
    assert!(!terminal.apply(Mutation::InvalidOutput).unwrap().1);
}

#[test]
fn usage_replaces_each_reservation_once_and_retains_unknown_paid_calls_after_restart() {
    let mut saved = run();
    let unknown = reservation();
    saved = saved.apply(Mutation::Reserve(unknown)).unwrap().0;
    let completed = reservation();
    saved = saved.apply(Mutation::Reserve(completed.clone())).unwrap().0;
    let response = turn(&completed, vec![]);
    saved = saved.apply(Mutation::Model(response.clone())).unwrap().0;
    saved = saved.apply(Mutation::Model(response)).unwrap().0;
    let restored: RunCheckpoint =
        serde_json::from_value(serde_json::to_value(saved).unwrap()).unwrap();
    let usage = RunUsage::from(&restored);
    assert_eq!(usage.tokens.prompt_tokens, 170);
    assert_eq!(usage.tokens.completion_tokens, 150);
    assert_eq!(usage.source, TokenUsageSource::Mixed);
    assert_eq!(usage.unresolved_calls, 1);
    assert_eq!(usage.unreported_calls, 1);
}
