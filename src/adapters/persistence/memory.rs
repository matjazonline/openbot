use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::memory::{
        LeasedMemoryJob, MemoryConnection, MemoryConnectionReadiness, MemoryJobKind,
        MemoryProviderKind, remote_memory_database_id,
    },
    use_cases::memory::MemoryConnectionPersistence,
};

#[derive(sqlx::FromRow)]
struct MemoryConnectionDb {
    company_id: Uuid,
    provider: String,
    remote_database_id: String,
    readiness: String,
    last_error: Option<String>,
}

#[derive(sqlx::FromRow)]
struct MemoryJobDb {
    id: Uuid,
    company_id: Option<Uuid>,
    provider: String,
    remote_database_id: String,
    attempts: i32,
    lease_token: Uuid,
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

fn leased_job(db: MemoryJobDb, kind: MemoryJobKind) -> AppResult<LeasedMemoryJob> {
    Ok(LeasedMemoryJob {
        id: db.id,
        kind,
        company_id: db.company_id,
        provider: provider(&db.provider)?,
        remote_database_id: db.remote_database_id,
        attempts: db.attempts,
        lease_token: db.lease_token,
    })
}

const CONNECTION_COLUMNS: &str = "company_id, provider, remote_database_id, readiness, last_error";

#[async_trait]
impl MemoryConnectionPersistence for PostgresPersistence {
    async fn connection(&self, company_id: Uuid) -> AppResult<Option<MemoryConnection>> {
        let query = format!(
            "SELECT {CONNECTION_COLUMNS} FROM memory_provider_connections \
             WHERE company_id = $1 AND provider = 'hydradb'"
        );
        sqlx::query_as::<_, MemoryConnectionDb>(&query)
            .bind(company_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?
            .map(TryInto::try_into)
            .transpose()
    }

    async fn select_provider(
        &self,
        company_id: Uuid,
        selected_provider: MemoryProviderKind,
    ) -> AppResult<MemoryConnection> {
        let remote_database_id = remote_memory_database_id(company_id);
        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;
        let updated = sqlx::query("UPDATE companies SET memory_provider = $2 WHERE id = $1")
            .bind(company_id)
            .bind(selected_provider.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(AppError::from)?;
        if updated.rows_affected() != 1 {
            return Err(AppError::NotFound("Company not found.".into()));
        }

        sqlx::query(
            r#"INSERT INTO memory_provider_connections
                   (company_id, provider, remote_database_id, readiness)
               VALUES ($1, $2, $3, 'pending')
               ON CONFLICT (company_id, provider) DO NOTHING"#,
        )
        .bind(company_id)
        .bind(selected_provider.as_str())
        .bind(&remote_database_id)
        .execute(&mut *transaction)
        .await
        .map_err(AppError::from)?;

        let current_readiness: String = sqlx::query_scalar(
            r#"SELECT readiness FROM memory_provider_connections
               WHERE company_id = $1 AND provider = $2"#,
        )
        .bind(company_id)
        .bind(selected_provider.as_str())
        .fetch_one(&mut *transaction)
        .await
        .map_err(AppError::from)?;

        if current_readiness != "ready" {
            sqlx::query(
                r#"INSERT INTO memory_provisioning_jobs
                       (id, company_id, provider, remote_database_id)
                   VALUES ($1, $2, $3, $4)
                   ON CONFLICT (company_id, provider) DO UPDATE
                   SET status = 'pending', attempts = 0,
                       available_at = CURRENT_TIMESTAMP, lease_token = NULL,
                       lease_expires_at = NULL, last_error = NULL,
                       updated_at = CURRENT_TIMESTAMP
                   WHERE memory_provisioning_jobs.status <> 'leased'"#,
            )
            .bind(Uuid::new_v4())
            .bind(company_id)
            .bind(selected_provider.as_str())
            .bind(&remote_database_id)
            .execute(&mut *transaction)
            .await
            .map_err(AppError::from)?;
        }
        transaction.commit().await.map_err(AppError::from)?;
        self.connection(company_id)
            .await?
            .ok_or_else(|| AppError::Internal("Memory connection was not created.".into()))
    }

    async fn disable_provider(&self, company_id: Uuid) -> AppResult<()> {
        let updated = sqlx::query("UPDATE companies SET memory_provider = NULL WHERE id = $1")
            .bind(company_id)
            .execute(&self.pool)
            .await
            .map_err(AppError::from)?;
        if updated.rows_affected() == 1 {
            Ok(())
        } else {
            Err(AppError::NotFound("Company not found.".into()))
        }
    }

    async fn retry_provisioning(&self, company_id: Uuid) -> AppResult<()> {
        let connection = self
            .connection(company_id)
            .await?
            .ok_or_else(|| AppError::BadRequest("HydraDB has not been selected.".into()))?;
        sqlx::query(
            r#"INSERT INTO memory_provisioning_jobs
                   (id, company_id, provider, remote_database_id)
               VALUES ($1, $2, $3, $4)
               ON CONFLICT (company_id, provider) DO UPDATE
               SET status = 'pending', attempts = 0, available_at = CURRENT_TIMESTAMP,
                   lease_token = NULL, lease_expires_at = NULL, last_error = NULL,
                   updated_at = CURRENT_TIMESTAMP
               WHERE memory_provisioning_jobs.status <> 'leased'"#,
        )
        .bind(Uuid::new_v4())
        .bind(company_id)
        .bind(connection.provider.as_str())
        .bind(connection.remote_database_id)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(())
    }

    async fn claim_provisioning_job(
        &self,
        lease_token: Uuid,
        lease_expires_at: DateTime<Utc>,
    ) -> AppResult<Option<LeasedMemoryJob>> {
        let row = sqlx::query_as::<_, MemoryJobDb>(
            r#"WITH candidate AS (
                   SELECT id FROM memory_provisioning_jobs
                   WHERE (status = 'pending' AND available_at <= CURRENT_TIMESTAMP)
                      OR (status = 'leased' AND
                          (lease_expires_at IS NULL OR lease_expires_at <= CURRENT_TIMESTAMP))
                   ORDER BY available_at, created_at, id
                   FOR UPDATE SKIP LOCKED
                   LIMIT 1
               )
               UPDATE memory_provisioning_jobs AS job
               SET status = 'leased', attempts = job.attempts + 1,
                   lease_token = $1, lease_expires_at = $2, updated_at = CURRENT_TIMESTAMP
               FROM candidate
               WHERE job.id = candidate.id
               RETURNING job.id, job.company_id, job.provider, job.remote_database_id,
                         job.attempts, job.lease_token"#,
        )
        .bind(lease_token)
        .bind(lease_expires_at)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;
        row.map(|row| leased_job(row, MemoryJobKind::Provision))
            .transpose()
    }

    async fn mark_provisioning(&self, job_id: Uuid, lease_token: Uuid) -> AppResult<bool> {
        let result = sqlx::query(
            r#"UPDATE memory_provider_connections AS connection
               SET readiness = 'provisioning', last_error = NULL,
                   updated_at = CURRENT_TIMESTAMP
               WHERE EXISTS (
                   SELECT 1 FROM memory_provisioning_jobs AS job
                   WHERE job.id = $1 AND job.company_id = connection.company_id
                     AND job.provider = connection.provider AND job.status = 'leased'
                     AND job.lease_token = $2 AND job.lease_expires_at > CURRENT_TIMESTAMP
               )"#,
        )
        .bind(job_id)
        .bind(lease_token)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(result.rows_affected() == 1)
    }

    async fn renew_provisioning_job(
        &self,
        job_id: Uuid,
        lease_token: Uuid,
        lease_expires_at: DateTime<Utc>,
    ) -> AppResult<bool> {
        renew_job_lease(
            self,
            "memory_provisioning_jobs",
            job_id,
            lease_token,
            lease_expires_at,
        )
        .await
    }

    async fn complete_provisioning(&self, job_id: Uuid, lease_token: Uuid) -> AppResult<bool> {
        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;
        let company_id = sqlx::query_scalar::<_, Uuid>(
            r#"UPDATE memory_provisioning_jobs
               SET status = 'completed', lease_token = NULL, lease_expires_at = NULL,
                   last_error = NULL, updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status = 'leased' AND lease_token = $2
                 AND lease_expires_at > CURRENT_TIMESTAMP
               RETURNING company_id"#,
        )
        .bind(job_id)
        .bind(lease_token)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(AppError::from)?;
        let Some(company_id) = company_id else {
            return Ok(false);
        };
        sqlx::query(
            r#"UPDATE memory_provider_connections
               SET readiness = 'ready', last_error = NULL, updated_at = CURRENT_TIMESTAMP
               WHERE company_id = $1 AND provider = 'hydradb'"#,
        )
        .bind(company_id)
        .execute(&mut *transaction)
        .await
        .map_err(AppError::from)?;
        transaction.commit().await.map_err(AppError::from)?;
        Ok(true)
    }

    async fn retry_provisioning_job(
        &self,
        job_id: Uuid,
        lease_token: Uuid,
        available_at: DateTime<Utc>,
        safe_error: &str,
        terminal: bool,
    ) -> AppResult<bool> {
        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;
        let status = if terminal { "failed" } else { "pending" };
        let company_id = sqlx::query_scalar::<_, Uuid>(
            r#"UPDATE memory_provisioning_jobs
               SET status = $3, available_at = $4, lease_token = NULL,
                   lease_expires_at = NULL, last_error = $5, updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status = 'leased' AND lease_token = $2
                 AND lease_expires_at > CURRENT_TIMESTAMP
               RETURNING company_id"#,
        )
        .bind(job_id)
        .bind(lease_token)
        .bind(status)
        .bind(available_at)
        .bind(safe_error)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(AppError::from)?;
        let Some(company_id) = company_id else {
            return Ok(false);
        };
        sqlx::query(
            r#"UPDATE memory_provider_connections
               SET readiness = $2, last_error = $3, updated_at = CURRENT_TIMESTAMP
               WHERE company_id = $1 AND provider = 'hydradb'"#,
        )
        .bind(company_id)
        .bind(if terminal { "failed" } else { "provisioning" })
        .bind(safe_error)
        .execute(&mut *transaction)
        .await
        .map_err(AppError::from)?;
        transaction.commit().await.map_err(AppError::from)?;
        Ok(true)
    }

    async fn claim_cleanup_job(
        &self,
        lease_token: Uuid,
        lease_expires_at: DateTime<Utc>,
    ) -> AppResult<Option<LeasedMemoryJob>> {
        let row = sqlx::query_as::<_, MemoryJobDb>(
            r#"WITH candidate AS (
                   SELECT id FROM memory_cleanup_jobs
                   WHERE (status = 'pending' AND available_at <= CURRENT_TIMESTAMP)
                      OR (status = 'leased' AND
                          (lease_expires_at IS NULL OR lease_expires_at <= CURRENT_TIMESTAMP))
                   ORDER BY available_at, created_at, id
                   FOR UPDATE SKIP LOCKED
                   LIMIT 1
               )
               UPDATE memory_cleanup_jobs AS job
               SET status = 'leased', attempts = job.attempts + 1,
                   lease_token = $1, lease_expires_at = $2, updated_at = CURRENT_TIMESTAMP
               FROM candidate
               WHERE job.id = candidate.id
               RETURNING job.id, NULL::uuid AS company_id, job.provider,
                         job.remote_database_id, job.attempts, job.lease_token"#,
        )
        .bind(lease_token)
        .bind(lease_expires_at)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;
        row.map(|row| leased_job(row, MemoryJobKind::Cleanup))
            .transpose()
    }

    async fn complete_cleanup(&self, job_id: Uuid, lease_token: Uuid) -> AppResult<bool> {
        let result = sqlx::query(
            r#"UPDATE memory_cleanup_jobs
               SET status = 'completed', lease_token = NULL, lease_expires_at = NULL,
                   last_error = NULL, updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status = 'leased' AND lease_token = $2
                 AND lease_expires_at > CURRENT_TIMESTAMP"#,
        )
        .bind(job_id)
        .bind(lease_token)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(result.rows_affected() == 1)
    }

    async fn renew_cleanup_job(
        &self,
        job_id: Uuid,
        lease_token: Uuid,
        lease_expires_at: DateTime<Utc>,
    ) -> AppResult<bool> {
        renew_job_lease(
            self,
            "memory_cleanup_jobs",
            job_id,
            lease_token,
            lease_expires_at,
        )
        .await
    }

    async fn retry_cleanup_job(
        &self,
        job_id: Uuid,
        lease_token: Uuid,
        available_at: DateTime<Utc>,
        safe_error: &str,
        terminal: bool,
    ) -> AppResult<bool> {
        let result = sqlx::query(
            r#"UPDATE memory_cleanup_jobs
               SET status = $3, available_at = $4, lease_token = NULL,
                   lease_expires_at = NULL, last_error = $5, updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status = 'leased' AND lease_token = $2
                 AND lease_expires_at > CURRENT_TIMESTAMP"#,
        )
        .bind(job_id)
        .bind(lease_token)
        .bind(if terminal { "failed" } else { "pending" })
        .bind(available_at)
        .bind(safe_error)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(result.rows_affected() == 1)
    }
}

async fn renew_job_lease(
    persistence: &PostgresPersistence,
    table: &'static str,
    job_id: Uuid,
    lease_token: Uuid,
    lease_expires_at: DateTime<Utc>,
) -> AppResult<bool> {
    debug_assert!(matches!(
        table,
        "memory_provisioning_jobs" | "memory_cleanup_jobs"
    ));
    let query = format!(
        "UPDATE {table} SET lease_expires_at = $3, updated_at = CURRENT_TIMESTAMP \
         WHERE id = $1 AND status = 'leased' AND lease_token = $2 \
         AND lease_expires_at > CURRENT_TIMESTAMP"
    );
    let result = sqlx::query(&query)
        .bind(job_id)
        .bind(lease_token)
        .bind(lease_expires_at)
        .execute(&persistence.pool)
        .await
        .map_err(AppError::from)?;
    Ok(result.rows_affected() == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        adapters::persistence::test_support::test_pool,
        use_cases::{
            company::{CompanyPersistence, CompanyWrite},
            user::UserPersistence,
        },
    };

    async fn memory_company(persistence: &PostgresPersistence) -> Uuid {
        let suffix = Uuid::new_v4().simple().to_string();
        let user = persistence
            .create_user(
                &format!("memory-{suffix}"),
                &format!("memory-{suffix}@example.com"),
                "hash",
            )
            .await
            .expect("memory test user");
        persistence
            .create(
                user.id,
                CompanyWrite {
                    name: "Memory test".into(),
                    slug: format!("memory-{suffix}"),
                    ..CompanyWrite::default()
                },
            )
            .await
            .expect("memory test company")
            .id
    }

    #[tokio::test]
    async fn provider_selection_is_idempotent_and_competing_claimants_claim_once() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());
        let company_id = memory_company(&persistence).await;

        let first = persistence
            .select_provider(company_id, MemoryProviderKind::Hydradb)
            .await
            .expect("first selection");
        let second = persistence
            .select_provider(company_id, MemoryProviderKind::Hydradb)
            .await
            .expect("repeat selection");
        assert_eq!(first.remote_database_id, second.remote_database_id);
        let jobs: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM memory_provisioning_jobs WHERE company_id = $1",
        )
        .bind(company_id)
        .fetch_one(&pool)
        .await
        .expect("job count");
        assert_eq!(jobs, 1);

        let expires = Utc::now() + chrono::Duration::minutes(2);
        let (left, right) = tokio::join!(
            persistence.claim_provisioning_job(Uuid::new_v4(), expires),
            persistence.claim_provisioning_job(Uuid::new_v4(), expires),
        );
        let claimed = [left.expect("left claimant"), right.expect("right claimant")]
            .into_iter()
            .flatten()
            .filter(|job| job.company_id == Some(company_id))
            .count();
        assert_eq!(claimed, 1, "a provisioning job is leased to one worker");
    }

    #[tokio::test]
    async fn disabling_preserves_the_connection_and_deletion_preserves_cleanup_work() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());
        let company_id = memory_company(&persistence).await;
        let connection = persistence
            .select_provider(company_id, MemoryProviderKind::Hydradb)
            .await
            .expect("selection");

        persistence
            .disable_provider(company_id)
            .await
            .expect("disable");
        assert!(persistence.connection(company_id).await.unwrap().is_some());

        persistence
            .delete(company_id)
            .await
            .expect("company deletion");
        let cleanup: Option<String> = sqlx::query_scalar(
            "SELECT status FROM memory_cleanup_jobs WHERE provider = 'hydradb' AND remote_database_id = $1",
        )
        .bind(connection.remote_database_id)
        .fetch_optional(&pool)
        .await
        .expect("cleanup lookup");
        assert_eq!(cleanup.as_deref(), Some("pending"));
    }
}
