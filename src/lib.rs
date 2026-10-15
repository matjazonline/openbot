// Clippy 1.99's `double_must_use` fires on every `#[async_trait]` expansion, not on code written
// here. Remove this once async-trait stops triggering it.
#![allow(clippy::double_must_use)]

pub mod adapters;
pub mod application;
pub mod domain;
pub mod infra;

#[cfg(test)]
pub(crate) mod test_support;

// Re-exports for shorter use statements.
pub use application::*;
pub use domain::workflow;
pub use domain::*;
