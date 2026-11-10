//! Host adapter: native actions own durable effects; integrations own preparation and bytes.
use crate::{
    adapters::persistence::{
        PostgresPersistence,
        workflow::action_dispatch::{PostgresActionDispatch, SqlActionAuthority, SqlLocalAction},
    },
    app_error::{AppError, AppResult},
    application::{integrations::*, workflow::actions::*},
    domain::integrations::*,
};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio_util::sync::CancellationToken;

pub(crate) struct IntegrationActionDispatch<A, L> {
    persistence: PostgresPersistence,
    native: Arc<PostgresActionDispatch<A, L>>,
    gateway: Arc<dyn ExecutionGateway>,
}
impl<A, L> IntegrationActionDispatch<A, L> {
    pub(crate) fn new(
        persistence: PostgresPersistence,
        native: PostgresActionDispatch<A, L>,
        gateway: Arc<dyn ExecutionGateway>,
    ) -> Self {
        Self {
            persistence,
            native: Arc::new(native),
            gateway,
        }
    }
}
type NativeObservation = Arc<Mutex<Option<AppResult<RemoteDispatchObservation>>>>;

#[async_trait]
impl<A: SqlActionAuthority + 'static, L: SqlLocalAction + 'static> ActionDispatch
    for IntegrationActionDispatch<A, L>
{
    async fn local(&self, request: &ActionDispatchRequest) -> AppResult<LocalDispatchResult> {
        self.native.local(request).await
    }
    async fn remote(
        &self,
        request: &ActionDispatchRequest,
        provider: &dyn RemoteAction,
        cancel: &CancellationToken,
    ) -> AppResult<RemoteDispatchObservation> {
        if !self.persistence.action_uses_integration(request).await? {
            return self.native.remote(request, provider, cancel).await;
        }
        if cancel.is_cancelled() {
            return Ok(RemoteDispatchObservation::Interrupted);
        }
        // HTTP/MCP currently advertise no provider replay guarantee. A new registration must
        // actually transmit its proof before this adapter can accept it.
        if provider.replay_contract().is_some() {
            return Ok(RemoteDispatchObservation::PreparationFailed(
                PreDispatchFailure::Invalid,
            ));
        }
        match self.native.preflight_remote(request, None).await? {
            RemoteReservationResult::Prepared(action)
                if action.request().context.integration.is_some() => {}
            other => return preflight_observation(other),
        }
        let prepared = self
            .persistence
            .prepare_integration_invocation(
                request.clone(),
                DispatchId::new(uuid::Uuid::new_v4()),
                Utc::now() + chrono::Duration::seconds(30),
                cancel.clone(),
            )
            .await;
        let (invocation, mut context) = match prepared {
            Ok(value) => value,
            Err(error) => {
                // This constructor cannot create a native entry. Preserve that phase even
                // when a valid native request exceeds the portable projection's bound.
                let failure = match error {
                    AppError::BadRequest(_) => PreDispatchFailure::Invalid,
                    AppError::Forbidden(_)
                    | AppError::NotFound(_)
                    | AppError::InvalidCredentials => PreDispatchFailure::Denied,
                    _ if cancel.is_cancelled() => PreDispatchFailure::Cancelled,
                    _ => PreDispatchFailure::Retry,
                };
                return Ok(RemoteDispatchObservation::PreparationFailed(failure));
            }
        };
        let observed = Arc::new(Mutex::new(None));
        context.dispatch = Arc::new(NativeBusinessDispatch {
            native: self.native.clone(),
            request: request.clone(),
            expected: invocation.clone(),
            cancel: cancel.clone(),
            observed: observed.clone(),
            entered: AtomicBool::new(false),
        });
        let result = match &invocation.target {
            OperationTarget::Http => self.gateway.execute_http(invocation, context).await,
            OperationTarget::Mcp { .. } => self.gateway.call_mcp(invocation, context).await,
        };
        if let Some(native) = observed.lock().map_err(|_| failed())?.take() {
            return native;
        }
        // No new native entry was made. Any retained earlier effect/proof stays authoritative.
        let failure = match result {
            Ok(ExecutionOutcome::PreDispatch { failure }) => failure,
            Err(IntegrationError::Denied | IntegrationError::NotFound) => {
                PreDispatchFailure::Denied
            }
            Err(IntegrationError::Invalid | IntegrationError::Limit) => PreDispatchFailure::Invalid,
            _ => PreDispatchFailure::Retry,
        };
        Ok(RemoteDispatchObservation::PreparationFailed(failure))
    }
}
fn preflight_observation(result: RemoteReservationResult) -> AppResult<RemoteDispatchObservation> {
    match result {
        RemoteReservationResult::Committed(receipt) => {
            Ok(RemoteDispatchObservation::Committed(receipt))
        }
        RemoteReservationResult::ApprovalRequired => {
            Ok(RemoteDispatchObservation::ApprovalRequired)
        }
        RemoteReservationResult::ApprovalParked(wait) => {
            Ok(RemoteDispatchObservation::ApprovalParked(wait))
        }
        RemoteReservationResult::PossibleDispatchExists => {
            Ok(RemoteDispatchObservation::PossibleDispatchExists)
        }
        RemoteReservationResult::Prepared(_) | RemoteReservationResult::Reserved(_) => {
            Err(failed())
        }
    }
}
struct NativeBusinessDispatch<A, L> {
    native: Arc<PostgresActionDispatch<A, L>>,
    request: ActionDispatchRequest,
    expected: AuthorizedInvocation,
    cancel: CancellationToken,
    observed: NativeObservation,
    entered: AtomicBool,
}
#[async_trait]
impl<A: SqlActionAuthority, L: SqlLocalAction> BusinessDispatch for NativeBusinessDispatch<A, L> {
    async fn dispatch(
        &self,
        i: &AuthorizedInvocation,
        call: &dyn PreparedBusinessCall,
    ) -> IntegrationResult<ExecutionOutcome> {
        if serde_json::to_value(i).map_err(|_| IntegrationError::Invalid)?
            != serde_json::to_value(&self.expected).map_err(|_| IntegrationError::Invalid)?
            || self.entered.swap(true, Ordering::AcqRel)
        {
            return Err(IntegrationError::Denied);
        }
        let native = self
            .native
            .remote(&self.request, &NativeCall(call), &self.cancel)
            .await;
        let projected = match &native {
            Ok(RemoteDispatchObservation::Committed(receipt)) => {
                ProviderData::from_redacted(receipt.result.clone())
                    .map(|data| ExecutionOutcome::Committed { data })
            }
            Ok(
                RemoteDispatchObservation::ApprovalRequired
                | RemoteDispatchObservation::ApprovalParked(_),
            ) => Ok(ExecutionOutcome::PreDispatch {
                failure: PreDispatchFailure::Denied,
            }),
            _ => Ok(ExecutionOutcome::Uncertain),
        };
        *self
            .observed
            .lock()
            .map_err(|_| IntegrationError::Unavailable)? = Some(native);
        projected
    }
}
struct NativeCall<'a>(&'a dyn PreparedBusinessCall);
#[async_trait]
impl RemoteAction for NativeCall<'_> {
    fn replay_contract(&self) -> Option<&ProviderReplayContract> {
        None
    }
    async fn invoke(&self, _: &FrozenAction, _: &ProviderInvocation) -> AppResult<Value> {
        Ok(self.0.call().await?.value().clone())
    }
}
fn failed() -> AppError {
    AppError::Internal("Integration dispatch state unavailable".into())
}
