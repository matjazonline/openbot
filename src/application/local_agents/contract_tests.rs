use super::*;
use crate::{application::local_agents::bounds::*, domain::local_agents::*};
use chrono::{Duration, Timelike, Utc};
use serde_json::json;
use sha2::Digest;

#[test]
fn cloudroom_contract_identities_validate_on_decode() {
    for value in ["", "../secret", "with space", "é", &"x".repeat(129)] {
        assert!(serde_json::from_value::<CloudroomThreadId>(json!(value)).is_err());
    }
    let uuid = uuid::Uuid::new_v4().to_string();
    assert!(serde_json::from_value::<MachineId>(json!(uuid)).is_ok());
    assert!(serde_json::from_value::<MachineId>(json!(uuid.to_uppercase())).is_err());
    assert!(serde_json::from_value::<MachineId>(json!(uuid::Uuid::nil())).is_err());
    for value in [0, i64::MAX as u64 + 1] {
        assert!(serde_json::from_value::<CredentialGeneration>(json!(value)).is_err());
        assert!(serde_json::from_value::<ConnectionGeneration>(json!(value)).is_err());
        assert!(serde_json::from_value::<EventSequence>(json!(value)).is_err());
    }
    assert_eq!(EventSequence::try_from(1).unwrap().get(), 1);
    assert_eq!(
        ModelId::try_from("provider/model:version".to_owned())
            .unwrap()
            .as_str(),
        "provider/model:version"
    );
}

#[test]
fn cloudroom_contract_commands_are_closed_and_controls_are_explicit() {
    assert!(
        serde_json::from_value::<LocalCommand>(json!({"kind":"shell","command":"pwd"})).is_err()
    );
    assert!(
        serde_json::from_value::<LocalCommand>(
            json!({"kind":"refresh_catalog","company_id":"foreign"})
        )
        .is_err()
    );
    assert!(serde_json::from_value::<LocalCommand>(json!({"kind":"stop_turn"})).is_err());
    let command = LocalCommand::StopThread {
        session_id: SessionId::new(uuid::Uuid::new_v4()).unwrap(),
        thread_id: CloudroomThreadId::try_from("thread_1".to_owned()).unwrap(),
    };
    assert!(!command.is_effect());
    assert_eq!(
        serde_json::from_value::<LocalCommand>(serde_json::to_value(&command).unwrap()).unwrap(),
        command
    );
    assert!(serde_json::from_value::<LocalProvider>(json!("opencode")).is_err());
    assert_eq!(
        serde_json::from_value::<LocalProvider>(json!("acp-opencode")).unwrap(),
        LocalProvider::OpenCode
    );
    assert!(serde_json::from_value::<LocalPermissions>(json!("bypass")).is_err());
    assert!(serde_json::from_value::<LocalWorkspace>(json!("remote")).is_err());
}

#[test]
fn cloudroom_bounds_prompt_counts_utf8_bytes_and_redacts_debug() {
    let accepted = LocalPrompt::try_from("é".repeat(MAX_PROMPT_BYTES / 2)).unwrap();
    assert_eq!(accepted.as_str().len(), MAX_PROMPT_BYTES);
    assert_eq!(format!("{accepted:?}"), "LocalPrompt([redacted])");
    for prompt in [
        "é".repeat(MAX_PROMPT_BYTES / 2 + 1),
        " ".into(),
        "a\0b".into(),
    ] {
        assert!(LocalPrompt::try_from(prompt).is_err());
    }
    assert!(
        serde_json::from_value::<LocalPrompt>(json!("x".repeat(MAX_PROMPT_BYTES + 1))).is_err()
    );
}

#[test]
fn cloudroom_bounds_deadline_rejects_elapsed_and_above_maximum() {
    let now = Utc::now();
    assert!(validate_deadline(now, now).is_err());
    assert!(validate_deadline(now, now - Duration::seconds(1)).is_err());
    assert!(validate_deadline(now, now + Duration::seconds(MAX_DEADLINE_SECONDS)).is_ok());
    assert!(
        validate_deadline(
            now,
            now + Duration::seconds(MAX_DEADLINE_SECONDS) + Duration::nanoseconds(1)
        )
        .is_err()
    );
}

#[test]
fn cloudroom_bounds_output_and_catalog_decode_at_their_limits() {
    assert!(LocalOutput::try_from("x".repeat(MAX_RESULT_BYTES)).is_ok());
    assert!(
        serde_json::from_value::<LocalOutput>(json!("x".repeat(MAX_RESULT_BYTES + 1))).is_err()
    );
    let projects = vec!["project_1"; MAX_PROJECTS];
    let models = vec![
        json!({"model":"provider/model","reasoning_levels":["none"]});
        MAX_MODELS_PER_PROVIDER
    ];
    let valid = json!({"projects":projects,"providers":[{"provider":"codex","permission_modes":["full","auto","accept_edits"],"models":models}]});
    assert!(serde_json::from_value::<LocalCatalog>(valid.clone()).is_ok());
    let mut too_many_projects = valid.clone();
    too_many_projects["projects"]
        .as_array_mut()
        .unwrap()
        .push(json!("project_2"));
    assert!(serde_json::from_value::<LocalCatalog>(too_many_projects).is_err());
    let mut too_many_models = valid.clone();
    too_many_models["providers"][0]["models"]
        .as_array_mut()
        .unwrap()
        .push(json!({"model":"model_2","reasoning_levels":["none"]}));
    assert!(serde_json::from_value::<LocalCatalog>(too_many_models).is_err());
    let provider =
        json!({"provider":"codex","permission_modes":["full","auto","accept_edits"],"models":[]});
    assert!(
        serde_json::from_value::<LocalCatalog>(
            json!({"projects":[],"providers":vec![provider.clone(); MAX_PROVIDERS]})
        )
        .is_ok()
    );
    assert!(
        serde_json::from_value::<LocalCatalog>(
            json!({"projects":[],"providers":vec![provider; MAX_PROVIDERS + 1]})
        )
        .is_err()
    );
}

#[test]
fn cloudroom_bounds_admission_preserves_control_capacity() {
    let create: LocalCommand = serde_json::from_value(json!({
        "kind":"create", "session_id":uuid::Uuid::new_v4(), "prompt":"hello",
        "harness":{"kind":"unconfigured"},
        "selection":{
            "machine_id":uuid::Uuid::new_v4(), "project_id":"project_1", "provider":"codex", "model":"gpt-model",
            "policy":{"workspace":"project_directory","permissions":"full","reasoning":"none"}
        }
    })).unwrap();
    assert!(
        validate_capacity(
            MAX_UNSETTLED_EFFECTS - 1,
            MAX_QUEUED_COMMANDS - RESERVED_CONTROL_COMMANDS - 1,
            &create
        )
        .is_ok()
    );
    assert!(validate_capacity(MAX_UNSETTLED_EFFECTS, 0, &create).is_err());
    assert!(
        validate_capacity(0, MAX_QUEUED_COMMANDS - RESERVED_CONTROL_COMMANDS, &create).is_err()
    );
    let control = LocalCommand::RefreshCatalog {};
    assert!(validate_capacity(MAX_UNSETTLED_EFFECTS, MAX_QUEUED_COMMANDS - 1, &control).is_ok());
    assert!(validate_capacity(0, MAX_QUEUED_COMMANDS, &control).is_err());
}

#[test]
fn cloudroom_contract_operations_freeze_digest_and_separate_dispatch_outcomes() {
    let deadline = Utc::now() + Duration::minutes(30);
    let operation = LocalAgentOperation::queued(
        OperationId::new(uuid::Uuid::new_v4()).unwrap(),
        LocalCommand::RefreshCatalog {},
        deadline,
    )
    .unwrap();
    let stored = serde_json::to_value(&operation).unwrap();
    let decoded: LocalAgentOperation = serde_json::from_value(stored.clone()).unwrap();
    assert_eq!(decoded.id(), operation.id());
    assert_eq!(decoded.payload_digest(), operation.payload_digest());
    assert_eq!(decoded.dispatch().delivery_state(), DeliveryState::Queued);
    assert_eq!(decoded.outcome(), None);
    let mut altered = stored.clone();
    altered["deadline"] = json!(operation.deadline() + Duration::seconds(1));
    assert!(serde_json::from_value::<LocalAgentOperation>(altered).is_err());
    let mut false_success = stored.clone();
    false_success["outcome"] = json!("succeeded");
    assert!(serde_json::from_value::<LocalAgentOperation>(false_success).is_err());
    let mut observed = stored.clone();
    observed["dispatch"] = json!({"kind":"catalog_snapshot","sequence":1});
    observed["outcome"] = json!("uncertain");
    let observed: LocalAgentOperation = serde_json::from_value(observed).unwrap();
    assert_eq!(
        observed.dispatch().delivery_state(),
        DeliveryState::Observed
    );
    assert_eq!(observed.outcome(), Some(OperationOutcome::Uncertain));
    assert_eq!(observed.payload_digest(), operation.payload_digest());
    for invalid in ["x".repeat(64), "a".repeat(63), "A".repeat(64)] {
        assert!(serde_json::from_value::<PayloadDigest>(json!(invalid)).is_err());
    }
    let mut invalid_evidence = stored;
    invalid_evidence["dispatch"] = json!({"kind":"accepted_locally","sequence":0});
    assert!(serde_json::from_value::<LocalAgentOperation>(invalid_evidence).is_err());
}

#[test]
fn cloudroom_contract_machine_capabilities_reference_an_installation_scoped_snapshot() {
    let machine = MachineId::new(uuid::Uuid::new_v4()).unwrap();
    let installation = InstallationId::new(uuid::Uuid::new_v4()).unwrap();
    let snapshot: CapabilitySnapshot = serde_json::from_value(json!({
        "machine_id":machine,"installation_id":installation,"sequence":42,
        "observed_at":Utc::now(),"catalog":{"projects":["project_1"],"providers":[]}
    }))
    .unwrap();
    let entity: LocalMachine = serde_json::from_value(json!({
        "id":machine,"company_id":uuid::Uuid::new_v4(),"installation_id":installation,
        "name":"local","state":"enabled","credential_generation":1,"last_seen_at":null,
        "capabilities_sequence":42
    }))
    .unwrap();
    assert_eq!(entity.capabilities_sequence, Some(snapshot.sequence));
    assert_eq!(entity.installation_id, snapshot.installation_id);
    assert_eq!(entity.id, snapshot.machine_id);
    assert_eq!(snapshot.catalog.projects[0].as_str(), "project_1");
}

#[test]
fn cloudroom_contract_effect_success_requires_its_thread_and_a_turn() {
    let operation = LocalAgentOperation::queued(
        OperationId::new(uuid::Uuid::new_v4()).unwrap(),
        LocalCommand::Send {
            session_id: SessionId::new(uuid::Uuid::new_v4()).unwrap(),
            thread_id: CloudroomThreadId::try_from("thr_target".to_owned()).unwrap(),
            prompt: LocalPrompt::try_from("work".to_owned()).unwrap(),
        },
        Utc::now() + Duration::minutes(30),
    )
    .unwrap();
    let mut stored = serde_json::to_value(&operation).unwrap();
    stored["outcome"] = json!("succeeded");
    stored["dispatch"] =
        json!({"kind":"native_identity","sequence":1,"thread_id":"thr_target","turn_id":null});
    assert!(serde_json::from_value::<LocalAgentOperation>(stored.clone()).is_err());
    stored["dispatch"]["turn_id"] = json!("turn_1");
    assert!(serde_json::from_value::<LocalAgentOperation>(stored.clone()).is_ok());
    stored["dispatch"]["thread_id"] = json!("thr_foreign");
    assert!(serde_json::from_value::<LocalAgentOperation>(stored).is_err());
}

#[test]
fn cloudroom_contract_catalog_success_requires_catalog_evidence() {
    let operation = LocalAgentOperation::queued(
        OperationId::new(uuid::Uuid::new_v4()).unwrap(),
        LocalCommand::RefreshCatalog {},
        Utc::now() + Duration::minutes(30),
    )
    .unwrap();
    let mut stored = serde_json::to_value(&operation).unwrap();
    stored["outcome"] = json!("succeeded");
    stored["dispatch"] = json!({"kind":"catalog_snapshot","sequence":1});
    let success: LocalAgentOperation = serde_json::from_value(stored.clone()).unwrap();
    assert_eq!(success.outcome(), Some(OperationOutcome::Succeeded));
    assert_eq!(success.dispatch().delivery_state(), DeliveryState::Observed);
    stored["dispatch"] =
        json!({"kind":"native_identity","sequence":1,"thread_id":"thr_unrelated","turn_id":null});
    assert!(serde_json::from_value::<LocalAgentOperation>(stored.clone()).is_err());
    stored["dispatch"] = json!({"kind":"accepted_locally","sequence":1});
    assert!(serde_json::from_value::<LocalAgentOperation>(stored.clone()).is_err());
    stored["dispatch"] = json!({"kind":"catalog_snapshot","sequence":0});
    assert!(serde_json::from_value::<LocalAgentOperation>(stored).is_err());

    let send = LocalAgentOperation::queued(
        operation.id(),
        LocalCommand::Send {
            session_id: SessionId::new(uuid::Uuid::new_v4()).unwrap(),
            thread_id: CloudroomThreadId::try_from("thr_target".to_owned()).unwrap(),
            prompt: LocalPrompt::try_from("work".to_owned()).unwrap(),
        },
        operation.deadline(),
    )
    .unwrap();
    let mut stored = serde_json::to_value(send).unwrap();
    stored["outcome"] = json!("succeeded");
    stored["dispatch"] = json!({"kind":"catalog_snapshot","sequence":1});
    assert!(serde_json::from_value::<LocalAgentOperation>(stored).is_err());
}

fn open_code_selection() -> ExecutionSelection {
    serde_json::from_value(json!({
        "machine_id":uuid::Uuid::new_v4(), "project_id":"proj_smoke", "provider":"acp-opencode",
        "model":"openai/gpt-6-luna", "policy":{
            "workspace":"project_directory", "permissions":"full", "reasoning":"low"
        }
    }))
    .unwrap()
}

#[test]
fn cloudroom_contract_reasoning_is_frozen_into_the_create_digest() {
    let selection = open_code_selection();
    assert_eq!(selection.policy.reasoning, LocalReasoningLevel::Low);
    let command = LocalCommand::Create {
        harness: crate::domain::local_agents::NativeHarnessSelection::Unconfigured,
        session_id: SessionId::new(uuid::Uuid::new_v4()).unwrap(),
        selection,
        prompt: LocalPrompt::try_from("smoke".to_owned()).unwrap(),
    };
    let operation = LocalAgentOperation::queued(
        OperationId::new(uuid::Uuid::new_v4()).unwrap(),
        command.clone(),
        Utc::now() + Duration::minutes(30),
    )
    .unwrap();
    let stored = serde_json::to_value(&operation).unwrap();
    let decoded: LocalAgentOperation = serde_json::from_value(stored.clone()).unwrap();
    assert_eq!(decoded.command(), &command);
    assert_eq!(decoded.payload_digest(), operation.payload_digest());
    let mut tampered = stored;
    tampered["command"]["selection"]["policy"]["reasoning"] = json!("high");
    assert!(serde_json::from_value::<LocalAgentOperation>(tampered).is_err());
    let mut other = command;
    if let LocalCommand::Create { selection, .. } = &mut other {
        selection.policy.reasoning = LocalReasoningLevel::High;
    }
    let other = LocalAgentOperation::queued(operation.id(), other, operation.deadline()).unwrap();
    assert_ne!(operation.payload_digest(), other.payload_digest());
    let mut invalid = serde_json::to_value(open_code_selection()).unwrap();
    invalid["policy"]["reasoning"] = json!("invented");
    assert!(serde_json::from_value::<ExecutionSelection>(invalid.clone()).is_err());
    invalid["policy"]
        .as_object_mut()
        .unwrap()
        .remove("reasoning");
    assert!(serde_json::from_value::<ExecutionSelection>(invalid).is_err());
}

#[test]
fn cloudroom_contract_catalog_refuses_unsupported_model_reasoning() {
    let mut selection = open_code_selection();
    let raw = json!({"projects":["proj_smoke"],"providers":[{
        "provider":"acp-opencode","permission_modes":["full","auto","accept_edits"],"models":[{"model":"openai/gpt-6-luna","reasoning_levels":["low","high"]}]
    }]});
    let catalog: LocalCatalog = serde_json::from_value(raw.clone()).unwrap();
    assert!(catalog.validate_selection(&selection).is_ok());
    selection.policy.reasoning = LocalReasoningLevel::None;
    assert!(catalog.validate_selection(&selection).is_err());
    selection.policy.reasoning = LocalReasoningLevel::Low;
    selection.provider = LocalProvider::Codex;
    assert!(catalog.validate_selection(&selection).is_err());
    let mut too_many = raw;
    too_many["providers"][0]["models"][0]["reasoning_levels"] = json!(vec!["low"; 9]);
    assert!(serde_json::from_value::<LocalCatalog>(too_many).is_err());
}

fn forged_deadline_record(
    operation: &LocalAgentOperation,
    deadline: chrono::DateTime<Utc>,
) -> serde_json::Value {
    let mut record = serde_json::to_value(operation).unwrap();
    record["deadline"] = json!(deadline);
    // Supply an otherwise correct hash: representability must be checked independently.
    record["payload_digest"] = json!(format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&(operation.command(), deadline)).unwrap())
    ));
    record
}

#[test]
fn cloudroom_contract_deadline_precision_is_frozen_before_hashing() {
    let deadline = (Utc::now() + Duration::minutes(30))
        .with_nanosecond(123_456_789)
        .unwrap();
    let operation = LocalAgentOperation::queued(
        OperationId::new(uuid::Uuid::new_v4()).unwrap(),
        LocalCommand::RefreshCatalog {},
        deadline,
    )
    .unwrap();
    assert_eq!(operation.deadline().timestamp_subsec_nanos(), 123_456_000);
    assert!(operation.deadline() <= deadline);
    let recovered: LocalAgentOperation =
        serde_json::from_value(serde_json::to_value(&operation).unwrap()).unwrap();
    assert_eq!(recovered.payload_digest(), operation.payload_digest());
    assert!(
        serde_json::from_value::<LocalAgentOperation>(forged_deadline_record(&operation, deadline))
            .is_err()
    );
}

#[test]
fn cloudroom_contract_aligned_leap_seconds_are_not_representable_operation_deadlines() {
    let leap: chrono::DateTime<Utc> = "2026-11-07T22:00:60.123456Z".parse().unwrap();
    assert_eq!(leap.timestamp_subsec_nanos() % 1_000, 0);
    assert!(
        LocalAgentOperation::queued(
            OperationId::new(uuid::Uuid::new_v4()).unwrap(),
            LocalCommand::RefreshCatalog {},
            leap
        )
        .is_err()
    );
    let operation = LocalAgentOperation::queued(
        OperationId::new(uuid::Uuid::new_v4()).unwrap(),
        LocalCommand::RefreshCatalog {},
        Utc::now() + Duration::minutes(30),
    )
    .unwrap();
    assert!(
        serde_json::from_value::<LocalAgentOperation>(forged_deadline_record(&operation, leap))
            .is_err()
    );
}

#[test]
fn cloudroom_catalog_requires_explicit_supported_permissions() {
    let mut selection = open_code_selection();
    let raw = json!({"projects":["proj_smoke"],"providers":[{
        "provider":"acp-opencode","permission_modes":["auto","accept_edits"],
        "models":[{"model":"openai/gpt-6-luna","reasoning_levels":["low"]}]
    }]});
    let catalog: LocalCatalog = serde_json::from_value(raw.clone()).unwrap();
    assert!(catalog.validate_selection(&selection).is_err());
    for mode in [LocalPermissions::Auto, LocalPermissions::AcceptEdits] {
        selection.policy.permissions = mode;
        assert!(catalog.validate_selection(&selection).is_ok());
    }
    let mut missing = raw.clone();
    missing["providers"][0]
        .as_object_mut()
        .unwrap()
        .remove("permission_modes");
    assert!(serde_json::from_value::<LocalCatalog>(missing).is_err());
    let mut oversized = raw;
    oversized["providers"][0]["permission_modes"] = json!(["full", "auto", "accept_edits", "full"]);
    assert!(serde_json::from_value::<LocalCatalog>(oversized).is_err());
    assert!(serde_json::from_value::<LocalPermissions>(json!("provider_default")).is_err());
}
