//! Pure views the authoring pages render (p09 B1a): the read-only graph of a compiled
//! definition, a bounded line diff between two sources, the publication selections a draft
//! asks for, and the step catalogue reference. Nothing here decides or grants anything; the
//! lifecycle commands still validate, compile and authorize every save and publication.
mod catalogue;
mod diff;
mod graph;
mod needs;

pub use catalogue::{CatalogueEntry, catalogue_reference};
pub use diff::{DiffLine, LineDiff, MAX_DIFF_CELLS, line_diff};
pub use graph::{END_NODE, GraphEdge, GraphEdgeKind, GraphNode, WorkflowGraph, workflow_graph};
pub use needs::{McpNeed, PublicationNeeds, ToolNeed, publication_needs};

#[cfg(test)]
mod tests;
