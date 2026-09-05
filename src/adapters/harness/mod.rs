//! The runtimes an agent can actually be executed on.
//!
//! Each submodule implements [`AgentHarness`] for one of them: it takes the harness-neutral
//! [`AgentCapabilitySpec`] the application handed it, compiles it into whatever its runtime
//! accepts, runs the turn, and translates the runtime's callbacks back into the application's
//! ports. Nothing above this directory names a runtime.
//!
//! The registry these register *into* lives in `src/application/services/harness/`, not here:
//! `src/AGENTS.md` -- an abstraction must not live inside the outer adapter it is intended to
//! abstract.
//!
//! [`AgentHarness`]: crate::services::harness::AgentHarness
//! [`AgentCapabilitySpec`]: crate::entities::harness::AgentCapabilitySpec

pub mod ai_agents;
