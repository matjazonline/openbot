use super::*;
use crate::adapters::workflow_source::decode;
use crate::application::workflow::compiler::{Diagnostic, PreparedInputs};
use crate::application::workflow::publication::{self, PublishedBundle};
use crate::domain::workflow::{Context, RunId, RunMetadata, StepId};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};

fn company() -> CompanyId {
    CompanyId::new(Uuid::from_u128(90))
}
fn step(value: &str) -> StepId {
    StepId::parse(value).unwrap()
}
fn build(fixture: Fixture, source: &str) -> Result<PublishedBundle, Diagnostic> {
    let children = fixture
        .child()
        .map(bundle)
        .map(Arc::new)
        .into_iter()
        .collect();
    publication::freeze(
        decode(source)?,
        company(),
        fixture.version_id(),
        fixture.snapshots(company()),
        children,
    )
}
fn bundle(fixture: Fixture) -> PublishedBundle {
    build(fixture, fixture.source()).unwrap_or_else(|error| panic!("{fixture:?}: {error:?}"))
}
fn context<'a>(
    input: &'a Value,
    params: &'a Value,
    outputs: &'a BTreeMap<StepId, Value>,
) -> Context<'a> {
    Context {
        input,
        params,
        step_outputs: outputs,
        run: RunMetadata {
            run_id: RunId::new(Uuid::from_u128(91)),
            parent_run_id: None,
        },
    }
}
fn prepare(
    bundle: &PublishedBundle,
    id: &str,
    outputs: &BTreeMap<StepId, Value>,
) -> PreparedInputs {
    let params = if bundle.compiled().source() == Fixture::AutonomousResponse.source() {
        json!({})
    } else {
        json!({"reviewer":"reviewer-1","deadline":"2030-01-01T00:00:00Z"})
    };
    bundle
        .prepare_step_inputs(
            &step(id),
            &context(
                &json!({"message":"Help","source_message":"message-1"}),
                &params,
                outputs,
            ),
        )
        .unwrap()
}

#[test]
fn workflow_fixtures_all_compile_and_freeze_with_real_child_content() {
    for fixture in ALL {
        let bundle = bundle(fixture);
        assert_eq!(bundle.compiled().source(), fixture.source());
        assert_eq!(
            bundle.children().len(),
            usize::from(fixture.child().is_some())
        );
        assert!(!bundle.content_hash().as_str().is_empty());
    }
}

#[test]
fn workflow_fixtures_autonomous_omission_uses_frozen_saved_defaults() {
    let bundle = bundle(Fixture::AutonomousResponse);
    let inputs = prepare(&bundle, "draft", &BTreeMap::new());
    assert!(inputs.value().get("capability_profile").is_none());
    assert!(
        bundle
            .compiled()
            .graph()
            .definition()
            .steps
            .values()
            .all(|s| s.step_type.as_str() != "ai.classify")
    );
    let saved = &bundle.snapshots().agents[0];
    assert_eq!(saved.tools[0].as_str(), "knowledge.search");
    assert_eq!(saved.skills[0].as_str(), "support");
    assert_eq!(inputs.value()["context"]["untrusted_message"], "Help");
    let mut changed = Fixture::AutonomousResponse.snapshots(company());
    changed.agents[0].instructions = "New instructions".into();
    let later = publication::freeze(
        decode(Fixture::AutonomousResponse.source()).unwrap(),
        company(),
        Fixture::AutonomousResponse.version_id(),
        changed,
        vec![],
    )
    .unwrap();
    assert_ne!(bundle.content_hash(), later.content_hash());
    assert!(
        bundle.snapshots().agents[0]
            .instructions
            .starts_with("Draft")
    );
}

#[test]
fn workflow_fixtures_review_routes_human_edits_and_requires_rejection_feedback() {
    let bundle = bundle(Fixture::ReviewedSupport);
    let mut outputs = BTreeMap::from([(step("draft"), json!({"body":"Original"}))]);
    assert_eq!(
        prepare(&bundle, "review", &outputs).value()["proposal"]["body"],
        "Original"
    );
    let edited = json!({"choice":"approve","data":{"body":"Edited"},"feedback":""});
    bundle
        .compiled()
        .validate_step_output(&step("review"), &edited)
        .unwrap();
    outputs.insert(step("review"), edited);
    assert_eq!(
        prepare(&bundle, "reply", &outputs).value()["content"]["body"],
        "Edited"
    );
    assert_eq!(outputs[&step("draft")]["body"], "Original");
    assert!(
        bundle
            .compiled()
            .validate_step_output(
                &step("review"),
                &json!({"choice":"reject","data":{"body":"Original"},"feedback":""})
            )
            .is_err()
    );
    let source: Value = serde_yaml::from_str(Fixture::ReviewedSupport.source()).unwrap();
    assert_eq!(
        source["steps"]["review"]["routes"]["choices"]["reject"],
        "$end"
    );
}

#[test]
fn workflow_fixtures_triage_selects_declared_routes_and_default() {
    let bundle = bundle(Fixture::TriageRouting);
    let rule = bundle.compiled().rule(&step("route")).unwrap();
    for label in ["support", "sales", "other"] {
        bundle
            .compiled()
            .validate_step_output(&step("classify"), &json!(label))
            .unwrap();
        let outputs = BTreeMap::from([(step("classify"), json!(label))]);
        let input = json!({"message":"Help"});
        let params = json!({});
        let context = context(&input, &params, &outputs);
        assert_eq!(
            rule.decide(&context, bundle.compiled().graph().context_limits())
                .unwrap()
                .as_str(),
            label
        );
    }
    assert!(
        bundle
            .compiled()
            .validate_step_output(&step("classify"), &json!("unknown"))
            .is_err()
    );
}

#[test]
fn workflow_fixtures_revision_contract_pins_rounds_and_accepted_data() {
    let parent = bundle(Fixture::RepeatedHumanRevision);
    let child = parent.children().values().next().unwrap();
    assert_eq!(
        child.compiled().source(),
        Fixture::HumanRevisionRound.source()
    );
    let review = json!({"choice":"accept","data":{"body":"Final edited draft"},"feedback":""});
    child.compiled().validate_output(&review).unwrap();
    let result = json!({"rounds":3,"result":review});
    parent
        .compiled()
        .validate_step_output(&step("revisions"), &result)
        .unwrap();
    let outputs = BTreeMap::from([(step("revisions"), result.clone())]);
    assert_eq!(
        prepare(&parent, "reply", &outputs).value()["content"],
        review["data"]
    );
    for rounds in [0, 4] {
        let mut invalid = result.clone();
        invalid["rounds"] = json!(rounds);
        assert!(
            parent
                .compiled()
                .validate_step_output(&step("revisions"), &invalid)
                .is_err()
        );
    }
    let mut revision = review;
    revision["choice"] = json!("revise");
    assert!(
        child
            .compiled()
            .validate_step_output(&step("review"), &revision)
            .is_err()
    );
    revision["feedback"] = json!("Add the missing explanation");
    child
        .compiled()
        .validate_step_output(&step("review"), &revision)
        .unwrap();
    for (choice, expected) in [("accept", "accepted"), ("revise", "exhausted")] {
        let outputs = BTreeMap::from([(
            step("revisions"),
            json!({"rounds":3,"result":{"choice":choice,"data":{"body":"Final"},"feedback":"Explain"}}),
        )]);
        let input = json!({});
        let params = json!({});
        assert_eq!(
            parent
                .compiled()
                .rule(&step("outcome"))
                .unwrap()
                .decide(
                    &context(&input, &params, &outputs),
                    parent.compiled().graph().context_limits()
                )
                .unwrap()
                .as_str(),
            expected
        );
    }
}

#[test]
fn workflow_fixtures_mcp_preserves_typed_arguments_results_and_missing_default() {
    let bundle = bundle(Fixture::McpLookup);
    let input = json!({"limit":3,"include_closed":false,"labels":["urgent"],"owner":null});
    bundle.compiled().validate_input(&input).unwrap();
    let params = json!({});
    let mut outputs = BTreeMap::new();
    let prepared = bundle
        .prepare_step_inputs(&step("lookup"), &context(&input, &params, &outputs))
        .unwrap();
    assert_eq!(
        prepared.value()["arguments"],
        json!({"limit":3,"include_closed":false,"labels":["urgent"],"filter":{"owner":null}})
    );
    let structured = json!({"count":1,"tickets":[{"id":7,"open":true}]});
    let output = json!({"isError":false,"content":[],"structuredContent":structured});
    bundle
        .compiled()
        .validate_step_output(&step("lookup"), &output)
        .unwrap();
    outputs.insert(step("lookup"), output);
    let result = bundle
        .prepare_step_inputs(&step("result"), &context(&input, &params, &outputs))
        .unwrap();
    assert_eq!(result.value()["value"], structured);
    bundle.compiled().validate_output(&structured).unwrap();
    outputs.insert(step("lookup"), json!({"isError":false,"content":[]}));
    assert_eq!(
        bundle
            .prepare_step_inputs(&step("result"), &context(&input, &params, &outputs))
            .unwrap()
            .value()["value"],
        json!({"count":0,"tickets":[]})
    );
    assert!(
        bundle
            .compiled()
            .validate_step_output(
                &step("lookup"),
                &json!({"isError":false,"content":[],"structuredContent":null})
            )
            .is_err()
    );
    let mut missing = input.clone();
    missing.as_object_mut().unwrap().remove("owner");
    assert!(bundle.compiled().validate_input(&missing).is_err());
    assert!(
        bundle
            .prepare_step_inputs(&step("lookup"), &context(&missing, &params, &outputs))
            .is_err()
    );
}

#[test]
fn workflow_fixtures_invalid_variants_return_source_located_errors() {
    let cases = [
        (
            Fixture::AutonomousResponse,
            "type: agent.run",
            "type: unknown",
            "step.type",
        ),
        (
            Fixture::AutonomousResponse,
            "success: reply",
            "success: absent",
            "graph",
        ),
        (
            Fixture::AutonomousResponse,
            "ref: /input/message",
            "ref: /steps/reply/output",
            "reference",
        ),
        (
            Fixture::AutonomousResponse,
            "type: object",
            "type: impossible",
            "schema",
        ),
        (
            Fixture::RepeatedHumanRevision,
            "    max_iterations: 3\n",
            "",
            "step.control",
        ),
        (
            Fixture::McpLookup,
            "limit: {ref: /input/limit}",
            "limit: {literal: three}",
            "binding",
        ),
        (Fixture::McpLookup, "kind: mcp", "kind: http", "registry"),
    ];
    for (fixture, from, to, code) in cases {
        let source = fixture.source().replacen(from, to, 1);
        assert_ne!(source, fixture.source());
        let error = build(fixture, &source)
            .err()
            .unwrap_or_else(|| panic!("{fixture:?}: mutation {from:?} unexpectedly compiled"));
        assert!(error.code.starts_with(code), "{fixture:?}: {error:?}");
        assert!(!error.field_path.is_empty());
        assert!(error.span.start <= error.span.end && error.span.end <= source.len());
        assert!(error.span.line > 0 && error.span.column > 0);
    }
}

#[test]
fn workflow_fixtures_mcp_required_result_and_recursive_child_are_rejected() {
    let fixture = Fixture::McpLookup;
    let mut source: Value = serde_yaml::from_str(fixture.source()).unwrap();
    source["steps"]["result"]["with"]["value"] =
        json!({"ref":"/steps/lookup/output/structuredContent"});
    let error = build(fixture, &source.to_string()).err().unwrap();
    assert_eq!(error.code, "reference.optional");
    let fixture = Fixture::RepeatedHumanRevision;
    let source = fixture.source().replacen(
        "workflow_id: 00000000-0000-0000-0000-000000000004",
        "workflow_id: 00000000-0000-0000-0000-000000000005",
        1,
    );
    assert!(build(fixture, &source).is_err());
}

#[test]
fn workflow_fixtures_mcp_constructed_arguments_check_allof_contracts() {
    let fixture = Fixture::McpLookup;
    let source: Value = serde_yaml::from_str(fixture.source()).unwrap();
    for invalid in [
        json!({"literal":"three"}),
        json!({"concat":[{"literal":"three"}]}),
    ] {
        let mut changed = source.clone();
        changed["steps"]["lookup"]["with"]["arguments"]["object"]["limit"] = invalid;
        assert_eq!(
            build(fixture, &changed.to_string()).err().unwrap().code,
            "binding.type"
        );
    }
    for field in ["include_closed", "filter"] {
        let mut changed = source.clone();
        changed["steps"]["lookup"]["with"]["arguments"]["object"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert_eq!(
            build(fixture, &changed.to_string()).err().unwrap().code,
            "binding.required"
        );
    }
    let mut changed = source;
    changed["steps"]["lookup"]["with"]["arguments"]["object"]["filter"]["object"]["unexpected"] =
        json!({"literal":true});
    assert_eq!(
        build(fixture, &changed.to_string()).err().unwrap().code,
        "binding.property"
    );
}

#[test]
fn workflow_fixtures_nested_defaults_and_allof_keep_compile_work_bounded() {
    let fixture = Fixture::McpLookup;
    let mut snapshots = fixture.snapshots(company());
    let mut schema = json!({"type":"integer","minimum":1,"maximum":20});
    for _ in 0..12 {
        schema = json!({"allOf":[schema]});
    }
    snapshots.tools[0].contract.input_schema["properties"]["limit"] = schema;
    for fallback in [json!(3), json!("three")] {
        let valid = fallback.is_number();
        let mut binding = json!({"literal":fallback});
        for _ in 0..12 {
            binding = json!({"default":{"ref":"/input/limit","value":binding}});
        }
        let mut source: Value = serde_yaml::from_str(fixture.source()).unwrap();
        source["steps"]["lookup"]["with"]["arguments"]["object"]["limit"] = binding;
        let result = publication::freeze(
            decode(&source.to_string()).unwrap(),
            company(),
            fixture.version_id(),
            snapshots.clone(),
            vec![],
        );
        if valid {
            assert!(result.is_ok(), "{:?}", result.err());
        } else {
            assert_eq!(result.err().unwrap().code, "binding.type");
        }
    }
}
