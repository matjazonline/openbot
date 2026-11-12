use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgConnection, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    application::notification::{
        ClaimedNotificationEvent, NotificationPersistence, NotificationProjectionCommit,
        NotificationProjectionCommitOutcome, NotificationProjectionFailure,
    },
    entities::{
        notification::{
            ActionableNotification, NotificationActionKind, NotificationCensus,
            NotificationDisposition, NotificationEvent, NotificationEventId, NotificationId,
            NotificationPage, NotificationPreferences, NotificationProjection,
            NotificationProjectionResult, NotificationRecipient, NotificationSourceKind,
            NotificationState,
        },
        transport::PrincipalId,
        value_objects::EmailAddress,
    },
    transport::{DeliveryCreation, ExecutionId, ExecutionLease, NewStandaloneDelivery, WorkerId},
};

#[derive(Debug, sqlx::FromRow)]
struct NotificationEventDb {
    id: Uuid,
    notification_id: Uuid,
    company_id: Uuid,
    source_kind: String,
    source_id: Uuid,
    action_kind: String,
    source_generation: i64,
    actor_principal_id: Option<Uuid>,
    occurred_at: DateTime<Utc>,
    execution_id: Option<Uuid>,
    owner_worker_id: Option<Uuid>,
    lock_expires_at: Option<DateTime<Utc>>,
}

impl NotificationEventDb {
    fn event(&self) -> AppResult<NotificationEvent> {
        Ok(NotificationEvent {
            id: NotificationEventId::new(self.id),
            notification_id: NotificationId::new(self.notification_id),
            company_id: self.company_id,
            source_kind: self.source_kind.parse().map_err(AppError::Internal)?,
            source_id: self.source_id,
            action_kind: self.action_kind.parse().map_err(AppError::Internal)?,
            source_generation: u64::try_from(self.source_generation)
                .map_err(|_| AppError::Internal("Invalid notification generation".into()))?,
            actor_principal_id: self.actor_principal_id.map(PrincipalId::new),
            occurred_at: self.occurred_at,
        })
    }

    fn claimed(self) -> AppResult<ClaimedNotificationEvent> {
        let execution = self.execution_id.ok_or_else(|| {
            AppError::Internal("Claimed notification event has no execution id".into())
        })?;
        let owner = self.owner_worker_id.ok_or_else(|| {
            AppError::Internal("Claimed notification event has no worker id".into())
        })?;
        let expires_at = self.lock_expires_at.ok_or_else(|| {
            AppError::Internal("Claimed notification event has no lease expiry".into())
        })?;
        let event = self.event()?;
        Ok(ClaimedNotificationEvent {
            lease: ExecutionLease {
                row: event.id,
                execution: ExecutionId::new(execution),
                owner: WorkerId::new(owner),
                expires_at,
            },
            event,
        })
    }
}

#[derive(Debug, sqlx::FromRow)]
struct NotificationRow {
    id: Uuid,
    company_id: Uuid,
    company_label: String,
    channel_id: Option<Uuid>,
    channel_label: String,
    source_kind: String,
    source_id: Uuid,
    action_kind: String,
    source_generation: i64,
    state: String,
    read_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    state_changed_at: DateTime<Utc>,
    unread_count: i64,
}

impl TryFrom<NotificationRow> for ActionableNotification {
    type Error = AppError;

    fn try_from(row: NotificationRow) -> AppResult<Self> {
        Ok(Self {
            id: NotificationId::new(row.id),
            company_id: row.company_id,
            company_label: row.company_label,
            channel_id: row.channel_id,
            channel_label: row.channel_label,
            source_kind: row.source_kind.parse().map_err(AppError::Internal)?,
            source_id: row.source_id,
            action_kind: row.action_kind.parse().map_err(AppError::Internal)?,
            source_generation: u64::try_from(row.source_generation)
                .map_err(|_| AppError::Internal("Invalid notification generation".into()))?,
            state: row.state.parse().map_err(AppError::Internal)?,
            read_at: row.read_at,
            created_at: row.created_at,
            state_changed_at: row.state_changed_at,
            href: format!("/ui/notifications/{}/open", row.id),
        })
    }
}

const EVENT_COLUMNS: &str = "event.id, event.notification_id, event.company_id, \
    event.source_kind, event.source_id, event.action_kind, event.source_generation, \
    event.actor_principal_id, event.occurred_at, event.execution_id, event.owner_worker_id, \
    event.lock_expires_at";

#[async_trait]
impl NotificationPersistence for PostgresPersistence {
    async fn claim_notification_events(
        &self,
        owner: WorkerId,
        lease_for: Duration,
        limit: i64,
    ) -> AppResult<Vec<ClaimedNotificationEvent>> {
        if limit <= 0 || limit > crate::application::notification::NOTIFICATION_EVENT_CLAIM_BATCH {
            return Err(AppError::BadRequest(
                "Notification claim limit is outside the supported range".into(),
            ));
        }
        let lease_seconds = i64::try_from(lease_for.as_secs())
            .map_err(|_| AppError::Internal("Notification lease duration is too large".into()))?;
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let query = format!(
            r#"WITH claimable AS (
                   SELECT id FROM notification_events
                    WHERE status = 'pending' AND available_at <= CURRENT_TIMESTAMP
                      AND attempt_count < max_attempts
                    ORDER BY available_at, occurred_at, id
                    FOR UPDATE SKIP LOCKED LIMIT $1
               )
               UPDATE notification_events AS event
                  SET status = 'processing', attempt_count = event.attempt_count + 1,
                      execution_id = gen_random_uuid(), owner_worker_id = $2,
                      locked_at = CURRENT_TIMESTAMP,
                      lock_expires_at = CURRENT_TIMESTAMP + $3 * INTERVAL '1 second',
                      last_error_class = NULL, last_error_detail = NULL
                 FROM claimable
                WHERE event.id = claimable.id
                RETURNING {EVENT_COLUMNS}"#
        );
        let rows = sqlx::query_as::<_, NotificationEventDb>(&query)
            .bind(limit)
            .bind(owner.as_uuid())
            .bind(lease_seconds)
            .fetch_all(&mut *tx)
            .await
            .map_err(AppError::from)?;
        tx.commit().await.map_err(AppError::from)?;
        rows.into_iter().map(NotificationEventDb::claimed).collect()
    }

    async fn resolve_notification_projection(
        &self,
        event: &NotificationEvent,
    ) -> AppResult<NotificationProjection> {
        let mut connection = self.pool.acquire().await.map_err(AppError::from)?;
        resolve_projection_on(&mut connection, event).await
    }

    async fn commit_notification_projection(
        &self,
        commit: NotificationProjectionCommit<'_>,
    ) -> AppResult<(
        NotificationProjectionCommitOutcome,
        NotificationProjectionResult,
    )> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let current = match control::owned_projection_on(&mut tx, &commit).await? {
            control::OwnedProjection::Current(current) => current,
            control::OwnedProjection::LeaseLost => {
                return Ok((
                    NotificationProjectionCommitOutcome::LeaseLost,
                    NotificationProjectionResult::default(),
                ));
            }
            control::OwnedProjection::Stale => {
                release_stale_projection(&mut tx, commit.fence).await?;
                tx.commit().await?;
                return Ok((
                    NotificationProjectionCommitOutcome::Stale,
                    NotificationProjectionResult::default(),
                ));
            }
        };
        let result = if current.disposition == NotificationDisposition::Control {
            use super::workflow::escalations::{ControlProjection, control_on};
            match control_on(&mut tx, &current.event, commit.escalation_ack_hours).await? {
                ControlProjection::Complete => NotificationProjectionResult::default(),
                ControlProjection::Deferred(deadline) => {
                    if !control::defer_control_on(&mut tx, commit.fence, deadline).await? {
                        return Ok((
                            NotificationProjectionCommitOutcome::LeaseLost,
                            NotificationProjectionResult::default(),
                        ));
                    }
                    tx.commit().await?;
                    return Ok((
                        NotificationProjectionCommitOutcome::Deferred,
                        NotificationProjectionResult::default(),
                    ));
                }
            }
        } else {
            apply_projection_on(&mut tx, &current, commit.email).await?
        };
        reviewer_notice::record_receipt_on(&mut tx, &current).await?;
        escalation_notice::record_receipt_on(&mut tx, &current).await?;
        if !control::finish_event_on(&mut tx, commit.fence).await? {
            return Ok((
                NotificationProjectionCommitOutcome::LeaseLost,
                NotificationProjectionResult::default(),
            ));
        }
        tx.commit().await.map_err(AppError::from)?;
        Ok((NotificationProjectionCommitOutcome::Applied, result))
    }

    async fn fail_notification_projection(
        &self,
        failure: NotificationProjectionFailure<'_>,
    ) -> AppResult<bool> {
        let detail = bounded_error_detail(failure.detail);
        let result = sqlx::query(
            r#"UPDATE notification_events
                  SET status = CASE WHEN attempt_count >= max_attempts
                                    THEN 'dead_letter' ELSE 'pending' END,
                      available_at = CURRENT_TIMESTAMP
                          + (2 * POWER(2, LEAST(attempt_count, 8))) * INTERVAL '1 second',
                      last_error_class = $4, last_error_detail = $5,
                      execution_id = NULL, owner_worker_id = NULL,
                      locked_at = NULL, lock_expires_at = NULL
                WHERE id = $1 AND status = 'processing'
                  AND execution_id = $2 AND owner_worker_id = $3"#,
        )
        .bind(failure.fence.row.as_uuid())
        .bind(failure.fence.execution.as_uuid())
        .bind(failure.fence.owner.as_uuid())
        .bind(failure.class)
        .bind(detail)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(result.rows_affected() == 1)
    }

    async fn reap_expired_notification_events(&self) -> AppResult<u64> {
        let result = sqlx::query(
            r#"UPDATE notification_events
                  SET status = CASE WHEN attempt_count >= max_attempts
                                    THEN 'dead_letter' ELSE 'pending' END,
                      available_at = CURRENT_TIMESTAMP,
                      last_error_class = 'lease_expired',
                      last_error_detail = 'Notification projection lease expired',
                      execution_id = NULL, owner_worker_id = NULL,
                      locked_at = NULL, lock_expires_at = NULL
                WHERE status = 'processing' AND lock_expires_at <= CURRENT_TIMESTAMP"#,
        )
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(result.rows_affected())
    }

    async fn notification_census(&self) -> AppResult<NotificationCensus> {
        let row: (Option<f64>, i64, i64) = sqlx::query_as(
            r#"SELECT EXTRACT(EPOCH FROM (
                          CURRENT_TIMESTAMP - MIN(created_at) FILTER (WHERE state = 'active')
                      ))::double precision,
                      (SELECT COUNT(*) FROM notification_events WHERE status = 'pending'),
                      (SELECT COUNT(*) FROM notification_events WHERE status = 'dead_letter')
                 FROM notifications"#,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(NotificationCensus {
            oldest_active_age_seconds: row.0,
            pending_events: nonnegative_u64(row.1, "pending notification events")?,
            dead_letter_events: nonnegative_u64(row.2, "dead-letter notification events")?,
        })
    }

    async fn list_notifications(
        &self,
        company_id: Uuid,
        user_id: Uuid,
        recipient_principal_id: PrincipalId,
        visible_channel_ids: &[Uuid],
    ) -> AppResult<NotificationPage> {
        let rows = sqlx::query_as::<_, NotificationRow>(
            r#"SELECT notification.id, notification.company_id, company.name AS company_label,
                      notification.channel_id, COALESCE(channel.name,'Company workflow') AS channel_label,
                      notification.source_kind, notification.source_id,
                      notification.action_kind, notification.source_generation,
                      notification.state, notification.read_at, notification.created_at,
                      notification.state_changed_at,
                      COUNT(*) FILTER (
                          WHERE notification.state = 'active' AND notification.read_at IS NULL
                      ) OVER () AS unread_count
                 FROM notifications AS notification
                 JOIN companies AS company ON company.id = notification.company_id
                 LEFT JOIN channels AS channel
                   ON channel.company_id = notification.company_id
                  AND channel.id = notification.channel_id
                WHERE notification.company_id = $1
                  AND notification.recipient_user_id = $2
                  AND notification.recipient_principal_id = $3
                  AND (notification.channel_id IS NULL OR notification.channel_id = ANY($4))
                  AND notification_principal_can_view(
                      notification.company_id, notification.channel_id,
                      notification.recipient_principal_id
                  )
                ORDER BY (notification.state = 'active') DESC,
                         notification.created_at DESC, notification.id DESC
                LIMIT $5"#,
        )
        .bind(company_id)
        .bind(user_id)
        .bind(recipient_principal_id.as_uuid())
        .bind(visible_channel_ids)
        .bind(crate::application::notification::NOTIFICATION_LIST_LIMIT)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;
        let unread_count = rows
            .first()
            .map(|row| nonnegative_u64(row.unread_count, "unread notifications"))
            .transpose()?
            .unwrap_or(0);
        Ok(NotificationPage {
            items: rows
                .into_iter()
                .map(TryInto::try_into)
                .collect::<AppResult<Vec<_>>>()?,
            unread_count,
        })
    }

    async fn notification_company_for_user(
        &self,
        user_id: Uuid,
        notification_id: NotificationId,
    ) -> AppResult<Option<Uuid>> {
        sqlx::query_scalar(
            r#"SELECT notification.company_id
                 FROM notifications AS notification
                 JOIN principals AS principal
                   ON principal.company_id = notification.company_id
                  AND principal.id = notification.recipient_principal_id
                WHERE notification.id = $1 AND notification.recipient_user_id = $2
                  AND principal.user_id = $2"#,
        )
        .bind(notification_id.as_uuid())
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)
    }

    async fn notification_href(
        &self,
        company_id: Uuid,
        user_id: Uuid,
        recipient_principal_id: PrincipalId,
        visible_channel_ids: &[Uuid],
        notification_id: NotificationId,
    ) -> AppResult<String> {
        let event_row = sqlx::query_as::<_, NotificationEventDb>(&format!(
            r#"SELECT {EVENT_COLUMNS}
                 FROM notifications AS notification
                 JOIN notification_events AS event ON event.id = notification.event_id
                WHERE notification.id = $1 AND notification.company_id = $2
                  AND notification.recipient_user_id = $3
                  AND notification.recipient_principal_id = $4
                  AND (notification.channel_id IS NULL OR notification.channel_id = ANY($5))
                  AND notification.state = 'active'"#
        ))
        .bind(notification_id.as_uuid())
        .bind(company_id)
        .bind(user_id)
        .bind(recipient_principal_id.as_uuid())
        .bind(visible_channel_ids)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::NotFound("Notification not found.".into()))?;
        let event = event_row.event()?;
        let projection = self.resolve_notification_projection(&event).await?;
        let NotificationDisposition::Active(recipient) = projection.disposition else {
            return Err(AppError::NotFound(
                "This notification is no longer actionable.".into(),
            ));
        };
        if recipient.user_id != user_id || recipient.principal_id != recipient_principal_id {
            return Err(AppError::NotFound("Notification not found.".into()));
        }
        Ok(source_href(&event, &recipient))
    }

    async fn open_notification(
        &self,
        company_id: Uuid,
        user_id: Uuid,
        recipient_principal_id: PrincipalId,
        visible_channel_ids: &[Uuid],
        notification_id: NotificationId,
    ) -> AppResult<String> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let event_row = sqlx::query_as::<_, NotificationEventDb>(&format!(
            r#"SELECT {EVENT_COLUMNS}
                 FROM notifications AS notification
                 JOIN notification_events AS event ON event.id = notification.event_id
                WHERE notification.id = $1 AND notification.company_id = $2
                  AND notification.recipient_user_id = $3
                  AND notification.recipient_principal_id = $4
                  AND (notification.channel_id IS NULL OR notification.channel_id = ANY($5))
                  AND notification.state = 'active'"#
        ))
        .bind(notification_id.as_uuid())
        .bind(company_id)
        .bind(user_id)
        .bind(recipient_principal_id.as_uuid())
        .bind(visible_channel_ids)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::NotFound("Notification not found.".into()))?;
        let event = event_row.event()?;
        let projection_lock::LockedProjection::Current(projection) = projection_lock::lock_on(
            &mut tx,
            &event,
            projection_lock::ProjectionPurpose::Open { user_id },
        )
        .await?
        else {
            return Err(AppError::Conflict(
                "Notification authority changed; reopen it".into(),
            ));
        };
        let found:Option<Uuid>=sqlx::query_scalar("SELECT id FROM notifications WHERE company_id=$1 AND id=$2 AND recipient_user_id=$3 AND recipient_principal_id=$4 AND state='active' FOR UPDATE")
            .bind(company_id).bind(notification_id.as_uuid()).bind(user_id).bind(recipient_principal_id.as_uuid()).fetch_optional(&mut *tx).await?;
        found.ok_or_else(|| AppError::NotFound("Notification not found.".into()))?;
        let projection = *projection;
        let NotificationDisposition::Active(recipient) = projection.disposition else {
            let state = match projection.disposition {
                NotificationDisposition::Resolved => "resolved",
                NotificationDisposition::Withdrawn => "withdrawn",
                NotificationDisposition::Active(_) => unreachable!(),
                NotificationDisposition::Control => {
                    return Err(AppError::NotFound("notification".into()));
                }
            };
            sqlx::query(
                r#"UPDATE notifications SET state = $2, state_changed_at = CURRENT_TIMESTAMP,
                          updated_at = CURRENT_TIMESTAMP
                    WHERE id = $1 AND state = 'active'"#,
            )
            .bind(notification_id.as_uuid())
            .bind(state)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
            tx.commit().await.map_err(AppError::from)?;
            return Err(AppError::NotFound(
                "This notification is no longer actionable.".into(),
            ));
        };
        if recipient.user_id != user_id || recipient.principal_id != recipient_principal_id {
            return Err(AppError::NotFound("Notification not found.".into()));
        }
        sqlx::query(
            r#"UPDATE notifications
                  SET read_at = COALESCE(read_at, CURRENT_TIMESTAMP), updated_at = CURRENT_TIMESTAMP
                WHERE id = $1"#,
        )
        .bind(notification_id.as_uuid())
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        tx.commit().await.map_err(AppError::from)?;
        Ok(source_href(&event, &recipient))
    }
}

async fn read_claimed_event(
    db: &mut PgConnection,
    fence: &ExecutionLease<NotificationEventId>,
) -> AppResult<Option<NotificationEvent>> {
    let row=sqlx::query_as::<_,NotificationEventDb>(&format!(
        "SELECT {EVENT_COLUMNS} FROM notification_events AS event WHERE event.id=$1 AND event.status='processing' \
         AND event.execution_id=$2 AND event.owner_worker_id=$3 AND event.lock_expires_at>clock_timestamp()"))
        .bind(fence.row.as_uuid()).bind(fence.execution.as_uuid()).bind(fence.owner.as_uuid()).fetch_optional(db).await?;
    row.map(|row| row.event()).transpose()
}

async fn locked_event(
    tx: &mut Transaction<'_, Postgres>,
    fence: &ExecutionLease<NotificationEventId>,
) -> AppResult<Option<NotificationEvent>> {
    let row = sqlx::query_as::<_, NotificationEventDb>(&format!(
        r#"SELECT {EVENT_COLUMNS} FROM notification_events AS event
            WHERE event.id = $1 AND event.status = 'processing'
              AND event.execution_id = $2 AND event.owner_worker_id = $3
              AND event.lock_expires_at > clock_timestamp()
            FOR UPDATE"#
    ))
    .bind(fence.row.as_uuid())
    .bind(fence.execution.as_uuid())
    .bind(fence.owner.as_uuid())
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?;
    row.map(|row| row.event()).transpose()
}

/// Hand a projection whose source moved under it back to the queue.
///
/// A stale projection is legitimate work, but the claim already charged it an attempt, so it
/// takes the fail path's rules: dead-lettered once its attempts are spent, otherwise backed off.
/// Without them a source that keeps changing hot-loops, and the last attempt coming back stale
/// leaves a `pending` row that can never be claimed again.
async fn release_stale_projection(
    tx: &mut Transaction<'_, Postgres>,
    fence: &ExecutionLease<NotificationEventId>,
) -> AppResult<()> {
    let released: Option<(String, i32)> = sqlx::query_as(
        r#"UPDATE notification_events
              SET status = CASE WHEN attempt_count >= max_attempts
                                THEN 'dead_letter' ELSE 'pending' END,
                  available_at = CURRENT_TIMESTAMP
                      + (2 * POWER(2, LEAST(attempt_count, 8))) * INTERVAL '1 second',
                  last_error_class = 'stale_projection',
                  last_error_detail = 'Notification projection changed before commit',
                  execution_id = NULL, owner_worker_id = NULL,
                  locked_at = NULL, lock_expires_at = NULL
            WHERE id = $1 AND status = 'processing'
              AND execution_id = $2 AND owner_worker_id = $3
        RETURNING status, attempt_count"#,
    )
    .bind(fence.row.as_uuid())
    .bind(fence.execution.as_uuid())
    .bind(fence.owner.as_uuid())
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?;
    if let Some((status, attempts)) = released
        && status == "dead_letter"
    {
        tracing::warn!(
            event_id = %fence.row,
            attempts,
            "A notification event went stale on its last attempt and was dead-lettered"
        );
    }
    Ok(())
}

fn bounded_error_detail(value: &str) -> String {
    let mut end = value.len().min(512);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

fn nonnegative_u64(value: i64, label: &str) -> AppResult<u64> {
    u64::try_from(value).map_err(|_| AppError::Internal(format!("Invalid {label} count")))
}

#[cfg(test)]
#[path = "notification_tests.rs"]
mod tests;

mod control;
mod escalation_failures;
mod escalation_notice;
pub(crate) use escalation_failures::refresh_person_escalations_on;
mod projection;
mod projection_lock;
mod reviewer_notice;
pub(crate) use escalation_notice::close_assignment_on as close_escalation_assignment_on;
use projection::{apply_projection_on, resolve_projection_on, source_href};
