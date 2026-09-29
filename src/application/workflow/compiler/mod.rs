//! Workflow source and compiler contracts. Decoding is adapter-owned; this layer
//! accepts a bounded, located syntax tree and never imports a YAML implementation.

/// Compatibility of rebuilt runtime validators and evaluation semantics with a
/// persisted publication. Bump before changing schema options, implicit defaults,
/// context/evaluation budgets, registry specialization or interpreter semantics,
/// even when the serialized representation would remain identical. Old revisions
/// require an explicit compatible loader; they must never be silently recompiled.
pub const SEMANTIC_REVISION: u32 = 2;
mod analysis;
mod binding;
mod compile;
mod fact_budget;
mod rebase;
mod schema;
mod schema_budget;
mod shape;
mod source;
pub use rebase::{MAX_SOURCE_BYTES, SourceDecoder, rebase_workflow_id};
mod syntax;
mod wire;

pub(crate) use analysis::{check_availability, check_dependencies};
pub(crate) use binding::parse_binding;
pub use compile::{
    CompileFacts, CompiledWorkflow, ControlKind, PreparedInputs, RouteContract, StepDescriptor,
    compile,
};
pub(crate) use schema::{PathGuarantee, Schema};
pub(crate) use shape::check_binding_shape;
pub use source::{DecodedSource, Diagnostic, LocatedNode, NodeValue, SourceSpan};
pub(crate) use syntax::{fields, json_value, required, scalar_string, sequence};
pub use wire::StepControl;
pub(crate) use wire::{ParsedWorkflow, parse_workflow};

pub(crate) use compile::{
    SpecializedDescriptor, charge_descriptor, check_resolved_budget, compile_resolved,
};

pub(crate) use source::input_span;

pub(crate) use fact_budget::FactBudget;
