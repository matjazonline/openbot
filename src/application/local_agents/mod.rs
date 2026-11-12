//! Ports and bounded inputs for company-owned Cloudroom execution.
pub mod bounds;
mod catalog;
pub mod lifecycle;
pub mod management;
pub mod mcp;
mod pairing;
pub mod ports;
pub mod protocol;
mod secrets;
pub mod workflow;

pub use crate::domain::local_agents::{LocalAgentOperation, LocalCommand, LocalPrompt};
pub use catalog::*;
pub use pairing::*;
pub use secrets::*;

#[cfg(test)]
mod contract_tests;

pub mod harness;
#[cfg(test)]
mod wire_tests;
