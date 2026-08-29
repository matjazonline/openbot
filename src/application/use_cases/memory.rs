use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::memory::{
        LeasedCleanupJob, LeasedProvisioningJob, MemoryConnection, MemoryProviderKind,
    },
    services::memory_provider::ConfiguredMemoryProviders,
    use_cases::company::{CompanyPersistence, owned_company},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveMemoryBinding {
    Disabled,
    NotReady(MemoryConnection),
    Ready(MemoryConnection),
    Misconfigured,
}

impl ActiveMemoryBinding {
    pub fn connection(self) -> Option<MemoryConnection> {
        match self {
            Self::NotReady(connection) | Self::Ready(connection) => Some(connection),
            Self::Disabled | Self::Misconfigured => None,
        }
    }

    pub fn state_name(&self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::NotReady(_) => "not_ready",
            Self::Ready(_) => "ready",
            Self::Misconfigured => "misconfigured",
        }
    }
}

/// The current runtime binding. Implementations must derive this from current company selection,
/// never from a company value serialized into queued work.
#[async_trait]
pub trait MemoryBindingPersistence: Send + Sync {
    async fn active_binding(&self, company_id: Uuid) -> AppResult<ActiveMemoryBinding>;
}

#[async_trait]
pub trait MemoryConnectionPersistence: Send + Sync {
    async fn select_provider(
        &self,
        company_id: Uuid,
        provider: MemoryProviderKind,
    ) -> AppResult<MemoryConnection>;
    async fn disable_provider(&self, company_id: Uuid) -> AppResult<()>;
    async fn retry_provisioning(&self, company_id: Uuid) -> AppResult<()>;

    async fn claim_provisioning_job(
        &self,
        lease_token: Uuid,
        lease_expires_at: DateTime<Utc>,
    ) -> AppResult<Option<LeasedProvisioningJob>>;
    async fn mark_provisioning(&self, job_id: Uuid, lease_token: Uuid) -> AppResult<bool>;
    async fn renew_provisioning_job(
        &self,
        job_id: Uuid,
        lease_token: Uuid,
        lease_expires_at: DateTime<Utc>,
    ) -> AppResult<bool>;
    async fn begin_readiness_polling(
        &self,
        job_id: Uuid,
        lease_token: Uuid,
        readiness_deadline: DateTime<Utc>,
        next_poll_at: DateTime<Utc>,
    ) -> AppResult<bool>;
    async fn schedule_readiness_poll(
        &self,
        job_id: Uuid,
        lease_token: Uuid,
        next_poll_at: DateTime<Utc>,
    ) -> AppResult<bool>;
    async fn complete_provisioning(&self, job_id: Uuid, lease_token: Uuid) -> AppResult<bool>;
    async fn retry_provisioning_job(
        &self,
        job_id: Uuid,
        lease_token: Uuid,
        available_at: DateTime<Utc>,
        safe_error: &str,
        terminal: bool,
    ) -> AppResult<bool>;
    async fn fail_provisioning_job(
        &self,
        job_id: Uuid,
        lease_token: Uuid,
        safe_error: &str,
    ) -> AppResult<bool>;

    async fn claim_cleanup_job(
        &self,
        lease_token: Uuid,
        lease_expires_at: DateTime<Utc>,
    ) -> AppResult<Option<LeasedCleanupJob>>;
    async fn complete_cleanup(&self, job_id: Uuid, lease_token: Uuid) -> AppResult<bool>;
    async fn renew_cleanup_job(
        &self,
        job_id: Uuid,
        lease_token: Uuid,
        lease_expires_at: DateTime<Utc>,
    ) -> AppResult<bool>;
    async fn retry_cleanup_job(
        &self,
        job_id: Uuid,
        lease_token: Uuid,
        available_at: DateTime<Utc>,
        safe_error: &str,
        terminal: bool,
    ) -> AppResult<bool>;
}

#[derive(Clone)]
pub struct MemoryUseCases {
    companies: Arc<dyn CompanyPersistence>,
    persistence: Arc<dyn MemoryConnectionPersistence>,
    bindings: Arc<dyn MemoryBindingPersistence>,
    configured: ConfiguredMemoryProviders,
}

impl MemoryUseCases {
    pub fn new(
        companies: Arc<dyn CompanyPersistence>,
        persistence: Arc<dyn MemoryConnectionPersistence>,
        bindings: Arc<dyn MemoryBindingPersistence>,
        configured: ConfiguredMemoryProviders,
    ) -> Self {
        Self {
            companies,
            persistence,
            bindings,
            configured,
        }
    }

    pub fn configured(&self) -> &ConfiguredMemoryProviders {
        &self.configured
    }

    pub async fn status(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Option<MemoryConnection>> {
        owned_company(self.companies.as_ref(), user_id, company_id).await?;
        let binding = self.bindings.active_binding(company_id).await?;
        // Gate on the *selected* provider: a connection to a provider this deployment no longer
        // configures has no runtime behind it, and reporting it as live would be a lie.
        Ok(binding
            .connection()
            .filter(|connection| self.configured.contains(connection.provider)))
    }

    pub async fn select_provider(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        provider: MemoryProviderKind,
    ) -> AppResult<MemoryConnection> {
        self.require_configured(provider)?;
        owned_company(self.companies.as_ref(), user_id, company_id).await?;
        self.persistence.select_provider(company_id, provider).await
    }

    pub async fn disable(&self, user_id: Uuid, company_id: Uuid) -> AppResult<()> {
        owned_company(self.companies.as_ref(), user_id, company_id).await?;
        self.persistence.disable_provider(company_id).await
    }

    pub async fn retry(&self, user_id: Uuid, company_id: Uuid) -> AppResult<()> {
        let company = owned_company(self.companies.as_ref(), user_id, company_id).await?;
        let Some(provider) = company.memory_provider else {
            return Err(AppError::BadRequest(
                "Select a memory provider before retrying provisioning.".into(),
            ));
        };
        self.require_configured(provider)?;
        self.persistence.retry_provisioning(company_id).await
    }

    fn require_configured(&self, provider: MemoryProviderKind) -> AppResult<()> {
        if self.configured.contains(provider) {
            Ok(())
        } else {
            Err(AppError::BadRequest(format!(
                "{} is not configured for this deployment.",
                provider.label()
            )))
        }
    }
}
