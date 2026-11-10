//! Guarded HTTP transport. Construction is owned by the production action composition.
use crate::{
    app_error::{AppError, AppResult},
    application::workflow::{actions::*, http::*},
};
use async_trait::async_trait;
use serde_json::Value;
use std::time::Duration;

pub use crate::adapters::guarded_http::HttpDestinationPolicy;

pub(crate) struct HttpProvider {
    pub persistence: crate::adapters::persistence::PostgresPersistence,
    pub policy: HttpDestinationPolicy,
}
pub struct HttpRuntime {
    inner: crate::application::workflow::http::HttpActions<
        crate::adapters::persistence::PostgresPersistence,
        crate::adapters::integrations::workflow_dispatch::IntegrationActionDispatch<
            crate::adapters::persistence::workflow::http::HttpActionAuthority,
            NoLocal,
        >,
        HttpProvider,
    >,
}
/// No same-database effect: remote providers only. Shared by the MCP composition.
pub(crate) struct NoLocal;
#[async_trait]
impl crate::adapters::persistence::workflow::action_dispatch::SqlLocalAction for NoLocal {
    async fn apply(
        &self,
        _: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        _: &FrozenAction,
        _: &crate::application::workflow::actions::ApprovalSubject,
    ) -> AppResult<Value> {
        Err(failed())
    }
}
impl HttpRuntime {
    pub fn new(
        persistence: crate::adapters::persistence::PostgresPersistence,
        policy: HttpDestinationPolicy,
        lease: crate::application::workflow::lease::LeasePolicy,
    ) -> Self {
        Self::with_approvals(persistence, policy, lease, None)
    }
    /// `notices` lets a protected action park for review; without them it fails closed.
    pub fn with_approvals(
        persistence: crate::adapters::persistence::PostgresPersistence,
        policy: HttpDestinationPolicy,
        lease: crate::application::workflow::lease::LeasePolicy,
        notices: Option<crate::application::workflow::decisions::DecisionNotices>,
    ) -> Self {
        Self::with_integrations(
            persistence,
            policy,
            lease,
            notices,
            std::sync::Arc::new(
                crate::application::integrations::unavailable::UnavailableIntegrations,
            ),
        )
    }
    pub fn with_integrations(
        persistence: crate::adapters::persistence::PostgresPersistence,
        policy: HttpDestinationPolicy,
        lease: crate::application::workflow::lease::LeasePolicy,
        notices: Option<crate::application::workflow::decisions::DecisionNotices>,
        gateway: std::sync::Arc<dyn crate::application::integrations::ExecutionGateway>,
    ) -> Self {
        use crate::adapters::persistence::workflow::{
            action_dispatch::PostgresActionDispatch, http::HttpActionAuthority,
        };
        let mut dispatch =
            PostgresActionDispatch::new(persistence.clone(), HttpActionAuthority, NoLocal);
        if let Some(notices) = notices {
            dispatch = dispatch.with_decision_notices(notices);
        }
        let dispatch =
            crate::adapters::integrations::workflow_dispatch::IntegrationActionDispatch::new(
                persistence.clone(),
                dispatch,
                gateway,
            );
        Self {
            inner: HttpActions::new(
                persistence.clone(),
                dispatch,
                HttpProvider {
                    persistence,
                    policy,
                },
                lease,
            ),
        }
    }
    pub async fn call_tool(
        &self,
        fence: crate::application::workflow::lease::WorkflowFence,
        call: HttpToolCall,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> AppResult<RemoteDispatchObservation> {
        self.inner.call_tool(fence, call, cancel).await
    }
    /// Returns the durable result or protected-action suspension.
    pub async fn run(
        &self,
        kind: &crate::application::workflow::polling::WorkflowStepKind,
        claim: &crate::application::workflow::lease::ClaimedWorkflow,
    ) -> crate::application::workflow::lease::WorkflowHandlerOutcome {
        Box::pin(self.inner.run(kind, claim)).await
    }
}
#[async_trait]
impl crate::application::workflow::worker::WorkflowHandler for HttpRuntime {
    fn supports(&self, kind: &crate::application::workflow::polling::WorkflowStepKind) -> bool {
        self.inner.supports(kind)
    }
    async fn execute(
        &self,
        kind: &crate::application::workflow::polling::WorkflowStepKind,
        claim: &crate::application::workflow::lease::ClaimedWorkflow,
    ) -> crate::application::workflow::lease::WorkflowHandlerOutcome {
        // Bound the external provider/SQL composition at the adapter seam.
        Box::pin(self.inner.execute(kind, claim)).await
    }
}
#[async_trait]
impl RemoteAction for HttpProvider {
    fn replay_contract(&self) -> Option<&ProviderReplayContract> {
        None
    }
    async fn invoke(&self, action: &FrozenAction, _: &ProviderInvocation) -> AppResult<Value> {
        let request = HttpRequest::parse(&action.request().arguments)?;
        tokio::time::timeout(Duration::from_secs(30), async {
            let (connection, authentication) = self.persistence.http_for_action(action).await?;
            authentication.into_owned()?;
            let client = client(&self.policy, &connection.origin).await?;
            // DNS may have waited: reload revocation and rotated credentials after resolution,
            // immediately before polling the HTTP request. The origin itself is immutable.
            let (connection, authentication) = self.persistence.http_for_action(action).await?;
            let secret = authentication.into_owned()?;
            send(client, &connection, secret.as_ref(), request).await
        })
        .await
        .map_err(|_| failed())?
    }
}

use crate::adapters::guarded_http::{client, send};

#[cfg(test)]
mod tests;

fn failed() -> AppError {
    AppError::BadRequest("HTTP action did not produce an accepted result".into())
}
