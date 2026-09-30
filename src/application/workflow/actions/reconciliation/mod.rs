//! Authorized past-effect evidence. No input or snapshot is a dispatch grant.
mod contracts;
mod service;
mod verification;
pub use contracts::*;
pub use service::*;
pub use verification::*;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod service_tests;
