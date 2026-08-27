use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::memory::{
        LeasedMemoryJob, MemoryConnection, MemoryConnectionReadiness, MemoryJobKind,
        MemoryProviderKind,
    },
};

#[derive(sqlx::FromRow)]
pub(super) struct MemoryConnectionDb {
    company_id: Uuid,
    provider: String,
    remote_database_id: String,
    readiness: String,
    last_error: Option<String>,
}

#[derive(sqlx::FromRow)]
pub(super) struct MemoryJobDb {
    pub(super) id: Uuid,
    pub(super) company_id: Option<Uuid>,
    pub(super) provider: String,
    pub(super) remote_database_id: String,
    pub(super) attempts: i32,
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

impl TryFrom<MemoryConnectionDb> for MemoryConnection {
    type Error = AppError;

    fn try_from(db: MemoryConnectionDb) -> Result<Self, Self::Error> {
        Ok(Self {
            company_id: db.company_id,
            provider: provider(&db.provider)?,
            remote_database_id: db.remote_database_id,
            readiness: readiness(&db.readiness)?,
            last_error: db.last_error,
        })
    }
}

pub(super) fn leased_job(db: MemoryJobDb, kind: MemoryJobKind) -> AppResult<LeasedMemoryJob> {
    Ok(LeasedMemoryJob {
        id: db.id,
        kind,
        company_id: db.company_id,
        provider: provider(&db.provider)?,
        remote_database_id: db.remote_database_id,
        attempts: db.attempts,
        lease_token: db.lease_token,
        operation_generation: db.operation_generation,
    })
}

pub(super) const CONNECTION_COLUMNS: &str =
    "company_id, provider, remote_database_id, readiness, last_error";
