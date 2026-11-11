use super::*;
use crate::application::agent::{AgentKey, SkillSnapshot};

fn skill(name_: &str, id: u128, required: Vec<TypeName>) -> SkillSnapshot {
    SkillSnapshot {
        description: "Skill instructions".into(),
        files: Vec::new(),
        company_id: company(),
        skill_id: Uuid::from_u128(id),
        name: name(name_),
        instructions: format!("{name_} instructions"),
        required_tools: required,
    }
}

fn capable_definition() -> crate::application::agent::ResolvedAgentDefinition {
    use crate::application::agent::{ResolvedAgentSkill, ResolvedAgentTool};
    use crate::entities::agent_capability::{
        AgentBindingId, AgentBindingSource, AgentSkillAssignmentId, AgentToolAlias,
        AgentToolBinding,
    };
    let mut definition = definition();
    definition.tools = ["search", "lookup"]
        .iter()
        .enumerate()
        .map(|(index, alias)| {
            let snapshot = tool_snapshot("tool.call");
            ResolvedAgentTool {
                binding: AgentToolBinding {
                    id: AgentBindingId::new(Uuid::from_u128(20 + index as u128)),
                    alias: AgentToolAlias::parse(*alias).unwrap(),
                    source: AgentBindingSource::Http {
                        connection: Uuid::from_u128(7),
                        operation: name(alias),
                    },
                },
                input_schema: snapshot.contract.input_schema,
                output_schema: snapshot.contract.output_schema,
                policy: snapshot.policy,
                approval_required: false,
            }
        })
        .collect();
    definition.skills = vec![
        ResolvedAgentSkill {
            assignment_id: AgentSkillAssignmentId::new(Uuid::from_u128(22)),
            skill: skill("style", 12, vec![]),
        },
        ResolvedAgentSkill {
            assignment_id: AgentSkillAssignmentId::new(Uuid::from_u128(23)),
            skill: skill("support", 11, vec![name("lookup")]),
        },
    ];
    definition
}
fn capable() -> DependencySnapshots {
    snapshots()
}
fn resolve(bundle: &PublishedBundle) -> Result<ResolvedAgent, crate::app_error::AppError> {
    let reference = bundle.agent_reference(company(), &AgentKey::parse("assistant").unwrap())?;
    assert_eq!(reference.agent_id, definition().agent_id);
    resolved(capable_definition())
}

#[test]
fn workflow_capabilities_agent_receives_exactly_its_saved_tools_and_skills_in_order() {
    let bundle = build(&source("agent.run"), snapshots()).unwrap();
    let resolved = resolve(&bundle).unwrap();
    assert_eq!(resolved.company_id(), company());
    assert_eq!(
        resolved
            .tools()
            .iter()
            .map(|tool| tool.binding.alias.as_str())
            .collect::<Vec<_>>(),
        ["search", "lookup"]
    );
    assert_eq!(resolved.skills()[0].skill.skill_id, Uuid::from_u128(12));
    assert_eq!(
        resolved.skills()[1].skill.instructions,
        "support instructions"
    );
    assert_eq!(
        resolved.agent().canonical_content().unwrap(),
        resolve(&bundle)
            .unwrap()
            .agent()
            .canonical_content()
            .unwrap()
    );
    let model_only = resolved.model_only();
    assert!(model_only.tools().is_empty());
    assert!(model_only.skills().is_empty());
    assert_eq!(
        model_only.agent().tools.len(),
        2,
        "projection does not mutate the canonical revision"
    );
}

#[test]
fn workflow_capabilities_agent_without_saved_capabilities_receives_none() {
    let selected = resolved(definition()).unwrap();
    assert!(selected.tools().is_empty());
    assert!(selected.skills().is_empty());
}

#[test]
fn workflow_capabilities_refuse_other_company_and_unknown_agent() {
    let bundle = build(&source("agent.run"), snapshots()).unwrap();
    assert!(matches!(
        bundle.agent_reference(
            CompanyId::new(Uuid::from_u128(999)),
            &AgentKey::parse("assistant").unwrap()
        ),
        Err(crate::app_error::AppError::NotFound(_))
    ));
    assert!(
        bundle
            .agent_reference(company(), &AgentKey::parse("other").unwrap())
            .is_err()
    );
}

#[test]
fn autonomous_agent_definition_skill_requirements_are_agent_local() {
    let mut definition = capable_definition();
    definition
        .tools
        .retain(|tool| tool.binding.alias.as_str() == "search");
    assert!(definition.validate().is_err());
}

#[test]
fn workflow_capabilities_step_and_workflow_narrowing_inputs_are_rejected() {
    let mut profiled = source("agent.run");
    profiled["steps"]["start"]["with"]["capability_profile"] =
        json!({"literal":{"tools":[],"skills":[]}});
    assert!(build(&profiled, capable()).is_err());
    let mut ceiling = source("agent.run");
    ceiling["capability_ceiling"] = json!({"tools":[],"skills":[]});
    assert!(build(&ceiling, capable()).is_err());
}

#[test]
fn workflow_capabilities_agent_universe_is_literal_or_finite_parameter_enum() {
    let decode =
        |value: &Value| crate::adapters::workflow_source::decode(&value.to_string()).unwrap();
    let literal = source("agent.run");
    assert_eq!(
        agent_universe(&decode(&literal)).unwrap(),
        vec![AgentKey::parse("assistant").unwrap()]
    );
    let mut enumerated = source("agent.run");
    enumerated["parameter_schema"] = json!({"type":"object","properties":{"agent":{"type":"string","enum":["sales","support"]}}});
    enumerated["steps"]["start"]["with"]["agent"] = json!({"ref":"/params/agent"});
    assert_eq!(
        agent_universe(&decode(&enumerated)).unwrap(),
        vec![
            AgentKey::parse("sales").unwrap(),
            AgentKey::parse("support").unwrap()
        ]
    );
    let mut open = enumerated.clone();
    open["parameter_schema"] = json!({"type":"object","properties":{"agent":{"type":"string"}}});
    assert!(agent_universe(&decode(&open)).is_err());
    let mut input = enumerated;
    input["steps"]["start"]["with"]["agent"] = json!({"ref":"/input/agent"});
    assert!(agent_universe(&decode(&input)).is_err());
}

/// 06-D6 B: `ai.classify` selects a frozen agent exactly like `agent.run`.
#[test]
fn workflow_capabilities_classify_captures_and_requires_its_agent() {
    let decode =
        |value: &Value| crate::adapters::workflow_source::decode(&value.to_string()).unwrap();
    let classify = source("ai.classify");
    assert_eq!(
        agent_universe(&decode(&classify)).unwrap(),
        vec![AgentKey::parse("assistant").unwrap()]
    );
    let bundle = build(&classify, snapshots()).unwrap();
    assert!(resolve(&bundle).is_ok());
    let mut missing = snapshots();
    missing.agents.clear();
    assert_eq!(
        build(&classify, missing).err().unwrap().code,
        "publication.agent"
    );
    let mut without = classify;
    without["steps"]["start"]["with"]
        .as_object_mut()
        .unwrap()
        .remove("agent");
    assert!(build(&without, snapshots()).is_err());
}
