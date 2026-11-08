//! Private authenticated execution, before entering the host's native effect protocol.
use super::{private::TypedInjection, registry::CodeOwnedRegistry, renewal::*, *};
use crate::domain::integrations::*;
use async_trait::async_trait;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[async_trait]
pub(crate) trait AuthenticatedTransports: Send + Sync {
    fn mcp_destination(
        &self,
        destination: &crate::domain::integrations::authority::ApprovedDestination,
    ) -> IntegrationResult<()>;
    async fn discover_mcp(
        &self,
        context: &OwnerDiscoveryContext,
        injection: TypedInjection,
        work: crate::application::services::mcp_client::McpPreparation,
    ) -> ResolutionResult<Box<dyn discovery::PreparedCatalog>>;
    fn http_url(&self, invocation: &AuthorizedInvocation) -> IntegrationResult<url::Url>;
    async fn prepare_http(
        &self,
        invocation: &AuthorizedInvocation,
        context: &ExecutionContext,
        injection: TypedInjection,
    ) -> ResolutionResult<Box<dyn PreparedBusinessCall>>;
    async fn prepare_mcp(
        &self,
        invocation: &AuthorizedInvocation,
        context: &ExecutionContext,
        injection: TypedInjection,
        work: crate::application::services::mcp_client::McpPreparation,
    ) -> ResolutionResult<Box<dyn PreparedBusinessCall>>;
}

pub(crate) struct AuthenticatedExecution {
    pub registry: Arc<CodeOwnedRegistry>,
    pub credentials: Arc<dyn ExecutionCredentials>,
    pub transports: Arc<dyn AuthenticatedTransports>,
}
impl AuthenticatedExecution {
    fn validate(&self, i: &AuthorizedInvocation) -> IntegrationResult<()> {
        i.target.validate()?;
        // Revalidate even an in-process projection, just as a decoded wire message would be.
        OperationArguments::new(i.arguments.value().clone())?;
        let entry = self.registry.get(&i.authorization.app)?;
        if self.registry.policy(&i.authorization.app)? != i.authorization.app_policy
            || !entry.app.destinations.is_empty()
                && !entry
                    .app
                    .destinations
                    .contains(&i.authorization.destination)
        {
            return Err(IntegrationError::Denied);
        }
        match &i.target {
            OperationTarget::Http => entry.app.transport.http(&self.transports.http_url(i)?),
            OperationTarget::Mcp { .. } => {
                entry.app.transport.mcp()?;
                self.transports
                    .mcp_destination(&i.authorization.destination)
            }
        }
    }
    async fn prepare(
        &self,
        i: &AuthorizedInvocation,
        c: &ExecutionContext,
    ) -> ResolutionResult<(Box<dyn PreparedBusinessCall>, CredentialGeneration)> {
        self.validate(i)?;
        for _ in 0..2 {
            let credential = self.credentials.resolve(i, c).await?;
            if credential.authorization != i.authorization {
                return Err(IntegrationError::Denied.into());
            }
            let generation = credential.generation;
            let call = match &i.target {
                OperationTarget::Http => {
                    self.transports
                        .prepare_http(i, c, credential.injection)
                        .await?
                }
                OperationTarget::Mcp { .. } => {
                    self.transports
                        .prepare_mcp(
                            i,
                            c,
                            credential.injection,
                            super::preparation::business(
                                self.credentials.clone(),
                                i,
                                c,
                                generation,
                            ),
                        )
                        .await?
                }
            };
            // DNS/catalog may have waited. The same genuine host guard is required again.
            let latest = match self.credentials.resolve(i, c).await {
                Ok(value) => value,
                Err(error) => {
                    let _ = call.close().await;
                    return Err(error);
                }
            };
            if latest.authorization != i.authorization {
                call.close().await?;
                return Err(IntegrationError::Denied.into());
            }
            if latest.generation == generation {
                return Ok((call, generation));
            }
            call.close().await?;
        }
        Err(ResolutionError::Retry(RetryReason::Unavailable, None))
    }
    async fn execute(&self, i: AuthorizedInvocation, c: ExecutionContext) -> ExecutionOutcome {
        tracing::debug!(correlation=i.correlation.as_str(), dispatch=%i.dispatch.as_uuid(), app=i.authorization.app.as_str(), "Preparing integration operation");
        let (call, generation) = match Box::pin(self.prepare(&i, &c)).await {
            Ok(prepared) => prepared,
            Err(error) => {
                return ExecutionOutcome::PreDispatch {
                    failure: preparation_failure(error),
                };
            }
        };
        let guarded = GuardedCall {
            call,
            credentials: self.credentials.clone(),
            invocation: i.clone(),
            context: c.clone(),
            generation,
            called: AtomicBool::new(false),
        };
        // Native dispatch supervises the actual work and settles bounded late truth. Do not
        // drop its settlement with another timeout here; native cancellation owns that boundary.
        let outcome = match c.dispatch.dispatch(&i, &guarded).await {
            Ok(outcome) => outcome,
            Err(_) => ExecutionOutcome::Uncertain,
        };
        if guarded.close().await.is_err() {
            tracing::warn!("Integration transport cleanup did not finish cleanly");
        }
        outcome
    }
}

fn preparation_failure(error: ResolutionError) -> PreDispatchFailure {
    match error {
        ResolutionError::ReconnectRequired => PreDispatchFailure::ReconnectRequired,
        ResolutionError::Configuration => PreDispatchFailure::Configuration,
        ResolutionError::Cancelled => PreDispatchFailure::Cancelled,
        ResolutionError::Retry(_, _) => PreDispatchFailure::Retry,
        ResolutionError::Integration(IntegrationError::Invalid | IntegrationError::Limit) => {
            PreDispatchFailure::Invalid
        }
        ResolutionError::Integration(IntegrationError::Storage | IntegrationError::Unavailable) => {
            PreDispatchFailure::Retry
        }
        ResolutionError::Integration(_) => PreDispatchFailure::Denied,
    }
}

struct GuardedCall {
    call: Box<dyn PreparedBusinessCall>,
    credentials: Arc<dyn ExecutionCredentials>,
    invocation: AuthorizedInvocation,
    context: ExecutionContext,
    generation: CredentialGeneration,
    called: AtomicBool,
}
#[async_trait]
impl PreparedBusinessCall for GuardedCall {
    async fn call(&self) -> IntegrationResult<ProviderData> {
        if self.called.swap(true, Ordering::AcqRel) {
            return Err(IntegrationError::Denied);
        }
        self.credentials
            .check_generation(&self.invocation, &self.context, self.generation)
            .await?;
        if self.context.cancellation.is_cancelled()
            || self.invocation.deadline <= chrono::Utc::now()
        {
            return Err(IntegrationError::Denied);
        }
        self.call.call().await
    }
    async fn close(&self) -> IntegrationResult<()> {
        self.call.close().await
    }
}
#[async_trait]
impl ExecutionGateway for AuthenticatedExecution {
    async fn discover_mcp(&self, c: OwnerDiscoveryContext) -> IntegrationResult<DiscoveredCatalog> {
        let entry = self.registry.get(&c.authorization.app)?;
        if self.registry.policy(&c.authorization.app)? != c.authorization.app_policy
            || !entry.app.destinations.is_empty()
                && !entry
                    .app
                    .destinations
                    .contains(&c.authorization.destination)
        {
            return Err(IntegrationError::Denied);
        }
        entry.app.transport.mcp()?;
        self.transports
            .mcp_destination(&c.authorization.destination)?;
        for _ in 0..2 {
            let credential = self
                .credentials
                .resolve_configuration(&c)
                .await
                .map_err(discovery_error)?;
            if credential.authorization != c.authorization {
                return Err(IntegrationError::Denied);
            }
            let generation = credential.generation;
            let catalog = self
                .transports
                .discover_mcp(
                    &c,
                    credential.injection,
                    super::preparation::configuration(self.credentials.clone(), &c, generation),
                )
                .await
                .map_err(discovery_error)?;
            let latest = self.credentials.resolve_configuration(&c).await;
            let tools = catalog.tools().to_vec();
            let closed = catalog.close().await;
            let latest = latest.map_err(discovery_error)?;
            closed?;
            if latest.authorization != c.authorization {
                return Err(IntegrationError::Denied);
            }
            if generation != latest.generation {
                continue;
            }
            self.credentials.check_configuration(&c, generation).await?;
            return DiscoveredCatalog::from_redacted(tools);
        }
        Err(IntegrationError::Conflict)
    }
    async fn execute_http(
        &self,
        i: AuthorizedInvocation,
        c: ExecutionContext,
    ) -> IntegrationResult<ExecutionOutcome> {
        if i.target != OperationTarget::Http {
            return Err(IntegrationError::Invalid);
        }
        Ok(self.execute(i, c).await)
    }
    async fn call_mcp(
        &self,
        i: AuthorizedInvocation,
        c: ExecutionContext,
    ) -> IntegrationResult<ExecutionOutcome> {
        if !matches!(i.target, OperationTarget::Mcp { .. }) {
            return Err(IntegrationError::Invalid);
        }
        Ok(self.execute(i, c).await)
    }
}

fn discovery_error(error: ResolutionError) -> IntegrationError {
    match error {
        ResolutionError::Integration(error) => error,
        ResolutionError::ReconnectRequired => IntegrationError::Credential,
        ResolutionError::Configuration | ResolutionError::Retry(_, _) => {
            IntegrationError::Unavailable
        }
        ResolutionError::Cancelled => IntegrationError::Denied,
    }
}
