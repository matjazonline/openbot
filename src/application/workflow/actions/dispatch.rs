//! Reservation is internal orchestration state, never authorization supplied by a caller.
use super::*;
use crate::app_error::{AppError, AppResult};
use crate::application::workflow::{
    compiler::{Schema, SourceSpan},
    lease::*,
};
use async_trait::async_trait;
use serde_json::Value;
use tokio::time::{Instant, sleep_until, timeout};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Clone)]
pub struct ActionDispatchRequest {
    pub subject: ApprovalSubject,
    pub fence: WorkflowFence,
    pub lease: LeasePolicy,
}
impl ActionDispatchRequest {
    pub fn scope(&self) -> ActionScope {
        ActionScope {
            company: self.fence.scope.company,
            run: self.fence.scope.run,
            execution: self.fence.scope.execution,
        }
    }
}

/// No Clone/Serialize and no public constructor. Only the dispatch writer creates this
/// after committing a first marker; only the service uses it to enter a provider.
pub(crate) struct RemoteReservation {
    pub(crate) marker: Uuid,
    pub(crate) entry: Uuid,
    pub(crate) action: FrozenAction,
    pub(crate) request: ActionDispatchRequest,
    pub(crate) window: LeaseWindow,
    pub(crate) provider: ProviderInvocation,
}
pub(crate) enum RemoteReservationResult {
    Reserved(Box<RemoteReservation>),
    Committed(ActionReceipt),
    ApprovalRequired,
    PossibleDispatchExists,
}
pub enum LocalDispatchResult {
    Committed(ActionReceipt),
    ApprovalRequired,
    PossibleDispatchExists,
}

#[async_trait]
pub trait ActionDispatch: Send + Sync {
    async fn local(&self, request: &ActionDispatchRequest) -> AppResult<LocalDispatchResult>;
    /// Provider-neutral orchestration; no caller-owned dispatch permission is exposed.
    async fn remote(
        &self,
        request: &ActionDispatchRequest,
        provider: &dyn RemoteAction,
        cancel: &CancellationToken,
    ) -> AppResult<RemoteDispatchObservation>;
}

/// Trusted writer protocol, available only inside this crate. Final entry consumes
/// the non-cloneable reservation so even internal callers cannot enter it twice.
#[async_trait]
pub(crate) trait RemoteDispatch: Send + Sync {
    async fn reserve_remote(
        &self,
        request: &ActionDispatchRequest,
        contract: Option<&ProviderReplayContract>,
    ) -> AppResult<RemoteReservationResult>;
    async fn enter_remote(&self, reservation: RemoteReservation) -> AppResult<RemoteEntry>;
    /// Consumes evidence of the exact entry. Commit effect truth before checking
    /// whether current authority still permits returning accepted output.
    async fn finish_remote(
        &self,
        entry: RemoteEntry,
        result: Value,
    ) -> AppResult<RemoteDispatchObservation>;
    async fn owns_remote(&self, request: &ActionDispatchRequest) -> AppResult<bool>;
}
pub(crate) struct RemoteEntry {
    pub action: FrozenAction,
    pub deadline: Instant,
    pub provider: ProviderInvocation,
    pub(crate) marker: Uuid,
    pub(crate) entry: Uuid,
    pub(crate) request: ActionDispatchRequest,
}

#[async_trait]
pub trait RemoteAction: Send + Sync {
    /// Trusted registered promise, never discovered from a remote response or inputs.
    /// This synchronous method performs no I/O. Every adapter states its behavior.
    fn replay_contract(&self) -> Option<&ProviderReplayContract>;
    /// The actual cancellable operation. No detached work and no I/O before polling.
    /// A response is only an observation; it never establishes safe replay.
    async fn invoke(
        &self,
        action: &FrozenAction,
        invocation: &ProviderInvocation,
    ) -> AppResult<Value>;
}

/// Only committed receipts are accepted step/tool output.
pub enum RemoteDispatchObservation {
    ApprovalRequired,
    PossibleDispatchExists,
    Committed(ActionReceipt),
    Interrupted,
}

impl<P: ActionDispatch> ActionService<P> {
    pub async fn dispatch_local(
        &self,
        request: &ActionDispatchRequest,
    ) -> AppResult<LocalDispatchResult> {
        self.persistence.local(request).await
    }

    pub async fn dispatch_remote(
        &self,
        request: &ActionDispatchRequest,
        provider: &impl RemoteAction,
        cancel: &CancellationToken,
    ) -> AppResult<RemoteDispatchObservation> {
        self.persistence.remote(request, provider, cancel).await
    }
}

pub(crate) async fn supervise_remote(
    writer: &impl RemoteDispatch,
    request: &ActionDispatchRequest,
    provider: &dyn RemoteAction,
    cancel: &CancellationToken,
) -> AppResult<RemoteDispatchObservation> {
    if cancel.is_cancelled() {
        return Ok(RemoteDispatchObservation::Interrupted);
    }
    let reservation = match writer
        .reserve_remote(request, provider.replay_contract())
        .await?
    {
        RemoteReservationResult::Reserved(reservation) => reservation,
        RemoteReservationResult::Committed(receipt) => {
            if cancel.is_cancelled() {
                return Ok(RemoteDispatchObservation::Interrupted);
            }
            return Ok(RemoteDispatchObservation::Committed(receipt));
        }
        RemoteReservationResult::ApprovalRequired => {
            return Ok(RemoteDispatchObservation::ApprovalRequired);
        }
        RemoteReservationResult::PossibleDispatchExists => {
            return Ok(RemoteDispatchObservation::PossibleDispatchExists);
        }
    };
    let entry = writer.enter_remote(*reservation).await?;
    let deadline = entry.deadline;
    if cancel.is_cancelled() || Instant::now() >= deadline {
        return Ok(RemoteDispatchObservation::Interrupted);
    }
    // No await from the final monotonic check to first provider polling. Box this
    // external seam to bound debug stacks; dropping it cancels the actual work.
    let mut work = Box::pin(provider.invoke(&entry.action, &entry.provider));
    let mut heartbeat = Instant::now() + request.lease.heartbeat();
    let result = loop {
        tokio::select! { biased;
            _ = cancel.cancelled() => break Ok(None),
            _ = sleep_until(deadline) => break Ok(None),
            _ = sleep_until(heartbeat) => {
                let valid = tokio::select! { biased;
                    _ = cancel.cancelled() => break Ok(None),
                    _ = sleep_until(deadline) => break Ok(None),
                    result = timeout(request.lease.persistence_timeout(), writer.owns_remote(request)) => result,
                };
                match valid {
                    Ok(Ok(true)) => heartbeat = Instant::now() + request.lease.heartbeat(),
                    Ok(Err(error)) => break Err(error),
                    _ => break Ok(None),
                }
            }
            result = &mut work => {
                break result.and_then(|result| validate_action_result(&entry.action, result))
                    .map(Some);
            }
        }
    };
    drop(work);
    let Some(result) = result? else {
        return Ok(RemoteDispatchObservation::Interrupted);
    };
    // Dropped provider work cannot outlive receipt settlement or a new owner.
    let observation = writer.finish_remote(entry, result).await?;
    if cancel.is_cancelled() || Instant::now() >= deadline {
        return Ok(RemoteDispatchObservation::Interrupted);
    }
    Ok(observation)
}

pub(crate) fn validate_action_result(action: &FrozenAction, result: Value) -> AppResult<Value> {
    let result = super::freeze::canonical(&result, super::freeze::MAX_ARGUMENT_BYTES)?;
    let span = SourceSpan {
        start: 0,
        end: 0,
        line: 1,
        column: 1,
    };
    Schema::compile(
        action.request().contract.contract.output_schema.clone(),
        "/output",
        span,
    )
    .and_then(|schema| schema.validate(&result, "/output", span))
    .map_err(|_| AppError::BadRequest("Invalid action result".into()))?;
    Ok(result)
}
