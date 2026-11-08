use super::*;
use crate::application::agent::ModelSettings;
use crate::application::workflow::registry::ModelStep;
use crate::domain::workflow::{ExecutionId, TypeName};
use crate::entities::access_level::AccessLevel;
use crate::entities::agent_capability::{AgentId, AgentRevisionId};
use crate::entities::response_contract::ResponseContract;
use serde_json::{Value, json};
use uuid::Uuid;

fn exchange(output: Value, level: i16) -> AgentLogicalExchange {
    AgentLogicalExchange {
        order: AgentExchangeOrder::try_from(1).unwrap(),
        conversation: AgentConversationId::new(Uuid::from_u128(1)),
        exchange: AgentExchangeId::for_execution(ExecutionId::new(Uuid::from_u128(2))),
        input: AgentLogicalInput::new(json!({"task":[1,null,"private input"]})).unwrap(),
        outcome: AgentExchangeOutcome::Completed(AgentLogicalOutput::new(output).unwrap()),
        access_level: AccessLevel::try_from(level).unwrap(),
        provenance: AgentExchangeProvenance {
            agent: AgentId::new(Uuid::from_u128(3)),
            revision: AgentRevisionId::new(Uuid::from_u128(4)),
            model: ModelSettings {
                provider: TypeName::parse("scripted").unwrap(),
                model: "m1".into(),
                max_output_tokens: 1000,
            },
            output_contract: ResponseContract::parse(
                r#"{"version":1,"format":"json_schema","schema":true}"#,
            )
            .unwrap(),
        },
    }
}

#[test]
fn agent_history_contract_exact_structured_values_roundtrip_without_prompt_expansion() {
    for value in [
        json!(null),
        json!(false),
        json!(0),
        json!(""),
        json!([]),
        json!({}),
        json!([{"history":"ordinary business data"},null]),
    ] {
        let input = AgentLogicalInput::new(value.clone()).unwrap();
        let stored = serde_json::to_value(&input).unwrap();
        assert_eq!(stored, value);
        let restored: AgentLogicalInput = serde_json::from_value(stored).unwrap();
        assert_eq!(restored.value(), &value);
        let output = AgentLogicalOutput::new(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(output).unwrap(), value);
    }
}

#[test]
fn agent_history_contract_value_budget_is_enforced_at_construction_and_decode() {
    assert!(AgentLogicalInput::new(json!("x".repeat(MAX_AGENT_HISTORY_VALUE_BYTES - 2))).is_ok());
    let too_large = json!("x".repeat(MAX_AGENT_HISTORY_VALUE_BYTES - 1));
    assert!(AgentLogicalInput::new(too_large.clone()).is_err());
    assert!(serde_json::from_value::<AgentLogicalInput>(too_large.clone()).is_err());
    assert!(serde_json::from_value::<AgentLogicalOutput>(too_large).is_err());
}

#[test]
fn agent_history_contract_terminal_states_do_not_follow_workflow_or_delivery_state() {
    use AgentExchangeState::*;
    assert!(Queued.permits(Active));
    for terminal in [Completed, Failed, Cancelled] {
        assert!(Active.permits(terminal));
        assert!(terminal.permits(terminal));
        assert!(!terminal.permits(Queued));
        assert!(!terminal.permits(Active));
    }
    assert!(!Completed.permits(Failed));
    assert!(!Completed.permits(Cancelled));
    assert!(!Queued.permits(Completed));
}

#[test]
fn agent_history_contract_accepted_null_is_distinct_from_failure_and_cancellation() {
    let mut record = exchange(Value::Null, 0);
    assert_eq!(
        record
            .completed_output_at(AccessLevel::PUBLIC)
            .unwrap()
            .value(),
        &Value::Null
    );
    for outcome in [
        AgentExchangeOutcome::Queued,
        AgentExchangeOutcome::Active,
        AgentExchangeOutcome::Failed,
        AgentExchangeOutcome::Cancelled,
    ] {
        record.outcome = outcome;
        assert!(record.completed_output_at(AccessLevel::PUBLIC).is_none());
    }
}

#[test]
fn agent_history_contract_schema_and_revision_remain_original_provenance() {
    let record = exchange(json!([1, false]), 0);
    let old = serde_json::to_value(&record.provenance).unwrap();
    let saved: AgentExchangeProvenance = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(saved.output_contract.schema(), &json!(true));
    assert_eq!(saved.revision, record.provenance.revision);
    let newer = ResponseContract::parse(
        r#"{"version":1,"format":"json_schema","schema":{"type":"string"}}"#,
    )
    .unwrap();
    assert_ne!(saved.output_contract.schema(), newer.schema());
    assert_eq!(
        record
            .completed_output_at(AccessLevel::PUBLIC)
            .unwrap()
            .value(),
        &json!([1, false])
    );
    assert_eq!(serde_json::to_value(saved).unwrap(), old);
}

#[test]
fn agent_history_contract_only_agent_run_is_eligible() {
    assert!(agent_history_eligible(ModelStep::AgentRun));
    assert!(!agent_history_eligible(ModelStep::Classify));
    assert!(!agent_history_eligible(ModelStep::Decide));
}

#[test]
fn agent_history_identity_activation_is_stable_across_attempts_and_distinct_for_repeats() {
    let first = ExecutionId::new(Uuid::from_u128(12));
    assert_eq!(
        AgentExchangeId::for_execution(first),
        AgentExchangeId::for_execution(first)
    );
    assert_ne!(
        AgentExchangeId::for_execution(first),
        AgentExchangeId::for_execution(ExecutionId::new(Uuid::from_u128(13)))
    );
    let request = AgentHistorySessionRef {
        from_run: crate::domain::workflow::RunId::new(Uuid::from_u128(14)),
    };
    assert_eq!(
        serde_json::from_value::<AgentHistorySessionRef>(serde_json::to_value(request).unwrap())
            .unwrap(),
        request
    );
    assert!(
        serde_json::from_value::<AgentHistorySessionRef>(
            json!({"from_run":request.from_run,"conversation_id":Uuid::new_v4()})
        )
        .is_err()
    );
}

#[test]
fn agent_history_level_visibility_changes_without_forking_or_hidden_pair_markers() {
    let record = exchange(json!({"pricing":"level five"}), 5);
    let conversation = record.conversation;
    assert!(record.completed_output_at(AccessLevel::PUBLIC).is_none());
    assert!(
        record
            .completed_output_at(AccessLevel::try_from(5).unwrap())
            .is_some()
    );
    assert!(record.completed_output_at(AccessLevel::PUBLIC).is_none());
    assert_eq!(record.conversation, conversation);
    assert_eq!(record.access_level, AccessLevel::try_from(5).unwrap());
}

#[test]
fn agent_history_contract_order_is_positive_bounded_and_survives_decode() {
    for value in [0, i64::MAX as u64 + 1, u64::MAX] {
        assert!(AgentExchangeOrder::try_from(value).is_err());
        assert!(serde_json::from_value::<AgentExchangeOrder>(json!(value)).is_err());
    }
    let max = AgentExchangeOrder::try_from(i64::MAX as u64).unwrap();
    assert_eq!(
        serde_json::from_value::<AgentExchangeOrder>(serde_json::to_value(max).unwrap()).unwrap(),
        max
    );
}
