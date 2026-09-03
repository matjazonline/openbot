//! `message_deliveries` and `message_delivery_parts`: the rows, the column lists every read
//! selects, and the one `SET` clause that ends a delivery attempt.
//!
//! The queue protocol is in [`queue`], creation in [`enqueue`], and the reader's projections in
//! [`read`]. Everything shared between them lives here so a column added to the table is one edit
//! per statement rather than one per file.

pub mod enqueue;
pub mod queue;
pub mod read;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        correlation::CorrelationId,
        message::CanonicalMessageId,
        transport::{
            ChannelBindingId, DeliveryId, DeliveryPartId, DeliveryPartStatus, DeliveryPurpose,
            DeliveryStatus, ExternalDestination, ExternalMessageKey, FailureClass, TransportKind,
        },
    },
    transport::{
        DeliveryAttribution, DeliveryBackoff, DeliveryKey, DeliveryRecord, ExecutionId,
        ExecutionLease, PartIndex, PartKey, RenderedPart, StoredPart, TransportPayload, WorkerId,
        delivery::ContentDigest,
    },
};

/// The columns every read of a delivery selects, named once so the claim, the paging read and the
/// single-row read cannot select different shapes.
pub(crate) const DELIVERY_COLUMNS: &str = r#"id, company_id, channel_id, message_id,
    source_binding_id, destination_binding_id, external_destination, task_id,
    depends_on_delivery_id, correlation_id, transport, purpose, idempotency_key, status,
    attempt_count, max_attempts, available_at, last_error_class, last_error_detail,
    execution_id, owner_worker_id, locked_at, lock_expires_at, delivered_at,
    created_at, updated_at"#;

/// The same list, qualified by a table alias.
///
/// The claim's `UPDATE ... FROM claimable` has two `id` columns in scope, so its `RETURNING` has to
/// say which. Derived from [`DELIVERY_COLUMNS`] rather than written twice: a column added to one
/// and not the other is a row struct that silently loses a field.
pub(crate) fn qualified_delivery_columns(alias: &str) -> String {
    DELIVERY_COLUMNS
        .split(',')
        .map(|column| format!("{alias}.{}", column.trim()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The same for one frozen part.
pub(crate) const PART_COLUMNS: &str = r#"id, company_id, delivery_id, part_index, part_key,
    payload, status, provider_message_key, content_digest, attempt_count,
    last_error_class, last_error_detail, request_started_at, delivered_at,
    created_at, updated_at"#;

/// The statuses a claim may take, as one SQL list.
///
/// Formatted from [`DeliveryStatus::is_claimable`] rather than typed out, so the predicate, the
/// partial index it has to match and the Rust rule are one fact. A status added to the enum and
/// not to the index would otherwise be claimable through a sequential scan and nobody would notice.
pub(crate) fn claimable_statuses_sql() -> String {
    sql_status_list(DeliveryStatus::ALL.iter().filter(|s| s.is_claimable()))
}

pub(crate) fn sql_status_list<'a>(statuses: impl Iterator<Item = &'a DeliveryStatus>) -> String {
    statuses
        .map(|status| format!("'{}'", status.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The backoff, as the SQL interval both attempt-ending paths add to `CURRENT_TIMESTAMP`.
///
/// `attempts_sql` is however the statement names the attempt count it is aging for -- usually
/// `attempt_count + 1`, since the attempt being charged is the one just spent.
///
/// The policy numbers come from [`DeliveryBackoff`], formatted in rather than retyped: one
/// definition, two statements. The jitter has to be applied here rather than in Rust because the
/// lease sweep ages a whole batch in one statement, and a single Rust-computed delay would land
/// every row in that batch on the same second -- which is the thundering herd the jitter exists to
/// break up.
pub(crate) fn retry_delay_sql(attempts_sql: &str) -> String {
    let DeliveryBackoff { base, cap, jitter } = DeliveryBackoff::DEFAULT;
    format!(
        "make_interval(secs => \
             LEAST({cap}, {base} * power(2, LEAST({attempts_sql}, 16)))::double precision \
             * ({low} + random() * {span}))",
        cap = cap.as_secs(),
        base = base.as_secs(),
        low = 1.0 - jitter,
        span = 2.0 * jitter,
    )
}

/// The `SET` clause that ends one delivery attempt: count it, back off, and dead-letter once the
/// attempts run out.
///
/// `class_sql` and `detail_sql` are however the statement names the failure -- bind parameters, or
/// literals for the sweep, which classifies every row it touches the same way.
///
/// Shared so an attempt that failed outright and one that stranded its lease age at the same rate.
/// A row that only ever expires must still reach a dead letter; while expiry was uncounted, such a
/// row was redelivered every lease period for ever.
pub(crate) fn attempt_failed_set(class_sql: &str, detail_sql: &str) -> String {
    let dead = DeliveryStatus::DeadLetter.as_str();
    let retryable = DeliveryStatus::Retryable.as_str();
    let delay = retry_delay_sql("attempt_count + 1");
    format!(
        r#"SET status = CASE WHEN attempt_count + 1 >= max_attempts
                             THEN '{dead}' ELSE '{retryable}' END,
               attempt_count = attempt_count + 1,
               last_error_class = {class_sql},
               last_error_detail = {detail_sql},
               available_at = CURRENT_TIMESTAMP + {delay},
               execution_id = NULL, owner_worker_id = NULL,
               locked_at = NULL, lock_expires_at = NULL,
               updated_at = CURRENT_TIMESTAMP"#
    )
}

/// One `message_deliveries` row as stored, before its `TEXT` vocabularies are parsed.
///
/// The field list is the `SELECT` list: every column [`DELIVERY_COLUMNS`] names appears here, so
/// the two cannot drift. A few are read by nothing in Rust yet -- the dependency link and the
/// claim timestamp are consumed entirely inside SQL -- and are kept rather than dropped, because a
/// row struct that omits a selected column is what silently loses the next one.
#[allow(dead_code)]
#[derive(sqlx::FromRow, Debug, Clone)]
pub(crate) struct DeliveryDb {
    pub id: Uuid,
    pub company_id: Option<Uuid>,
    pub channel_id: Option<Uuid>,
    pub message_id: Option<Uuid>,
    pub source_binding_id: Option<Uuid>,
    pub destination_binding_id: Option<Uuid>,
    pub external_destination: Option<String>,
    pub task_id: Option<Uuid>,
    pub depends_on_delivery_id: Option<Uuid>,
    pub correlation_id: Uuid,
    pub transport: String,
    pub purpose: String,
    pub idempotency_key: String,
    pub status: String,
    pub attempt_count: i32,
    pub max_attempts: i32,
    pub available_at: DateTime<Utc>,
    pub last_error_class: Option<String>,
    pub last_error_detail: Option<String>,
    pub execution_id: Option<Uuid>,
    pub owner_worker_id: Option<Uuid>,
    pub locked_at: Option<DateTime<Utc>>,
    pub lock_expires_at: Option<DateTime<Utc>>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl DeliveryDb {
    pub(crate) fn status(&self) -> AppResult<DeliveryStatus> {
        DeliveryStatus::from_str(&self.status).map_err(row_error("message_deliveries.status"))
    }

    pub(crate) fn transport(&self) -> AppResult<TransportKind> {
        TransportKind::from_str(&self.transport).map_err(row_error("message_deliveries.transport"))
    }

    pub(crate) fn purpose(&self) -> AppResult<DeliveryPurpose> {
        DeliveryPurpose::from_str(&self.purpose).map_err(row_error("message_deliveries.purpose"))
    }

    pub(crate) fn failure_class(&self) -> AppResult<Option<FailureClass>> {
        self.last_error_class
            .as_deref()
            .map(FailureClass::from_str)
            .transpose()
            .map_err(row_error("message_deliveries.last_error_class"))
    }

    /// The durable identity the worker and the sender read.
    pub(crate) fn record(&self) -> AppResult<DeliveryRecord> {
        let transport = self.transport()?;
        let external_destination = self
            .external_destination
            .as_deref()
            .map(|value| ExternalDestination::parse(transport, value))
            .transpose()
            .map_err(|error| {
                AppError::Internal(format!(
                    "Delivery {} stores a destination its {transport} interface cannot address: {error}",
                    self.id
                ))
            })?;
        let attribution = match (
            self.company_id,
            self.channel_id,
            self.message_id,
            self.source_binding_id,
            self.destination_binding_id,
        ) {
            (Some(company), Some(channel), Some(message), Some(source), Some(destination)) => {
                Some(DeliveryAttribution {
                    company_id: company,
                    channel_id: channel,
                    message_id: CanonicalMessageId::new(message),
                    source_binding_id: ChannelBindingId::new(source),
                    destination_binding_id: ChannelBindingId::new(destination),
                })
            }
            (None, None, None, None, None) => None,
            _ => {
                return Err(AppError::Internal(format!(
                    "Delivery {} has partial canonical attribution",
                    self.id
                )));
            }
        };
        Ok(DeliveryRecord {
            id: DeliveryId::new(self.id),
            attribution,
            external_destination,
            task_id: self.task_id,
            correlation_id: CorrelationId::from(self.correlation_id),
            transport,
            purpose: self.purpose()?,
            idempotency_key: DeliveryKey::parse(self.idempotency_key.clone())
                .map_err(row_error("message_deliveries.idempotency_key"))?,
            attempt_count: self.attempt_count,
            max_attempts: self.max_attempts,
        })
    }

    /// The lease this row carries, for a row a claim just leased.
    ///
    /// Every field or none: `message_deliveries_lease_check` makes a partial lease
    /// unrepresentable, so a missing one here means the row is not `sending` rather than that the
    /// lease is damaged.
    pub(crate) fn lease(&self) -> Option<ExecutionLease<DeliveryId>> {
        Some(ExecutionLease {
            row: DeliveryId::new(self.id),
            execution: ExecutionId::new(self.execution_id?),
            owner: WorkerId::new(self.owner_worker_id?),
            expires_at: self.lock_expires_at?,
        })
    }
}

/// One `message_delivery_parts` row as stored. See [`DeliveryDb`] on why unread columns stay.
#[allow(dead_code)]
#[derive(sqlx::FromRow, Debug, Clone)]
pub(crate) struct PartDb {
    pub id: Uuid,
    pub company_id: Option<Uuid>,
    pub delivery_id: Uuid,
    pub part_index: i32,
    pub part_key: String,
    pub payload: Value,
    pub status: String,
    pub provider_message_key: Option<String>,
    pub content_digest: String,
    pub attempt_count: i32,
    pub last_error_class: Option<String>,
    pub last_error_detail: Option<String>,
    pub request_started_at: Option<DateTime<Utc>>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl PartDb {
    pub(crate) fn status(&self) -> AppResult<DeliveryPartStatus> {
        DeliveryPartStatus::from_str(&self.status)
            .map_err(row_error("message_delivery_parts.status"))
    }

    pub(crate) fn failure_class(&self) -> AppResult<Option<FailureClass>> {
        self.last_error_class
            .as_deref()
            .map(FailureClass::from_str)
            .transpose()
            .map_err(row_error("message_delivery_parts.last_error_class"))
    }

    /// The frozen part, decoded.
    ///
    /// Every stored value is re-validated rather than trusted: the payload's bound, the key's
    /// bound and the index's range are all applied again here, because the row may have been
    /// written by an older deployment or by hand.
    pub(crate) fn stored(&self) -> AppResult<StoredPart> {
        let payload: TransportPayload =
            serde_json::from_value(self.payload.clone()).map_err(|error| {
                AppError::Internal(format!(
                    "Delivery part {} holds a payload this deployment cannot read: {error}",
                    self.id
                ))
            })?;
        let index = u16::try_from(self.part_index).map_err(|_| {
            AppError::Internal(format!(
                "Delivery part {} has an out-of-range index {}",
                self.id, self.part_index
            ))
        })?;
        Ok(StoredPart {
            id: DeliveryPartId::new(self.id),
            rendered: RenderedPart {
                index: PartIndex::new(index),
                key: PartKey::parse(self.part_key.clone())
                    .map_err(row_error("message_delivery_parts.part_key"))?,
                payload,
                digest: ContentDigest::parse(self.content_digest.clone())
                    .map_err(row_error("message_delivery_parts.content_digest"))?,
            },
            status: self.status()?,
            attempt_count: self.attempt_count,
            request_started_at: self.request_started_at,
            provider_message_key: self
                .provider_message_key
                .clone()
                .map(ExternalMessageKey::parse)
                .transpose()
                .map_err(row_error("message_delivery_parts.provider_message_key"))?,
        })
    }
}

/// A stored value the current types cannot read, named by its column.
///
/// `src/adapters/persistence/AGENTS.md`: persisted values are untrusted input, converted fallibly
/// with row context attached, never `expect`ed.
fn row_error<E: std::fmt::Display>(column: &'static str) -> impl Fn(E) -> AppError {
    move |error| {
        AppError::Internal(format!(
            "{column} holds a value this build cannot read: {error}"
        ))
    }
}
