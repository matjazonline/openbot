use super::*;
use crate::application::agent::{ModelSettings, ResolvedAgentDefinition, ResolvedAgentTool};
use crate::entities::agent_capability::{
    AgentBindingId, AgentBindingSource, AgentToolAlias, AgentToolBinding,
};

fn protected_request(
    bundle: &Arc<PublishedBundle>,
    tool: ResolvedAgentTool,
    output: Option<Value>,
) -> SampleRequest {
    let reference = &bundle.snapshots().agents[0];
    let mut request = request(vec![
        script(1, "start", vec![json!("done")]),
        script(
            2,
            "start",
            vec![
                json!({"saved":[{"id":"00000000-0000-4000-8000-000000000001","revision":1}],"access_level":0,"replayed":false}),
            ],
        ),
    ]);
    request.scripts[0].responses[0].tools = vec![ScriptedToolCall {
        name: name(tool.binding.alias.as_str()),
        input: json!({}),
        output,
    }];
    request.agents = vec![ResolvedAgentDefinition {
        native_permissions: Default::default(),
        company_id: bundle.company_id(),
        agent_id: reference.agent_id,
        key: reference.key.clone(),
        instructions: "Test a protected canonical binding".into(),
        model: Some(ModelSettings {
            provider: name("test"),
            model: "sample".into(),
            max_output_tokens: 1000,
        }),
        tools: vec![tool],
        skills: vec![],
    }];
    request
}

fn assert_explicit_approval(bundle: Arc<PublishedBundle>, request: SampleRequest) {
    for approval in [None, Some(false), Some(true)] {
        let mut request = request.clone();
        request.scripts[0].responses[0].approval = approval;
        let report = super::super::super::execute(
            PreparedSample::new(bundle.clone(), request, vec![]).unwrap(),
        )
        .unwrap();
        if approval == Some(true) {
            complete(&report);
            assert!(!report.effects.is_empty());
        } else {
            assert!(matches!(report.status, SampleStatus::Stopped));
            assert!(report.diagnostic.unwrap().contains("approval:true"));
            assert!(
                report.effects.is_empty(),
                "approval precedes the simulated effect"
            );
        }
    }
}

#[test]
fn autonomous_agent_samples_protected_http_binding_requires_explicit_simulated_approval() {
    let bundle = build(
        &source("agent.run"),
        Fixture::AutonomousResponse.snapshots(company()),
        vec![],
    );
    let tool = ResolvedAgentTool {
        binding: AgentToolBinding {
            id: AgentBindingId::new(Uuid::from_u128(501)),
            alias: AgentToolAlias::parse("crm.write").unwrap(),
            source: AgentBindingSource::Http {
                connection: Uuid::from_u128(7),
                operation: name("crm.write"),
            },
        },
        input_schema: json!({"type":"object"}),
        output_schema: json!({"type":"object"}),
        policy: ApprovedActionPolicy {
            capability: name("http.request"),
            policy_revision: 1,
            effect: ActionEffect::Write,
            recovery: ActionRecovery::SafeRepeat,
        },
        approval_required: true,
    };
    let request = protected_request(&bundle, tool, Some(json!({})));
    assert_explicit_approval(bundle, request);
}

#[test]
fn autonomous_agent_samples_protected_child_binding_requires_explicit_simulated_approval() {
    let bundle = tool_bundle();
    let snapshot = bundle
        .snapshots()
        .tools
        .iter()
        .find(|tool| tool.contract.name.as_str() == "memory.child")
        .unwrap();
    let pin = snapshot.workflow.as_ref().unwrap();
    let tool = ResolvedAgentTool {
        binding: AgentToolBinding {
            id: AgentBindingId::new(Uuid::from_u128(502)),
            alias: AgentToolAlias::parse("memory.child").unwrap(),
            source: AgentBindingSource::Workflow {
                registration: snapshot.contract.name.clone(),
                workflow: pin.workflow_id,
                version: pin.version_id,
                params: pin.params.clone(),
            },
        },
        input_schema: snapshot.contract.input_schema.clone(),
        output_schema: snapshot.contract.output_schema.clone(),
        policy: snapshot.policy.clone(),
        approval_required: true,
    };
    let request = protected_request(&bundle, tool, None);
    assert_explicit_approval(bundle, request);
}
