use super::*;
use crate::adapters::workflow_source::WorkflowSourceDecoder;
use crate::application::workflow::compiler::{SourceEncoder, rebase_workflow_id};
use crate::application::workflow::registry::CatalogueFacts;
use crate::application::workflow::templates::{
    AGENT_BASE_SOURCE, TemplateCatalogue, TemplateOffer,
};
use crate::domain::workflow::{TemplateId, TemplateRevision, WorkflowId};

fn plain(source: &str) -> Value {
    json_value(&WorkflowSourceDecoder.decode(source).unwrap().root, "").unwrap()
}

fn composed(workflow: &AgentWorkflow) -> String {
    let source = workflow
        .compose(&WorkflowSourceDecoder, AGENT_BASE_SOURCE)
        .unwrap();
    let text = WorkflowSourceDecoder.encode(&source).unwrap();
    compiles(&text);
    text
}

/// As the shipped catalogue validates a template: decoded, compiled against illustrative facts.
fn compiles(source: &str) {
    let facts = CatalogueFacts::default();
    let dependencies =
        BTreeMap::from([(WorkflowId::new(uuid::Uuid::from_u128(2)), BTreeSet::new())]);
    TemplateCatalogue::build(
        &[TemplateOffer {
            id: TemplateId::parse("composed").unwrap(),
            revision: TemplateRevision::new(1).unwrap(),
            title: "Composed",
            description: "",
            source,
            facts: &facts,
            dependencies: &dependencies,
        }],
        &WorkflowSourceDecoder,
    )
    .unwrap_or_else(|error| panic!("{error:?}\n{source}"));
}

fn step(value: Value) -> CustomStep {
    serde_json::from_value(value).unwrap()
}

fn classify() -> CustomStep {
    step(
        json!({"id": "topic", "slot": "before_draft", "type": "ai.classify", "with": {
        "agent": {"literal": "assistant"},
        "context": {"object": {"untrusted_message": {"ref": "/input/message"}}},
        "output_schema": {"literal": {"type": "string", "enum": ["billing", "other"]}}}}),
    )
}

fn gate() -> CustomStep {
    step(
        json!({"id": "gate", "slot": "before_reply", "type": "decision.rule",
        "with": {"data": {"ref": "/steps/topic/output"},
            "data_schema": {"literal": {"type": "string", "enum": ["billing", "other"]}}},
        "rule": {"cases": [{"when": {"eq": [{"ref": "/steps/topic/output"}, {"literal": "billing"}]},
            "choice": "send"}], "default": "hold"},
        "routes": {"choices": {"send": "$next", "hold": "$next"}}}),
    )
}

fn note() -> CustomStep {
    step(
        json!({"id": "note", "slot": "after_reply", "type": "data.map", "with": {
        "value": {"object": {"topic": {"ref": "/steps/topic/output"}}},
        "output_schema": {"literal": {"type": "object", "required": ["topic"],
            "additionalProperties": false,
            "properties": {"topic": {"type": "string", "enum": ["billing", "other"]}}}}}}),
    )
}

/// The base is its own composition, and the reviewed mode composes to the shipped reviewed
/// template: one definition of the base steps behind both.
#[test]
fn the_base_composes_to_itself_and_the_reviewed_mode_to_its_template() {
    assert_eq!(
        plain(&composed(&AgentWorkflow::base())),
        plain(AGENT_BASE_SOURCE)
    );
    let reviewed = rebase_workflow_id(
        &WorkflowSourceDecoder,
        &composed(&AgentWorkflow::base().reviewed()),
        WorkflowId::new(uuid::Uuid::from_u128(1)),
    )
    .unwrap();
    assert_eq!(
        plain(&reviewed),
        plain(include_str!("../fixtures/reviewed-support.yaml"))
    );
}

/// Each custom step joins the chain in its slot, `$next` names the step after it, a step before
/// the draft hands its output to the agent, and the steps are written in the order they run.
#[test]
fn custom_steps_join_the_base_in_their_slots() {
    let text =
        composed(
            &AgentWorkflow::base()
                .reviewed()
                .with_steps(vec![note(), gate(), classify()]),
        );
    let source = plain(&text);
    let order = [
        "context", "topic", "draft", "gate", "review", "reply", "note",
    ];
    let at = order.map(|id| text.find(&format!("\n  {id}:\n")).unwrap());
    assert!(at.is_sorted(), "{text}");
    assert_eq!(source["entry"], "context");
    let steps = &source["steps"];
    assert_eq!(steps["context"]["routes"], json!({"success": "topic"}));
    assert_eq!(steps["topic"]["routes"], json!({"success": "draft"}));
    assert_eq!(steps["draft"]["routes"], json!({"success": "gate"}));
    assert_eq!(
        steps["gate"]["routes"],
        json!({"choices": {"send": "review", "hold": "review"}})
    );
    assert_eq!(
        steps["review"]["routes"],
        json!({"choices": {"approve": "reply", "reject": "review_rejected"}})
    );
    assert_eq!(steps["reply"]["routes"], json!({"success": "note"}));
    assert_eq!(
        steps["note"]["routes"],
        json!({"success": "finish_response"})
    );
    assert_eq!(
        steps["finish_response"]["with"]["value"],
        json!({"ref": "/steps/reply/output"})
    );
    assert_eq!(steps["review_rejected"]["type"], "channel.no_reply");
    assert_eq!(
        steps["draft"]["with"]["context"]["object"]["untrusted_topic"],
        json!({"ref": "/steps/topic/output"})
    );
    assert_eq!(
        steps["reply"]["with"]["value"],
        json!({"ref": "/steps/review/output/data"})
    );
    // Without review the reply sends the draft, and a gate continues straight to it.
    let source = plain(&composed(
        &AgentWorkflow::base().with_steps(vec![classify(), gate()]),
    ));
    assert_eq!(
        source["steps"]["gate"]["routes"],
        json!({"choices": {"send": "reply", "hold": "reply"}})
    );
    assert_eq!(
        source["steps"]["reply"]["with"]["value"],
        json!({"ref": "/steps/draft/output"})
    );
}

/// A custom step cannot replace, reorder or crowd out the base.
#[test]
fn custom_steps_that_would_break_the_base_are_refused() {
    let named = |id: &str| {
        let mut step = note();
        step.id = id.into();
        step
    };
    let mut backwards = gate();
    backwards.definition["routes"] = json!({"choices": {"send": "draft", "hold": "$end"}});
    let mut untyped = note();
    untyped.definition.remove("type");
    let mut shadowing = classify();
    shadowing.id = "history".into();
    for (steps, field) in [
        (vec![named("draft")], "/steps/draft"),
        (vec![named("review")], "/steps/review"),
        (vec![note(), note()], "/steps/note"),
        (vec![named("1st")], "/steps/1st"),
        (vec![untyped], "/steps/note"),
        (vec![backwards], "/steps/gate/routes"),
        (vec![shadowing], "/steps/history"),
        (
            (0..=MAX_CUSTOM_STEPS)
                .map(|n| named(&format!("note_{n}")))
                .collect(),
            "/steps",
        ),
    ] {
        let error = AgentWorkflow::base()
            .with_steps(steps)
            .compose(&WorkflowSourceDecoder, AGENT_BASE_SOURCE)
            .unwrap_err();
        assert_eq!(error.code, "compose.agent_workflow");
        assert_eq!(&*error.field_path, field, "{}", error.message);
    }
}

/// The most a workflow can add still runs within the base's own step limit, so raising
/// `MAX_CUSTOM_STEPS` alone fails here rather than in a long run.
#[test]
fn the_largest_composition_fits_the_base_step_limit() {
    // Each note reads the classifier's label.
    let mut steps = vec![classify()];
    steps.extend((1..MAX_CUSTOM_STEPS).map(|n| {
        let mut step = note();
        step.id = format!("note_{n}");
        step
    }));
    // An autonomous composition has room for all six custom nodes and the saved-response tail.
    let source = plain(&composed(&AgentWorkflow::base().with_steps(steps.clone())));
    let limit = source["limits"]["max_steps"].as_u64().unwrap();
    assert_eq!(source["steps"].as_object().unwrap().len() as u64, limit);
    // Review adds one activation on the approved path: refuse it before resource creation.
    assert!(
        AgentWorkflow::base()
            .reviewed()
            .with_steps(steps)
            .compose(&WorkflowSourceDecoder, AGENT_BASE_SOURCE)
            .is_err()
    );
}

#[test]
fn channel_reply_authoring_custom_end_requires_terminal_no_reply() {
    for routes in [
        json!({"success":"$end"}),
        json!({"error":"$end"}),
        json!({"choices":{"approve":"$next","reject":"$end"}}),
    ] {
        let mut custom = classify();
        custom.definition.insert("routes".into(), routes);
        let error = AgentWorkflow::base()
            .with_steps(vec![custom])
            .compose(&WorkflowSourceDecoder, AGENT_BASE_SOURCE)
            .unwrap_err();
        assert!(error.message.contains("Finish without reply"));
    }
    let silence = step(
        json!({"id":"silence","slot":"after_reply","type":"channel.no_reply",
        "with":{"value":{"ref":"/steps/reply/output"},
        "output_schema":{"literal":{"type":"object","properties":{"body":{"type":"string","maxLength":16384}},"required":["body"],"additionalProperties":false}}},
        "routes":{"success":"$end"}}),
    );
    let source = plain(&composed(&AgentWorkflow::base().with_steps(vec![silence])));
    assert_eq!(source["steps"]["silence"]["routes"]["success"], "$end");
    assert!(source["steps"].get("finish_response").is_none());
}
