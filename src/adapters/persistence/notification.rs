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

#[derive(Debug)]
struct SourceSnapshot {
    channel_id: Uuid,
    thread_id: Option<Uuid>,
    task_id: Option<Uuid>,
    correlation_id: Option<Uuid>,
    responsible_principal_id: Option<Uuid>,
    lifecycle: SourceLifecycle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceLifecycle {
    Active,
    Resolved,
    Withdrawn,
}

#[derive(Debug, sqlx::FromRow)]
struct TaskSourceRow {
    channel_id: Uuid,
    thread_id: Option<Uuid>,
    correlation_id: Uuid,
    status: String,
    owner_principal_id: Option<Uuid>,
    owner_principal_kind: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct HandoffSourceRow {
    channel_id: Uuid,
    thread_id: Option<Uuid>,
    correlation_id: Option<Uuid>,
    status: String,
    responsible_principal_id: Option<Uuid>,
}

#[derive(Debug, sqlx::FromRow)]
struct ReviewSourceRow {
    channel_id: Uuid,
    thread_id: Uuid,
    task_id: Option<Uuid>,
    correlation_id: Option<Uuid>,
    status: String,
    reviewer_principal_id: Uuid,
}

#[derive(Debug, sqlx::FromRow)]
struct DelegationSourceRow {
    channel_id: Uuid,
    thread_id: Option<Uuid>,
    task_id: Uuid,
    correlation_id: Uuid,
    status: String,
    owner_principal_id: Option<Uuid>,
    owner_principal_kind: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct DeliverySourceRow {
    channel_id: Uuid,
    thread_id: Option<Uuid>,
    task_id: Option<Uuid>,
    correlation_id: Uuid,
    status: String,
    owner_principal_id: Option<Uuid>,
    owner_principal_kind: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct NotificationRow {
    id: Uuid,
    company_id: Uuid,
    company_label: String,
    channel_id: Uuid,
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
        let Some(event) = locked_event(&mut tx, commit.fence).await? else {
            return Ok((
                NotificationProjectionCommitOutcome::LeaseLost,
                NotificationProjectionResult::default(),
            ));
        };
        let current = resolve_projection_on(&mut tx, &event).await?;
        if current != *commit.expected {
            release_stale_projection(&mut tx, commit.fence).await?;
            tx.commit().await.map_err(AppError::from)?;
            return Ok((
                NotificationProjectionCommitOutcome::Stale,
                NotificationProjectionResult::default(),
            ));
        }

        let result = apply_projection_on(&mut tx, &current, commit.email).await?;
        let completed = sqlx::query(
            r#"UPDATE notification_events
                  SET status = 'projected', projected_at = CURRENT_TIMESTAMP,
                      execution_id = NULL, owner_worker_id = NULL,
                      locked_at = NULL, lock_expires_at = NULL
                WHERE id = $1 AND status = 'processing'
                  AND execution_id = $2 AND owner_worker_id = $3"#,
        )
        .bind(commit.fence.row.as_uuid())
        .bind(commit.fence.execution.as_uuid())
        .bind(commit.fence.owner.as_uuid())
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        if completed.rows_affected() != 1 {
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
                      notification.channel_id, channel.name AS channel_label,
                      notification.source_kind, notification.source_id,
                      notification.action_kind, notification.source_generation,
                      notification.state, notification.read_at, notification.created_at,
                      notification.state_changed_at,
                      COUNT(*) FILTER (
                          WHERE notification.state = 'active' AND notification.read_at IS NULL
                      ) OVER () AS unread_count
                 FROM notifications AS notification
                 JOIN companies AS company ON company.id = notification.company_id
                 JOIN channels AS channel
                   ON channel.company_id = notification.company_id
                  AND channel.id = notification.channel_id
                WHERE notification.company_id = $1
                  AND notification.recipient_user_id = $2
                  AND notification.recipient_principal_id = $3
                  AND notification.channel_id = ANY($4)
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
                  AND notification.channel_id = ANY($5)
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
                  AND notification.channel_id = ANY($5)
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
        let projection = resolve_projection_on(&mut tx, &event).await?;
        let NotificationDisposition::Active(recipient) = projection.disposition else {
            let state = match projection.disposition {
                NotificationDisposition::Resolved => "resolved",
                NotificationDisposition::Withdrawn => "withdrawn",
                NotificationDisposition::Active(_) => unreachable!(),
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

async fn locked_event(
    tx: &mut Transaction<'_, Postgres>,
    fence: &ExecutionLease<NotificationEventId>,
) -> AppResult<Option<NotificationEvent>> {
    let row = sqlx::query_as::<_, NotificationEventDb>(&format!(
        r#"SELECT {EVENT_COLUMNS} FROM notification_events AS event
            WHERE event.id = $1 AND event.status = 'processing'
              AND event.execution_id = $2 AND event.owner_worker_id = $3
              AND event.lock_expires_at > CURRENT_TIMESTAMP
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

async fn release_stale_projection(
    tx: &mut Transaction<'_, Postgres>,
    fence: &ExecutionLease<NotificationEventId>,
) -> AppResult<()> {
    sqlx::query(
        r#"UPDATE notification_events
              SET status = 'pending', available_at = CURRENT_TIMESTAMP,
                  execution_id = NULL, owner_worker_id = NULL,
                  locked_at = NULL, lock_expires_at = NULL
            WHERE id = $1 AND status = 'processing'
              AND execution_id = $2 AND owner_worker_id = $3"#,
    )
    .bind(fence.row.as_uuid())
    .bind(fence.execution.as_uuid())
    .bind(fence.owner.as_uuid())
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(())
}

async fn resolve_projection_on(
    connection: &mut PgConnection,
    event: &NotificationEvent,
) -> AppResult<NotificationProjection> {
    let Some(source) = source_snapshot(connection, event).await? else {
        return Ok(NotificationProjection {
            event: event.clone(),
            disposition: NotificationDisposition::Withdrawn,
        });
    };
    let newer_exists: bool = sqlx::query_scalar(
        r#"SELECT EXISTS (
               SELECT 1 FROM notification_events AS newer
                WHERE newer.company_id = $1 AND newer.source_kind = $2
                  AND newer.source_id = $3 AND newer.action_kind = $4
                  AND newer.source_generation > $5
           )"#,
    )
    .bind(event.company_id)
    .bind(event.source_kind.as_str())
    .bind(event.source_id)
    .bind(event.action_kind.as_str())
    .bind(i64::try_from(event.source_generation).unwrap_or(i64::MAX))
    .fetch_one(&mut *connection)
    .await
    .map_err(AppError::from)?;
    let disposition = if newer_exists {
        NotificationDisposition::Withdrawn
    } else {
        match source.lifecycle {
            SourceLifecycle::Resolved => NotificationDisposition::Resolved,
            SourceLifecycle::Withdrawn => NotificationDisposition::Withdrawn,
            SourceLifecycle::Active => match source.responsible_principal_id {
                Some(principal_id) => recipient_for_source(connection, event, source, principal_id)
                    .await?
                    .map(NotificationDisposition::Active)
                    .unwrap_or(NotificationDisposition::Withdrawn),
                None => NotificationDisposition::Withdrawn,
            },
        }
    };
    Ok(NotificationProjection {
        event: event.clone(),
        disposition,
    })
}

async fn source_snapshot(
    connection: &mut PgConnection,
    event: &NotificationEvent,
) -> AppResult<Option<SourceSnapshot>> {
    match (event.source_kind, event.action_kind) {
        (NotificationSourceKind::Task, NotificationActionKind::Assignment) => {
            let row = sqlx::query_as::<_, TaskSourceRow>(
                r#"SELECT channel_id, thread_id, correlation_id, status,
                              owner_principal_id, owner_principal_kind
                         FROM background_tasks AS task
                        WHERE task.company_id = $1 AND task.id = $2
                        FOR SHARE OF task"#,
            )
            .bind(event.company_id)
            .bind(event.source_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(AppError::from)?;
            Ok(row.map(|row| {
                let lifecycle = match row.status.as_str() {
                    "completed" | "stopped" | "dead_letter" => SourceLifecycle::Resolved,
                    _ if row.owner_principal_kind.as_deref() == Some("person") => {
                        SourceLifecycle::Active
                    }
                    _ => SourceLifecycle::Withdrawn,
                };
                SourceSnapshot {
                    channel_id: row.channel_id,
                    thread_id: row.thread_id,
                    task_id: Some(event.source_id),
                    correlation_id: Some(row.correlation_id),
                    responsible_principal_id: row.owner_principal_id,
                    lifecycle,
                }
            }))
        }
        (NotificationSourceKind::Handoff, NotificationActionKind::Assignment) => {
            let row = sqlx::query_as::<_, HandoffSourceRow>(
                r#"SELECT channel_id, thread_id, correlation_id, status,
                              responsible_principal_id
                         FROM manual_handoffs AS handoff
                        WHERE handoff.company_id = $1 AND handoff.id = $2
                        FOR SHARE OF handoff"#,
            )
            .bind(event.company_id)
            .bind(event.source_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(AppError::from)?;
            Ok(row.map(|row| SourceSnapshot {
                channel_id: row.channel_id,
                thread_id: row.thread_id,
                task_id: None,
                correlation_id: row.correlation_id,
                responsible_principal_id: row.responsible_principal_id,
                lifecycle: match row.status.as_str() {
                    "open" => SourceLifecycle::Active,
                    "resolved" => SourceLifecycle::Resolved,
                    _ => SourceLifecycle::Withdrawn,
                },
            }))
        }
        (NotificationSourceKind::ResponseReview, NotificationActionKind::ResponseReview) => {
            let row = sqlx::query_as::<_, ReviewSourceRow>(
                r#"SELECT draft.channel_id, draft.thread_id, draft.task_id,
                              task.correlation_id, review.status, review.reviewer_principal_id
                         FROM response_reviews AS review
                         JOIN response_drafts AS draft
                           ON (draft.company_id, draft.id, draft.version) =
                              (review.company_id, review.draft_id, review.draft_version)
                         LEFT JOIN background_tasks AS task
                           ON task.company_id = draft.company_id AND task.id = draft.task_id
                        WHERE review.company_id = $1 AND review.draft_id = $2
                        ORDER BY review.draft_version DESC LIMIT 1
                        FOR SHARE OF review"#,
            )
            .bind(event.company_id)
            .bind(event.source_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(AppError::from)?;
            Ok(row.map(|row| SourceSnapshot {
                channel_id: row.channel_id,
                thread_id: Some(row.thread_id),
                task_id: row.task_id,
                correlation_id: row.correlation_id,
                responsible_principal_id: Some(row.reviewer_principal_id),
                lifecycle: match row.status.as_str() {
                    "pending" => SourceLifecycle::Active,
                    "superseded" => SourceLifecycle::Withdrawn,
                    _ => SourceLifecycle::Resolved,
                },
            }))
        }
        (NotificationSourceKind::Delegation, NotificationActionKind::DelegationTimeout) => {
            let row = sqlx::query_as::<_, DelegationSourceRow>(
                r#"SELECT task.channel_id, task.thread_id, task.id AS task_id, task.correlation_id,
                              outreach.status, task.owner_principal_id, task.owner_principal_kind
                         FROM task_outreaches AS outreach
                         JOIN background_tasks AS task ON task.id = outreach.task_id
                        WHERE task.company_id = $1 AND outreach.id = $2
                        FOR SHARE OF outreach, task"#,
            )
            .bind(event.company_id)
            .bind(event.source_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(AppError::from)?;
            Ok(row.map(|row| SourceSnapshot {
                channel_id: row.channel_id,
                thread_id: row.thread_id,
                task_id: Some(row.task_id),
                correlation_id: Some(row.correlation_id),
                responsible_principal_id: row.owner_principal_id,
                lifecycle: if row.status == "timeout_pending_approval" {
                    if row.owner_principal_kind.as_deref() == Some("person") {
                        SourceLifecycle::Active
                    } else {
                        SourceLifecycle::Withdrawn
                    }
                } else {
                    SourceLifecycle::Resolved
                },
            }))
        }
        (NotificationSourceKind::Task, NotificationActionKind::TaskFailure) => {
            let row = sqlx::query_as::<_, TaskSourceRow>(
                r#"SELECT channel_id, thread_id, correlation_id, status,
                              owner_principal_id, owner_principal_kind
                         FROM background_tasks AS task
                        WHERE task.company_id = $1 AND task.id = $2
                        FOR SHARE OF task"#,
            )
            .bind(event.company_id)
            .bind(event.source_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(AppError::from)?;
            Ok(row.map(|row| SourceSnapshot {
                channel_id: row.channel_id,
                thread_id: row.thread_id,
                task_id: Some(event.source_id),
                correlation_id: Some(row.correlation_id),
                responsible_principal_id: row.owner_principal_id,
                lifecycle: if row.status == "dead_letter" {
                    if row.owner_principal_kind.as_deref() == Some("person") {
                        SourceLifecycle::Active
                    } else {
                        SourceLifecycle::Withdrawn
                    }
                } else {
                    SourceLifecycle::Resolved
                },
            }))
        }
        (NotificationSourceKind::Delivery, NotificationActionKind::DeliveryFailure) => {
            let row = sqlx::query_as::<_, DeliverySourceRow>(
                r#"SELECT delivery.channel_id, task.thread_id, delivery.task_id,
                          delivery.correlation_id, delivery.status,
                          task.owner_principal_id, task.owner_principal_kind
                     FROM message_deliveries AS delivery
                     JOIN background_tasks AS task
                       ON task.company_id = delivery.company_id AND task.id = delivery.task_id
                    WHERE delivery.company_id = $1 AND delivery.id = $2
                    FOR SHARE OF delivery, task"#,
            )
            .bind(event.company_id)
            .bind(event.source_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(AppError::from)?;
            Ok(row.map(|row| SourceSnapshot {
                channel_id: row.channel_id,
                thread_id: row.thread_id,
                task_id: row.task_id,
                correlation_id: Some(row.correlation_id),
                responsible_principal_id: row.owner_principal_id,
                lifecycle: if matches!(row.status.as_str(), "dead_letter" | "outcome_unknown") {
                    if row.owner_principal_kind.as_deref() == Some("person") {
                        SourceLifecycle::Active
                    } else {
                        SourceLifecycle::Withdrawn
                    }
                } else {
                    SourceLifecycle::Resolved
                },
            }))
        }
        _ => Err(AppError::Internal(format!(
            "Invalid notification source/action pair {}/{}",
            event.source_kind, event.action_kind
        ))),
    }
}

async fn recipient_for_source(
    connection: &mut PgConnection,
    event: &NotificationEvent,
    source: SourceSnapshot,
    principal_id: Uuid,
) -> AppResult<Option<NotificationRecipient>> {
    #[derive(sqlx::FromRow)]
    struct RecipientRow {
        user_id: Uuid,
        email: String,
        company_label: String,
        channel_label: String,
        assignment_email_enabled: bool,
        response_review_email_enabled: bool,
        delegation_timeout_email_enabled: bool,
        task_failure_email_enabled: bool,
        delivery_failure_email_enabled: bool,
    }
    let row = sqlx::query_as::<_, RecipientRow>(
        r#"SELECT principal.user_id, account.email,
                  company.name AS company_label, channel.name AS channel_label,
                  COALESCE(preference.assignment_email_enabled, TRUE)
                      AS assignment_email_enabled,
                  COALESCE(preference.response_review_email_enabled, TRUE)
                      AS response_review_email_enabled,
                  COALESCE(preference.delegation_timeout_email_enabled, TRUE)
                      AS delegation_timeout_email_enabled,
                  COALESCE(preference.task_failure_email_enabled, TRUE)
                      AS task_failure_email_enabled,
                  COALESCE(preference.delivery_failure_email_enabled, TRUE)
                      AS delivery_failure_email_enabled
             FROM principals AS principal
             JOIN users AS account ON account.id = principal.user_id
             JOIN company_members AS member
               ON member.company_id = principal.company_id
              AND member.user_id = principal.user_id
             JOIN companies AS company ON company.id = principal.company_id
             JOIN channels AS channel
               ON channel.company_id = principal.company_id AND channel.id = $3
             LEFT JOIN user_notification_preferences AS preference
               ON preference.user_id = account.id
            WHERE principal.company_id = $1 AND principal.id = $2
              AND principal.kind = 'person'
              AND notification_principal_can_view($1, $3, $2)
            FOR SHARE OF principal, account, member, company, channel"#,
    )
    .bind(event.company_id)
    .bind(principal_id)
    .bind(source.channel_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(AppError::from)?;
    Ok(row.map(|row| {
        let preferences = NotificationPreferences {
            assignment_email_enabled: row.assignment_email_enabled,
            response_review_email_enabled: row.response_review_email_enabled,
            delegation_timeout_email_enabled: row.delegation_timeout_email_enabled,
            task_failure_email_enabled: row.task_failure_email_enabled,
            delivery_failure_email_enabled: row.delivery_failure_email_enabled,
        };
        NotificationRecipient {
            principal_id: PrincipalId::new(principal_id),
            user_id: row.user_id,
            email: EmailAddress::from(row.email),
            company_label: row.company_label,
            channel_label: row.channel_label,
            channel_id: source.channel_id,
            thread_id: source.thread_id,
            task_id: source.task_id,
            correlation_id: source
                .correlation_id
                .map(Into::into)
                .unwrap_or_else(|| event.id.as_uuid().into()),
            email_enabled: event.action_kind.email_preference(preferences),
        }
    }))
}

async fn apply_projection_on(
    tx: &mut Transaction<'_, Postgres>,
    projection: &NotificationProjection,
    email: Option<&NewStandaloneDelivery>,
) -> AppResult<NotificationProjectionResult> {
    let state = match projection.disposition {
        NotificationDisposition::Active(_) => NotificationState::Withdrawn,
        NotificationDisposition::Resolved => NotificationState::Resolved,
        NotificationDisposition::Withdrawn => NotificationState::Withdrawn,
    };
    let active_identity = match &projection.disposition {
        NotificationDisposition::Active(recipient) => Some(recipient.user_id),
        NotificationDisposition::Resolved | NotificationDisposition::Withdrawn => None,
    };
    let settled: Vec<DateTime<Utc>> = sqlx::query_scalar(
        r#"UPDATE notifications
              SET state = $5, state_changed_at = CURRENT_TIMESTAMP,
                  updated_at = CURRENT_TIMESTAMP
            WHERE company_id = $1 AND source_kind = $2 AND source_id = $3
              AND action_kind = $4 AND state = 'active'
              AND ($6::uuid IS NULL OR recipient_user_id <> $6 OR source_generation <> $7)
            RETURNING created_at"#,
    )
    .bind(projection.event.company_id)
    .bind(projection.event.source_kind.as_str())
    .bind(projection.event.source_id)
    .bind(projection.event.action_kind.as_str())
    .bind(state.as_str())
    .bind(active_identity)
    .bind(i64::try_from(projection.event.source_generation).unwrap_or(i64::MAX))
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)?;
    let mut result = NotificationProjectionResult {
        resolved: if state == NotificationState::Resolved {
            settled.len() as u64
        } else {
            0
        },
        withdrawn: if state == NotificationState::Withdrawn {
            settled.len() as u64
        } else {
            0
        },
        action_latency_seconds: settled
            .into_iter()
            .min()
            .map(|created| (Utc::now() - created).num_milliseconds().max(0) as f64 / 1_000.0),
        ..Default::default()
    };

    let NotificationDisposition::Active(recipient) = &projection.disposition else {
        return Ok(result);
    };
    let inserted_id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO notifications (
               id, company_id, recipient_user_id, recipient_principal_id, event_id,
               source_kind, source_id, action_kind, source_generation, channel_id
           ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
           ON CONFLICT ON CONSTRAINT notifications_identity_key DO UPDATE
             SET recipient_principal_id = EXCLUDED.recipient_principal_id,
                 event_id = EXCLUDED.event_id, channel_id = EXCLUDED.channel_id,
                 state = 'active', state_changed_at = CURRENT_TIMESTAMP,
                 updated_at = CURRENT_TIMESTAMP
           RETURNING id"#,
    )
    .bind(projection.event.notification_id.as_uuid())
    .bind(projection.event.company_id)
    .bind(recipient.user_id)
    .bind(recipient.principal_id.as_uuid())
    .bind(projection.event.id.as_uuid())
    .bind(projection.event.source_kind.as_str())
    .bind(projection.event.source_id)
    .bind(projection.event.action_kind.as_str())
    .bind(i64::try_from(projection.event.source_generation).unwrap_or(i64::MAX))
    .bind(recipient.channel_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::from)?;
    result.created = 1;

    if projection.should_email() {
        let delivery = email.ok_or_else(|| {
            AppError::Internal("Enabled notification email was not composed".into())
        })?;
        let creation =
            super::delivery::enqueue::insert_standalone_delivery_on(tx, delivery).await?;
        let delivery_id = match creation {
            DeliveryCreation::Created(id) | DeliveryCreation::Absorbed(id) => id,
        };
        sqlx::query(
            r#"UPDATE notifications SET email_delivery_id = $2, updated_at = CURRENT_TIMESTAMP
                WHERE id = $1 AND email_delivery_id IS NULL"#,
        )
        .bind(inserted_id)
        .bind(delivery_id.as_uuid())
        .execute(&mut **tx)
        .await
        .map_err(AppError::from)?;
    } else if email.is_some() {
        return Err(AppError::Internal(
            "A suppressed notification email was supplied for projection".into(),
        ));
    }
    Ok(result)
}

fn source_href(event: &NotificationEvent, recipient: &NotificationRecipient) -> String {
    match event.action_kind {
        NotificationActionKind::ResponseReview => format!(
            "/reviews/{}?company_id={}",
            event.source_id, event.company_id
        ),
        NotificationActionKind::DeliveryFailure => {
            format!("/ui/deliveries?company_id={}", event.company_id)
        }
        NotificationActionKind::TaskFailure | NotificationActionKind::DelegationTimeout => {
            format!(
                "/ui/tasks?company_id={}&view=board&correlation_id={}",
                event.company_id, recipient.correlation_id
            )
        }
        NotificationActionKind::Assignment => match recipient.thread_id {
            Some(thread_id) => format!(
                "/ui?company_id={}&channel_id={}&thread_id={thread_id}",
                event.company_id, recipient.channel_id
            ),
            None => recipient.task_id.map_or_else(
                || format!("/ui/work?company_id={}", event.company_id),
                |task_id| {
                    format!(
                        "/ui/tasks?company_id={}&task_id={task_id}&view=list",
                        event.company_id
                    )
                },
            ),
        },
    }
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
