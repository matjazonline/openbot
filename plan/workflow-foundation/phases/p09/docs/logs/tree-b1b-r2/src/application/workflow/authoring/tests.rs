use super::*;
use crate::adapters::workflow_source::{WorkflowSourceDecoder, decode};
use crate::application::workflow::compiler::parse_workflow;
use crate::application::workflow::registry::TYPES;
use crate::domain::workflow::{VersionId, validate};
use serde_json::{Value, json};
use uuid::Uuid;

fn definition(source: &str) -> crate::domain::workflow::WorkflowDefinition {
    let parsed = parse_workflow(&decode(source).unwrap(), VersionId::new(Uuid::nil())).unwrap();
    validate(parsed.definition).unwrap().definition().clone()
}

const SUPPORT: &str = include_str!("../fixtures/reviewed-support.yaml");
const TRIAGE: &str = include_str!("../fixtures/triage-routing.yaml");
const MCP: &str = include_str!("../fixtures/mcp-lookup.yaml");

#[test]
fn the_graph_layers_every_route_downward_and_ends_below_every_step() {
    let graph = workflow_graph(&definition(SUPPORT));
    let layer = |id: &str| graph.node(id).unwrap().layer;
    assert!(graph.node("draft").unwrap().entry);
    assert_eq!((layer("draft"), layer("review"), layer("reply")), (0, 1, 2));
    assert_eq!(layer(END_NODE), 3);
    // Every edge points strictly down, so the drawing never needs a back edge.
    for edge in &graph.edges {
        assert!(layer(&edge.from) < layer(&edge.to), "{edge:?}");
    }
    let review: Vec<_> = graph
        .edges
        .iter()
        .filter(|edge| edge.from == "review")
        .map(|edge| (edge.label.as_str(), edge.to.as_str(), edge.kind))
        .collect();
    assert_eq!(
        review,
        [
            ("approve", "reply", GraphEdgeKind::Choice),
            ("reject", END_NODE, GraphEdgeKind::Choice)
        ]
    );
    assert_eq!(graph.layers, 4);
    assert_eq!(graph.nodes.len(), 4);
}

#[test]
fn a_branching_graph_places_siblings_side_by_side() {
    let definition = definition(TRIAGE);
    let graph = workflow_graph(&definition);
    assert_eq!(graph.nodes.len(), definition.steps.len() + 1);
    assert!(graph.width >= 2, "{graph:?}");
    let mut seen = std::collections::BTreeSet::new();
    for node in &graph.nodes {
        assert!(
            seen.insert((node.layer, node.slot)),
            "two nodes share a cell: {node:?}"
        );
    }
    for edge in &graph.edges {
        assert!(
            graph.node(&edge.from).unwrap().layer < graph.node(&edge.to).unwrap().layer,
            "{edge:?}"
        );
    }
}

#[test]
fn the_diff_aligns_changed_lines_between_shared_ones() {
    let diff = line_diff("a\nb\nc\nd\n", "a\nc\nx\nd\n");
    assert!(diff.exact && diff.changed());
    assert_eq!(
        diff.lines,
        [
            DiffLine::Same("a"),
            DiffLine::Removed("b"),
            DiffLine::Same("c"),
            DiffLine::Added("x"),
            DiffLine::Same("d"),
        ]
    );
    assert!(!line_diff("same\n", "same\n").changed());
    assert_eq!(
        line_diff("", "new").lines,
        [DiffLine::Added("new")],
        "an empty side"
    );
}

#[test]
fn a_diff_past_the_cell_bound_shows_the_middle_as_two_blocks() {
    let side = |tag: &str| {
        (0..1001)
            .map(|line| format!("{tag}{line}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let (old, new) = (
        format!("head\n{}\ntail", side("old")),
        format!("head\n{}\ntail", side("new")),
    );
    let diff = line_diff(&old, &new);
    assert!(!diff.exact);
    assert_eq!(diff.lines.first(), Some(&DiffLine::Same("head")));
    assert_eq!(diff.lines.last(), Some(&DiffLine::Same("tail")));
    assert_eq!(diff.lines[1], DiffLine::Removed("old0"));
    assert_eq!(diff.lines[1002], DiffLine::Added("new0"));
    assert_eq!(diff.lines.len(), 2 + 2 * 1001);
}

#[test]
fn needs_name_each_literal_mcp_slot_tool_and_called_tool_once() {
    let needs = publication_needs(&decode(MCP).unwrap()).unwrap();
    assert_eq!(needs.mcp.len(), 1);
    assert_eq!(
        (needs.mcp[0].slot.as_str(), needs.mcp[0].tool.as_str()),
        ("tickets", "lookup_tickets")
    );
    assert_eq!(needs.mcp[0].steps[0].as_str(), "lookup");
    assert!(needs.tools.is_empty());
    let tool = registry::example("tool.call").unwrap();
    let needs = publication_needs(&decode(&tool.source).unwrap()).unwrap();
    assert_eq!(needs.tools.len(), 1);
    assert_eq!(needs.tools[0].tool.as_str(), "lookup");
    assert!(
        publication_needs(&decode(SUPPORT).unwrap())
            .unwrap()
            .is_empty()
    );
    assert!(publication_needs(&decode("format_version: 1").unwrap()).is_err());
}

use crate::application::workflow::registry;

#[test]
fn the_catalogue_covers_every_runnable_type_with_a_compiling_example() {
    let refused = ["memory.load", "memory.save"];
    let entries = catalogue_reference(&WorkflowSourceDecoder, &refused);
    let names: Vec<_> = entries.iter().map(|entry| entry.type_name).collect();
    let expected: Vec<_> = TYPES
        .iter()
        .copied()
        .filter(|t| !refused.contains(t))
        .collect();
    assert_eq!(names, expected);
    for entry in &entries {
        assert!(entry.input_schema.is_object(), "{}", entry.type_name);
        assert!(!entry.routes.is_empty(), "{}", entry.type_name);
        assert!(
            !entry.notes.contains("Execution unavailable"),
            "{}",
            entry.type_name
        );
        assert!(
            entry.example.contains(entry.type_name),
            "{}",
            entry.type_name
        );
    }
    let human = entries
        .iter()
        .find(|e| e.type_name == "decision.human")
        .unwrap();
    assert!(human.declared_choices);
    let repeat = entries
        .iter()
        .find(|e| e.type_name == "flow.repeat")
        .unwrap();
    assert_eq!((repeat.control, repeat.declared_choices), ("repeat", false));
}

#[test]
fn parameter_fields_follow_the_schema_and_build_the_params_object() {
    let schema = json!({
        "type": "object",
        "properties": {
            "reviewer": {"type": "string", "maxLength": 128, "description": "Who reviews"},
            "tone": {"enum": ["warm", "brief"]},
            "limit": {"type": "integer"},
            "ratio": {"type": "number"},
            "loud": {"type": "boolean"},
            "extra": {"type": "object"},
            "mixed": {"enum": [1, "two"]}
        },
        "required": ["reviewer"]
    });
    let fields = parameter_fields(Some(&schema)).unwrap();
    let by_name = |name: &str| fields.iter().find(|field| field.name == name).unwrap();
    assert_eq!(
        by_name("reviewer").input,
        ParameterInput::Text {
            max_length: Some(128)
        }
    );
    assert!(by_name("reviewer").required && !by_name("tone").required);
    assert_eq!(
        by_name("reviewer").description.as_deref(),
        Some("Who reviews")
    );
    assert_eq!(
        by_name("tone").input,
        ParameterInput::Choice(vec!["warm".into(), "brief".into()])
    );
    assert_eq!(by_name("limit").input, ParameterInput::Integer);
    assert_eq!(by_name("ratio").input, ParameterInput::Number);
    assert_eq!(by_name("loud").input, ParameterInput::Boolean);
    assert_eq!(by_name("extra").input, ParameterInput::Json);
    // A non-string enum has no select; it is typed as JSON.
    assert_eq!(by_name("mixed").input, ParameterInput::Json);

    let typed = std::collections::HashMap::from([
        ("reviewer", "u-1"),
        ("tone", "warm"),
        ("limit", " 7 "),
        ("ratio", "0.5"),
        ("loud", "false"),
        ("extra", r#"{"a": [1]}"#),
        ("mixed", ""),
    ]);
    let params = parameters_from_fields(&fields, |name| {
        typed.get(name).map(|value| value.to_string())
    })
    .unwrap();
    assert_eq!(
        params,
        json!({"reviewer": "u-1", "tone": "warm", "limit": 7, "ratio": 0.5, "loud": false,
               "extra": {"a": [1]}})
    );
    // The stored params come back as the same field values.
    for field in &fields {
        let value = field_value(field, &params);
        let again =
            parameters_from_fields(std::slice::from_ref(field), |_| Some(value.clone())).unwrap();
        assert_eq!(
            again.get(&field.name),
            params.get(&field.name),
            "{}",
            field.name
        );
    }
    for (name, raw, message) in [
        ("limit", "7.5", "Parameter limit must be a whole number."),
        ("ratio", "x", "Parameter ratio must be a number."),
        ("loud", "yes", "Parameter loud must be true or false."),
        ("extra", "{", "Parameter extra must be valid JSON."),
    ] {
        let error =
            parameters_from_fields(&fields, |field| (field == name).then(|| raw.to_owned()))
                .unwrap_err();
        assert_eq!(error, message);
    }
}

#[test]
fn a_schema_without_properties_takes_the_params_as_one_json_object() {
    assert_eq!(parameter_fields(None), None);
    assert_eq!(parameter_fields(Some(&json!(true))), None);
    assert_eq!(parameter_fields(Some(&json!({"type": "object"}))), None);
    let many: serde_json::Map<String, Value> = (0..=MAX_PARAMETER_FIELDS)
        .map(|index| (format!("p{index}"), json!({"type": "string"})))
        .collect();
    assert_eq!(parameter_fields(Some(&json!({"properties": many}))), None);
    assert_eq!(parameters_from_json("  "), Ok(json!({})));
    assert_eq!(parameters_from_json(r#"{"a": 1}"#), Ok(json!({"a": 1})));
    assert_eq!(
        parameters_from_json("[1]"),
        Err("Parameters must be a JSON object.".into())
    );
    assert_eq!(
        parameters_from_json("{"),
        Err("Parameters must be valid JSON.".into())
    );
}

#[test]
fn stored_params_open_as_fields_only_when_saving_them_unchanged_stores_the_same_object() {
    let schema = json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "count": {"type": "integer"},
            "ratio": {"type": "number"},
            "loud": {"type": "boolean"},
            "tone": {"enum": ["warm", "brief"]},
            "extra": {"type": "object"}
        }
    });
    let round_trip = |params: serde_json::Value| {
        let fields = fields_for(Some(&schema), &params)?;
        let values: std::collections::HashMap<String, String> = fields
            .iter()
            .map(|field| (field.name.clone(), field_value(field, &params)))
            .collect();
        let saved = parameters_from_fields(&fields, |name| values.get(name).cloned()).unwrap();
        assert_eq!(saved, params);
        Some(())
    };
    // Integers stay integers, however large; a whole-valued number keeps its written form.
    assert!(round_trip(json!({"count": 5, "ratio": 5})).is_some());
    assert!(round_trip(json!({"count": 9_007_199_254_740_993_i64, "ratio": 0.1})).is_some());
    assert!(round_trip(json!({"ratio": 5.0, "loud": false, "tone": "brief"})).is_some());
    assert!(round_trip(json!({"name": "x", "extra": {"a": [1, {"b": null}]}})).is_some());
    assert!(round_trip(json!({})).is_some());
    // What the fields cannot carry exactly opens as the whole object instead.
    assert_eq!(
        fields_for(Some(&schema), &json!({"name": "x", "other": 1})),
        None
    );
    assert_eq!(fields_for(Some(&schema), &json!({"name": ""})), None);
    assert_eq!(fields_for(Some(&schema), &json!({"name": "  "})), None);
    assert_eq!(fields_for(Some(&schema), &json!({"count": "seven"})), None);
    // A typed integer field refuses a fraction; a number field keeps what was typed.
    let fields = parameter_fields(Some(&schema)).unwrap();
    let typed = |name: &'static str, raw: &'static str| {
        parameters_from_fields(&fields, |field| (field == name).then(|| raw.to_owned()))
    };
    assert_eq!(typed("count", "12"), Ok(json!({"count": 12})));
    assert!(typed("count", "1.5").is_err());
    assert_eq!(typed("ratio", "2"), Ok(json!({"ratio": 2})));
    assert_eq!(typed("ratio", "2.50"), Ok(json!({"ratio": 2.5})));
}
