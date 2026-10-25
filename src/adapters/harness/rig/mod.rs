//! Rig construction stays at the adapter boundary: the workflow model step's durable loop and
//! the one-shot classifier.
mod http;
pub mod providers;
mod usage;

#[cfg(test)]
mod provider_tests;

#[cfg(test)]
mod mcp_tests;

#[cfg(test)]
mod batch_tests;

#[cfg(test)]
mod test_support;

mod classifier;
mod execution;
mod protocol;
mod workflow;
pub use classifier::RigTextClassifier;
pub use workflow::WorkflowAgentRuntime;

mod final_response;

mod diagnostics;

#[cfg(test)]
mod diagnostics_tests;

#[cfg(test)]
mod provider_failure_tests;
