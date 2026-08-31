//! The `email_outbox` row, the columns every read of it selects, and the retry/backoff clause
//! shared by the paths that end a delivery attempt.

use chrono::{DateTime, Utc};
use serde_json::Value;
use std::str::FromStr;
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::outbox::{OutboxEntry, OutboxStatus},
};

#[derive(sqlx::FromRow, Debug)]
pub struct OutboxEmail {
    pub id: Uuid,
    pub payload: Value,
    /// The stable key this send was queued under. The delivered Message-ID is derived from it, so
    /// every attempt at the same logical send goes out under the same Message-ID — and so a caller
    /// that queued the row can predict that Message-ID without waiting for delivery.
    pub idempotency_key: String,
}
/// One `email_outbox` row as stored, before its `TEXT` status is parsed.
#[derive(sqlx::FromRow, Debug)]
pub struct OutboxEntryDb {
    pub id: Uuid,
    pub company_id: Uuid,
    pub channel_id: Option<Uuid>,
    pub task_id: Option<Uuid>,
    pub status: String,
    pub idempotency_key: String,
    pub payload: Value,
    pub retry_count: i32,
    pub last_error: Option<String>,
    pub provider_message_id: Option<String>,
    pub available_at: DateTime<Utc>,
    pub sent_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TryFrom<OutboxEntryDb> for OutboxEntry {
    type Error = AppError;

    fn try_from(db: OutboxEntryDb) -> AppResult<Self> {
        let status =
            OutboxStatus::from_str(&db.status).map_err(|e| AppError::Internal(e.to_string()))?;

        Ok(OutboxEntry {
            id: db.id,
            company_id: db.company_id,
            channel_id: db.channel_id,
            task_id: db.task_id,
            status,
            idempotency_key: db.idempotency_key,
            payload: db.payload,
            retry_count: db.retry_count,
            last_error: db.last_error,
            provider_message_id: db.provider_message_id,
            available_at: db.available_at,
            sent_at: db.sent_at,
            created_at: db.created_at,
            updated_at: db.updated_at,
        })
    }
}

/// The columns [`OutboxEntryDb`] reads, named once so the list and the single-row read cannot
/// select different shapes.
pub(crate) const OUTBOX_COLUMNS: &str = r#"id, company_id, channel_id, task_id, status, idempotency_key, payload,
              retry_count, last_error, provider_message_id, available_at, sent_at,
              created_at, updated_at"#;

/// The most delivery attempts one outbox row gets before it is dead-lettered.
pub(crate) const OUTBOX_MAX_ATTEMPTS: i32 = 5;

/// The `SET` clause that ends one delivery attempt: count it, back off, and dead-letter once the
/// attempts run out. `error_sql` is however that statement names the error — a bind parameter or a
/// literal.
///
/// Shared so an attempt that failed outright and one that stranded its lease age at the same rate.
/// A row that only ever expires must still reach `failed`; while expiry was uncounted, the poller
/// redelivered such a row every lease period forever.
pub(crate) fn outbox_attempt_failed_set(error_sql: &str) -> String {
    format!(
        r#"SET status = CASE WHEN retry_count + 1 >= {OUTBOX_MAX_ATTEMPTS} THEN 'failed' ELSE 'pending' END,
                   retry_count = retry_count + 1, last_error = {error_sql},
                   available_at = CURRENT_TIMESTAMP
                       + make_interval(secs => power(2, LEAST(retry_count + 1, 8))::double precision),
                   worker_id = NULL, locked_at = NULL, lock_expires_at = NULL,
                   updated_at = CURRENT_TIMESTAMP"#
    )
}
