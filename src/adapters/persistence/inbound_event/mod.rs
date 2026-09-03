//! PostgreSQL implementation of the durable inbound inbox and its execution fence.

use std::{collections::BTreeMap, str::FromStr, time::Duration};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{
        correlation::CorrelationId,
        transport::{
            ExternalEventKey, InboundEventId, InboundEventIgnoreReason, InboundEventStatus,
            InstallationId, TransportKind,
        },
    },
    transport::{
        AuthenticatedInboundEvent, ClaimedInboundEvent, ExecutionId, ExecutionLease,
        InboundContentType, InboundEventCensus, InboundEventFailure, InboundEventInbox,
        InboundEventPayload, InboundEventQueue, InboundEventReaping, InboundEventRecord,
        InboundEventRetention, InboundEventStoreOutcome, InboundEventTransition,
        InboundPayloadDigest, InboundRetentionPolicy, SafeHeaderFacts, WorkerId,
    },
};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

const EVENT_COLUMNS: &str = "\
    event.id, event.company_id, event.installation_id, event.transport, \
    event.external_event_key, event.correlation_id, event.raw_payload, event.content_type, \
    event.content_hash, event.safe_header_facts, event.status, event.attempt_count, \
    event.max_attempts, event.available_at, event.last_error_class, event.last_error_detail, \
    event.ignore_reason, event.execution_id, event.owner_worker_id, event.locked_at, \
    event.lock_expires_at, event.received_at, event.processed_at, event.created_at, event.updated_at";

#[allow(dead_code)]
#[derive(sqlx::FromRow)]
struct InboundEventDb {
    id: Uuid,
    company_id: Uuid,
    installation_id: Option<Uuid>,
    transport: String,
    external_event_key: String,
    correlation_id: Uuid,
    raw_payload: Vec<u8>,
    content_type: Option<String>,
    content_hash: Vec<u8>,
    safe_header_facts: serde_json::Value,
    status: String,
    attempt_count: i32,
    max_attempts: i32,
    available_at: DateTime<Utc>,
    last_error_class: Option<String>,
    last_error_detail: Option<String>,
    ignore_reason: Option<String>,
    execution_id: Option<Uuid>,
    owner_worker_id: Option<Uuid>,
    locked_at: Option<DateTime<Utc>>,
    lock_expires_at: Option<DateTime<Utc>>,
    received_at: DateTime<Utc>,
    processed_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl InboundEventDb {
    fn record(&self) -> AppResult<InboundEventRecord> {
        let facts: BTreeMap<String, String> =
            serde_json::from_value(self.safe_header_facts.clone())
                .map_err(|error| row_error(self.id, "safe_header_facts", error))?;
        Ok(InboundEventRecord {
            id: InboundEventId::new(self.id),
            company_id: self.company_id,
            installation_id: self.installation_id.map(InstallationId::new),
            transport: TransportKind::from_str(&self.transport)
                .map_err(|error| row_error(self.id, "transport", error))?,
            external_event_key: ExternalEventKey::parse(self.external_event_key.clone())
                .map_err(|error| row_error(self.id, "external_event_key", error))?,
            correlation_id: CorrelationId::from(self.correlation_id),
            payload: InboundEventPayload::parse(self.raw_payload.clone())
                .map_err(|error| row_error(self.id, "raw_payload", error))?,
            payload_digest: InboundPayloadDigest::parse(self.content_hash.clone())
                .map_err(|error| row_error(self.id, "content_hash", error))?,
            content_type: self
                .content_type
                .clone()
                .map(InboundContentType::parse)
                .transpose()
                .map_err(|error| row_error(self.id, "content_type", error))?,
            safe_header_facts: SafeHeaderFacts::parse(facts)
                .map_err(|error| row_error(self.id, "safe_header_facts", error))?,
            attempt_count: self.attempt_count,
            max_attempts: self.max_attempts,
            received_at: self.received_at,
        })
    }

    fn claimed(&self) -> AppResult<ClaimedInboundEvent> {
        let execution = self.execution_id.ok_or_else(|| {
            AppError::Internal(format!(
                "Claimed inbound event {} has no execution id",
                self.id
            ))
        })?;
        let owner = self.owner_worker_id.ok_or_else(|| {
            AppError::Internal(format!("Claimed inbound event {} has no owner", self.id))
        })?;
        let expires_at = self.lock_expires_at.ok_or_else(|| {
            AppError::Internal(format!("Claimed inbound event {} has no expiry", self.id))
        })?;
        Ok(ClaimedInboundEvent {
            lease: ExecutionLease {
                row: InboundEventId::new(self.id),
                execution: ExecutionId::new(execution),
                owner: WorkerId::new(owner),
                expires_at,
            },
            record: self.record()?,
        })
    }
}

fn row_error(id: Uuid, column: &str, error: impl std::fmt::Display) -> AppError {
    AppError::Internal(format!(
        "Inbound event {id} has unreadable {column}: {error}"
    ))
}

#[async_trait]
impl InboundEventInbox for PostgresPersistence {
    async fn store_authenticated(
        &self,
        event: AuthenticatedInboundEvent,
    ) -> AppResult<InboundEventStoreOutcome> {
        let id = InboundEventId::random();
        let digest = event.digest();
        let safe_header_facts = serde_json::to_value(&event.safe_header_facts)
            .map_err(|error| AppError::Internal(error.to_string()))?;

        let inserted: Option<Uuid> = sqlx::query_scalar(
            r#"INSERT INTO inbound_events
                   (id, company_id, installation_id, transport, external_event_key,
                    correlation_id, raw_payload, content_type, content_hash, safe_header_facts,
                    received_at)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
               ON CONFLICT (transport, external_event_key) DO NOTHING
               RETURNING id"#,
        )
        .bind(id.as_uuid())
        .bind(event.company_id)
        .bind(event.installation_id.map(InstallationId::as_uuid))
        .bind(event.transport.as_str())
        .bind(event.external_event_key.as_str())
        .bind(event.correlation_id.as_uuid())
        .bind(event.payload.as_bytes())
        .bind(event.content_type.as_ref().map(InboundContentType::as_str))
        .bind(digest.as_bytes().as_slice())
        .bind(safe_header_facts)
        .bind(event.received_at)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        if let Some(inserted) = inserted {
            return Ok(InboundEventStoreOutcome::Stored(InboundEventId::new(
                inserted,
            )));
        }

        let existing: Option<(Uuid, Uuid, Option<Uuid>, Vec<u8>)> = sqlx::query_as(
            r#"SELECT id, company_id, installation_id, content_hash
                 FROM inbound_events
                WHERE transport = $1 AND external_event_key = $2"#,
        )
        .bind(event.transport.as_str())
        .bind(event.external_event_key.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;
        let Some((existing_id, company_id, installation_id, content_hash)) = existing else {
            return Err(AppError::Internal(
                "Inbound event deduplication conflict vanished before it could be read".into(),
            ));
        };
        if company_id != event.company_id
            || installation_id != event.installation_id.map(InstallationId::as_uuid)
            || content_hash.as_slice() != digest.as_bytes()
        {
            return Err(AppError::Conflict(format!(
                "Inbound event key {} was reused with a different authenticated scope or payload",
                event.external_event_key
            )));
        }
        Ok(InboundEventStoreOutcome::Duplicate(InboundEventId::new(
            existing_id,
        )))
    }
}

#[async_trait]
impl InboundEventQueue for PostgresPersistence {
    async fn claim_inbound_events(
        &self,
        owner: WorkerId,
        lease_for: Duration,
        limit: i64,
    ) -> AppResult<Vec<ClaimedInboundEvent>> {
        let limit = limit.clamp(1, 100);
        let lease_seconds = lease_for.as_secs().clamp(1, 3_600) as f64;
        let rows = sqlx::query_as::<_, InboundEventDb>(&format!(
            r#"WITH claimable AS (
                   SELECT event.id
                     FROM inbound_events AS event
                    WHERE event.status IN ('pending', 'retryable')
                      AND event.available_at <= CURRENT_TIMESTAMP
                    ORDER BY event.available_at, event.received_at, event.id
                      FOR UPDATE SKIP LOCKED
                    LIMIT $1
               )
               UPDATE inbound_events AS event
                  SET status = 'processing', execution_id = gen_random_uuid(),
                      owner_worker_id = $2, locked_at = CURRENT_TIMESTAMP,
                      lock_expires_at = CURRENT_TIMESTAMP
                          + make_interval(secs => $3::double precision),
                      last_error_class = NULL, last_error_detail = NULL,
                      processed_at = NULL, updated_at = CURRENT_TIMESTAMP
                 FROM claimable
                WHERE event.id = claimable.id
             RETURNING {EVENT_COLUMNS}"#,
        ))
        .bind(limit)
        .bind(owner.as_uuid())
        .bind(lease_seconds)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;
        rows.iter().map(InboundEventDb::claimed).collect()
    }

    async fn renew_inbound_event_lease(
        &self,
        fence: &ExecutionLease<InboundEventId>,
        until: DateTime<Utc>,
    ) -> AppResult<bool> {
        let result = sqlx::query(
            r#"UPDATE inbound_events
                  SET lock_expires_at = $4, updated_at = CURRENT_TIMESTAMP
                WHERE id = $1 AND status = 'processing'
                  AND execution_id = $2 AND owner_worker_id = $3
                  AND lock_expires_at > CURRENT_TIMESTAMP AND $4 > CURRENT_TIMESTAMP"#,
        )
        .bind(fence.row.as_uuid())
        .bind(fence.execution.as_uuid())
        .bind(fence.owner.as_uuid())
        .bind(until)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(result.rows_affected() == 1)
    }

    async fn complete_inbound_event(
        &self,
        fence: &ExecutionLease<InboundEventId>,
    ) -> AppResult<InboundEventTransition> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let applied = complete_inbound_event_on(&mut tx, fence).await?;
        tx.commit().await.map_err(AppError::from)?;
        Ok(transition(applied, InboundEventStatus::Completed))
    }

    async fn ignore_inbound_event(
        &self,
        fence: &ExecutionLease<InboundEventId>,
        reason: InboundEventIgnoreReason,
    ) -> AppResult<InboundEventTransition> {
        let result = sqlx::query(
            r#"UPDATE inbound_events
                  SET status = 'ignored', ignore_reason = $4, processed_at = CURRENT_TIMESTAMP,
                      execution_id = NULL, owner_worker_id = NULL,
                      locked_at = NULL, lock_expires_at = NULL,
                      updated_at = CURRENT_TIMESTAMP
                WHERE id = $1 AND status = 'processing'
                  AND execution_id = $2 AND owner_worker_id = $3
                  AND lock_expires_at > CURRENT_TIMESTAMP"#,
        )
        .bind(fence.row.as_uuid())
        .bind(fence.execution.as_uuid())
        .bind(fence.owner.as_uuid())
        .bind(reason.as_str())
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(transition(
            result.rows_affected() == 1,
            InboundEventStatus::Ignored,
        ))
    }

    async fn retry_inbound_event(
        &self,
        failure: InboundEventFailure<'_>,
    ) -> AppResult<InboundEventTransition> {
        settle_failure(self, failure, false).await
    }

    async fn dead_letter_inbound_event(
        &self,
        failure: InboundEventFailure<'_>,
    ) -> AppResult<InboundEventTransition> {
        settle_failure(self, failure, true).await
    }

    async fn reap_expired_inbound_events(&self) -> AppResult<InboundEventReaping> {
        let delay = retry_delay_sql("attempt_count + 1");
        let result = sqlx::query(&format!(
            r#"UPDATE inbound_events
                  SET status = CASE WHEN attempt_count + 1 >= max_attempts
                                    THEN 'dead_letter' ELSE 'retryable' END,
                      attempt_count = attempt_count + 1,
                      available_at = CURRENT_TIMESTAMP + {delay},
                      last_error_class = 'lease_expired',
                      last_error_detail = 'The inbound event lease expired without a result',
                      processed_at = CASE WHEN attempt_count + 1 >= max_attempts
                                          THEN CURRENT_TIMESTAMP ELSE NULL END,
                      execution_id = NULL, owner_worker_id = NULL,
                      locked_at = NULL, lock_expires_at = NULL,
                      updated_at = CURRENT_TIMESTAMP
                WHERE status = 'processing'
                  AND (execution_id IS NULL OR owner_worker_id IS NULL OR locked_at IS NULL
                       OR lock_expires_at IS NULL OR lock_expires_at <= locked_at
                       OR lock_expires_at <= CURRENT_TIMESTAMP)"#,
        ))
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(InboundEventReaping {
            leases_expired: result.rows_affected(),
        })
    }

    async fn inbound_event_census(&self) -> AppResult<InboundEventCensus> {
        #[derive(sqlx::FromRow)]
        struct CensusDb {
            pending: i64,
            processing: i64,
            retryable: i64,
            completed: i64,
            ignored: i64,
            dead_letter: i64,
            oldest_ready_seconds: Option<f64>,
        }
        let row = sqlx::query_as::<_, CensusDb>(
            r#"SELECT COUNT(*) FILTER (WHERE status = 'pending') AS pending,
                      COUNT(*) FILTER (WHERE status = 'processing') AS processing,
                      COUNT(*) FILTER (WHERE status = 'retryable') AS retryable,
                      COUNT(*) FILTER (WHERE status = 'completed') AS completed,
                      COUNT(*) FILTER (WHERE status = 'ignored') AS ignored,
                      COUNT(*) FILTER (WHERE status = 'dead_letter') AS dead_letter,
                      EXTRACT(EPOCH FROM CURRENT_TIMESTAMP - MIN(received_at) FILTER (
                          WHERE status IN ('pending', 'retryable')
                            AND available_at <= CURRENT_TIMESTAMP
                      ))::DOUBLE PRECISION AS oldest_ready_seconds
                 FROM inbound_events"#,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(InboundEventCensus {
            pending: row.pending.max(0) as u64,
            processing: row.processing.max(0) as u64,
            retryable: row.retryable.max(0) as u64,
            completed: row.completed.max(0) as u64,
            ignored: row.ignored.max(0) as u64,
            dead_letter: row.dead_letter.max(0) as u64,
            oldest_ready_age: row
                .oldest_ready_seconds
                .map(|seconds| Duration::from_secs_f64(seconds.max(0.0))),
        })
    }

    async fn purge_inbound_events(
        &self,
        policy: InboundRetentionPolicy,
    ) -> AppResult<InboundEventRetention> {
        let batch_size = policy.batch_size.clamp(1, 10_000);
        let completed_deleted = delete_retained(
            &self.pool,
            InboundEventStatus::Completed,
            policy.completed_for,
            batch_size,
        )
        .await?;
        let ignored_deleted = delete_retained(
            &self.pool,
            InboundEventStatus::Ignored,
            policy.ignored_for,
            batch_size,
        )
        .await?;
        let dead_letters_deleted = delete_retained(
            &self.pool,
            InboundEventStatus::DeadLetter,
            policy.dead_letters_for,
            batch_size,
        )
        .await?;
        Ok(InboundEventRetention {
            completed_deleted,
            ignored_deleted,
            dead_letters_deleted,
        })
    }
}

pub(crate) async fn complete_inbound_event_on(
    tx: &mut Transaction<'_, Postgres>,
    fence: &ExecutionLease<InboundEventId>,
) -> AppResult<bool> {
    let result = sqlx::query(
        r#"UPDATE inbound_events
              SET status = 'completed', processed_at = CURRENT_TIMESTAMP,
                  execution_id = NULL, owner_worker_id = NULL,
                  locked_at = NULL, lock_expires_at = NULL,
                  updated_at = CURRENT_TIMESTAMP
            WHERE id = $1 AND status = 'processing'
              AND execution_id = $2 AND owner_worker_id = $3
              AND lock_expires_at > CURRENT_TIMESTAMP"#,
    )
    .bind(fence.row.as_uuid())
    .bind(fence.execution.as_uuid())
    .bind(fence.owner.as_uuid())
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(result.rows_affected() == 1)
}

async fn settle_failure(
    persistence: &PostgresPersistence,
    failure: InboundEventFailure<'_>,
    terminal: bool,
) -> AppResult<InboundEventTransition> {
    let sql = if terminal {
        r#"UPDATE inbound_events
              SET status = 'dead_letter',
                  attempt_count = LEAST(attempt_count + 1, max_attempts),
                  last_error_class = $4, last_error_detail = $5,
                  processed_at = CURRENT_TIMESTAMP,
                  execution_id = NULL, owner_worker_id = NULL,
                  locked_at = NULL, lock_expires_at = NULL,
                  updated_at = CURRENT_TIMESTAMP
            WHERE id = $1 AND status = 'processing'
              AND execution_id = $2 AND owner_worker_id = $3
              AND lock_expires_at > CURRENT_TIMESTAMP
            RETURNING status"#
            .to_string()
    } else {
        let delay = retry_delay_sql("attempt_count + 1");
        format!(
            r#"UPDATE inbound_events
                  SET status = CASE WHEN attempt_count + 1 >= max_attempts
                                    THEN 'dead_letter' ELSE 'retryable' END,
                      attempt_count = attempt_count + 1,
                      available_at = CURRENT_TIMESTAMP + {delay},
                      last_error_class = $4, last_error_detail = $5,
                      processed_at = CASE WHEN attempt_count + 1 >= max_attempts
                                          THEN CURRENT_TIMESTAMP ELSE NULL END,
                      execution_id = NULL, owner_worker_id = NULL,
                      locked_at = NULL, lock_expires_at = NULL,
                      updated_at = CURRENT_TIMESTAMP
                WHERE id = $1 AND status = 'processing'
                  AND execution_id = $2 AND owner_worker_id = $3
                  AND lock_expires_at > CURRENT_TIMESTAMP
                RETURNING status"#,
        )
    };
    let status: Option<String> = sqlx::query_scalar(&sql)
        .bind(failure.fence.row.as_uuid())
        .bind(failure.fence.execution.as_uuid())
        .bind(failure.fence.owner.as_uuid())
        .bind(failure.class.as_str())
        .bind(failure.detail.as_str())
        .fetch_optional(&persistence.pool)
        .await
        .map_err(AppError::from)?;
    let Some(status) = status else {
        return Ok(InboundEventTransition::LeaseLost);
    };
    let status = InboundEventStatus::from_str(&status)
        .map_err(|error| AppError::Internal(error.to_string()))?;
    Ok(InboundEventTransition::Applied(status))
}

fn transition(applied: bool, status: InboundEventStatus) -> InboundEventTransition {
    if applied {
        InboundEventTransition::Applied(status)
    } else {
        InboundEventTransition::LeaseLost
    }
}

fn retry_delay_sql(attempts: &str) -> String {
    format!(
        "make_interval(secs => LEAST(600, 2 * power(2, LEAST({attempts}, 16)))::double precision \
         * (0.8 + random() * 0.4))"
    )
}

async fn delete_retained(
    pool: &sqlx::PgPool,
    status: InboundEventStatus,
    retention: Duration,
    batch_size: i64,
) -> AppResult<u64> {
    let seconds = retention.as_secs().min(i64::MAX as u64) as f64;
    let result = sqlx::query(
        r#"WITH expired AS (
               SELECT id
                 FROM inbound_events
                WHERE status = $1
                  AND processed_at < CURRENT_TIMESTAMP
                      - make_interval(secs => $2::double precision)
                ORDER BY processed_at, id
                LIMIT $3
           )
           DELETE FROM inbound_events AS event
            USING expired
            WHERE event.id = expired.id"#,
    )
    .bind(status.as_str())
    .bind(seconds)
    .bind(batch_size)
    .execute(pool)
    .await
    .map_err(AppError::from)?;
    Ok(result.rows_affected())
}
