//! Application-owned workflow boundaries. Admission atomically owns the run snapshot,
//! first execution and job; cancellation owns its compare-and-set invalidation.
//! Lifecycle authorization checks current membership and related visibility.
//! Resource/effect authorization, production persistence and workers follow.

pub mod actions;
pub mod activation;
pub mod agent;
pub mod authoring;
mod authorization;
pub mod batch;
pub mod binding;
pub mod budget;
mod canonical;
pub mod capacity;
pub mod checkpoints;
pub mod children;
pub mod compiler;
pub mod completion;
mod contracts;
pub mod decisions;
pub mod fixtures;
pub mod history;
pub mod http;
pub mod jev;
pub mod lease;
pub mod lifecycle;
pub mod manual;
pub mod mcp;
pub mod message;
pub mod message_admission;
pub mod metrics;
mod ports;
pub mod provisioning;
pub mod publication;
pub mod reads;
pub mod registry;
pub mod run_views;
pub mod sample;
pub mod schedule_admission;
mod service;
pub mod supervise;
pub mod templates;
pub mod tools;
pub mod triggers;
pub mod waits;
pub mod wakeups;

pub use authorization::*;
pub use contracts::*;
pub use ports::*;
pub use service::WorkflowService;

#[cfg(test)]
mod tests;

pub mod polling;
pub mod worker;
