//! Application-owned workflow boundaries. Admission atomically owns the run snapshot,
//! first execution and job; cancellation owns its compare-and-set invalidation.
//! Lifecycle authorization checks current membership and related visibility.
//! Resource/effect authorization, production persistence and workers follow.

pub mod actions;
pub mod activation;
mod authorization;
pub mod batch;
pub mod binding;
pub mod budget;
pub mod capacity;
pub mod compiler;
pub mod completion;
mod contracts;
pub mod fixtures;
pub mod lease;
pub mod lifecycle;
mod ports;
pub mod publication;
pub mod registry;
mod service;
pub mod supervise;
pub mod templates;
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
