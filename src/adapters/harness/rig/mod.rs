//! Rig construction stays at the adapter boundary. Runtime registration is a later step.
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

pub mod budget;
pub mod compile;
pub mod config;
pub mod skills;

mod builtins;
mod schema;
mod template;
mod tool_hook;
pub mod tools;

mod execution;
mod protocol;
pub use execution::RigHarness;

mod direct;
mod final_response;

mod diagnostics;
mod trace;

#[cfg(test)]
mod diagnostics_tests;

#[cfg(test)]
mod provider_failure_tests;
