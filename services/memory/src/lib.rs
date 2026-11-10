//! Standalone memory service (plan/.archive/memory-service). Layers: `domain` is pure, `application`
//! holds use cases and ports, `adapters` talk to PostgreSQL, HTTP and MCP, `infra` wires them.
pub mod adapters;
pub mod application;
pub mod domain;
pub mod infra;

#[cfg(test)]
mod boundary_tests;
