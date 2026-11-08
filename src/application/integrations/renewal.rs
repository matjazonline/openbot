//! Private credential resolution and provider renewal. Never a gateway capability.
use super::{AuthorizedInvocation, ExecutionContext, private::TypedInjection};
use crate::domain::integrations::*;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use secrecy::SecretString;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetryReason {
    LockBusy,
    Cooldown,
    Pending,
    AmbiguousCommit,
    Unavailable,
    TokenMargin,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ResolutionError {
    #[error(transparent)]
    Integration(IntegrationError),
    #[error("Credential resolution is retryable")]
    Retry(RetryReason, Option<DateTime<Utc>>),
    #[error("Provider authorization requires reconnect")]
    ReconnectRequired,
    #[error("Provider client configuration is unavailable")]
    Configuration,
    #[error("Credential resolution cancelled")]
    Cancelled,
}
impl From<IntegrationError> for ResolutionError {
    fn from(error: IntegrationError) -> Self {
        match error {
            IntegrationError::Storage => Self::Retry(RetryReason::Unavailable, None),
            other => Self::Integration(other),
        }
    }
}
pub(crate) type ResolutionResult<T> = Result<T, ResolutionError>;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionPolicy {
    Stateless,
    RebuildOnGeneration,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RevocationScope {
    LocalOnly,
    CombinedProject,
    Installation,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenewalProtocol {
    None,
    Reusable,
    StrictRotating,
}
#[derive(Clone)]
pub(crate) enum RenewalStrategy {
    NoRenewal,
    ReusableRefresh(Arc<dyn RenewalProvider>),
    StrictRotating(Arc<dyn RenewalProvider>),
}
pub(crate) enum RenewalDecision {
    Cached { expires_at: DateTime<Utc> },
    Request,
    Cooldown(DateTime<Utc>),
}
#[derive(Debug, Clone, Copy)]
pub(crate) enum RenewalFailure {
    Reconnect,
    Temporary,
    Configuration,
    Malformed,
    Cancelled,
}
pub(crate) struct RenewalWork {
    pub deadline: DateTime<Utc>,
    pub cancellation: CancellationToken,
}
#[async_trait]
pub(crate) trait RenewalProvider: Send + Sync {
    fn session_policy(&self) -> SessionPolicy;
    fn revocation_scope(&self) -> RevocationScope;
    fn assess(
        &self,
        payload: &SecretString,
        generation: CredentialGeneration,
        now: DateTime<Utc>,
    ) -> ResolutionResult<RenewalDecision>;
    fn cooldown(
        &self,
        payload: &SecretString,
        now: DateTime<Utc>,
    ) -> IntegrationResult<SecretString>;
    async fn renew(
        &self,
        payload: &SecretString,
        next: CredentialGeneration,
        work: &RenewalWork,
    ) -> Result<SecretString, RenewalFailure>;
}
#[derive(Debug)]
pub(crate) struct ResolvedCredential {
    pub injection: TypedInjection,
    pub authorization: crate::domain::integrations::authority::AuthorizationSnapshot,
    pub generation: CredentialGeneration,
    pub expires_at: Option<DateTime<Utc>>,
    pub session_policy: SessionPolicy,
}
#[async_trait]
pub(crate) trait CredentialResolver: Send + Sync {
    async fn resolve(
        &self,
        invocation: &AuthorizedInvocation,
        context: &ExecutionContext,
    ) -> ResolutionResult<ResolvedCredential>;
}

/// Required final check of the private credential selected during preparation.
#[async_trait]
pub(crate) trait ExecutionCredentials: CredentialResolver {
    async fn resolve_configuration(
        &self,
        context: &super::OwnerDiscoveryContext,
    ) -> ResolutionResult<ResolvedCredential>;
    async fn check_configuration(
        &self,
        context: &super::OwnerDiscoveryContext,
        generation: CredentialGeneration,
    ) -> IntegrationResult<()>;
    async fn check_generation(
        &self,
        invocation: &AuthorizedInvocation,
        context: &ExecutionContext,
        generation: CredentialGeneration,
    ) -> IntegrationResult<()>;
}

impl RenewalStrategy {
    pub fn protocol(&self) -> RenewalProtocol {
        match self {
            Self::NoRenewal => RenewalProtocol::None,
            Self::ReusableRefresh(_) => RenewalProtocol::Reusable,
            Self::StrictRotating(_) => RenewalProtocol::StrictRotating,
        }
    }
    pub fn session_policy(&self) -> SessionPolicy {
        match self {
            Self::NoRenewal => SessionPolicy::Stateless,
            Self::ReusableRefresh(p) | Self::StrictRotating(p) => p.session_policy(),
        }
    }
    pub fn revocation_scope(&self) -> RevocationScope {
        match self {
            Self::NoRenewal => RevocationScope::LocalOnly,
            Self::ReusableRefresh(p) | Self::StrictRotating(p) => p.revocation_scope(),
        }
    }
}
