use super::*;
use crate::domain::workflow::{CompanyId, TypeName};
use crate::entities::agent_capability::*;
use serde_json::json;
use uuid::Uuid;

fn facts() -> AgentResolutionFacts {
    let company = CompanyId::new(Uuid::new_v4());
    let binding = AgentToolBinding::builtin(AgentBuiltinTool::MessageReply);
    let tool = crate::application::workflow::message::builtin_message_snapshot(
        company,
        crate::application::workflow::message::MessageKind::Reply,
    )
    .unwrap();
    AgentResolutionFacts {
        draft: AgentDraft {
            company_id: company,
            agent_id: AgentId::new(Uuid::new_v4()),
            revision: AgentDraftRevision::new(1).unwrap(),
            key: AgentKey::parse("assistant").unwrap(),
            instructions: "Answer concisely.".into(),
            model: Some(ModelSettings {
                provider: TypeName::parse("openai").unwrap(),
                model: "fixture-model".into(),
                max_output_tokens: 1000,
            }),
            bindings: vec![binding.clone()],
            skills: vec![],
        },
        tools: vec![ResolvedAgentTool {
            binding,
            input_schema: tool.contract.input_schema,
            output_schema: tool.contract.output_schema,
            policy: tool.policy,
            approval_required: false,
        }],
        skills: vec![],
        model_available: true,
    }
}

fn assigned(facts: &mut AgentResolutionFacts, name: &str, required: &str) {
    let assignment = AgentSkillAssignmentId::new(Uuid::new_v4());
    let skill_id = Uuid::new_v4();
    facts.draft.skills.push(AgentSkillAssignment {
        id: assignment,
        skill_id,
    });
    facts.skills.push(ResolvedAgentSkill {
        assignment_id: assignment,
        skill: SkillSnapshot {
            company_id: facts.draft.company_id,
            skill_id,
            name: TypeName::parse(name).unwrap(),
            instructions: format!("Instructions for {name}."),
            required_tools: vec![TypeName::parse(required).unwrap()],
        },
    });
}

#[test]
fn autonomous_agent_resolution_needs_no_workflow_slot_channel_or_run() {
    let definition = resolve_agent(facts()).unwrap();
    assert_eq!(definition.tools[0].binding.alias.as_str(), "message.reply");
    assert_eq!(definition.model.model, "fixture-model");
    assert_eq!(definition.instructions, "Answer concisely.");
    assert_eq!(
        ResolvedAgentDefinition::decode(&definition.canonical_content().unwrap())
            .unwrap()
            .content_hash()
            .unwrap(),
        definition.content_hash().unwrap()
    );
}

#[test]
fn autonomous_agent_resolution_same_alias_on_different_agents_is_independent() {
    let first = resolve_agent(facts()).unwrap();
    let mut second = facts();
    second.draft.company_id = first.company_id;
    let second = resolve_agent(second).unwrap();
    assert_eq!(first.company_id, second.company_id);
    assert_eq!(first.tools[0].binding.alias, second.tools[0].binding.alias);
    assert_ne!(first.agent_id, second.agent_id);
    assert_ne!(first.tools[0].binding.id, second.tools[0].binding.id);
}

#[test]
fn autonomous_agent_resolution_alias_collision_and_reserved_spoofing_are_refused() {
    let mut collision = facts();
    let mut duplicate = collision.tools[0].clone();
    duplicate.binding.id = AgentBindingId::new(Uuid::new_v4());
    collision.draft.bindings.push(duplicate.binding.clone());
    collision.tools.push(duplicate);
    assert!(resolve_agent(collision).is_err());
    let mut forged = facts();
    forged.draft.bindings[0].source = AgentBindingSource::Http {
        connection: Uuid::new_v4(),
        operation: TypeName::parse("lookup").unwrap(),
    };
    forged.tools[0].binding = forged.draft.bindings[0].clone();
    assert!(resolve_agent(forged).is_err());
}

#[test]
fn autonomous_agent_resolution_requires_exact_binding_identity_source_and_order() {
    let mut changed = facts();
    changed.tools[0].binding.id = AgentBindingId::new(Uuid::new_v4());
    assert!(resolve_agent(changed).is_err());
    let mut source = facts();
    source.tools[0].binding.source = AgentBindingSource::Builtin {
        tool: AgentBuiltinTool::MessageSend,
    };
    assert!(resolve_agent(source).is_err());
    let mut absent = facts();
    absent.tools.clear();
    assert!(resolve_agent(absent).is_err());
}

#[test]
fn autonomous_agent_resolution_validates_schema_and_explicit_available_model() {
    let mut schema = facts();
    schema.tools[0].input_schema = json!({"type":"not-a-schema-type"});
    assert!(resolve_agent(schema).is_err());
    let mut missing = facts();
    missing.draft.model = None;
    assert!(resolve_agent(missing).is_err());
    let mut unavailable = facts();
    unavailable.model_available = false;
    assert!(resolve_agent(unavailable).is_err());
    let mut bounded = facts();
    bounded.draft.model.as_mut().unwrap().max_output_tokens = 0;
    assert!(resolve_agent(bounded).is_err());
}

#[test]
fn autonomous_agent_skills_require_explicit_aliases_without_adding_tools() {
    let mut missing = facts();
    assigned(&mut missing, "research", "crm.lookup");
    assert!(resolve_agent(missing).is_err());
    let mut present = facts();
    assigned(&mut present, "reply", "message.reply");
    let definition = resolve_agent(present).unwrap();
    assert_eq!(definition.tools.len(), 1);
    assert_eq!(
        definition.skills[0].skill.required_tools[0].as_str(),
        "message.reply"
    );
}

#[test]
fn autonomous_agent_skills_preserve_order_and_reject_substituted_assignments() {
    let mut ordered = facts();
    assigned(&mut ordered, "first", "message.reply");
    assigned(&mut ordered, "second", "message.reply");
    let definition = resolve_agent(ordered).unwrap();
    assert_eq!(
        definition
            .skills
            .iter()
            .map(|skill| skill.skill.name.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    let mut reversed = facts();
    assigned(&mut reversed, "first", "message.reply");
    assigned(&mut reversed, "second", "message.reply");
    reversed.skills.swap(0, 1);
    assert!(resolve_agent(reversed).is_err());
}

#[test]
fn autonomous_agent_skills_refuse_foreign_tenant_facts_and_missing_requirements() {
    let mut foreign = facts();
    assigned(&mut foreign, "reply", "message.reply");
    foreign.skills[0].skill.company_id = CompanyId::new(Uuid::new_v4());
    assert!(resolve_agent(foreign).is_err());
    let mut missing = facts();
    assigned(&mut missing, "reply", "message.reply");
    missing.skills.clear();
    assert!(resolve_agent(missing).is_err());
}
