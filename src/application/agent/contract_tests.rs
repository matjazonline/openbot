use super::*;
use crate::domain::entities::agent_capability::*;
use crate::domain::workflow::{RunId, VersionId, WorkflowId};
use serde_json::json;
use uuid::Uuid;

fn definition() -> ResolvedAgentDefinition {
    ResolvedAgentDefinition {
        company_id: CompanyId::new(Uuid::new_v4()),
        agent_id: AgentId::new(Uuid::new_v4()),
        key: AgentKey::parse("assistant").unwrap(),
        instructions: "Use the configured capabilities.".into(),
        model: ModelSettings {
            provider: TypeName::parse("openai").unwrap(),
            model: "scripted-model".into(),
            max_output_tokens: 1000,
        },
        tools: vec![ResolvedAgentTool {
            binding: AgentToolBinding::builtin(AgentBuiltinTool::SearchKnowledge),
            input_schema: json!({"type":"object","additionalProperties":false}),
            output_schema: json!({"type":"object"}),
            policy: ApprovedActionPolicy {
                capability: TypeName::parse("search_knowledge").unwrap(),
                policy_revision: 1,
                effect: ActionEffect::Read,
                recovery: ActionRecovery::SafeRepeat,
            },
            approval_required: false,
        }],
        skills: Vec::new(),
    }
}

#[test]
fn autonomous_agent_contract_target_variants_and_remote_aliases_round_trip() {
    let mut binding = AgentToolBinding::builtin(AgentBuiltinTool::SearchKnowledge);
    binding.alias = AgentToolAlias::parse("lookup").unwrap();
    for source in [
        AgentBindingSource::Builtin {
            tool: AgentBuiltinTool::SearchKnowledge,
        },
        AgentBindingSource::Http {
            connection: Uuid::new_v4(),
            operation: TypeName::parse("crm.lookup").unwrap(),
        },
        AgentBindingSource::Mcp {
            connection: Uuid::new_v4(),
            operation: TypeName::parse("crm.lookup").unwrap(),
            reviewed_version: Uuid::new_v4(),
            remote_name: "remote/name".to_owned().try_into().unwrap(),
        },
        AgentBindingSource::Workflow {
            registration: TypeName::parse("crm.lookup").unwrap(),
            workflow: WorkflowId::new(Uuid::new_v4()),
            version: VersionId::new(Uuid::new_v4()),
            params: json!({"region":"eu"}),
        },
    ] {
        binding.source = source;
        binding.validate().unwrap();
        let stored = serde_json::to_vec(&binding).unwrap();
        assert_eq!(
            serde_json::from_slice::<AgentToolBinding>(&stored).unwrap(),
            binding
        );
    }
}

#[test]
fn autonomous_agent_contract_alias_and_identity_uniqueness_is_agent_local() {
    let one = AgentToolBinding::builtin(AgentBuiltinTool::SearchKnowledge);
    let mut two = one.clone();
    two.id = AgentBindingId::new(Uuid::new_v4());
    two.source = AgentBindingSource::Http {
        connection: Uuid::new_v4(),
        operation: TypeName::parse("search").unwrap(),
    };
    two.alias = AgentToolAlias::parse("lookup").unwrap();
    let mut other = two.clone();
    other.id = AgentBindingId::new(Uuid::new_v4());
    other.source = AgentBindingSource::Http {
        connection: Uuid::new_v4(),
        operation: TypeName::parse("find").unwrap(),
    };
    validate_agent_bindings(&[two.clone()]).unwrap();
    validate_agent_bindings(&[other.clone()]).unwrap();
    assert!(validate_agent_bindings(&[two.clone(), other]).is_err());
    two.alias = AgentToolAlias::parse("different").unwrap();
    two.id = one.id;
    assert!(validate_agent_bindings(&[one, two]).is_err());
}

#[test]
fn autonomous_agent_contract_reserved_names_cannot_manufacture_builtin_authority() {
    for alias in [
        "message.send",
        "message.unknown",
        "create_agent_channel",
        "list_company_agents",
        "search_knowledge",
    ] {
        let mut binding = AgentToolBinding::builtin(AgentBuiltinTool::MessageReply);
        binding.alias = AgentToolAlias::parse(alias).unwrap();
        assert!(binding.validate().is_err(), "{alias}");
        binding.source = AgentBindingSource::Http {
            connection: Uuid::new_v4(),
            operation: TypeName::parse("registered").unwrap(),
        };
        assert!(binding.validate().is_err(), "{alias}");
    }
    AgentToolBinding::builtin(AgentBuiltinTool::CreateAgentChannel)
        .validate()
        .unwrap();
}

#[test]
fn autonomous_agent_contract_skills_require_local_aliases_without_granting_tools() {
    let mut value = definition();
    value.tools[0].binding.alias = AgentToolAlias::parse("lookup").unwrap();
    value.skills.push(ResolvedAgentSkill {
        assignment_id: AgentSkillAssignmentId::new(Uuid::new_v4()),
        skill: SkillSnapshot {
            company_id: value.company_id,
            skill_id: Uuid::new_v4(),
            name: TypeName::parse("support").unwrap(),
            instructions: "Look up facts first.".into(),
            required_tools: vec![TypeName::parse("lookup").unwrap()],
        },
    });
    value.validate().unwrap();
    value.skills[0].skill.required_tools = vec![TypeName::parse("search_knowledge").unwrap()];
    assert!(value.validate().is_err());
    assert_eq!(value.tools.len(), 1);
}

#[test]
fn autonomous_agent_contract_definition_decoding_is_bounded_and_rejects_extra_secrets() {
    assert!(ResolvedAgentDefinition::decode(&vec![b' '; MAX_AGENT_DEFINITION_BYTES + 1]).is_err());
    let value = definition();
    let mut json = serde_json::to_value(&value).unwrap();
    json["model"]["api_key"] = json!("must-never-be-captured");
    assert!(ResolvedAgentDefinition::decode(&serde_json::to_vec(&json).unwrap()).is_err());
    let mut value = value;
    value.instructions = "x".repeat(MAX_AGENT_TEXT_BYTES + 1);
    assert!(value.canonical_content().is_err());
    value.instructions.clear();
    value.model.max_output_tokens = 0;
    assert!(value.validate().is_err());
}

#[test]
fn autonomous_agent_contract_binding_count_and_parameter_bytes_fail_before_execution() {
    let mut bindings = Vec::new();
    for index in 0..33 {
        let mut binding = AgentToolBinding::builtin(AgentBuiltinTool::MessageReply);
        binding.alias = AgentToolAlias::parse(format!("tool_{index}")).unwrap();
        bindings.push(binding);
    }
    validate_agent_bindings(&bindings[..32]).unwrap();
    assert!(validate_agent_bindings(&bindings).is_err());
    bindings[0].source = AgentBindingSource::Workflow {
        registration: TypeName::parse("child").unwrap(),
        workflow: WorkflowId::new(Uuid::new_v4()),
        version: VersionId::new(Uuid::new_v4()),
        params: json!({"input":"x".repeat(MAX_AGENT_BINDING_BYTES)}),
    };
    assert!(bindings[0].validate().is_err());
}

#[test]
fn autonomous_agent_contract_canonical_content_keeps_order_and_all_effect_facts() {
    let mut value = definition();
    let original = value.canonical_content().unwrap();
    let restored = ResolvedAgentDefinition::decode(&original).unwrap();
    assert_eq!(
        value.content_hash().unwrap(),
        restored.content_hash().unwrap()
    );
    value.tools[0].approval_required = true;
    assert_ne!(
        value.content_hash().unwrap(),
        restored.content_hash().unwrap()
    );
    value.tools.push(value.tools[0].clone());
    value.tools[1].binding = AgentToolBinding::builtin(AgentBuiltinTool::MessageReply);
    let ordered = value.content_hash().unwrap();
    value.tools.reverse();
    assert_ne!(ordered, value.content_hash().unwrap());
}

#[test]
fn autonomous_agent_contract_manifest_is_canonical_unique_bounded_identity_content() {
    let mut manifest = DependencyManifest {
        company_id: CompanyId::new(Uuid::new_v4()),
        root_run_id: RunId::new(Uuid::new_v4()),
        agents: (0..2)
            .map(|_| AgentRevisionSelection {
                agent_id: AgentId::new(Uuid::new_v4()),
                revision_id: AgentRevisionId::new(Uuid::new_v4()),
            })
            .collect(),
        workflows: Vec::new(),
    };
    let digest = manifest.content_hash().unwrap();
    manifest.agents.reverse();
    assert_eq!(digest, manifest.content_hash().unwrap());
    let restored = DependencyManifest::decode(&manifest.canonical_content().unwrap()).unwrap();
    assert_eq!(restored.content_hash().unwrap(), digest);
    manifest.agents.push(manifest.agents[0].clone());
    assert!(manifest.validate().is_err());
    manifest.agents = (0..257)
        .map(|_| AgentRevisionSelection {
            agent_id: AgentId::new(Uuid::new_v4()),
            revision_id: AgentRevisionId::new(Uuid::new_v4()),
        })
        .collect();
    assert!(manifest.validate().is_err());
    assert!(DependencyManifest::decode(&vec![b' '; MAX_AGENT_MANIFEST_BYTES + 1]).is_err());
    assert!(AgentDraftRevision::new(0).is_err());
    assert!(AgentDraftRevision::new(i64::MAX as u64 + 1).is_err());
    assert_eq!(
        serde_json::from_str::<AgentDraftRevision>("1")
            .unwrap()
            .get(),
        1
    );
}
