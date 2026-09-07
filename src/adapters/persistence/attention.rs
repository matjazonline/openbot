use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    application::attention::{AttentionPersistence, validate_handoff},
    entities::{
        attention::{
            AttentionCursor, AttentionItem, AttentionPage, AttentionQuery, AttentionResponsibility,
            AttentionSourceCommand, AttentionSourceKind, AttentionView, HandoffResolution,
            NewManualHandoff, OperationalSummary, ResolveHandoffCommand,
        },
        correlation::CorrelationId,
        transport::PrincipalId,
    },
};

#[derive(sqlx::FromRow)]
struct AttentionRow {
    source_kind: String,
    source_id: Uuid,
    company_id: Uuid,
    channel_id: Uuid,
    thread_id: Option<Uuid>,
    task_id: Option<Uuid>,
    correlation_id: Option<Uuid>,
    state: String,
    responsible_principal_id: Option<Uuid>,
    responsibility_label: String,
    title: String,
    next_action: String,
    business_priority: String,
    due_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
    version: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    as_of: DateTime<Utc>,
    bounded_count: i64,
}

impl TryFrom<AttentionRow> for AttentionItem {
    type Error = AppError;

    fn try_from(row: AttentionRow) -> AppResult<Self> {
        Ok(Self {
            source_kind: row.source_kind.parse().map_err(AppError::Internal)?,
            source_id: row.source_id,
            company_id: row.company_id,
            channel_id: row.channel_id,
            thread_id: row.thread_id,
            task_id: row.task_id,
            correlation_id: row.correlation_id.map(CorrelationId::from),
            state: row.state,
            responsibility: row
                .responsible_principal_id
                .map(|id| AttentionResponsibility::Principal(PrincipalId::new(id)))
                .unwrap_or(AttentionResponsibility::ChannelTeam),
            responsibility_label: row.responsibility_label,
            title: row.title,
            next_action: row.next_action,
            priority: row.business_priority.parse().map_err(AppError::Internal)?,
            due_at: row.due_at,
            expires_at: row.expires_at,
            version: u64::try_from(row.version)
                .map_err(|_| AppError::Internal("Invalid attention source version".into()))?,
            created_at: row.created_at,
            updated_at: row.updated_at,
            href: None,
        })
    }
}

const ATTENTION_SQL: &str = r#"
WITH params AS (
    SELECT COALESCE($5::timestamptz, CURRENT_TIMESTAMP) AS as_of
), raw AS (
    SELECT 'task'::text AS source_kind, task.id AS source_id, task.company_id,
           task.channel_id, task.thread_id, task.id AS task_id, task.correlation_id,
           task.status AS state,
           CASE WHEN task.owner_principal_kind = 'person' THEN task.owner_principal_id END
               AS responsible_principal_id,
           CASE WHEN task.owner_principal_kind = 'person'
                THEN COALESCE(owner.display_label, 'Channel team') ELSE 'Channel team' END
               AS responsibility_label,
           task.task_type AS title,
           CASE task.status
             WHEN 'pending_approval' THEN 'Review the pending approval'
             WHEN 'dead_letter' THEN 'Decide how to recover the failed task'
             WHEN 'failed' THEN 'Decide how to recover the failed task'
             ELSE 'Complete or reassign the task'
           END AS next_action,
           task.business_priority, task.business_due_at AS due_at,
           task.wait_expires_at AS expires_at,
           task.attention_version AS version,
           task.created_at, task.updated_at
    FROM background_tasks AS task
    LEFT JOIN principals AS owner
      ON owner.company_id = task.company_id AND owner.id = task.owner_principal_id
    WHERE task.company_id = $1 AND task.channel_id = ANY($2)
      AND task.status IN ('pending', 'processing', 'pending_approval',
                          'waiting_for_third_party_reply', 'failed', 'dead_letter')
      AND (task.owner_principal_kind = 'person' OR task.owner_principal_id IS NULL
           OR task.status IN ('pending_approval', 'failed', 'dead_letter'))
      AND NOT EXISTS (
          SELECT 1 FROM response_reviews AS review
          WHERE review.company_id = task.company_id AND review.status = 'pending'
            AND EXISTS (
                SELECT 1 FROM response_drafts AS draft
                WHERE draft.company_id = task.company_id AND draft.id = review.draft_id
                  AND draft.version = review.draft_version AND draft.task_id = task.id
                  AND draft.status = 'pending_review'
            )
      )
      AND NOT EXISTS (
          SELECT 1 FROM task_outreaches AS outreach
          WHERE outreach.task_id = task.id AND outreach.status = 'timeout_pending_approval'
      )
      AND NOT EXISTS (
          SELECT 1 FROM message_deliveries AS delivery
          WHERE delivery.company_id = task.company_id AND delivery.task_id = task.id
            AND delivery.status IN ('outcome_unknown', 'dead_letter')
            AND delivery.last_error_class IS DISTINCT FROM 'superseded'
      )

    UNION ALL

    SELECT 'handoff', handoff.id, handoff.company_id, handoff.channel_id, handoff.thread_id,
           NULL::uuid, handoff.correlation_id, handoff.status,
           handoff.responsible_principal_id,
           COALESCE(responsible.display_label, 'Channel team'), handoff.title,
           handoff.next_action, handoff.business_priority, handoff.business_due_at,
           NULL::timestamptz, handoff.version, handoff.created_at, handoff.updated_at
    FROM manual_handoffs AS handoff
    LEFT JOIN principals AS responsible
      ON responsible.company_id = handoff.company_id
     AND responsible.id = handoff.responsible_principal_id
    WHERE handoff.company_id = $1 AND handoff.channel_id = ANY($2)
      AND handoff.status = 'open'

    UNION ALL

    SELECT 'response_review', review.draft_id, review.company_id, draft.channel_id,
           draft.thread_id, draft.task_id, task.correlation_id, review.status,
           review.reviewer_principal_id, reviewer.display_label,
           draft.subject, 'Review the proposed external response',
           COALESCE(task.business_priority, 'normal'), task.business_due_at,
           review.expires_at, draft.version::bigint, review.created_at, review.updated_at
    FROM response_reviews AS review
    JOIN response_drafts AS draft
      ON draft.company_id = review.company_id AND draft.id = review.draft_id
     AND draft.version = review.draft_version AND draft.status = 'pending_review'
    JOIN principals AS reviewer
      ON reviewer.company_id = review.company_id AND reviewer.id = review.reviewer_principal_id
    LEFT JOIN background_tasks AS task
      ON task.company_id = draft.company_id AND task.id = draft.task_id
    WHERE review.company_id = $1 AND draft.channel_id = ANY($2) AND review.status = 'pending'

    UNION ALL

    SELECT 'delegation_decision', outreach.id, task.company_id, task.channel_id,
           task.thread_id, task.id, task.correlation_id, outreach.status,
           CASE WHEN task.owner_principal_kind = 'person' THEN task.owner_principal_id END,
           CASE WHEN task.owner_principal_kind = 'person'
                THEN COALESCE(owner.display_label, 'Channel team') ELSE 'Channel team' END,
           outreach.subject, 'Review the delegation timeout', task.business_priority,
           CASE WHEN task.business_due_at IS NULL THEN outreach.expires_at
                ELSE LEAST(task.business_due_at, outreach.expires_at) END,
           outreach.expires_at, outreach.version, outreach.created_at, outreach.updated_at
    FROM task_outreaches AS outreach
    JOIN background_tasks AS task ON task.id = outreach.task_id
    LEFT JOIN principals AS owner
      ON owner.company_id = task.company_id AND owner.id = task.owner_principal_id
    WHERE task.company_id = $1 AND task.channel_id = ANY($2)
      AND outreach.status = 'timeout_pending_approval'

    UNION ALL

    SELECT 'delivery_failure', delivery.id, delivery.company_id, delivery.channel_id,
           task.thread_id, delivery.task_id, delivery.correlation_id, delivery.status,
           CASE WHEN task.owner_principal_kind = 'person' THEN task.owner_principal_id END,
           CASE WHEN task.owner_principal_kind = 'person'
                THEN COALESCE(owner.display_label, 'Channel team') ELSE 'Channel team' END,
           message.subject,
           CASE delivery.status WHEN 'outcome_unknown' THEN 'Resolve the unknown delivery outcome'
                ELSE 'Repair or dismiss the permanent delivery failure' END,
           COALESCE(task.business_priority, 'normal'), task.business_due_at,
           NULL::timestamptz, GREATEST(delivery.attempt_count, 1)::bigint,
           delivery.created_at, delivery.updated_at
    FROM message_deliveries AS delivery
    JOIN messages AS message
      ON message.company_id = delivery.company_id AND message.id = delivery.message_id
    LEFT JOIN background_tasks AS task
      ON task.company_id = delivery.company_id AND task.id = delivery.task_id
    LEFT JOIN principals AS owner
      ON owner.company_id = task.company_id AND owner.id = task.owner_principal_id
    WHERE delivery.company_id = $1 AND delivery.channel_id = ANY($2)
      AND delivery.status IN ('outcome_unknown', 'dead_letter')
      AND delivery.last_error_class IS DISTINCT FROM 'superseded'
      AND NOT EXISTS (
          SELECT 1 FROM task_outreach_targets AS target
          JOIN task_outreaches AS outreach ON outreach.id = target.outreach_id
          WHERE target.delivery_id = delivery.id
            AND outreach.status = 'timeout_pending_approval'
      )
), ranked AS (
    SELECT raw.*, params.as_of,
           CASE WHEN raw.due_at < params.as_of THEN 0
                WHEN raw.due_at <= params.as_of + interval '24 hours' THEN 1 ELSE 2 END AS due_rank,
           CASE raw.business_priority WHEN 'urgent' THEN 0 WHEN 'high' THEN 1 ELSE 2 END
               AS priority_rank
    FROM raw CROSS JOIN params
    WHERE ($4 = 'team_work'
           OR ($4 = 'my_work' AND raw.responsible_principal_id = $3)
           OR ($4 = 'unassigned' AND raw.responsible_principal_id IS NULL))
), after_cursor AS (
    SELECT * FROM ranked
    WHERE NOT $6 OR (due_rank, priority_rank, created_at, source_kind, source_id)
          > ($7, $8, $9, $10, $11)
    ORDER BY due_rank, priority_rank, created_at, source_kind, source_id
    LIMIT $12
), counted AS (
    SELECT after_cursor.*, COUNT(*) OVER () AS bounded_count FROM after_cursor
)
SELECT * FROM counted
ORDER BY due_rank, priority_rank, created_at, source_kind, source_id
LIMIT $13
"#;

fn item_href(item: &AttentionItem) -> String {
    match item.source_kind {
        AttentionSourceKind::ResponseReview => format!("/reviews/{}", item.source_id),
        AttentionSourceKind::DeliveryFailure => {
            format!("/ui/deliveries?company_id={}", item.company_id)
        }
        AttentionSourceKind::Task | AttentionSourceKind::DelegationDecision => item
            .correlation_id
            .map(|correlation| {
                format!(
                    "/ui/tasks?company_id={}&view=board&correlation_id={correlation}",
                    item.company_id
                )
            })
            .unwrap_or_else(|| format!("/ui/tasks?company_id={}", item.company_id)),
        AttentionSourceKind::Handoff => match item.thread_id {
            Some(thread_id) => format!(
                "/ui?company_id={}&channel_id={}&thread_id={thread_id}",
                item.company_id, item.channel_id
            ),
            None => format!(
                "/ui?company_id={}&channel_id={}",
                item.company_id, item.channel_id
            ),
        },
    }
}

fn command_fingerprint<T: serde::Serialize>(value: &T) -> AppResult<String> {
    let encoded = serde_json::to_vec(value).map_err(|error| {
        AppError::Internal(format!("Could not encode attention command: {error}"))
    })?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

async fn actor_is_manager(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    actor: PrincipalId,
) -> AppResult<bool> {
    sqlx::query_scalar(
        r#"SELECT EXISTS (
               SELECT 1 FROM principals AS principal
               JOIN companies AS company ON company.id = principal.company_id
               LEFT JOIN company_members AS member
                 ON member.company_id = principal.company_id AND member.user_id = principal.user_id
               WHERE principal.company_id = $1 AND principal.id = $2 AND principal.kind = 'person'
                 AND (company.user_id = principal.user_id OR member.role = 'admin')
           )"#,
    )
    .bind(company_id)
    .bind(actor.as_uuid())
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::from)
}

async fn require_human_principal(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    principal_id: PrincipalId,
) -> AppResult<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM principals WHERE company_id = $1 AND id = $2 AND kind = 'person')",
    )
    .bind(company_id)
    .bind(principal_id.as_uuid())
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::from)?;
    if !exists {
        return Err(AppError::NotFound("Attention source not found.".into()));
    }
    Ok(())
}

async fn require_channel_principal(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    channel_id: Uuid,
    principal_id: PrincipalId,
) -> AppResult<()> {
    let may_view: bool = sqlx::query_scalar(
        r#"SELECT EXISTS (
               SELECT 1 FROM principals AS principal
               JOIN channels AS channel
                 ON channel.company_id = principal.company_id AND channel.id = $2
               JOIN companies AS company ON company.id = principal.company_id
               LEFT JOIN channel_principal_grants AS access_grant
                 ON access_grant.company_id = channel.company_id
                AND access_grant.channel_id = channel.id
                AND access_grant.principal_id = principal.id
                AND access_grant.capability = 'view'
               WHERE principal.company_id = $1 AND principal.id = $3
                 AND principal.kind = 'person'
                 AND (company.user_id = principal.user_id
                      OR channel.access_mode IN ('team', 'public')
                      OR access_grant.principal_id IS NOT NULL)
           )"#,
    )
    .bind(company_id)
    .bind(channel_id)
    .bind(principal_id.as_uuid())
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::from)?;
    if !may_view {
        return Err(AppError::NotFound("Attention source not found.".into()));
    }
    Ok(())
}

#[derive(sqlx::FromRow)]
struct ExistingEvent {
    command_fingerprint: String,
    to_version: i64,
}

async fn lock_attention_source(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    source_kind: &str,
    source_id: Uuid,
) -> AppResult<()> {
    let key = format!("{company_id}:{source_kind}:{source_id}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(key)
        .execute(&mut **tx)
        .await
        .map_err(AppError::from)?;
    Ok(())
}

async fn existing_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    source_kind: &str,
    source_id: Uuid,
    command_id: Uuid,
    fingerprint: &str,
) -> AppResult<Option<u64>> {
    let existing = sqlx::query_as::<_, ExistingEvent>(
        r#"SELECT command_fingerprint, to_version FROM attention_source_events
           WHERE company_id = $1 AND source_kind = $2 AND source_id = $3 AND command_id = $4"#,
    )
    .bind(company_id)
    .bind(source_kind)
    .bind(source_id)
    .bind(command_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?;
    let Some(existing) = existing else {
        return Ok(None);
    };
    if existing.command_fingerprint != fingerprint {
        return Err(AppError::Conflict(
            "Attention command id was already used with different parameters.".into(),
        ));
    }
    Ok(Some(u64::try_from(existing.to_version).map_err(|_| {
        AppError::Internal("Invalid audited attention version".into())
    })?))
}

#[async_trait]
impl AttentionPersistence for PostgresPersistence {
    async fn list_attention(&self, query: AttentionQuery<'_>) -> AppResult<AttentionPage> {
        let limit = query.clamped_limit();
        let cursor = query.cursor;
        let as_of = cursor.map(|cursor| cursor.as_of);
        let view = match query.view {
            AttentionView::MyWork => "my_work",
            AttentionView::Unassigned => "unassigned",
            AttentionView::TeamWork => "team_work",
        };
        let sentinel_time = DateTime::<Utc>::UNIX_EPOCH;
        let sentinel_id = Uuid::nil();
        let rows = sqlx::query_as::<_, AttentionRow>(ATTENTION_SQL)
            .bind(query.company_id)
            .bind(query.visible_channel_ids)
            .bind(query.principal_id.as_uuid())
            .bind(view)
            .bind(as_of)
            .bind(cursor.is_some())
            .bind(cursor.map_or(0_i32, |cursor| i32::from(cursor.due_rank)))
            .bind(cursor.map_or(0_i32, |cursor| i32::from(cursor.priority_rank)))
            .bind(cursor.map_or(sentinel_time, |cursor| cursor.created_at))
            .bind(cursor.map_or("", |cursor| cursor.source_kind.as_str()))
            .bind(cursor.map_or(sentinel_id, |cursor| cursor.source_id))
            .bind((AttentionQuery::MAX_WORKING_SET + 1) as i64)
            .bind((limit + 1) as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::from)?;

        let bounded_count = rows.first().map_or(0, |row| row.bounded_count);
        let result_as_of = rows
            .first()
            .map_or_else(|| as_of.unwrap_or_else(Utc::now), |row| row.as_of);
        let has_next = rows.len() > limit;
        let mut items = rows
            .into_iter()
            .take(limit)
            .map(TryInto::try_into)
            .collect::<AppResult<Vec<AttentionItem>>>()?;
        for item in &mut items {
            item.href = Some(item_href(item));
        }
        let next_cursor = has_next
            .then(|| {
                items
                    .last()
                    .map(|item| AttentionCursor::for_item(item, result_as_of).to_string())
            })
            .flatten();
        Ok(AttentionPage {
            items,
            next_cursor,
            as_of: result_as_of,
            working_set_size: usize::try_from(bounded_count).map_err(|_| {
                AppError::Internal("Attention working-set count cannot be negative".into())
            })?,
            truncated: bounded_count > AttentionQuery::MAX_WORKING_SET as i64,
        })
    }

    async fn create_handoff(&self, handoff: NewManualHandoff) -> AppResult<u64> {
        validate_handoff(&handoff)?;
        let fingerprint = command_fingerprint(&handoff)?;
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        lock_attention_source(&mut tx, handoff.company_id, "handoff", handoff.id).await?;
        if let Some(version) = existing_event(
            &mut tx,
            handoff.company_id,
            "handoff",
            handoff.id,
            handoff.command_id,
            &fingerprint,
        )
        .await?
        {
            return Ok(version);
        }
        require_channel_principal(
            &mut tx,
            handoff.company_id,
            handoff.channel_id,
            handoff.actor_principal_id,
        )
        .await?;
        if let Some(responsible) = handoff.responsible_principal_id {
            require_channel_principal(&mut tx, handoff.company_id, handoff.channel_id, responsible)
                .await?;
        }
        sqlx::query(
            r#"INSERT INTO manual_handoffs (
                   id, company_id, channel_id, thread_id, correlation_id, title, next_action,
                   responsible_principal_id, business_priority, business_due_at,
                   created_by_principal_id
               ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)"#,
        )
        .bind(handoff.id)
        .bind(handoff.company_id)
        .bind(handoff.channel_id)
        .bind(handoff.thread_id)
        .bind(handoff.correlation_id.map(CorrelationId::as_uuid))
        .bind(&handoff.title)
        .bind(&handoff.next_action)
        .bind(handoff.responsible_principal_id.map(PrincipalId::as_uuid))
        .bind(handoff.priority.as_str())
        .bind(handoff.due_at)
        .bind(handoff.actor_principal_id.as_uuid())
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        sqlx::query(
            r#"INSERT INTO attention_source_events (
                   company_id, source_kind, source_id, command_id, command_fingerprint, operation,
                   actor_principal_id, from_version, to_version, new_priority, new_due_at,
                   new_responsible_principal_id
               ) VALUES ($1, 'handoff', $2, $3, $4, 'created', $5, 0, 1, $6, $7, $8)"#,
        )
        .bind(handoff.company_id)
        .bind(handoff.id)
        .bind(handoff.command_id)
        .bind(fingerprint)
        .bind(handoff.actor_principal_id.as_uuid())
        .bind(handoff.priority.as_str())
        .bind(handoff.due_at)
        .bind(handoff.responsible_principal_id.map(PrincipalId::as_uuid))
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        tx.commit().await.map_err(AppError::from)?;
        Ok(1)
    }

    async fn change_source_attributes(&self, command: AttentionSourceCommand) -> AppResult<u64> {
        if !matches!(
            command.source_kind,
            AttentionSourceKind::Task | AttentionSourceKind::Handoff
        ) {
            return Err(AppError::BadRequest(
                "Priority and due dates must be changed through their task or handoff source."
                    .into(),
            ));
        }
        if command.source_kind == AttentionSourceKind::Task
            && command.responsible_principal_id.is_some()
        {
            return Err(AppError::BadRequest(
                "Task responsibility must be changed through task ownership.".into(),
            ));
        }
        let source_kind = command.source_kind.as_str();
        let fingerprint = command_fingerprint(&command)?;
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        lock_attention_source(&mut tx, command.company_id, source_kind, command.source_id).await?;
        if let Some(version) = existing_event(
            &mut tx,
            command.company_id,
            source_kind,
            command.source_id,
            command.command_id,
            &fingerprint,
        )
        .await?
        {
            return Ok(version);
        }
        require_human_principal(&mut tx, command.company_id, command.actor_principal_id).await?;
        let manager =
            actor_is_manager(&mut tx, command.company_id, command.actor_principal_id).await?;

        let (old_priority, old_due, old_responsible, old_version, channel_id) =
            match command.source_kind {
                AttentionSourceKind::Task => {
                    let row: Option<(String, Option<DateTime<Utc>>, Option<Uuid>, i64, Uuid)> =
                        sqlx::query_as(
                            r#"SELECT business_priority, business_due_at, owner_principal_id,
                              attention_version, channel_id
                       FROM background_tasks
                       WHERE company_id = $1 AND id = $2 AND channel_id = ANY($3) FOR UPDATE"#,
                        )
                        .bind(command.company_id)
                        .bind(command.source_id)
                        .bind(&command.visible_channel_ids)
                        .fetch_optional(&mut *tx)
                        .await
                        .map_err(AppError::from)?;
                    row.ok_or_else(|| AppError::NotFound("Attention source not found.".into()))?
                }
                AttentionSourceKind::Handoff => {
                    let row: Option<(String, Option<DateTime<Utc>>, Option<Uuid>, i64, Uuid)> =
                        sqlx::query_as(
                            r#"SELECT business_priority, business_due_at, responsible_principal_id,
                              version, channel_id
                       FROM manual_handoffs
                       WHERE company_id = $1 AND id = $2 AND channel_id = ANY($3)
                         AND status = 'open' FOR UPDATE"#,
                        )
                        .bind(command.company_id)
                        .bind(command.source_id)
                        .bind(&command.visible_channel_ids)
                        .fetch_optional(&mut *tx)
                        .await
                        .map_err(AppError::from)?;
                    row.ok_or_else(|| AppError::NotFound("Attention source not found.".into()))?
                }
                _ => unreachable!(),
            };
        if let Some(responsible) = command.responsible_principal_id {
            require_channel_principal(&mut tx, command.company_id, channel_id, responsible).await?;
        }
        let old_version_u64 = u64::try_from(old_version)
            .map_err(|_| AppError::Internal("Invalid attention source version".into()))?;
        if old_version_u64 != command.expected_version {
            return Err(AppError::Conflict(format!(
                "Attention source changed from version {} to {}; refresh and try again.",
                command.expected_version, old_version_u64
            )));
        }
        let actor_id = command.actor_principal_id.as_uuid();
        if !manager && old_responsible != Some(actor_id) {
            let self_claim = command.source_kind == AttentionSourceKind::Handoff
                && old_responsible.is_none()
                && command.responsible_principal_id == Some(command.actor_principal_id);
            if !self_claim {
                return Err(AppError::NotFound("Attention source not found.".into()));
            }
        }
        let new_version = old_version
            .checked_add(1)
            .ok_or_else(|| AppError::Conflict("Attention version exhausted.".into()))?;
        let new_responsible = if command.source_kind == AttentionSourceKind::Task {
            old_responsible
        } else {
            command.responsible_principal_id.map(PrincipalId::as_uuid)
        };
        match command.source_kind {
            AttentionSourceKind::Task => {
                sqlx::query(
                    r#"UPDATE background_tasks
                       SET business_priority = $3, business_due_at = $4,
                           attention_version = $5, updated_at = CURRENT_TIMESTAMP
                       WHERE company_id = $1 AND id = $2 AND attention_version = $6"#,
                )
                .bind(command.company_id)
                .bind(command.source_id)
                .bind(command.priority.as_str())
                .bind(command.due_at)
                .bind(new_version)
                .bind(old_version)
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;
            }
            AttentionSourceKind::Handoff => {
                sqlx::query(
                    r#"UPDATE manual_handoffs
                       SET business_priority = $3, business_due_at = $4,
                           responsible_principal_id = $5, version = $6,
                           updated_at = CURRENT_TIMESTAMP
                       WHERE company_id = $1 AND id = $2 AND version = $7"#,
                )
                .bind(command.company_id)
                .bind(command.source_id)
                .bind(command.priority.as_str())
                .bind(command.due_at)
                .bind(new_responsible)
                .bind(new_version)
                .bind(old_version)
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;
            }
            _ => unreachable!(),
        }
        let operation = if old_responsible != new_responsible {
            "reassigned"
        } else {
            "attributes_changed"
        };
        sqlx::query(
            r#"INSERT INTO attention_source_events (
                   company_id, source_kind, source_id, command_id, command_fingerprint, operation,
                   actor_principal_id, from_version, to_version, previous_priority, new_priority,
                   previous_due_at, new_due_at, previous_responsible_principal_id,
                   new_responsible_principal_id
               ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)"#,
        )
        .bind(command.company_id)
        .bind(source_kind)
        .bind(command.source_id)
        .bind(command.command_id)
        .bind(fingerprint)
        .bind(operation)
        .bind(actor_id)
        .bind(old_version)
        .bind(new_version)
        .bind(&old_priority)
        .bind(command.priority.as_str())
        .bind(old_due)
        .bind(command.due_at)
        .bind(old_responsible)
        .bind(new_responsible)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        tx.commit().await.map_err(AppError::from)?;
        u64::try_from(new_version)
            .map_err(|_| AppError::Internal("Invalid attention source version".into()))
    }

    async fn resolve_handoff(&self, command: ResolveHandoffCommand) -> AppResult<u64> {
        let fingerprint = command_fingerprint(&command)?;
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        lock_attention_source(&mut tx, command.company_id, "handoff", command.handoff_id).await?;
        if let Some(version) = existing_event(
            &mut tx,
            command.company_id,
            "handoff",
            command.handoff_id,
            command.command_id,
            &fingerprint,
        )
        .await?
        {
            return Ok(version);
        }
        require_human_principal(&mut tx, command.company_id, command.actor_principal_id).await?;
        let row: Option<(String, Option<DateTime<Utc>>, Option<Uuid>, i64)> = sqlx::query_as(
            r#"SELECT business_priority, business_due_at, responsible_principal_id, version
               FROM manual_handoffs
               WHERE company_id = $1 AND id = $2 AND channel_id = ANY($3)
                 AND status = 'open' FOR UPDATE"#,
        )
        .bind(command.company_id)
        .bind(command.handoff_id)
        .bind(&command.visible_channel_ids)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::from)?;
        let (priority, due_at, responsible, old_version) =
            row.ok_or_else(|| AppError::NotFound("Handoff not found.".into()))?;
        let expected = i64::try_from(command.expected_version)
            .map_err(|_| AppError::Conflict("Attention version exhausted.".into()))?;
        if old_version != expected {
            return Err(AppError::Conflict(format!(
                "Handoff changed from version {} to {}; refresh and try again.",
                command.expected_version, old_version
            )));
        }
        let manager =
            actor_is_manager(&mut tx, command.company_id, command.actor_principal_id).await?;
        if !manager && responsible != Some(command.actor_principal_id.as_uuid()) {
            return Err(AppError::NotFound("Handoff not found.".into()));
        }
        let new_version = old_version
            .checked_add(1)
            .ok_or_else(|| AppError::Conflict("Attention version exhausted.".into()))?;
        let (status, operation) = match command.resolution {
            HandoffResolution::Resolved => ("resolved", "resolved"),
            HandoffResolution::Withdrawn => ("withdrawn", "withdrawn"),
        };
        sqlx::query(
            r#"UPDATE manual_handoffs
               SET status = $3, version = $4, resolved_by_principal_id = $5,
                   resolved_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP
               WHERE company_id = $1 AND id = $2 AND version = $6 AND status = 'open'"#,
        )
        .bind(command.company_id)
        .bind(command.handoff_id)
        .bind(status)
        .bind(new_version)
        .bind(command.actor_principal_id.as_uuid())
        .bind(old_version)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        sqlx::query(
            r#"INSERT INTO attention_source_events (
                   company_id, source_kind, source_id, command_id, command_fingerprint, operation,
                   actor_principal_id, from_version, to_version, previous_priority, new_priority,
                   previous_due_at, new_due_at, previous_responsible_principal_id,
                   new_responsible_principal_id
               ) VALUES ($1, 'handoff', $2, $3, $4, $5, $6, $7, $8,
                         $9, $9, $10, $10, $11, $11)"#,
        )
        .bind(command.company_id)
        .bind(command.handoff_id)
        .bind(command.command_id)
        .bind(fingerprint)
        .bind(operation)
        .bind(command.actor_principal_id.as_uuid())
        .bind(old_version)
        .bind(new_version)
        .bind(priority)
        .bind(due_at)
        .bind(responsible)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        tx.commit().await.map_err(AppError::from)?;
        u64::try_from(new_version)
            .map_err(|_| AppError::Internal("Invalid attention source version".into()))
    }

    async fn operational_summary(
        &self,
        company_id: Uuid,
        visible_channel_ids: &[Uuid],
    ) -> AppResult<OperationalSummary> {
        #[derive(sqlx::FromRow)]
        struct SummaryRow {
            as_of: DateTime<Utc>,
            unassigned_count: i64,
            oldest_actionable_age_seconds: Option<f64>,
            average_time_to_claim_seconds: Option<f64>,
            average_delegation_wait_seconds: Option<f64>,
            average_review_turnaround_seconds: Option<f64>,
            timeout_cancel_reassign_rate: Option<f64>,
            average_time_to_logical_external_response_seconds: Option<f64>,
            permanent_delivery_failure_count: i64,
        }
        let row = sqlx::query_as::<_, SummaryRow>(
            r#"WITH clock AS (SELECT CURRENT_TIMESTAMP AS as_of),
               current_actionable AS (
                 SELECT task.created_at,
                        CASE WHEN task.owner_principal_kind = 'person'
                             THEN task.owner_principal_id END AS responsible_principal_id
                 FROM background_tasks AS task
                 WHERE task.company_id = $1 AND task.channel_id = ANY($2)
                   AND task.status IN ('pending','processing','pending_approval',
                                       'waiting_for_third_party_reply','failed','dead_letter')
                   AND (task.owner_principal_kind = 'person'
                        OR task.owner_principal_id IS NULL
                        OR task.status IN ('pending_approval','failed','dead_letter'))
                   AND NOT EXISTS (
                       SELECT 1 FROM response_reviews AS review
                       JOIN response_drafts AS draft
                         ON draft.company_id = review.company_id
                        AND draft.id = review.draft_id
                        AND draft.version = review.draft_version
                       WHERE review.company_id = task.company_id
                         AND review.status = 'pending'
                         AND draft.task_id = task.id AND draft.status = 'pending_review'
                   )
                   AND NOT EXISTS (
                       SELECT 1 FROM task_outreaches AS outreach
                       WHERE outreach.task_id = task.id
                         AND outreach.status = 'timeout_pending_approval'
                   )
                   AND NOT EXISTS (
                       SELECT 1 FROM message_deliveries AS delivery
                       WHERE delivery.company_id = task.company_id
                         AND delivery.task_id = task.id
                         AND delivery.status IN ('outcome_unknown','dead_letter')
                         AND delivery.last_error_class IS DISTINCT FROM 'superseded'
                   )
                 UNION ALL
                 SELECT handoff.created_at, handoff.responsible_principal_id
                 FROM manual_handoffs AS handoff
                 WHERE handoff.company_id = $1 AND handoff.channel_id = ANY($2)
                   AND handoff.status = 'open'
                 UNION ALL
                 SELECT review.created_at, review.reviewer_principal_id
                 FROM response_reviews AS review
                 JOIN response_drafts AS draft
                   ON draft.company_id = review.company_id AND draft.id = review.draft_id
                  AND draft.version = review.draft_version AND draft.status = 'pending_review'
                 WHERE review.company_id = $1 AND draft.channel_id = ANY($2)
                   AND review.status = 'pending'
                 UNION ALL
                 SELECT outreach.created_at,
                        CASE WHEN task.owner_principal_kind = 'person'
                             THEN task.owner_principal_id END
                 FROM task_outreaches AS outreach
                 JOIN background_tasks AS task ON task.id = outreach.task_id
                 WHERE task.company_id = $1 AND task.channel_id = ANY($2)
                   AND outreach.status = 'timeout_pending_approval'
                 UNION ALL
                 SELECT delivery.created_at,
                        CASE WHEN task.owner_principal_kind = 'person'
                             THEN task.owner_principal_id END
                 FROM message_deliveries AS delivery
                 LEFT JOIN background_tasks AS task
                   ON task.company_id = delivery.company_id AND task.id = delivery.task_id
                 WHERE delivery.company_id = $1 AND delivery.channel_id = ANY($2)
                   AND delivery.status IN ('outcome_unknown','dead_letter')
                   AND delivery.last_error_class IS DISTINCT FROM 'superseded'
                   AND NOT EXISTS (
                       SELECT 1 FROM task_outreach_targets AS target
                       JOIN task_outreaches AS outreach ON outreach.id = target.outreach_id
                       WHERE target.delivery_id = delivery.id
                         AND outreach.status = 'timeout_pending_approval'
                   )
               ), first_claim AS (
                 SELECT DISTINCT ON (event.task_id) event.task_id, event.occurred_at
                 FROM task_ownership_events AS event
                 WHERE event.company_id = $1 AND event.operation = 'claim'
                 ORDER BY event.task_id, event.occurred_at, event.id
               ), control_counts AS (
                 SELECT (
                     SELECT COUNT(DISTINCT affected.outreach_id)::float8 FROM (
                         SELECT command.outreach_id
                         FROM delegation_control_commands AS command
                         JOIN background_tasks AS task ON task.id = command.task_id
                         WHERE command.company_id = $1 AND task.channel_id = ANY($2)
                           AND command.operation IN (
                               'cancel_target','cancel_outreach','reassign_internal_target'
                           )
                         UNION
                         SELECT event.related_outreach_id
                         FROM task_status_events AS event
                         JOIN background_tasks AS task ON task.id = event.task_id
                         WHERE event.company_id = $1 AND task.channel_id = ANY($2)
                           AND event.reason = 'outreach_timed_out'
                           AND event.related_outreach_id IS NOT NULL
                     ) AS affected
                 ) AS adverse,
                 (
                     SELECT COUNT(*)::float8
                     FROM task_outreaches AS outreach
                     JOIN background_tasks AS task ON task.id = outreach.task_id
                     WHERE outreach.company_id = $1 AND task.channel_id = ANY($2)
                 ) AS total
               )
               SELECT clock.as_of,
                 (SELECT COUNT(*) FROM current_actionable
                   WHERE responsible_principal_id IS NULL)::bigint AS unassigned_count,
                 (SELECT EXTRACT(EPOCH FROM (clock.as_of - MIN(created_at)))::float8
                    FROM current_actionable) AS oldest_actionable_age_seconds,
                 (SELECT AVG(EXTRACT(EPOCH FROM (claim.occurred_at - task.created_at)))::float8
                    FROM first_claim AS claim JOIN background_tasks AS task ON task.id = claim.task_id
                    WHERE task.channel_id = ANY($2))
                    AS average_time_to_claim_seconds,
                 (SELECT AVG(EXTRACT(EPOCH FROM (
                         CASE WHEN outreach.status IN ('waiting','timeout_pending_approval')
                              THEN clock.as_of ELSE outreach.updated_at END - outreach.created_at
                     )))::float8 FROM task_outreaches AS outreach
                     JOIN background_tasks AS task ON task.id = outreach.task_id
                     WHERE outreach.company_id = $1 AND task.channel_id = ANY($2))
                    AS average_delegation_wait_seconds,
                 (SELECT AVG(EXTRACT(EPOCH FROM (
                         COALESCE((
                             SELECT MIN(command.created_at)
                             FROM response_review_commands AS command
                             WHERE command.company_id = review.company_id
                               AND command.draft_id = review.draft_id
                               AND command.expected_draft_version = review.draft_version
                               AND command.action IN ('approve', 'edit', 'reject')
                         ), review.updated_at) - review.created_at
                     )))::float8
                    FROM response_reviews AS review
                    JOIN response_drafts AS draft
                      ON draft.company_id = review.company_id AND draft.id = review.draft_id
                     AND draft.version = review.draft_version
                    WHERE review.company_id = $1 AND draft.channel_id = ANY($2)
                      AND review.status <> 'pending')
                    AS average_review_turnaround_seconds,
                 (SELECT adverse / NULLIF(total, 0) FROM control_counts)
                    AS timeout_cancel_reassign_rate,
                 (SELECT AVG(EXTRACT(EPOCH FROM (event.transitioned_at - task.created_at)))::float8
                    FROM task_status_events AS event
                    JOIN background_tasks AS task ON task.id = event.task_id
                    WHERE event.company_id = $1 AND task.channel_id = ANY($2)
                      AND event.to_status = 'completed')
                    AS average_time_to_logical_external_response_seconds,
                 (SELECT COUNT(*) FROM message_deliveries
                    WHERE company_id = $1 AND channel_id = ANY($2) AND status = 'dead_letter'
                      AND last_error_class IS DISTINCT FROM 'superseded')::bigint
                    AS permanent_delivery_failure_count
               FROM clock"#,
        )
        .bind(company_id)
        .bind(visible_channel_ids)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(OperationalSummary {
            as_of: row.as_of,
            unassigned_count: row.unassigned_count,
            oldest_actionable_age_seconds: row.oldest_actionable_age_seconds,
            average_time_to_claim_seconds: row.average_time_to_claim_seconds,
            average_delegation_wait_seconds: row.average_delegation_wait_seconds,
            average_review_turnaround_seconds: row.average_review_turnaround_seconds,
            timeout_cancel_reassign_rate: row.timeout_cancel_reassign_rate,
            average_time_to_logical_external_response_seconds: row
                .average_time_to_logical_external_response_seconds,
            permanent_delivery_failure_count: row.permanent_delivery_failure_count,
        })
    }
}

#[cfg(test)]
#[path = "attention_tests.rs"]
mod tests;
