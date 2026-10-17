//! Read-only effect-state hint for trusted provider runtimes.
use super::contracts::{ActionScope, ApprovalSubject};
use crate::application::app_error::AppResult;
use async_trait::async_trait;

/// The projection of durable action facts for one invocation. It is never authority, a
/// replay grant or effect truth: reservation re-decides every case under its locks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionEffectState {
    /// Intent only: no possible-dispatch marker and no receipt.
    Prepared,
    PossibleDispatch,
    NeedsReconciliation,
    /// Verified not-applied evidence exists; reservation still decides whether it is usable.
    NotApplied,
    /// Applied truth without a usable receipt is never a replay grant.
    AppliedWithoutResult,
    Committed,
}

#[async_trait]
pub trait ActionEffectStates: Send + Sync {
    /// Lets a runtime decide whether pre-reservation transport work can still fail cleanly
    /// (only while `Prepared`). Errors propagate; there is no default state.
    async fn effect_state(
        &self,
        scope: ActionScope,
        subject: &ApprovalSubject,
    ) -> AppResult<ActionEffectState>;
}
