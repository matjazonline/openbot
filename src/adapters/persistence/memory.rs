use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::memory::{
        LeasedCleanupJob, LeasedProvisioningJob, MEMORY_READINESS_WINDOW_SECONDS, MemoryConnection,
        MemoryProviderKind, remote_memory_database_id,
    },
    use_cases::memory::{
        ActiveMemoryBinding, MemoryBindingPersistence, MemoryConnectionPersistence,
    },
};

#[path = "memory_rows.rs"]
mod rows;

use rows::{
    ActiveMemoryBindingDb, CONNECTION_COLUMNS, CleanupJobDb, MemoryConnectionDb, ProvisioningJobDb,
    leased_cleanup_job, leased_provisioning_job,
};

#[async_trait]
impl MemoryBindingPersistence for PostgresPersistence {
    async fn active_binding(&self, company_id: Uuid) -> AppResult<ActiveMemoryBinding> {
        let row = sqlx::query_as::<_, ActiveMemoryBindingDb>(
            r#"SELECT company.memory_provider AS selected_provider,
                      company.id AS company_id,
                      connection.provider,
                      connection.remote_database_id,
                      connection.readiness,
                      connection.last_error,
                      job.phase AS provisioning_phase,
                      COALESCE(job.failure_attempts, 0) AS failure_attempts,
                      job.readiness_deadline
               FROM companies AS company
               LEFT JOIN memory_provider_connections AS connection
                 ON connection.company_id = company.id
                AND connection.provider = company.memory_provider
               LEFT JOIN memory_provisioning_jobs AS job
                 ON job.company_id = connection.company_id
                AND job.provider = connection.provider
               WHERE company.id = $1"#,
        )
        .bind(company_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::NotFound("Company not found.".into()))?;
        Ok(row.into_binding())
    }
}

impl PostgresPersistence {
    async fn memory_connection(&self, company_id: Uuid) -> AppResult<Option<MemoryConnection>> {
        let query = format!(
            "SELECT {CONNECTION_COLUMNS} FROM memory_provider_connections AS connection \
             LEFT JOIN memory_provisioning_jobs AS job \
               ON job.company_id = connection.company_id AND job.provider = connection.provider \
             WHERE connection.company_id = $1 AND connection.provider = 'hydradb'"
        );
        sqlx::query_as::<_, MemoryConnectionDb>(&query)
            .bind(company_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?
            .map(TryInto::try_into)
            .transpose()
    }
}

#[async_trait]
impl MemoryConnectionPersistence for PostgresPersistence {
    async fn select_provider(
        &self,
        company_id: Uuid,
        selected_provider: MemoryProviderKind,
    ) -> AppResult<MemoryConnection> {
        let remote_database_id = remote_memory_database_id(company_id);
        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;
        let previous_provider = sqlx::query_scalar::<_, Option<String>>(
            "SELECT memory_provider FROM companies WHERE id = $1 FOR UPDATE",
        )
        .bind(company_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::NotFound("Company not found.".into()))?;
        sqlx::query("UPDATE companies SET memory_provider = $2 WHERE id = $1")
            .bind(company_id)
            .bind(selected_provider.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(AppError::from)?;

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

        sqlx::query(
            r#"INSERT INTO memory_remote_resource_lifecycles
                   (provider, remote_database_id, company_id, desired_state)
               VALUES ($1, $2, $3, 'present')
               ON CONFLICT (provider, remote_database_id) DO UPDATE
               SET company_id = EXCLUDED.company_id, desired_state = 'present',
                   quiesce_until = CURRENT_TIMESTAMP, last_error = NULL,
                   updated_at = CURRENT_TIMESTAMP"#,
        )
        .bind(selected_provider.as_str())
        .bind(&remote_database_id)
        .bind(company_id)
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

        let reenable_ready_connection = current_readiness == "ready"
            && previous_provider.as_deref() != Some(selected_provider.as_str());
        if reenable_ready_connection {
            let readiness_deadline =
                Utc::now() + chrono::Duration::seconds(MEMORY_READINESS_WINDOW_SECONDS);
            let queued = sqlx::query(
                r#"INSERT INTO memory_provisioning_jobs
                       (id, company_id, provider, remote_database_id, phase,
                        readiness_deadline, next_poll_at)
                   VALUES ($1, $2, $3, $4, 'waiting_ready', $5, CURRENT_TIMESTAMP)
                   ON CONFLICT (company_id, provider) DO UPDATE
                   SET status = 'pending', phase = 'waiting_ready', failure_attempts = 0,
                       available_at = CURRENT_TIMESTAMP, lease_token = NULL,
                       lease_expires_at = NULL, operation_generation = NULL,
                       readiness_deadline = EXCLUDED.readiness_deadline,
                       next_poll_at = CURRENT_TIMESTAMP, last_error = NULL,
                       updated_at = CURRENT_TIMESTAMP
                   WHERE memory_provisioning_jobs.status <> 'leased'"#,
            )
            .bind(Uuid::new_v4())
            .bind(company_id)
            .bind(selected_provider.as_str())
            .bind(&remote_database_id)
            .bind(readiness_deadline)
            .execute(&mut *transaction)
            .await
            .map_err(AppError::from)?;
            if queued.rows_affected() == 1 {
                sqlx::query(
                    r#"UPDATE memory_provider_connections
                       SET readiness = 'pending', last_error = NULL,
                           updated_at = CURRENT_TIMESTAMP
                       WHERE company_id = $1 AND provider = $2"#,
                )
                .bind(company_id)
                .bind(selected_provider.as_str())
                .execute(&mut *transaction)
                .await
                .map_err(AppError::from)?;
            }
        } else if current_readiness != "ready"
            && previous_provider.as_deref() != Some(selected_provider.as_str())
        {
            sqlx::query(
                r#"INSERT INTO memory_provisioning_jobs
                       (id, company_id, provider, remote_database_id)
                   VALUES ($1, $2, $3, $4)
                   ON CONFLICT (company_id, provider) DO UPDATE
                   SET status = 'pending', phase = 'create_pending', failure_attempts = 0,
                       available_at = CURRENT_TIMESTAMP, lease_token = NULL,
                       lease_expires_at = NULL, operation_generation = NULL,
                       readiness_deadline = NULL, next_poll_at = NULL, last_error = NULL,
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
        self.memory_connection(company_id)
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
            .memory_connection(company_id)
            .await?
            .ok_or_else(|| AppError::BadRequest("HydraDB has not been selected.".into()))?;
        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;
        let desired = sqlx::query(
            r#"UPDATE memory_remote_resource_lifecycles AS lifecycle
               SET desired_state = 'present', company_id = $1, quiesce_until = CURRENT_TIMESTAMP,
                   last_error = NULL, updated_at = CURRENT_TIMESTAMP
               WHERE lifecycle.provider = $2 AND lifecycle.remote_database_id = $3
                 AND EXISTS (
                     SELECT 1 FROM companies
                     WHERE id = $1 AND memory_provider = lifecycle.provider
                 )"#,
        )
        .bind(company_id)
        .bind(connection.provider.as_str())
        .bind(&connection.remote_database_id)
        .execute(&mut *transaction)
        .await
        .map_err(AppError::from)?;
        if desired.rows_affected() != 1 {
            return Err(AppError::BadRequest(
                "HydraDB is no longer selected for this company.".into(),
            ));
        }
        sqlx::query(
            r#"INSERT INTO memory_provisioning_jobs
                   (id, company_id, provider, remote_database_id)
               VALUES ($1, $2, $3, $4)
               ON CONFLICT (company_id, provider) DO UPDATE
               SET status = 'pending', phase = 'create_pending', failure_attempts = 0,
                   available_at = CURRENT_TIMESTAMP, lease_token = NULL,
                   lease_expires_at = NULL, operation_generation = NULL,
                   readiness_deadline = NULL, next_poll_at = NULL, last_error = NULL,
                   updated_at = CURRENT_TIMESTAMP
               WHERE memory_provisioning_jobs.status <> 'leased'"#,
        )
        .bind(Uuid::new_v4())
        .bind(company_id)
        .bind(connection.provider.as_str())
        .bind(connection.remote_database_id)
        .execute(&mut *transaction)
        .await
        .map_err(AppError::from)?;
        sqlx::query(
            r#"UPDATE memory_provider_connections
               SET readiness = 'pending', last_error = NULL, updated_at = CURRENT_TIMESTAMP
               WHERE company_id = $1 AND provider = $2"#,
        )
        .bind(company_id)
        .bind(connection.provider.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(AppError::from)?;
        transaction.commit().await.map_err(AppError::from)?;
        Ok(())
    }

    async fn claim_provisioning_job(
        &self,
        lease_token: Uuid,
        lease_expires_at: DateTime<Utc>,
    ) -> AppResult<Option<LeasedProvisioningJob>> {
        let row = sqlx::query_as::<_, ProvisioningJobDb>(
            r#"WITH candidate AS (
                   SELECT job.id, lifecycle.provider, lifecycle.remote_database_id
                   FROM memory_provisioning_jobs AS job
                   JOIN memory_remote_resource_lifecycles AS lifecycle
                     ON lifecycle.provider = job.provider
                    AND lifecycle.remote_database_id = job.remote_database_id
                   WHERE (
                           (job.status = 'pending' AND
                               CASE job.phase
                                   WHEN 'waiting_ready' THEN job.next_poll_at
                                   ELSE job.available_at
                               END <= CURRENT_TIMESTAMP)
                           OR
                           (job.status = 'leased' AND job.lease_expires_at <= CURRENT_TIMESTAMP)
                         )
                     AND lifecycle.desired_state = 'present'
                     AND lifecycle.company_id = job.company_id
                     AND (
                           lifecycle.operation_lease_expires_at IS NULL
                           OR lifecycle.operation_lease_expires_at <= CURRENT_TIMESTAMP
                         )
                   ORDER BY CASE job.phase
                                WHEN 'waiting_ready' THEN job.next_poll_at
                                ELSE job.available_at
                            END,
                            job.created_at, job.id
                   FOR UPDATE OF job, lifecycle SKIP LOCKED
                   LIMIT 1
               ), claimed_lifecycle AS (
                   UPDATE memory_remote_resource_lifecycles AS lifecycle
                   SET operation_generation = lifecycle.operation_generation + 1,
                       operation_lease_token = $1, operation_lease_expires_at = $2,
                       last_error = NULL, updated_at = CURRENT_TIMESTAMP
                   FROM candidate
                   WHERE lifecycle.provider = candidate.provider
                     AND lifecycle.remote_database_id = candidate.remote_database_id
                   RETURNING lifecycle.provider, lifecycle.remote_database_id,
                             lifecycle.operation_generation
               )
               UPDATE memory_provisioning_jobs AS job
               SET status = 'leased',
                   failure_attempts = job.failure_attempts
                       + CASE WHEN job.status = 'leased' THEN 1 ELSE 0 END,
                   lease_token = $1, lease_expires_at = $2,
                   operation_generation = claimed_lifecycle.operation_generation,
                   updated_at = CURRENT_TIMESTAMP
               FROM candidate
               JOIN claimed_lifecycle
                 ON claimed_lifecycle.provider = candidate.provider
                AND claimed_lifecycle.remote_database_id = candidate.remote_database_id
               WHERE job.id = candidate.id
               RETURNING job.id, job.company_id, job.provider, job.remote_database_id,
                         job.phase, job.failure_attempts, job.readiness_deadline,
                         job.lease_token, job.operation_generation"#,
        )
        .bind(lease_token)
        .bind(lease_expires_at)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;
        row.map(leased_provisioning_job).transpose()
    }

    async fn mark_provisioning(&self, job_id: Uuid, lease_token: Uuid) -> AppResult<bool> {
        let result = sqlx::query(
            r#"UPDATE memory_provider_connections AS connection
               SET readiness = 'provisioning', last_error = NULL,
                   updated_at = CURRENT_TIMESTAMP
               WHERE EXISTS (
                   SELECT 1 FROM memory_provisioning_jobs AS job
                   JOIN memory_remote_resource_lifecycles AS lifecycle
                     ON lifecycle.provider = job.provider
                    AND lifecycle.remote_database_id = job.remote_database_id
                   WHERE job.id = $1 AND job.company_id = connection.company_id
                     AND job.provider = connection.provider AND job.status = 'leased'
                     AND job.lease_token = $2 AND job.lease_expires_at > CURRENT_TIMESTAMP
                     AND lifecycle.desired_state = 'present'
                     AND lifecycle.company_id = job.company_id
                     AND lifecycle.operation_generation = job.operation_generation
                     AND lifecycle.operation_lease_token = job.lease_token
                     AND lifecycle.operation_lease_expires_at > CURRENT_TIMESTAMP
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
            "present",
            job_id,
            lease_token,
            lease_expires_at,
        )
        .await
    }

    async fn begin_readiness_polling(
        &self,
        job_id: Uuid,
        lease_token: Uuid,
        readiness_deadline: DateTime<Utc>,
        next_poll_at: DateTime<Utc>,
    ) -> AppResult<bool> {
        transition_provisioning_job(
            self,
            job_id,
            lease_token,
            ProvisioningTransition::Waiting {
                readiness_deadline: Some(readiness_deadline),
                next_poll_at,
            },
        )
        .await
    }

    async fn schedule_readiness_poll(
        &self,
        job_id: Uuid,
        lease_token: Uuid,
        next_poll_at: DateTime<Utc>,
    ) -> AppResult<bool> {
        transition_provisioning_job(
            self,
            job_id,
            lease_token,
            ProvisioningTransition::Waiting {
                readiness_deadline: None,
                next_poll_at,
            },
        )
        .await
    }

    async fn complete_provisioning(&self, job_id: Uuid, lease_token: Uuid) -> AppResult<bool> {
        let company_id = sqlx::query_scalar::<_, Uuid>(
            r#"WITH current_job AS (
                   SELECT job.id, job.company_id, job.provider, job.remote_database_id,
                          job.operation_generation
                   FROM memory_provisioning_jobs AS job
                   WHERE job.id = $1 AND job.status = 'leased' AND job.lease_token = $2
                     AND job.phase = 'waiting_ready'
                     AND job.lease_expires_at > CURRENT_TIMESTAMP
                     AND EXISTS (
                         SELECT 1 FROM memory_remote_resource_lifecycles AS lifecycle
                         WHERE lifecycle.provider = job.provider
                           AND lifecycle.remote_database_id = job.remote_database_id
                           AND lifecycle.company_id = job.company_id
                           AND lifecycle.desired_state = 'present'
                           AND lifecycle.operation_generation = job.operation_generation
                           AND lifecycle.operation_lease_token = job.lease_token
                           AND lifecycle.operation_lease_expires_at > CURRENT_TIMESTAMP
                     )
                   FOR UPDATE
               ), completed_job AS (
                   UPDATE memory_provisioning_jobs AS job
                   SET status = 'completed', phase = 'ready',
                       lease_token = NULL, lease_expires_at = NULL,
                       operation_generation = NULL, last_error = NULL,
                       updated_at = CURRENT_TIMESTAMP
                   FROM current_job
                   WHERE job.id = current_job.id
                   RETURNING current_job.company_id, current_job.provider,
                             current_job.remote_database_id,
                             current_job.operation_generation
               ), released_lifecycle AS (
                   UPDATE memory_remote_resource_lifecycles AS lifecycle
                   SET operation_lease_token = NULL, operation_lease_expires_at = NULL,
                       last_error = NULL, updated_at = CURRENT_TIMESTAMP
                   FROM completed_job
                   WHERE lifecycle.provider = completed_job.provider
                     AND lifecycle.remote_database_id = completed_job.remote_database_id
                     AND lifecycle.desired_state = 'present'
                     AND lifecycle.company_id = completed_job.company_id
                     AND lifecycle.operation_generation = completed_job.operation_generation
                     AND lifecycle.operation_lease_token = $2
                   RETURNING lifecycle.company_id
               )
               UPDATE memory_provider_connections AS connection
               SET readiness = 'ready', last_error = NULL, updated_at = CURRENT_TIMESTAMP
               FROM released_lifecycle
               WHERE connection.company_id = released_lifecycle.company_id
                 AND connection.provider = 'hydradb'
               RETURNING connection.company_id"#,
        )
        .bind(job_id)
        .bind(lease_token)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(company_id.is_some())
    }

    async fn retry_provisioning_job(
        &self,
        job_id: Uuid,
        lease_token: Uuid,
        available_at: DateTime<Utc>,
        safe_error: &str,
        terminal: bool,
    ) -> AppResult<bool> {
        let status = if terminal { "failed" } else { "pending" };
        let company_id = sqlx::query_scalar::<_, Uuid>(
            r#"WITH current_job AS (
                   SELECT job.id, job.company_id, job.provider, job.remote_database_id,
                          job.operation_generation
                   FROM memory_provisioning_jobs AS job
                   WHERE job.id = $1 AND job.status = 'leased' AND job.lease_token = $2
                     AND job.lease_expires_at > CURRENT_TIMESTAMP
                     AND EXISTS (
                         SELECT 1 FROM memory_remote_resource_lifecycles AS lifecycle
                         WHERE lifecycle.provider = job.provider
                           AND lifecycle.remote_database_id = job.remote_database_id
                           AND lifecycle.company_id = job.company_id
                           AND lifecycle.desired_state = 'present'
                           AND lifecycle.operation_generation = job.operation_generation
                           AND lifecycle.operation_lease_token = job.lease_token
                           AND lifecycle.operation_lease_expires_at > CURRENT_TIMESTAMP
                     )
                   FOR UPDATE
               ), retried_job AS (
                   UPDATE memory_provisioning_jobs AS job
                   SET status = $3,
                       phase = CASE WHEN $3 = 'failed' THEN 'failed' ELSE job.phase END,
                       failure_attempts = job.failure_attempts + 1,
                       available_at = CASE WHEN job.phase = 'create_pending'
                                           THEN $4 ELSE job.available_at END,
                       next_poll_at = CASE WHEN job.phase = 'waiting_ready'
                                           THEN $4 ELSE job.next_poll_at END,
                       lease_token = NULL,
                       lease_expires_at = NULL, operation_generation = NULL,
                       last_error = $5, updated_at = CURRENT_TIMESTAMP
                   FROM current_job
                   WHERE job.id = current_job.id
                   RETURNING current_job.company_id, current_job.provider,
                             current_job.remote_database_id,
                             current_job.operation_generation
               ), released_lifecycle AS (
                   UPDATE memory_remote_resource_lifecycles AS lifecycle
                   SET operation_lease_token = NULL, operation_lease_expires_at = NULL,
                       last_error = $5, updated_at = CURRENT_TIMESTAMP
                   FROM retried_job
                   WHERE lifecycle.provider = retried_job.provider
                     AND lifecycle.remote_database_id = retried_job.remote_database_id
                     AND lifecycle.desired_state = 'present'
                     AND lifecycle.company_id = retried_job.company_id
                     AND lifecycle.operation_generation = retried_job.operation_generation
                     AND lifecycle.operation_lease_token = $2
                   RETURNING lifecycle.company_id
               )
               UPDATE memory_provider_connections AS connection
               SET readiness = $6, last_error = $5, updated_at = CURRENT_TIMESTAMP
               FROM released_lifecycle
               WHERE connection.company_id = released_lifecycle.company_id
                 AND connection.provider = 'hydradb'
               RETURNING connection.company_id"#,
        )
        .bind(job_id)
        .bind(lease_token)
        .bind(status)
        .bind(available_at)
        .bind(safe_error)
        .bind(if terminal { "failed" } else { "provisioning" })
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(company_id.is_some())
    }

    async fn fail_provisioning_job(
        &self,
        job_id: Uuid,
        lease_token: Uuid,
        safe_error: &str,
    ) -> AppResult<bool> {
        transition_provisioning_job(
            self,
            job_id,
            lease_token,
            ProvisioningTransition::Failed { safe_error },
        )
        .await
    }

    async fn claim_cleanup_job(
        &self,
        lease_token: Uuid,
        lease_expires_at: DateTime<Utc>,
    ) -> AppResult<Option<LeasedCleanupJob>> {
        let row = sqlx::query_as::<_, CleanupJobDb>(
            r#"WITH candidate AS (
                   SELECT job.id, lifecycle.provider, lifecycle.remote_database_id
                   FROM memory_cleanup_jobs AS job
                   JOIN memory_remote_resource_lifecycles AS lifecycle
                     ON lifecycle.provider = job.provider
                    AND lifecycle.remote_database_id = job.remote_database_id
                   WHERE (
                           (job.status = 'pending' AND job.available_at <= CURRENT_TIMESTAMP)
                           OR
                           (job.status = 'leased' AND job.lease_expires_at <= CURRENT_TIMESTAMP)
                         )
                     AND lifecycle.desired_state = 'absent'
                     AND lifecycle.quiesce_until <= CURRENT_TIMESTAMP
                     AND (
                           lifecycle.operation_lease_expires_at IS NULL
                           OR lifecycle.operation_lease_expires_at <= CURRENT_TIMESTAMP
                         )
                   ORDER BY job.available_at, job.created_at, job.id
                   FOR UPDATE OF job, lifecycle SKIP LOCKED
                   LIMIT 1
               ), claimed_lifecycle AS (
                   UPDATE memory_remote_resource_lifecycles AS lifecycle
                   SET operation_generation = lifecycle.operation_generation + 1,
                       operation_lease_token = $1, operation_lease_expires_at = $2,
                       last_error = NULL, updated_at = CURRENT_TIMESTAMP
                   FROM candidate
                   WHERE lifecycle.provider = candidate.provider
                     AND lifecycle.remote_database_id = candidate.remote_database_id
                   RETURNING lifecycle.provider, lifecycle.remote_database_id,
                             lifecycle.operation_generation
               )
               UPDATE memory_cleanup_jobs AS job
               SET status = 'leased', attempts = job.attempts + 1,
                   lease_token = $1, lease_expires_at = $2,
                   operation_generation = claimed_lifecycle.operation_generation,
                   updated_at = CURRENT_TIMESTAMP
               FROM candidate
               JOIN claimed_lifecycle
                 ON claimed_lifecycle.provider = candidate.provider
                AND claimed_lifecycle.remote_database_id = candidate.remote_database_id
               WHERE job.id = candidate.id
               RETURNING job.id, job.provider,
                         job.remote_database_id, job.attempts AS failure_attempts, job.lease_token,
                         job.operation_generation"#,
        )
        .bind(lease_token)
        .bind(lease_expires_at)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;
        row.map(leased_cleanup_job).transpose()
    }

    async fn complete_cleanup(&self, job_id: Uuid, lease_token: Uuid) -> AppResult<bool> {
        let result = sqlx::query_scalar::<_, String>(
            r#"WITH current_job AS (
                   SELECT job.id, job.provider, job.remote_database_id,
                          job.operation_generation
                   FROM memory_cleanup_jobs AS job
                   WHERE job.id = $1 AND job.status = 'leased' AND job.lease_token = $2
                     AND job.lease_expires_at > CURRENT_TIMESTAMP
                     AND EXISTS (
                         SELECT 1 FROM memory_remote_resource_lifecycles AS lifecycle
                         WHERE lifecycle.provider = job.provider
                           AND lifecycle.remote_database_id = job.remote_database_id
                           AND lifecycle.desired_state = 'absent'
                           AND lifecycle.quiesce_until <= CURRENT_TIMESTAMP
                           AND lifecycle.operation_generation = job.operation_generation
                           AND lifecycle.operation_lease_token = job.lease_token
                           AND lifecycle.operation_lease_expires_at > CURRENT_TIMESTAMP
                     )
                   FOR UPDATE
               ), completed_job AS (
                   UPDATE memory_cleanup_jobs AS job
                   SET status = 'completed', lease_token = NULL, lease_expires_at = NULL,
                       operation_generation = NULL, last_error = NULL,
                       updated_at = CURRENT_TIMESTAMP
                   FROM current_job
                   WHERE job.id = current_job.id
                   RETURNING current_job.provider, current_job.remote_database_id,
                             current_job.operation_generation
               )
               UPDATE memory_remote_resource_lifecycles AS lifecycle
               SET operation_lease_token = NULL, operation_lease_expires_at = NULL,
                   last_error = NULL, updated_at = CURRENT_TIMESTAMP
               FROM completed_job
               WHERE lifecycle.provider = completed_job.provider
                 AND lifecycle.remote_database_id = completed_job.remote_database_id
                 AND lifecycle.desired_state = 'absent'
                 AND lifecycle.operation_generation = completed_job.operation_generation
                 AND lifecycle.operation_lease_token = $2
               RETURNING lifecycle.remote_database_id"#,
        )
        .bind(job_id)
        .bind(lease_token)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(result.is_some())
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
            "absent",
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
        let result = sqlx::query_scalar::<_, String>(
            r#"WITH current_job AS (
                   SELECT job.id, job.provider, job.remote_database_id,
                          job.operation_generation
                   FROM memory_cleanup_jobs AS job
                   WHERE job.id = $1 AND job.status = 'leased' AND job.lease_token = $2
                     AND job.lease_expires_at > CURRENT_TIMESTAMP
                     AND EXISTS (
                         SELECT 1 FROM memory_remote_resource_lifecycles AS lifecycle
                         WHERE lifecycle.provider = job.provider
                           AND lifecycle.remote_database_id = job.remote_database_id
                           AND lifecycle.desired_state = 'absent'
                           AND lifecycle.operation_generation = job.operation_generation
                           AND lifecycle.operation_lease_token = job.lease_token
                           AND lifecycle.operation_lease_expires_at > CURRENT_TIMESTAMP
                     )
                   FOR UPDATE
               ), retried_job AS (
                   UPDATE memory_cleanup_jobs AS job
                   SET status = $3, available_at = $4, lease_token = NULL,
                       lease_expires_at = NULL, operation_generation = NULL,
                       last_error = $5, updated_at = CURRENT_TIMESTAMP
                   FROM current_job
                   WHERE job.id = current_job.id
                   RETURNING current_job.provider, current_job.remote_database_id,
                             current_job.operation_generation
               )
               UPDATE memory_remote_resource_lifecycles AS lifecycle
               SET operation_lease_token = NULL, operation_lease_expires_at = NULL,
                   last_error = $5, updated_at = CURRENT_TIMESTAMP
               FROM retried_job
               WHERE lifecycle.provider = retried_job.provider
                 AND lifecycle.remote_database_id = retried_job.remote_database_id
                 AND lifecycle.desired_state = 'absent'
                 AND lifecycle.operation_generation = retried_job.operation_generation
                 AND lifecycle.operation_lease_token = $2
               RETURNING lifecycle.remote_database_id"#,
        )
        .bind(job_id)
        .bind(lease_token)
        .bind(if terminal { "failed" } else { "pending" })
        .bind(available_at)
        .bind(safe_error)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(result.is_some())
    }
}

enum ProvisioningTransition<'a> {
    Waiting {
        readiness_deadline: Option<DateTime<Utc>>,
        next_poll_at: DateTime<Utc>,
    },
    Failed {
        safe_error: &'a str,
    },
}

async fn transition_provisioning_job(
    persistence: &PostgresPersistence,
    job_id: Uuid,
    lease_token: Uuid,
    transition: ProvisioningTransition<'_>,
) -> AppResult<bool> {
    let company_id = match transition {
        ProvisioningTransition::Waiting {
            readiness_deadline,
            next_poll_at,
        } => sqlx::query_scalar::<_, Uuid>(
            r#"WITH current_job AS (
                       SELECT job.id, job.company_id, job.provider, job.remote_database_id,
                              job.operation_generation
                       FROM memory_provisioning_jobs AS job
                       WHERE job.id = $1 AND job.status = 'leased' AND job.lease_token = $2
                         AND job.lease_expires_at > CURRENT_TIMESTAMP
                         AND job.phase = CASE WHEN $3::timestamptz IS NULL
                                              THEN 'waiting_ready' ELSE 'create_pending' END
                         AND EXISTS (
                             SELECT 1 FROM memory_remote_resource_lifecycles AS lifecycle
                             WHERE lifecycle.provider = job.provider
                               AND lifecycle.remote_database_id = job.remote_database_id
                               AND lifecycle.company_id = job.company_id
                               AND lifecycle.desired_state = 'present'
                               AND lifecycle.operation_generation = job.operation_generation
                               AND lifecycle.operation_lease_token = job.lease_token
                               AND lifecycle.operation_lease_expires_at > CURRENT_TIMESTAMP
                         )
                       FOR UPDATE
                   ), updated_job AS (
                       UPDATE memory_provisioning_jobs AS job
                       SET status = 'pending', phase = 'waiting_ready',
                           readiness_deadline = COALESCE($3, job.readiness_deadline),
                           next_poll_at = $4, lease_token = NULL, lease_expires_at = NULL,
                           operation_generation = NULL, last_error = NULL,
                           updated_at = CURRENT_TIMESTAMP
                       FROM current_job
                       WHERE job.id = current_job.id
                       RETURNING current_job.company_id, current_job.provider,
                                 current_job.remote_database_id,
                                 current_job.operation_generation
                   ), released_lifecycle AS (
                       UPDATE memory_remote_resource_lifecycles AS lifecycle
                       SET operation_lease_token = NULL, operation_lease_expires_at = NULL,
                           last_error = NULL, updated_at = CURRENT_TIMESTAMP
                       FROM updated_job
                       WHERE lifecycle.provider = updated_job.provider
                         AND lifecycle.remote_database_id = updated_job.remote_database_id
                         AND lifecycle.company_id = updated_job.company_id
                         AND lifecycle.desired_state = 'present'
                         AND lifecycle.operation_generation = updated_job.operation_generation
                         AND lifecycle.operation_lease_token = $2
                       RETURNING lifecycle.company_id
                   )
                   UPDATE memory_provider_connections AS connection
                   SET readiness = 'provisioning', last_error = NULL,
                       updated_at = CURRENT_TIMESTAMP
                   FROM released_lifecycle
                   WHERE connection.company_id = released_lifecycle.company_id
                     AND connection.provider = 'hydradb'
                   RETURNING connection.company_id"#,
        )
        .bind(job_id)
        .bind(lease_token)
        .bind(readiness_deadline)
        .bind(next_poll_at)
        .fetch_optional(&persistence.pool)
        .await
        .map_err(AppError::from)?,
        ProvisioningTransition::Failed { safe_error } => sqlx::query_scalar::<_, Uuid>(
            r#"WITH current_job AS (
                       SELECT job.id, job.company_id, job.provider, job.remote_database_id,
                              job.operation_generation
                       FROM memory_provisioning_jobs AS job
                       WHERE job.id = $1 AND job.status = 'leased' AND job.lease_token = $2
                         AND job.lease_expires_at > CURRENT_TIMESTAMP
                         AND EXISTS (
                             SELECT 1 FROM memory_remote_resource_lifecycles AS lifecycle
                             WHERE lifecycle.provider = job.provider
                               AND lifecycle.remote_database_id = job.remote_database_id
                               AND lifecycle.company_id = job.company_id
                               AND lifecycle.desired_state = 'present'
                               AND lifecycle.operation_generation = job.operation_generation
                               AND lifecycle.operation_lease_token = job.lease_token
                               AND lifecycle.operation_lease_expires_at > CURRENT_TIMESTAMP
                         )
                       FOR UPDATE
                   ), failed_job AS (
                       UPDATE memory_provisioning_jobs AS job
                       SET status = 'failed', phase = 'failed', lease_token = NULL,
                           lease_expires_at = NULL, operation_generation = NULL,
                           last_error = $3, updated_at = CURRENT_TIMESTAMP
                       FROM current_job
                       WHERE job.id = current_job.id
                       RETURNING current_job.company_id, current_job.provider,
                                 current_job.remote_database_id,
                                 current_job.operation_generation
                   ), released_lifecycle AS (
                       UPDATE memory_remote_resource_lifecycles AS lifecycle
                       SET operation_lease_token = NULL, operation_lease_expires_at = NULL,
                           last_error = $3, updated_at = CURRENT_TIMESTAMP
                       FROM failed_job
                       WHERE lifecycle.provider = failed_job.provider
                         AND lifecycle.remote_database_id = failed_job.remote_database_id
                         AND lifecycle.company_id = failed_job.company_id
                         AND lifecycle.desired_state = 'present'
                         AND lifecycle.operation_generation = failed_job.operation_generation
                         AND lifecycle.operation_lease_token = $2
                       RETURNING lifecycle.company_id
                   )
                   UPDATE memory_provider_connections AS connection
                   SET readiness = 'failed', last_error = $3, updated_at = CURRENT_TIMESTAMP
                   FROM released_lifecycle
                   WHERE connection.company_id = released_lifecycle.company_id
                     AND connection.provider = 'hydradb'
                   RETURNING connection.company_id"#,
        )
        .bind(job_id)
        .bind(lease_token)
        .bind(safe_error)
        .fetch_optional(&persistence.pool)
        .await
        .map_err(AppError::from)?,
    };
    Ok(company_id.is_some())
}

async fn renew_job_lease(
    persistence: &PostgresPersistence,
    table: &'static str,
    desired_state: &'static str,
    job_id: Uuid,
    lease_token: Uuid,
    lease_expires_at: DateTime<Utc>,
) -> AppResult<bool> {
    debug_assert!(matches!(
        table,
        "memory_provisioning_jobs" | "memory_cleanup_jobs"
    ));
    debug_assert!(matches!(desired_state, "present" | "absent"));
    let query = format!(
        "WITH current_job AS (\
             SELECT id, provider, remote_database_id, operation_generation \
             FROM {table} \
             WHERE id = $1 AND status = 'leased' AND lease_token = $2 \
               AND lease_expires_at > CURRENT_TIMESTAMP \
             FOR UPDATE\
         ), renewed_lifecycle AS (\
             UPDATE memory_remote_resource_lifecycles AS lifecycle \
             SET operation_lease_expires_at = $3, updated_at = CURRENT_TIMESTAMP \
             FROM current_job \
             WHERE lifecycle.provider = current_job.provider \
               AND lifecycle.remote_database_id = current_job.remote_database_id \
               AND lifecycle.desired_state = $4 \
               AND lifecycle.operation_generation = current_job.operation_generation \
               AND lifecycle.operation_lease_token = $2 \
               AND lifecycle.operation_lease_expires_at > CURRENT_TIMESTAMP \
             RETURNING current_job.id\
         ) \
         UPDATE {table} AS job \
         SET lease_expires_at = $3, updated_at = CURRENT_TIMESTAMP \
         FROM renewed_lifecycle \
         WHERE job.id = renewed_lifecycle.id"
    );
    let result = sqlx::query(&query)
        .bind(job_id)
        .bind(lease_token)
        .bind(lease_expires_at)
        .bind(desired_state)
        .execute(&persistence.pool)
        .await
        .map_err(AppError::from)?;
    Ok(result.rows_affected() == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        adapters::persistence::test_support::{UNSCOPED_CLAIM, test_pool},
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
        let _claim_guard = UNSCOPED_CLAIM.lock().await;
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
            .filter(|job| job.company_id == company_id)
            .count();
        assert_eq!(claimed, 1, "a provisioning job is leased to one worker");
    }

    #[tokio::test]
    async fn readiness_claim_is_exclusive_and_timeout_retry_resets_the_same_job() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let _claim_guard = UNSCOPED_CLAIM.lock().await;
        let persistence = PostgresPersistence::new(pool.clone());
        let company_id = memory_company(&persistence).await;
        persistence
            .select_provider(company_id, MemoryProviderKind::Hydradb)
            .await
            .expect("provider selection");
        let create_token = Uuid::new_v4();
        let create = persistence
            .claim_provisioning_job(create_token, Utc::now() + chrono::Duration::minutes(2))
            .await
            .expect("create claim")
            .expect("create job");
        let original_job_id = create.id;
        assert!(
            persistence
                .begin_readiness_polling(
                    create.id,
                    create_token,
                    Utc::now() - chrono::Duration::seconds(1),
                    Utc::now(),
                )
                .await
                .expect("begin polling")
        );

        let expires = Utc::now() + chrono::Duration::minutes(2);
        let left_token = Uuid::new_v4();
        let right_token = Uuid::new_v4();
        let (left, right) = tokio::join!(
            persistence.claim_provisioning_job(left_token, expires),
            persistence.claim_provisioning_job(right_token, expires),
        );
        let claims: Vec<_> = [left.expect("left claim"), right.expect("right claim")]
            .into_iter()
            .flatten()
            .filter(|job| job.company_id == company_id)
            .collect();
        assert_eq!(claims.len(), 1, "only one claimant polls this generation");
        let claimed = &claims[0];
        assert_eq!(
            claimed.phase,
            crate::entities::memory::MemoryProvisioningPhase::WaitingReady
        );
        assert!(
            persistence
                .fail_provisioning_job(claimed.id, claimed.lease_token, "readiness timed out")
                .await
                .expect("terminal timeout")
        );
        assert!(
            !persistence
                .fail_provisioning_job(claimed.id, claimed.lease_token, "readiness timed out")
                .await
                .expect("duplicate timeout is suppressed")
        );

        persistence
            .retry_provisioning(company_id)
            .await
            .expect("manual retry");
        let reset: (
            Uuid,
            String,
            String,
            i32,
            Option<DateTime<Utc>>,
            Option<DateTime<Utc>>,
        ) = sqlx::query_as(
            "SELECT id, status, phase, failure_attempts, readiness_deadline, next_poll_at \
                 FROM memory_provisioning_jobs WHERE company_id = $1",
        )
        .bind(company_id)
        .fetch_one(&pool)
        .await
        .expect("reset job");
        assert_eq!(
            reset.0, original_job_id,
            "manual retry reuses the durable job"
        );
        assert_eq!(
            (reset.1.as_str(), reset.2.as_str(), reset.3),
            ("pending", "create_pending", 0)
        );
        assert_eq!((reset.4, reset.5), (None, None));
        let connection = persistence
            .active_binding(company_id)
            .await
            .expect("binding query")
            .connection()
            .expect("connection");
        assert_eq!(
            connection.readiness,
            crate::entities::memory::MemoryConnectionReadiness::Pending
        );
        assert!(connection.last_error.is_none());

        let provider_failure_token = Uuid::new_v4();
        let provider_failure = persistence
            .claim_provisioning_job(
                provider_failure_token,
                Utc::now() + chrono::Duration::minutes(2),
            )
            .await
            .expect("provider failure claim")
            .expect("provider failure job");
        assert!(
            persistence
                .retry_provisioning_job(
                    provider_failure.id,
                    provider_failure_token,
                    Utc::now(),
                    "memory provider authentication failed",
                    true,
                )
                .await
                .expect("terminal provider failure")
        );
        persistence
            .retry_provisioning(company_id)
            .await
            .expect("manual retry after provider failure");
        let provider_reset: (Uuid, String, i32) = sqlx::query_as(
            "SELECT id, phase, failure_attempts FROM memory_provisioning_jobs \
             WHERE company_id = $1",
        )
        .bind(company_id)
        .fetch_one(&pool)
        .await
        .expect("provider failure reset");
        assert_eq!(
            provider_reset,
            (original_job_id, "create_pending".into(), 0)
        );

        // Do not leave a globally claimable row behind for another queue test.
        let cleanup_token = Uuid::new_v4();
        let cleanup = persistence
            .claim_provisioning_job(cleanup_token, Utc::now() + chrono::Duration::minutes(2))
            .await
            .expect("test cleanup claim")
            .expect("test cleanup job");
        assert_eq!(cleanup.id, original_job_id);
        assert!(
            persistence
                .retry_provisioning_job(
                    cleanup.id,
                    cleanup_token,
                    Utc::now(),
                    "test cleanup",
                    true,
                )
                .await
                .expect("terminal test cleanup")
        );
    }

    #[tokio::test]
    async fn deletion_fences_a_leased_provision_and_cleanup_waits_for_quiescence() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let _claim_guard = UNSCOPED_CLAIM.lock().await;
        let persistence = PostgresPersistence::new(pool.clone());
        let company_id = memory_company(&persistence).await;
        let connection = persistence
            .select_provider(company_id, MemoryProviderKind::Hydradb)
            .await
            .expect("selection");
        let provision_token = Uuid::new_v4();
        let provision = persistence
            .claim_provisioning_job(provision_token, Utc::now() + chrono::Duration::minutes(2))
            .await
            .expect("provision claim")
            .expect("provision job");
        assert!(
            persistence
                .mark_provisioning(provision.id, provision_token)
                .await
                .expect("mark provisioning")
        );

        persistence
            .delete(company_id)
            .await
            .expect("company deletion");

        let lifecycle: (Option<Uuid>, String) = sqlx::query_as(
            r#"SELECT company_id, desired_state
               FROM memory_remote_resource_lifecycles
               WHERE provider = 'hydradb' AND remote_database_id = $1"#,
        )
        .bind(&connection.remote_database_id)
        .fetch_one(&pool)
        .await
        .expect("surviving lifecycle");
        assert_eq!(lifecycle, (None, "absent".into()));
        assert!(
            !persistence
                .complete_provisioning(provision.id, provision_token)
                .await
                .expect("stale completion is suppressed")
        );
        assert!(
            persistence
                .claim_cleanup_job(Uuid::new_v4(), Utc::now() + chrono::Duration::minutes(2),)
                .await
                .expect("early cleanup claim")
                .is_none(),
            "cleanup cannot observe an early 404 during the quiescence window"
        );

        sqlx::query(
            r#"UPDATE memory_remote_resource_lifecycles
               SET quiesce_until = CURRENT_TIMESTAMP,
                   operation_lease_expires_at = CURRENT_TIMESTAMP - INTERVAL '1 second'
               WHERE provider = 'hydradb' AND remote_database_id = $1"#,
        )
        .bind(&connection.remote_database_id)
        .execute(&pool)
        .await
        .expect("advance past operation deadline");
        sqlx::query(
            r#"UPDATE memory_cleanup_jobs SET available_at = CURRENT_TIMESTAMP
               WHERE provider = 'hydradb' AND remote_database_id = $1"#,
        )
        .bind(&connection.remote_database_id)
        .execute(&pool)
        .await
        .expect("make cleanup due");

        let expires = Utc::now() + chrono::Duration::minutes(2);
        let left_token = Uuid::new_v4();
        let right_token = Uuid::new_v4();
        let (left, right) = tokio::join!(
            persistence.claim_cleanup_job(left_token, expires),
            persistence.claim_cleanup_job(right_token, expires),
        );
        let claimed: Vec<_> = [
            left.expect("left cleanup claimant"),
            right.expect("right cleanup claimant"),
        ]
        .into_iter()
        .flatten()
        .filter(|job| job.remote_database_id == connection.remote_database_id)
        .collect();
        assert_eq!(
            claimed.len(),
            1,
            "one cleanup execution owns the generation"
        );
        let cleanup = &claimed[0];
        assert!(
            persistence
                .complete_cleanup(cleanup.id, cleanup.lease_token)
                .await
                .expect("confirmed cleanup")
        );
    }

    #[tokio::test]
    async fn expired_operation_is_reclaimed_with_a_new_generation() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let _claim_guard = UNSCOPED_CLAIM.lock().await;
        let persistence = PostgresPersistence::new(pool.clone());
        let company_id = memory_company(&persistence).await;
        let connection = persistence
            .select_provider(company_id, MemoryProviderKind::Hydradb)
            .await
            .expect("selection");
        let old_token = Uuid::new_v4();
        let old = persistence
            .claim_provisioning_job(old_token, Utc::now() + chrono::Duration::minutes(2))
            .await
            .expect("old claim")
            .expect("old job");
        assert_eq!(old.company_id, company_id);

        sqlx::query(
            r#"UPDATE memory_provisioning_jobs
               SET lease_expires_at = CURRENT_TIMESTAMP - INTERVAL '1 second'
               WHERE id = $1"#,
        )
        .bind(old.id)
        .execute(&pool)
        .await
        .expect("expire job lease");
        sqlx::query(
            r#"UPDATE memory_remote_resource_lifecycles
               SET operation_lease_expires_at = CURRENT_TIMESTAMP - INTERVAL '1 second'
               WHERE provider = 'hydradb' AND remote_database_id = $1"#,
        )
        .bind(connection.remote_database_id)
        .execute(&pool)
        .await
        .expect("expire operation lease");

        let replacement_token = Uuid::new_v4();
        let replacement = persistence
            .claim_provisioning_job(replacement_token, Utc::now() + chrono::Duration::minutes(2))
            .await
            .expect("replacement claim")
            .expect("replacement job");
        assert!(replacement.operation_generation > old.operation_generation);
        assert!(
            !persistence
                .complete_provisioning(old.id, old_token)
                .await
                .expect("stale completion")
        );
        assert!(
            persistence
                .begin_readiness_polling(
                    replacement.id,
                    replacement_token,
                    Utc::now() + chrono::Duration::minutes(15),
                    Utc::now(),
                )
                .await
                .expect("replacement enters readiness polling")
        );
        let ready_token = Uuid::new_v4();
        let ready = persistence
            .claim_provisioning_job(ready_token, Utc::now() + chrono::Duration::minutes(2))
            .await
            .expect("readiness claim")
            .expect("readiness job");
        assert!(
            persistence
                .complete_provisioning(ready.id, ready_token)
                .await
                .expect("replacement completion")
        );
    }

    #[tokio::test]
    async fn legacy_connection_writes_are_dual_written_during_rollout() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let _claim_guard = UNSCOPED_CLAIM.lock().await;
        let persistence = PostgresPersistence::new(pool.clone());
        let company_id = memory_company(&persistence).await;
        let remote_database_id = remote_memory_database_id(company_id);

        let mut transaction = pool.begin().await.expect("legacy write transaction");
        sqlx::query(
            r#"INSERT INTO memory_provider_connections
                   (company_id, provider, remote_database_id, readiness)
               VALUES ($1, 'hydradb', $2, 'pending')"#,
        )
        .bind(company_id)
        .bind(&remote_database_id)
        .execute(&mut *transaction)
        .await
        .expect("legacy connection insert");
        sqlx::query(
            r#"INSERT INTO memory_provisioning_jobs
                   (id, company_id, provider, remote_database_id)
               VALUES ($1, $2, 'hydradb', $3)"#,
        )
        .bind(Uuid::new_v4())
        .bind(company_id)
        .bind(&remote_database_id)
        .execute(&mut *transaction)
        .await
        .expect("legacy provisioning insert satisfies lifecycle foreign key");
        transaction.commit().await.expect("legacy selection commit");

        let desired: String = sqlx::query_scalar(
            r#"SELECT desired_state FROM memory_remote_resource_lifecycles
               WHERE provider = 'hydradb' AND remote_database_id = $1"#,
        )
        .bind(&remote_database_id)
        .fetch_one(&pool)
        .await
        .expect("dual-written present lifecycle");
        assert_eq!(desired, "present");

        let mut transaction = pool.begin().await.expect("legacy delete transaction");
        sqlx::query(
            r#"INSERT INTO memory_cleanup_jobs (id, provider, remote_database_id)
               VALUES ($1, 'hydradb', $2)
               ON CONFLICT (provider, remote_database_id) DO NOTHING"#,
        )
        .bind(Uuid::new_v4())
        .bind(&remote_database_id)
        .execute(&mut *transaction)
        .await
        .expect("legacy cleanup insert");
        sqlx::query("DELETE FROM companies WHERE id = $1")
            .bind(company_id)
            .execute(&mut *transaction)
            .await
            .expect("legacy company deletion");
        transaction.commit().await.expect("legacy deletion commit");

        let lifecycle: (Option<Uuid>, String) = sqlx::query_as(
            r#"SELECT company_id, desired_state
               FROM memory_remote_resource_lifecycles
               WHERE provider = 'hydradb' AND remote_database_id = $1"#,
        )
        .bind(&remote_database_id)
        .fetch_one(&pool)
        .await
        .expect("dual-written absent lifecycle");
        assert_eq!(lifecycle, (None, "absent".into()));
        let cleanup_status: String = sqlx::query_scalar(
            r#"SELECT status FROM memory_cleanup_jobs
               WHERE provider = 'hydradb' AND remote_database_id = $1"#,
        )
        .bind(remote_database_id)
        .fetch_one(&pool)
        .await
        .expect("surviving cleanup");
        assert_eq!(cleanup_status, "pending");
    }

    #[tokio::test]
    async fn disabling_preserves_the_connection_and_deletion_preserves_cleanup_work() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let _claim_guard = UNSCOPED_CLAIM.lock().await;
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
        assert_eq!(
            persistence.active_binding(company_id).await.unwrap(),
            ActiveMemoryBinding::Disabled
        );
        assert!(
            persistence
                .memory_connection(company_id)
                .await
                .unwrap()
                .is_some()
        );

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

    #[tokio::test]
    async fn active_binding_reports_misconfigured_when_selection_has_no_connection() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());
        let company_id = memory_company(&persistence).await;
        sqlx::query("UPDATE companies SET memory_provider = 'hydradb' WHERE id = $1")
            .bind(company_id)
            .execute(&pool)
            .await
            .expect("orphan provider selection");

        assert_eq!(
            persistence.active_binding(company_id).await.unwrap(),
            ActiveMemoryBinding::Misconfigured
        );
    }

    #[tokio::test]
    async fn reenable_reuses_ready_connection_and_queues_only_readiness_reconciliation() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let _claim_guard = UNSCOPED_CLAIM.lock().await;
        let persistence = PostgresPersistence::new(pool.clone());
        let company_id = memory_company(&persistence).await;
        let original = persistence
            .select_provider(company_id, MemoryProviderKind::Hydradb)
            .await
            .expect("selection");
        sqlx::query(
            "UPDATE memory_provider_connections SET readiness = 'ready' WHERE company_id = $1",
        )
        .bind(company_id)
        .execute(&pool)
        .await
        .expect("ready connection");

        persistence
            .disable_provider(company_id)
            .await
            .expect("disable");
        let reenabled = persistence
            .select_provider(company_id, MemoryProviderKind::Hydradb)
            .await
            .expect("reenable");
        assert_eq!(reenabled.remote_database_id, original.remote_database_id);
        assert_eq!(
            reenabled.readiness,
            crate::entities::memory::MemoryConnectionReadiness::Pending
        );
        let job: (String, String) = sqlx::query_as(
            "SELECT status, phase FROM memory_provisioning_jobs WHERE company_id = $1",
        )
        .bind(company_id)
        .fetch_one(&pool)
        .await
        .expect("readiness reconciliation");
        assert_eq!(job, ("pending".into(), "waiting_ready".into()));

        persistence
            .select_provider(company_id, MemoryProviderKind::Hydradb)
            .await
            .expect("idempotent repeated selection");
        let phase: String =
            sqlx::query_scalar("SELECT phase FROM memory_provisioning_jobs WHERE company_id = $1")
                .bind(company_id)
                .fetch_one(&pool)
                .await
                .expect("unchanged reconciliation");
        assert_eq!(phase, "waiting_ready");

        let lease_token = Uuid::new_v4();
        let job = persistence
            .claim_provisioning_job(lease_token, Utc::now() + chrono::Duration::minutes(2))
            .await
            .expect("claim readiness reconciliation")
            .expect("reconciliation job");
        assert!(
            persistence
                .mark_provisioning(job.id, lease_token)
                .await
                .expect("mark reconciliation")
        );
        persistence
            .disable_provider(company_id)
            .await
            .expect("disable during reconciliation");
        assert!(
            persistence
                .complete_provisioning(job.id, lease_token)
                .await
                .expect("finish stale reconciliation")
        );
        assert_eq!(
            persistence.active_binding(company_id).await.unwrap(),
            ActiveMemoryBinding::Disabled,
            "stale readiness work must not restore provider selection"
        );
    }
}
