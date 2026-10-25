use super::*;
use crate::adapters::workflow_source::{WorkflowSourceDecoder, decode};
use crate::application::workflow::compiler::parse_workflow;
use crate::application::workflow::registry::TYPES;
use crate::domain::workflow::{VersionId, validate};
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
