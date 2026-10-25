use super::*;
fn tool_bundle() -> Arc<PublishedBundle> {
    let mut leaf = source("memory.save");
    leaf["workflow_id"] = json!(Uuid::from_u128(2));
    leaf["input_schema"] = json!({"type":"object"});
    leaf["parameter_schema"] = json!({"type":"object"});
    leaf["output_schema"] = json!({"type":"object","properties":{"saved_ids":{"type":"array","items":{"type":"string"}}},"required":["saved_ids"],"additionalProperties":false});
    let child = build(&leaf, DependencySnapshots::default(), vec![]);
    let definition = child.compiled().graph().definition();
    let tool = ToolSnapshot {
        company_id: company(),
        contract: registry::ToolContract {
            name: name("memory.child"),
            connection: None,
            input_schema: definition.input_schema.clone().unwrap(),
            output_schema: definition.output_schema.clone().unwrap(),
        },
        policy: ApprovedActionPolicy {
            capability: name("workflow.tool"),
            policy_revision: 1,
            effect: ActionEffect::Write,
            recovery: ActionRecovery::SafeRepeat,
        },
        workflow: Some(WorkflowToolPin {
            workflow_id: definition.workflow_id,
            version_id: definition.version_id,
            params: json!({}),
        }),
    };
    let mut facts = Fixture::AutonomousResponse.snapshots(company());
    facts.agents[0].skills.clear();
    facts.skills.clear();
    facts.agents[0].tools.push(tool.contract.name.clone());
    facts.tools.push(tool);
    build(&source("agent.run"), facts, vec![child])
}
#[test]
fn direct_agent_tools_and_pinned_workflow_tools_stay_in_sample() {
    let bundle = tool_bundle();
    let mut req = request(vec![
        script(1, "start", vec![json!("done")]),
        script(2, "start", vec![json!({"saved_ids":["sample-only"]})]),
    ]);
    req.scripts[0].responses[0].tools = vec![
        ScriptedToolCall {
            name: name("knowledge.search"),
            input: json!({"query":"test"}),
            output: Some(json!(["fact"])),
        },
        ScriptedToolCall {
            name: name("memory.child"),
            input: json!({}),
            output: None,
        },
    ];
    let report = execute(bundle.clone(), req.clone()).unwrap();
    complete(&report);
    assert_eq!(report.usage.model_calls, 2);
    assert_eq!(report.usage.direct_tool_calls, 2);
    assert_eq!(report.effects.len(), 3);
    assert!(
        report
            .effects
            .iter()
            .any(|effect| effect.operation.as_str() == "memory.save"
                && effect.kind == SampleEffectKind::InterceptedWrite)
    );
    assert!(
        report
            .effects
            .iter()
            .any(|effect| effect.kind == SampleEffectKind::WorkflowChild)
    );
    req.scripts[0].responses[0].tools[1].output = Some(json!({"saved_ids":[]}));
    assert!(
        execute(bundle.clone(), req.clone())
            .unwrap()
            .diagnostic
            .unwrap()
            .contains("not a fixture")
    );
    req.scripts[0].responses[0].tools[0].name = name("ungranted");
    assert!(
        execute(bundle, req)
            .unwrap()
            .diagnostic
            .unwrap()
            .contains("not granted")
    );
}
#[test]
fn direct_tool_inputs_and_outputs_are_validated() {
    let bundle = tool_bundle();
    for (input, output) in [
        (json!({"wrong":true}), Some(json!([]))),
        (json!({"query":"q"}), Some(json!("wrong"))),
        (json!({"query":"q"}), None),
    ] {
        let mut req = request(vec![script(1, "start", vec![json!("done")])]);
        req.scripts[0].responses[0].tools = vec![ScriptedToolCall {
            name: name("knowledge.search"),
            input,
            output,
        }];
        assert_eq!(
            execute(bundle.clone(), req).unwrap().status,
            SampleStatus::Stopped
        );
    }
}

#[test]
fn protected_step_workflow_tool_needs_approval_and_executes_child() {
    let original = tool_bundle();
    let tool = original
        .snapshots()
        .tools
        .iter()
        .find(|tool| tool.workflow.is_some())
        .unwrap()
        .clone();
    let child = original.children().values().next().unwrap().clone();
    let mut raw = source("tool.call");
    raw["steps"]["start"]["with"]["tool"] = json!({"literal":"memory.child"});
    raw["steps"]["start"]["with"]["arguments"] = json!({"literal":{}});
    raw["steps"]["start"]["with"]["approval"] = json!({"literal":{"reviewers":[{"kind":"user","id":Uuid::from_u128(6)}],"deadline":"2030-01-01T00:00:00Z"}});
    let bundle = build(
        &raw,
        DependencySnapshots {
            tools: vec![tool],
            ..Default::default()
        },
        vec![child],
    );
    let mut req = request(vec![
        script(1, "start", vec![json!(null)]),
        script(2, "start", vec![json!({"saved_ids":["sample"]})]),
    ]);
    assert!(
        execute(bundle.clone(), req.clone())
            .unwrap()
            .diagnostic
            .unwrap()
            .contains("approval:true")
    );
    req.scripts[0].responses[0].approval = Some(true);
    let report = execute(bundle.clone(), req.clone()).unwrap();
    complete(&report);
    assert_eq!(report.effects.len(), 2);
    req.scripts[0].responses[0].output = json!({"saved_ids":["replacement"]});
    assert_eq!(execute(bundle, req).unwrap().status, SampleStatus::Stopped);
}

#[test]
fn direct_mcp_tool_uses_the_production_result_envelope() {
    let mut raw = source("agent.run");
    raw["resources"] = json!([{"slot":"tickets","kind":"mcp"}]);
    let mut facts = Fixture::AutonomousResponse.snapshots(company());
    facts.agents[0].skills.clear();
    facts.skills.clear();
    facts.agents[0].tools = vec![name("lookup_tickets")];
    facts.tools = Fixture::McpLookup.snapshots(company()).tools;
    let bundle = build(&raw, facts, vec![]);
    let input = json!({"limit":2,"include_closed":false,"labels":[],"filter":{"owner":null}});
    let mut req = request(vec![script(1, "start", vec![json!("done")])]);
    req.scripts[0].responses[0].tools = vec![ScriptedToolCall {
        name: name("lookup_tickets"),
        input,
        output: Some(
            json!({"content":[],"isError":false,"structuredContent":{"count":0,"tickets":[]}}),
        ),
    }];
    complete(&execute(bundle.clone(), req.clone()).unwrap());
    req.scripts[0].responses[0].tools[0].output = Some(json!({"count":0,"tickets":[]}));
    assert_eq!(execute(bundle, req).unwrap().status, SampleStatus::Stopped);
}
