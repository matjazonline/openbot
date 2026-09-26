//! Application-owned workflow boundaries. Admission atomically owns the run snapshot,
//! first execution and job; cancellation owns its compare-and-set invalidation.
//! Lifecycle authorization checks current membership and related visibility.
//! Resource/effect authorization, production persistence and workers follow.

mod authorization;
mod contracts;
mod ports;
mod service;

pub use authorization::*;
pub use contracts::*;
pub use ports::*;
pub use service::WorkflowService;

#[cfg(test)]
mod tests;
