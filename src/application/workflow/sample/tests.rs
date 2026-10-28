use super::*;
use crate::adapters::workflow_source::decode;
use crate::application::workflow::{fixtures::Fixture, publication::*, registry};
use crate::domain::workflow::{CompanyId, TypeName, VersionId};
use serde_json::json;

#[path = "tools_tests.rs"]
mod tools_tests;
fn company() -> CompanyId {
    CompanyId::new(Uuid::from_u128(800))
}
fn name(value: &str) -> TypeName {
    TypeName::parse(value).unwrap()
}
fn fixture(kind: Fixture) -> Arc<PublishedBundle> {
    let children = kind.child().map(fixture).into_iter().collect();
    Arc::new(
        freeze(
            decode(kind.source()).unwrap(),
            company(),
            kind.version_id(),
            kind.snapshots(company()),
            children,
        )
        .unwrap(),
    )
}
fn source(kind: &str) -> Value {
    serde_json::from_str(&registry::example(kind).unwrap().source).unwrap()
}
fn build(
    source: &Value,
    facts: DependencySnapshots,
    children: Vec<Arc<PublishedBundle>>,
) -> Arc<PublishedBundle> {
    Arc::new(
        freeze(
            decode(&source.to_string()).unwrap(),
            company(),
            VersionId::new(Uuid::new_v4()),
            facts,
            children,
        )
        .unwrap(),
    )
}
fn response(output: Value) -> ScriptedResponse {
    ScriptedResponse {
        output,
        tools: vec![],
        tokens: 0,
        approval: None,
    }
}
fn script(workflow: u128, step: &str, outputs: Vec<Value>) -> StepScript {
    StepScript {
        workflow_id: WorkflowId::new(Uuid::from_u128(workflow)),
        step: StepId::parse(step).unwrap(),
        responses: outputs.into_iter().map(response).collect(),
    }
}
fn request(scripts: Vec<StepScript>) -> SampleRequest {
    SampleRequest {
        input: json!({}),
        params: json!({}),
        scripts,
    }
}
fn message_result() -> Value {
    json!({"status":"accepted","messages":[{"destination":{"kind":"reply","id":Uuid::from_u128(700)},"message_id":Uuid::from_u128(701),"status":"accepted"}]})
}
fn complete(report: &SampleReport) {
    assert_eq!(report.status, SampleStatus::Completed, "{report:?}");
}

#[test]
fn pure_and_scripted_routing_share_compiled_validation() {
    let bundle = build(
        &source("decision.rule"),
        DependencySnapshots::default(),
        vec![],
    );
    let report = execute(bundle, request(vec![])).unwrap();
    complete(&report);
    assert_eq!(report.output.unwrap()["choice"], "continue");
    let bundle = build(
        &source("decision.human"),
        DependencySnapshots::default(),
        vec![],
    );
    let report = execute(
        bundle.clone(),
        request(vec![script(
            1,
            "start",
            vec![json!({"choice":"revise","data":"edited","feedback":""})],
        )]),
    )
    .unwrap();
    assert_eq!(report.status, SampleStatus::Stopped);
    assert!(report.diagnostic.unwrap().contains("schema"));
    let report = execute(
        bundle.clone(),
        request(vec![script(
            1,
            "start",
            vec![json!({"choice":"revise","data":"edited","feedback":"fix"})],
        )]),
    )
    .unwrap();
    complete(&report);
    assert_eq!(report.output.unwrap()["data"], "edited");
    let report = execute(
        bundle,
        request(vec![script(
            1,
            "start",
            vec![json!({"choice":"invalid","data":"edited","feedback":"fix"})],
        )]),
    )
    .unwrap();
    assert_eq!(report.status, SampleStatus::Stopped);
}
#[test]
fn repeated_human_revision_carries_edited_artifact_feedback_and_captures_reply() {
    let mut req = request(vec![
        script(
            4,
            "context",
            vec![json!({"items":[],"token_count":0,"omitted":0})],
        ),
        script(
            5,
            "draft",
            vec![json!({"body":"first"}), json!({"body":"second"})],
        ),
        script(
            5,
            "review",
            vec![
                json!({"choice":"revise","data":{"body":"edited first"},"feedback":"shorter"}),
                json!({"choice":"accept","data":{"body":"approved second"},"feedback":""}),
            ],
        ),
        script(4, "reply", vec![message_result()]),
    ]);
    req.input = json!({"message":"help","source_message":Uuid::from_u128(700)});
    req.params = json!({"reviewer":Uuid::from_u128(6),"deadline":"2030-01-01T00:00:00Z"});
    let report = execute(fixture(Fixture::RepeatedHumanRevision), req).unwrap();
    complete(&report);
    assert_eq!(report.usage.repetitions, 2);
    assert_eq!(report.usage.model_calls, 2);
    assert_eq!(report.unused_responses, 0);
    let drafts: Vec<_> = report
        .steps
        .iter()
        .filter(|step| step.step.as_str() == "draft")
        .collect();
    assert_eq!(drafts[1].inputs["context"]["previous_body"], "edited first");
    assert_eq!(drafts[1].inputs["context"]["feedback"], "shorter");
    assert!(drafts[1].path.as_str().contains("round-2"));
    let writes: Vec<_> = report
        .effects
        .iter()
        .filter(|effect| effect.kind == SampleEffectKind::InterceptedWrite)
        .collect();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].input["content"]["body"], "approved second");
}
#[test]
fn missing_invalid_unused_and_oversized_scripts_fail_explicitly() {
    let bundle = fixture(Fixture::AutonomousResponse);
    let mut req = request(vec![]);
    req.input = json!({"message":"hi","source_message":Uuid::from_u128(700)});
    assert!(
        execute(bundle.clone(), req.clone())
            .unwrap()
            .diagnostic
            .unwrap()
            .contains("Missing scripted")
    );
    req.scripts = vec![script(2, "draft", vec![json!({"wrong":true})])];
    assert_eq!(execute(bundle, req).unwrap().status, SampleStatus::Stopped);
    let pure = build(&source("data.map"), DependencySnapshots::default(), vec![]);
    let report = execute(
        pure.clone(),
        request(vec![script(1, "unused", vec![json!(null)])]),
    )
    .unwrap();
    complete(&report);
    assert_eq!(report.unused_responses, 1);
    assert!(
        execute(
            pure.clone(),
            request(vec![script(1, "x", vec![]), script(1, "x", vec![])])
        )
        .is_err()
    );
    let mut req = request(vec![]);
    req.input = json!("x".repeat(MAX_SAMPLE_BYTES));
    assert!(execute(pure, req).is_err());
}
#[test]
fn bounded_root_budget_and_external_kinds_never_fall_back() {
    for (kind, output) in [
        (
            "context.history",
            json!({"items":[],"token_count":0,"omitted":0}),
        ),
        ("memory.load", json!({"items":[]})),
        ("memory.save", json!({"saved_ids":["sample"]})),
        ("http.request", json!({"status":200,"headers":{},"body":{}})),
        ("message.reply", message_result()),
        ("wait.timer", json!({"deadline":"2030-01-01T00:00:00Z"})),
        ("wait.event", json!("event")),
    ] {
        let bundle = build(&source(kind), DependencySnapshots::default(), vec![]);
        let report = execute(bundle, request(vec![script(1, "start", vec![output])])).unwrap();
        complete(&report);
        if !kind.starts_with("wait.") {
            assert_eq!(report.effects.len(), 1, "{kind}");
        }
    }
    let mut raw = source("agent.run");
    raw["limits"]["root_budget"] = json!({"activations":1,"model_calls":1,"repetitions":1});
    let mut facts = Fixture::AutonomousResponse.snapshots(company());
    facts.agents[0].skills.clear();
    facts.skills.clear();
    let bundle = build(&raw, facts, vec![]);
    let mut req = request(vec![script(1, "start", vec![json!("done")])]);
    req.scripts[0].responses[0].tools = vec![ScriptedToolCall {
        name: name("knowledge.search"),
        input: json!({"query":"q"}),
        output: Some(json!([])),
    }];
    let report = execute(bundle, req).unwrap();
    assert_eq!(report.status, SampleStatus::Stopped);
    assert!(report.diagnostic.unwrap().contains("ModelCall budget"));
}

#[test]
fn descendants_cannot_replenish_root_budgets_and_report_size_is_bounded() {
    let child = fixture(Fixture::HumanRevisionRound);
    let mut parent: Value = serde_yaml::from_str(Fixture::RepeatedHumanRevision.source()).unwrap();
    parent["limits"]["root_budget"] = json!({"activations":2,"model_calls":100,"repetitions":100});
    let bundle = build(
        &parent,
        Fixture::RepeatedHumanRevision.snapshots(company()),
        vec![child],
    );
    let mut req = request(vec![script(
        4,
        "context",
        vec![json!({"items":[],"token_count":0,"omitted":0})],
    )]);
    req.input = json!({"message":"help","source_message":Uuid::from_u128(700)});
    req.params = json!({"reviewer":Uuid::from_u128(6),"deadline":"2030-01-01T00:00:00Z"});
    let report = execute(bundle, req).unwrap();
    assert_eq!(report.usage.activations, 2);
    assert!(
        report
            .diagnostic
            .unwrap()
            .contains("Activation budget exhausted")
    );
    let mut raw = source("data.map");
    raw["input_schema"] = json!({"type":"object","properties":{"value":{"type":"string","maxLength":10000}},"required":["value"]});
    raw["entry"] = json!("s0");
    raw["steps"] = json!({});
    for index in 0..70 {
        let target = if index == 69 {
            "$end".to_string()
        } else {
            format!("s{}", index + 1)
        };
        raw["steps"][format!("s{index}")] = json!({"type":"data.map","with":{"value":{"ref":"/input/value"},"output_schema":{"literal":{"type":"string","maxLength":10000}}},"routes":{"success":target}});
    }
    let report = execute(
        build(&raw, DependencySnapshots::default(), vec![]),
        SampleRequest {
            input: json!({"value":"x".repeat(8192)}),
            ..request(vec![])
        },
    )
    .unwrap();
    assert_eq!(report.status, SampleStatus::Stopped);
    assert!(
        report
            .diagnostic
            .as_ref()
            .unwrap()
            .contains("report exceeds"),
        "{report:?}"
    );
    assert!(serde_json::to_vec(&report).unwrap().len() <= MAX_REPORT_BYTES);
}

#[path = "jev_tests.rs"]
mod jev_tests;
