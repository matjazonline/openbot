use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::memory::{
        LeasedCleanupJob, LeasedProvisioningJob, MemoryConnection, MemoryProviderKind,
    },
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
    hydradb_configured: bool,
}

impl MemoryUseCases {
    pub fn new(
        companies: Arc<dyn CompanyPersistence>,
        persistence: Arc<dyn MemoryConnectionPersistence>,
        bindings: Arc<dyn MemoryBindingPersistence>,
        hydradb_configured: bool,
    ) -> Self {
        Self {
            companies,
            persistence,
            bindings,
            hydradb_configured,
        }
    }

    pub fn hydradb_configured(&self) -> bool {
        self.hydradb_configured
    }

    pub async fn status(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Option<MemoryConnection>> {
        owned_company(self.companies.as_ref(), user_id, company_id).await?;
        let binding = self.bindings.active_binding(company_id).await?;
        if !self.hydradb_configured {
            return Ok(None);
        }
        Ok(binding.connection())
    }

    pub async fn select_hydradb(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<MemoryConnection> {
        if !self.hydradb_configured {
            return Err(AppError::BadRequest(
                "HydraDB is not configured for this deployment.".into(),
            ));
        }
        owned_company(self.companies.as_ref(), user_id, company_id).await?;
        self.persistence
            .select_provider(company_id, MemoryProviderKind::Hydradb)
            .await
    }

    pub async fn disable(&self, user_id: Uuid, company_id: Uuid) -> AppResult<()> {
        owned_company(self.companies.as_ref(), user_id, company_id).await?;
        self.persistence.disable_provider(company_id).await
    }

    pub async fn retry(&self, user_id: Uuid, company_id: Uuid) -> AppResult<()> {
        if !self.hydradb_configured {
            return Err(AppError::BadRequest(
                "HydraDB is not configured for this deployment.".into(),
            ));
        }
        let company = owned_company(self.companies.as_ref(), user_id, company_id).await?;
        if company.memory_provider != Some(MemoryProviderKind::Hydradb) {
            return Err(AppError::BadRequest(
                "Select HydraDB before retrying provisioning.".into(),
            ));
        }
        self.persistence.retry_provisioning(company_id).await
    }
}
