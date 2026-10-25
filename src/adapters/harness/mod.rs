//! The runtime a workflow model step executes on.
//!
//! `rig` drives one checkpointed model loop per workflow execution
//! ([`rig::WorkflowAgentRuntime`]) and the system-prompt generator's one-shot completion
//! ([`rig::RigTextClassifier`]). The legacy task harness and its registry are gone (p08 B4b).

pub mod rig;
