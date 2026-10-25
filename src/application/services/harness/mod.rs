//! What a model step reports and records, independent of the provider that ran it: the run
//! checkpoint (`runs`), its output and token usage (`output`), the secret scrubbing every output
//! passes through (`redaction`), and the one-shot [`TextClassifier`] the system-prompt generator
//! uses. The legacy task harness, its ports and registry are gone (p08 B4b).

pub mod classifier;
pub mod output;
pub mod redaction;
pub mod runs;

pub use classifier::{ClassificationRequest, TextClassifier};
pub use output::{AgentExecutionOutput, TokenUsage};
pub use redaction::sanitize_text;
