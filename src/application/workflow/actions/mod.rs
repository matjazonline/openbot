//! Shared action preparation. Frozen intent is neither authorization nor dispatch permission.
//! Dispatch records possible effects; remote receipt settlement remains a separate phase.
mod authorization;
mod contracts;
mod dispatch;
mod freeze;
mod reconciliation;
mod replay;
mod service;
pub use authorization::*;
pub use contracts::*;
pub use dispatch::{
    ActionDispatch, ActionDispatchRequest, LocalDispatchResult, RemoteAction,
    RemoteDispatchObservation,
};
pub(crate) use dispatch::{
    RemoteDispatch, RemoteEntry, RemoteReservation, RemoteReservationResult, supervise_remote,
    validate_action_result,
};
pub use reconciliation::*;
pub use replay::{ProviderInvocation, ProviderReplayContract, ProviderReplayMode};
pub(crate) use replay::{prepare_provider, replay_subject};
pub use service::*;

#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
mod authorization_tests;
