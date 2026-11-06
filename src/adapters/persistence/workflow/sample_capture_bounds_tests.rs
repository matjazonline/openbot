use super::*;
use crate::application::agent::{
    ActionEffect, ActionRecovery, ApprovedActionPolicy, ModelSettings, ResolvedAgentTool,
};
use crate::application::workflow::publication::{DependencySnapshots, ToolSnapshot, freeze};
use crate::entities::agent_capability::{
    AgentBindingId, AgentBindingSource, AgentToolAlias, AgentToolBinding,
};
use serde_json::{Value, json};

fn name(value: &str) -> crate::domain::workflow::TypeName {
    crate::domain::workflow::TypeName::parse(value).unwrap()
}

fn padded_workflow(company: CompanyId, index: u128) -> Arc<PublishedBundle> {
    let mut source: Value =
        serde_json::from_str(&registry::example("memory.save").unwrap().source).unwrap();
    source["workflow_id"] = json!(Uuid::from_u128(1000 + index));
    source["input_schema"] = json!({"type":"object"});
    source["parameter_schema"] = json!({"type":"object"});
    source["output_schema"] = json!({"type":"object"});
    let tools = (0..24).map(|tool| ToolSnapshot {
        company_id: company,
        contract: registry::ToolContract {
            name: name(&format!("padding_{tool}")), connection:None,
            input_schema: json!({"type":"object","properties":{"value":{"type":"string","enum":["x".repeat(10_000)]}}}),
            output_schema: json!({"type":"object"}),
        },
        policy: ApprovedActionPolicy { capability:name("http.request"),policy_revision:1,effect:ActionEffect::Read,recovery:ActionRecovery::SafeRepeat },
        workflow:None,
    }).collect();
    Arc::new(
        freeze(
            crate::adapters::workflow_source::decode(&source.to_string()).unwrap(),
            company,
            VersionId::new(Uuid::from_u128(2000 + index)),
            DependencySnapshots {
                tools,
                ..Default::default()
            },
            vec![],
        )
        .unwrap(),
    )
}

fn root_workflow(company: CompanyId) -> Arc<PublishedBundle> {
    let fixture = crate::application::workflow::fixtures::Fixture::AutonomousResponse;
    let decoded =
        crate::adapters::workflow_source::decode(&registry::example("agent.run").unwrap().source)
            .unwrap();
    let uses = crate::application::workflow::publication::agent_reference_uses(&decoded).unwrap();
    let mut facts = fixture.snapshots(company);
    for reference in &mut facts.agents {
        reference.uses = uses[&reference.key].clone();
    }
    Arc::new(
        freeze(
            decoded,
            company,
            VersionId::new(Uuid::from_u128(100)),
            facts,
            vec![],
        )
        .unwrap(),
    )
}

fn request(root: &PublishedBundle, children: &[Arc<PublishedBundle>]) -> SampleRequest {
    let reference = &root.snapshots().agents[0];
    let tools = children
        .iter()
        .enumerate()
        .map(|(index, child)| {
            let definition = child.compiled().graph().definition();
            ResolvedAgentTool {
                binding: AgentToolBinding {
                    id: AgentBindingId::new(Uuid::from_u128(3000 + index as u128)),
                    alias: AgentToolAlias::parse(format!("child_{index}")).unwrap(),
                    source: AgentBindingSource::Workflow {
                        registration: name(&format!("child_{index}")),
                        workflow: definition.workflow_id,
                        version: definition.version_id,
                        params: json!({}),
                    },
                },
                input_schema: definition.input_schema.clone().unwrap(),
                output_schema: definition.output_schema.clone().unwrap(),
                policy: ApprovedActionPolicy {
                    capability: name("workflow.tool"),
                    policy_revision: 1,
                    effect: ActionEffect::Write,
                    recovery: ActionRecovery::SafeRepeat,
                },
                approval_required: false,
            }
        })
        .collect();
    SampleRequest {
        input:json!({}),params:json!({}),
        agents:vec![ResolvedAgentDefinition { company_id:root.company_id(),agent_id:reference.agent_id,key:reference.key.clone(),instructions:"Bounded sample".into(),
            model:ModelSettings {provider:name("test"),model:"sample".into(),max_output_tokens:1000},tools,skills:vec![] }],
        scripts:serde_json::from_value(json!([{"workflow_id":root.compiled().graph().definition().workflow_id,"step":"start","responses":[{"output":"done"}]}])).unwrap(),
    }
}

#[test]
fn autonomous_agent_samples_workflow_payload_limit_covers_adapter_cache_and_sealed_input() {
    let company = CompanyId::new(Uuid::from_u128(800));
    let root = root_workflow(company);
    let mut bytes = store_bundle(&root).unwrap().len();
    let mut children = vec![];
    for index in 1..=32 {
        let child = padded_workflow(company, index);
        let size = store_bundle(&child).unwrap().len();
        assert!(size < MAX_AGENT_MANIFEST_BYTES);
        children.push(child);
        bytes += size;
        if bytes > MAX_AGENT_MANIFEST_BYTES {
            break;
        }
    }
    assert!(bytes > MAX_AGENT_MANIFEST_BYTES);
    let extra = children.pop().unwrap();
    assert!(!children.is_empty());
    let target = DefinitionTarget {
        company,
        actor: WorkflowActor::authenticated(Uuid::new_v4()).unwrap(),
        workflow: root.compiled().graph().definition().workflow_id,
    };
    let mut cache = Closure::new(target, Instant::now() + Duration::from_secs(5));
    cache.cache(root.clone()).unwrap();
    for child in &children {
        cache.cache(child.clone()).unwrap();
    }
    assert!(cache.workflow_bytes <= MAX_AGENT_MANIFEST_BYTES);
    let error = cache.cache(extra.clone()).unwrap_err();
    assert!(error.to_string().contains("4 MiB"), "{error}");
    assert_eq!(cache.workflows.len(), children.len() + 1);

    let sealed =
        PreparedSample::new(root.clone(), request(&root, &children), children.clone()).unwrap();
    let report = crate::application::workflow::sample::execute(sealed).unwrap();
    assert_eq!(
        report.status,
        crate::application::workflow::sample::SampleStatus::Completed,
        "{:?}",
        report.diagnostic
    );
    assert_eq!(report.workflow_selections.len(), children.len() + 1);
    assert!(report.effects.is_empty());
    children.push(extra);
    let error = PreparedSample::new(root.clone(), request(&root, &children), children)
        .err()
        .expect("aggregateworkflows must fail before sample execution");
    assert!(error.to_string().contains("4 MiB"), "{error}");
}
