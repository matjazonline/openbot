//! Pure views the authoring pages render (p09 B1a, B1b): the read-only graph of a compiled
//! definition, a bounded line diff between two sources, the publication selections a draft
//! asks for, the step catalogue reference, and the binding form's parameter fields. Nothing
//! here decides or grants anything; the lifecycle commands still validate, compile and
//! authorize every save, publication and binding.
mod catalogue;
mod diff;
mod graph;
mod needs;
mod params;

pub use catalogue::{CatalogueEntry, catalogue_reference};
pub use diff::{DiffLine, LineDiff, MAX_DIFF_CELLS, line_diff};
pub use graph::{END_NODE, GraphEdge, GraphEdgeKind, GraphNode, WorkflowGraph, workflow_graph};
pub use needs::{McpNeed, PublicationNeeds, ToolNeed, publication_needs};
pub use params::{
    MAX_PARAMETER_FIELDS, ParameterField, ParameterInput, field_value, fields_for,
    parameter_fields, parameters_from_fields, parameters_from_json,
};

#[cfg(test)]
mod tests;
