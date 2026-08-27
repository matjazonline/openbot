use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::memory::{
        LeasedCleanupJob, LeasedProvisioningJob, MemoryConnection, MemoryConnectionReadiness,
        MemoryProviderKind, MemoryProvisioningPhase,
    },
    use_cases::memory::ActiveMemoryBinding,
};

#[derive(sqlx::FromRow)]
pub(super) struct ActiveMemoryBindingDb {
    selected_provider: Option<String>,
    company_id: Uuid,
    provider: Option<String>,
    remote_database_id: Option<String>,
    readiness: Option<String>,
    last_error: Option<String>,
    provisioning_phase: Option<String>,
    failure_attempts: Option<i32>,
    readiness_deadline: Option<DateTime<Utc>>,
}

#[derive(sqlx::FromRow)]
pub(super) struct MemoryConnectionDb {
    company_id: Uuid,
    provider: String,
    remote_database_id: String,
    readiness: String,
    last_error: Option<String>,
    provisioning_phase: Option<String>,
    failure_attempts: i32,
    readiness_deadline: Option<DateTime<Utc>>,
}

#[derive(sqlx::FromRow)]
pub(super) struct ProvisioningJobDb {
    pub(super) id: Uuid,
    pub(super) company_id: Uuid,
    pub(super) provider: String,
    pub(super) remote_database_id: String,
    pub(super) phase: String,
    pub(super) failure_attempts: i32,
    pub(super) readiness_deadline: Option<DateTime<Utc>>,
    pub(super) lease_token: Uuid,
    pub(super) operation_generation: i64,
}

#[derive(sqlx::FromRow)]
pub(super) struct CleanupJobDb {
    pub(super) id: Uuid,
    pub(super) provider: String,
    pub(super) remote_database_id: String,
    pub(super) failure_attempts: i32,
    pub(super) lease_token: Uuid,
    pub(super) operation_generation: i64,
}

fn provider(value: &str) -> AppResult<MemoryProviderKind> {
    match value {
        "hydradb" => Ok(MemoryProviderKind::Hydradb),
        _ => Err(AppError::Internal(
            "Stored memory provider is not supported.".into(),
        )),
    }
}

fn readiness(value: &str) -> AppResult<MemoryConnectionReadiness> {
    match value {
        "pending" => Ok(MemoryConnectionReadiness::Pending),
        "provisioning" => Ok(MemoryConnectionReadiness::Provisioning),
        "ready" => Ok(MemoryConnectionReadiness::Ready),
        "failed" => Ok(MemoryConnectionReadiness::Failed),
        _ => Err(AppError::Internal(
            "Stored memory readiness is not supported.".into(),
        )),
    }
}

fn provisioning_phase(value: &str) -> AppResult<MemoryProvisioningPhase> {
    match value {
        "create_pending" => Ok(MemoryProvisioningPhase::CreatePending),
        "waiting_ready" => Ok(MemoryProvisioningPhase::WaitingReady),
        "ready" => Ok(MemoryProvisioningPhase::Ready),
        "failed" => Ok(MemoryProvisioningPhase::Failed),
        _ => Err(AppError::Internal(
            "Stored memory provisioning phase is not supported.".into(),
        )),
    }
}

impl TryFrom<MemoryConnectionDb> for MemoryConnection {
    type Error = AppError;

    fn try_from(db: MemoryConnectionDb) -> Result<Self, Self::Error> {
        Ok(Self {
            company_id: db.company_id,
            provider: provider(&db.provider)?,
            remote_database_id: db.remote_database_id,
            readiness: readiness(&db.readiness)?,
            last_error: db.last_error,
            provisioning_phase: db
                .provisioning_phase
                .as_deref()
                .map(provisioning_phase)
                .transpose()?,
            failure_attempts: db.failure_attempts,
            readiness_deadline: db.readiness_deadline,
        })
    }
}

impl ActiveMemoryBindingDb {
    pub(super) fn into_binding(self) -> ActiveMemoryBinding {
        let Some(selected_provider) = self.selected_provider.as_deref() else {
            return ActiveMemoryBinding::Disabled;
        };
        let (Some(stored_provider), Some(remote_database_id), Some(stored_readiness)) = (
            self.provider.as_deref(),
            self.remote_database_id,
            self.readiness.as_deref(),
        ) else {
            return ActiveMemoryBinding::Misconfigured;
        };
        let (Ok(selected_provider), Ok(stored_provider), Ok(readiness)) = (
            provider(selected_provider),
            provider(stored_provider),
            readiness(stored_readiness),
        ) else {
            return ActiveMemoryBinding::Misconfigured;
        };
        if selected_provider != stored_provider {
            return ActiveMemoryBinding::Misconfigured;
        }
        let provisioning_phase = match self
            .provisioning_phase
            .as_deref()
            .map(provisioning_phase)
            .transpose()
        {
            Ok(phase) => phase,
            Err(_) => return ActiveMemoryBinding::Misconfigured,
        };
        let connection = MemoryConnection {
            company_id: self.company_id,
            provider: stored_provider,
            remote_database_id,
            readiness,
            last_error: self.last_error,
            provisioning_phase,
            failure_attempts: self.failure_attempts.unwrap_or_default(),
            readiness_deadline: self.readiness_deadline,
        };
        if readiness == MemoryConnectionReadiness::Ready {
            ActiveMemoryBinding::Ready(connection)
        } else {
            ActiveMemoryBinding::NotReady(connection)
        }
    }
}

pub(super) fn leased_provisioning_job(db: ProvisioningJobDb) -> AppResult<LeasedProvisioningJob> {
    Ok(LeasedProvisioningJob {
        id: db.id,
        company_id: db.company_id,
        provider: provider(&db.provider)?,
        remote_database_id: db.remote_database_id,
        phase: provisioning_phase(&db.phase)?,
        failure_attempts: db.failure_attempts,
        readiness_deadline: db.readiness_deadline,
        lease_token: db.lease_token,
        operation_generation: db.operation_generation,
    })
}

pub(super) fn leased_cleanup_job(db: CleanupJobDb) -> AppResult<LeasedCleanupJob> {
    Ok(LeasedCleanupJob {
        id: db.id,
        provider: provider(&db.provider)?,
        remote_database_id: db.remote_database_id,
        failure_attempts: db.failure_attempts,
        lease_token: db.lease_token,
        operation_generation: db.operation_generation,
    })
}

pub(super) const CONNECTION_COLUMNS: &str = "connection.company_id, connection.provider, connection.remote_database_id, \
     connection.readiness, connection.last_error, job.phase AS provisioning_phase, \
     COALESCE(job.failure_attempts, 0) AS failure_attempts, job.readiness_deadline";
