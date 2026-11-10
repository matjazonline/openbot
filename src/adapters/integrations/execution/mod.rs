//! Credential-bearing transport composition; host handlers receive only sanitized results.
mod http;
mod mcp;
use crate::{
    application::integrations::{
        execute::AuthenticatedTransports, private::TypedInjection, renewal::*, *,
    },
    domain::integrations::*,
};
use async_trait::async_trait;
use chrono::Utc;
use std::sync::Arc;

pub(crate) struct IntegrationTransports {
    pub http: crate::adapters::guarded_http::HttpDestinationPolicy,
    pub mcp: Arc<dyn crate::application::services::mcp_client::McpClient>,
}

#[cfg(test)]
impl IntegrationTransports {
    /// Fixture-only HTTPS metadata -> owned plaintext loopback mapping. It uses the same
    /// guarded client/header/body/call path; it is not TLS or live-provider evidence.
    pub(crate) async fn prepare_http_loopback(
        &self,
        i: &AuthorizedInvocation,
        c: &ExecutionContext,
        injection: TypedInjection,
        origin: crate::application::workflow::http::HttpOrigin,
    ) -> ResolutionResult<Box<dyn PreparedBusinessCall>> {
        Box::pin(http::prepare_loopback(self, i, c, injection, origin))
            .await
            .map_err(Into::into)
    }
}

async fn preflight<T>(
    deadline: chrono::DateTime<Utc>,
    cancellation: &tokio_util::sync::CancellationToken,
    work: impl std::future::Future<Output = IntegrationResult<T>>,
) -> IntegrationResult<T> {
    let remaining = (deadline - Utc::now())
        .to_std()
        .map_err(|_| IntegrationError::Denied)?
        .min(std::time::Duration::from_secs(30));
    tokio::select! { biased;
        _ = cancellation.cancelled() => Err(IntegrationError::Denied),
        _ = tokio::time::sleep(remaining) => Err(IntegrationError::Unavailable),
        result = work => result,
    }
}

#[async_trait]
impl AuthenticatedTransports for IntegrationTransports {
    fn mcp_destination(
        &self,
        destination: &crate::domain::integrations::authority::ApprovedDestination,
    ) -> IntegrationResult<()> {
        let endpoint = crate::entities::mcp::McpEndpoint::try_from(destination.as_str().to_owned())
            .map_err(|_| IntegrationError::Invalid)?;
        self.mcp
            .validate_endpoint(&endpoint)
            .map_err(|_| IntegrationError::Denied)
    }
    async fn discover_mcp(
        &self,
        c: &OwnerDiscoveryContext,
        injection: TypedInjection,
        work: crate::application::services::mcp_client::McpPreparation,
    ) -> ResolutionResult<Box<dyn discovery::PreparedCatalog>> {
        mcp::discover(self, c, injection, work).await
    }
    fn http_url(&self, i: &AuthorizedInvocation) -> IntegrationResult<url::Url> {
        http::url(i)
    }
    async fn prepare_http(
        &self,
        i: &AuthorizedInvocation,
        c: &ExecutionContext,
        injection: TypedInjection,
    ) -> ResolutionResult<Box<dyn PreparedBusinessCall>> {
        http::prepare(self, i, c, injection)
            .await
            .map_err(Into::into)
    }
    async fn prepare_mcp(
        &self,
        i: &AuthorizedInvocation,
        c: &ExecutionContext,
        injection: TypedInjection,
        work: crate::application::services::mcp_client::McpPreparation,
    ) -> ResolutionResult<Box<dyn PreparedBusinessCall>> {
        mcp::prepare(self, i, c, injection, work).await
    }
}
