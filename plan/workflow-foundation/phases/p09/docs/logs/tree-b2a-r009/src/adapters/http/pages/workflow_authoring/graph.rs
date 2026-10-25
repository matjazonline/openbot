//! The read-only workflow graph: an SVG drawing and the same steps and routes as a list, both
//! from one [`WorkflowGraph`] built from the compiled definition. Colours are theme tokens, so
//! the drawing follows the light and dark themes.
use super::*;
use crate::application::workflow::authoring::{END_NODE, GraphEdgeKind, WorkflowGraph};
use std::collections::BTreeMap;

/// Past this many nodes the drawing would not be readable; the list still shows every step.
pub const MAX_DRAWN_NODES: usize = 60;

const NODE_WIDTH: usize = 176;
const NODE_HEIGHT: usize = 46;
const GAP_X: usize = 28;
const GAP_Y: usize = 64;
const MARGIN: usize = 16;

/// How a run marks a step on its graph (p09 B2); authoring draws every step plain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeMark {
    Done,
    Current,
    Failed,
    /// Started but never finished, in a run that has ended.
    Stopped,
}

impl NodeMark {
    fn style(self) -> &'static str {
        match self {
            Self::Done => {
                "fill: var(--color-base-200); stroke: var(--color-success); stroke-width: 2"
            }
            Self::Current => {
                "fill: var(--color-base-200); stroke: var(--color-primary); stroke-width: 3"
            }
            Self::Failed => {
                "fill: var(--color-base-200); stroke: var(--color-error); stroke-width: 3"
            }
            Self::Stopped => {
                "fill: var(--color-base-200); stroke: var(--color-warning); stroke-width: 2; stroke-dasharray: 5 3"
            }
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Current => "current",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
        }
    }
}

fn truncated(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        text.to_owned()
    } else {
        format!("{}…", text.chars().take(limit - 1).collect::<String>())
    }
}

fn centre(graph: &WorkflowGraph, layer: usize, slot: usize) -> (usize, usize) {
    let in_layer = graph
        .nodes
        .iter()
        .filter(|node| node.layer == layer)
        .count()
        .max(1);
    let total = graph.width.max(1) * (NODE_WIDTH + GAP_X);
    let used = in_layer * (NODE_WIDTH + GAP_X);
    let x = MARGIN + (total - used) / 2 + slot * (NODE_WIDTH + GAP_X) + NODE_WIDTH / 2;
    let y = MARGIN + layer * (NODE_HEIGHT + GAP_Y) + NODE_HEIGHT / 2;
    (x, y)
}

/// The drawing and the list. `marks` highlights steps of a run; empty for authoring.
pub fn workflow_graph_view(graph: &WorkflowGraph, marks: &BTreeMap<String, NodeMark>) -> String {
    let drawing = if graph.nodes.len() > MAX_DRAWN_NODES {
        format!(
            r#"<p class="text-sm opacity-70">This workflow has {} steps, too many to draw; the list below shows every step and route.</p>"#,
            graph.nodes.len() - 1
        )
    } else {
        graph_svg(graph, marks)
    };
    format!(
        r#"<div class="space-y-3">
            <div class="overflow-x-auto rounded-box border border-base-300 bg-base-100 p-2">{drawing}</div>
            {list}
        </div>"#,
        list = graph_list(graph, marks),
    )
}

fn graph_svg(graph: &WorkflowGraph, marks: &BTreeMap<String, NodeMark>) -> String {
    let width = 2 * MARGIN + graph.width.max(1) * (NODE_WIDTH + GAP_X) - GAP_X;
    let height = 2 * MARGIN + graph.layers.max(1) * (NODE_HEIGHT + GAP_Y) - GAP_Y;
    let position: BTreeMap<&str, (usize, usize)> = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), centre(graph, node.layer, node.slot)))
        .collect();
    let edges: String = graph
        .edges
        .iter()
        .filter_map(|edge| {
            let (&(x1, y1), &(x2, y2)) = (position.get(edge.from.as_str())?, position.get(edge.to.as_str())?);
            let (y1, y2) = (y1 + NODE_HEIGHT / 2, y2 - NODE_HEIGHT / 2);
            let middle = (y1 + y2) / 2;
            let dash = if edge.kind == GraphEdgeKind::Error {
                r#" stroke-dasharray="5 4""#
            } else {
                ""
            };
            let label = if edge.kind == GraphEdgeKind::Success {
                String::new()
            } else {
                format!(
                    r#"<text x="{lx}" y="{ly}" text-anchor="middle" font-size="11" style="fill: var(--color-base-content); opacity: 0.75">{text}</text>"#,
                    lx = (x1 + x2) / 2,
                    ly = middle - 4,
                    text = escape_html_text(&truncated(&edge.label, 20)),
                )
            };
            Some(format!(
                r#"<path d="M{x1} {y1} C{x1} {middle}, {x2} {middle}, {x2} {y2}" fill="none" style="stroke: var(--color-base-content); opacity: 0.45" stroke-width="1.5"{dash} marker-end="url(#workflow-arrow)"/>{label}"#
            ))
        })
        .collect();
    let nodes: String = graph
        .nodes
        .iter()
        .map(|node| {
            let (x, y) = position[node.id.as_str()];
            if node.id == END_NODE {
                return format!(
                    r#"<circle cx="{x}" cy="{y}" r="14" style="fill: var(--color-base-300); stroke: var(--color-base-content)" stroke-width="1"/><text x="{x}" y="{ty}" text-anchor="middle" font-size="10" style="fill: var(--color-base-content)">end</text>"#,
                    ty = y + 4,
                );
            }
            let style = marks
                .get(&node.id)
                .map(|mark| mark.style())
                .unwrap_or("fill: var(--color-base-200); stroke: var(--color-base-content); stroke-opacity: 0.35; stroke-width: 1");
            let entry = if node.entry { "▶ " } else { "" };
            format!(
                r#"<g><title>{id_title} ({kind_title})</title><rect x="{rx}" y="{ry}" width="{NODE_WIDTH}" height="{NODE_HEIGHT}" rx="8" style="{style}"/><text x="{x}" y="{t1}" text-anchor="middle" font-size="12" font-weight="600" style="fill: var(--color-base-content)">{entry}{id}</text><text x="{x}" y="{t2}" text-anchor="middle" font-size="10" font-family="monospace" style="fill: var(--color-base-content); opacity: 0.65">{kind}</text></g>"#,
                rx = x - NODE_WIDTH / 2,
                ry = y - NODE_HEIGHT / 2,
                t1 = y - 3,
                t2 = y + 13,
                id = escape_html_text(&truncated(&node.id, 22)),
                kind = escape_html_text(&node.kind),
                id_title = escape_html_text(&node.id),
                kind_title = escape_html_text(&node.kind),
            )
        })
        .collect();
    format!(
        r#"<svg role="img" aria-label="Workflow graph: {count} steps; the list below has the same steps and routes" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
            <defs><marker id="workflow-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0 0 L10 5 L0 10 z" style="fill: var(--color-base-content); opacity: 0.6"/></marker></defs>
            {edges}{nodes}
        </svg>"#,
        count = graph.nodes.len().saturating_sub(1),
    )
}

/// Every step with its routes, in the drawing's order: the accessible form of the graph.
fn graph_list(graph: &WorkflowGraph, marks: &BTreeMap<String, NodeMark>) -> String {
    let items: String = graph
        .nodes
        .iter()
        .filter(|node| node.id != END_NODE)
        .map(|node| {
            let routes: String = graph
                .edges
                .iter()
                .filter(|edge| edge.from == node.id)
                .map(|edge| {
                    let target = if edge.to == END_NODE {
                        "end".to_owned()
                    } else {
                        edge.to.clone()
                    };
                    format!(
                        r#"<li><span class="font-mono">{label}</span> → <span class="font-mono">{target}</span></li>"#,
                        label = escape_html_text(&edge.label),
                        target = escape_html_text(&target),
                    )
                })
                .collect();
            let mark = marks
                .get(&node.id)
                .map(|mark| {
                    format!(
                        r#" <span class="badge badge-sm badge-outline">{}</span>"#,
                        mark.label()
                    )
                })
                .unwrap_or_default();
            format!(
                r#"<li class="py-1"><span class="font-semibold">{id}</span> <span class="font-mono text-xs opacity-70">{kind}</span>{entry}{mark}<ul class="ml-4 list-disc text-xs">{routes}</ul></li>"#,
                id = escape_html_text(&node.id),
                kind = escape_html_text(&node.kind),
                entry = if node.entry {
                    r#" <span class="badge badge-sm badge-ghost">entry</span>"#
                } else {
                    ""
                },
            )
        })
        .collect();
    format!(
        r#"<details class="text-sm"><summary class="cursor-pointer">Steps and routes as a list</summary><ol class="mt-2 divide-y divide-base-300">{items}</ol></details>"#
    )
}
