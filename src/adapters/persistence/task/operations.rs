//! The `PostgresPersistence` implementation of [`TaskPersistence`].
//!
//! A trait impl is a single Rust item and cannot be split across files, so every operation the
//! worker reaches through this port lands here. The board reads forward to [`super::board`]; the
//! rest keep their bodies inline, because a forwarding `async fn` materialises the future it
//! calls and this is the bottom of the deepest `await` chain in the process.
//!
//! That leaves this file over the ~1,000-line threshold in `src/AGENTS.md`, and deliberately so:
//! the two ways to get under it are both worse than the size. Forwarding every method costs a
//! future frame each on the worker's dispatch chain -- the chain that aborted the process on
//! 2026-08-29 at 1,997 KiB of a 2,080 KiB stack. Splitting [`TaskPersistence`] into supertraits
//! would get there honestly, but it is an API change across six mock implementors rather than the
//! structural cleanup this phase is scoped to. Recorded as deferred debt; revisit with the split.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::postgres::types::PgInterval;
use sqlx::{Postgres, QueryBuilder};
use std::collections::HashMap;
use std::str::FromStr;
use uuid::Uuid;

use super::*;
use crate::{
    adapters::persistence::{
        PostgresPersistence, delivery::enqueue::insert_delivery_on, thread::insert_message_on,
    },
    app_error::{AppError, AppResult},
    entities::{
        collaboration::CollaborationSummary,
        correlation::CorrelationId,
        outreach::{DueOutreach, OutreachProgress, OutreachReplyMatch, OutreachStatus},
        runtime_metrics::{MachineIdentity, MachineRegion},
        stuck_work::{StuckWorkCensus, StuckWorkThresholds},
        task::{
            BackgroundTask, NewTask, ResumeActor, StopActor, TaskAttemptOutcome, TaskAttemptRecord,
            TaskAttemptRef, TaskAttemptStatus, TaskBoardFilter, TaskChainBoard, TaskChainDetail,
            TaskFailure, TaskFilter, TaskLeaseRef, TaskOwner, TaskOwnerCandidate, TaskOwnerFilter,
            TaskOwnerTarget, TaskOwnership, TaskOwnershipCommand, TaskOwnershipEvent, TaskStatus,
            TaskStatusEvent, TaskStatusEventCursor, TaskStopReason, TaskTransitionReason,
            ThreadActivity, ThreadWorkSummary, TokenUsage, TransitionActor,
        },
        transport::{DeliveryId, PrincipalId},
        value_objects::MessageId,
    },
    task_queue::{
        AssignmentNotificationRecipient, CollaborationReadScope, HumanTaskCompletion,
        HumanTaskCompletionResult,
    },
    transport::{DeliveryCreation, NewDelivery},
};

#[derive(sqlx::FromRow)]
struct ResponseDraftDb {
    version: i32,
    status: String,
    channel_id: Uuid,
    thread_id: Uuid,
    task_id: Option<Uuid>,
    author_principal_id: Uuid,
    subject: String,
    body: String,
    recipient_snapshot: serde_json::Value,
}

struct OutreachTargetColumns<'a> {
    delivery_address: &'a str,
    kind: &'static str,
    internal_channel_id: Option<Uuid>,
    external_transport: Option<&'a str>,
    external_namespace: Option<&'a str>,
    external_subject: Option<&'a str>,
}

fn outreach_target_columns(
    target: &crate::task_queue::OutreachTargetRequest,
) -> AppResult<OutreachTargetColumns<'_>> {
    let delivery_address = target
        .delivery
        .external_destination
        .as_ref()
        .ok_or_else(|| {
            AppError::Internal("An outreach target delivery has no external destination".into())
        })?
        .as_str();
    match &target.target {
        crate::task_queue::OutreachTargetIdentity::InternalChannel { channel_id } => {
            Ok(OutreachTargetColumns {
                delivery_address,
                kind: "internal_channel",
                internal_channel_id: Some(*channel_id),
                external_transport: None,
                external_namespace: None,
                external_subject: None,
            })
        }
        crate::task_queue::OutreachTargetIdentity::External { identity } => {
            if identity.transport() != target.delivery.transport
                || identity.subject().as_str() != delivery_address
            {
                return Err(AppError::Internal(
                    "Outreach target identity and delivery destination disagree".into(),
                ));
            }
            Ok(OutreachTargetColumns {
                delivery_address,
                kind: "external",
                internal_channel_id: None,
                external_transport: Some(identity.transport().as_str()),
                external_namespace: Some(identity.namespace().as_str()),
                external_subject: Some(identity.subject().as_str()),
            })
        }
    }
}

/// Retire the questions an outreach has not sent yet.
///
/// Reached when the outreach stops waiting -- quorum met, or the run that owns it completed. The
/// shape this replaces asked "is this delivery still wanted?" once per claimed row, which cost a
/// round trip per send and answered from state that could change a millisecond later. Deciding
/// here, in the transaction that closes the outreach, is both cheaper and correct.
///
/// Claimable rows only: one already `sending` is owned by a worker holding a live lease, and
/// writing past that fence would overwrite an outcome a provider had already given.
async fn cancel_unsent_outreach_questions(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    outreach_id: Uuid,
) -> AppResult<()> {
    sqlx::query(
        r#"UPDATE message_deliveries AS delivery
              SET status = 'dead_letter', attempt_count = max_attempts,
                  last_error_class = 'superseded',
                  last_error_detail = 'The outreach this question belonged to stopped waiting',
                  updated_at = CURRENT_TIMESTAMP
             FROM task_outreach_targets AS target
            WHERE target.outreach_id = $1
              AND target.delivery_id = delivery.id
              AND delivery.status IN ('pending', 'retryable')"#,
    )
    .bind(outreach_id)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(())
}

/// Record one canonical thread-message association as an outreach response on an existing
/// transaction. Inbound ingress uses this so the response and the task wake-up cannot disagree.
pub(crate) async fn record_outreach_reply_on(
    connection: &mut sqlx::PgConnection,
    matched: &OutreachReplyMatch,
    response_association_id: Uuid,
) -> AppResult<OutreachProgress> {
    let mut outreach = sqlx::query_as::<_, OutreachDb>(
        r#"SELECT id, task_id, status,
                  required_threshold_percent::double precision AS required_threshold_percent,
                  expires_at
           FROM task_outreaches WHERE id = $1 FOR UPDATE"#,
    )
    .bind(matched.outreach_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(AppError::from)?;

    sqlx::query(
        r#"UPDATE task_outreach_targets
           SET responded_at = CURRENT_TIMESTAMP, response_association_id = $3
           WHERE outreach_id = $1 AND email = $2 AND responded_at IS NULL"#,
    )
    .bind(matched.outreach_id)
    .bind(matched.target_email.as_str())
    .bind(response_association_id)
    .execute(&mut *connection)
    .await
    .map_err(AppError::from)?;

    let (target_count, response_count): (i64, i64) = sqlx::query_as(
        r#"SELECT COUNT(*)::bigint,
                  COUNT(*) FILTER (WHERE responded_at IS NOT NULL)::bigint
           FROM task_outreach_targets WHERE outreach_id = $1"#,
    )
    .bind(matched.outreach_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(AppError::from)?;
    let required = required_response_count(target_count, outreach.required_threshold_percent);
    let current_status = OutreachStatus::from_str(&outreach.status)
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let reached = response_count >= required as i64;

    if reached
        && matches!(
            current_status,
            OutreachStatus::Waiting | OutreachStatus::TimeoutPendingApproval
        )
    {
        let attribution = TransitionAttribution::new(
            TaskTransitionReason::OutreachReplyReceived,
            TransitionActor::Outreach(outreach.id),
        );
        sqlx::query(
            r#"UPDATE task_outreaches SET status = 'threshold_met',
                   updated_at = CURRENT_TIMESTAMP WHERE id = $1"#,
        )
        .bind(matched.outreach_id)
        .execute(&mut *connection)
        .await
        .map_err(AppError::from)?;
        sqlx::query(
            r#"UPDATE message_deliveries AS delivery
                  SET status = 'dead_letter', attempt_count = max_attempts,
                      last_error_class = 'superseded',
                      last_error_detail = 'The outreach this question belonged to stopped waiting',
                      updated_at = CURRENT_TIMESTAMP
                 FROM task_outreach_targets AS target
                WHERE target.outreach_id = $1
                  AND target.delivery_id = delivery.id
                  AND delivery.status IN ('pending', 'retryable')"#,
        )
        .bind(matched.outreach_id)
        .execute(&mut *connection)
        .await
        .map_err(AppError::from)?;
        sqlx::query(&format!(
            r#"UPDATE background_tasks SET status = 'pending', run_at = CURRENT_TIMESTAMP,
                   wait_expires_at = NULL, worker_id = NULL, execution_generation = NULL, locked_at = NULL,
                   lock_expires_at = NULL, updated_at = CURRENT_TIMESTAMP, {attribution}
               WHERE id = $1 AND status IN (
                   'waiting_for_third_party_reply', 'pending_approval'
               )"#,
            attribution = attribution.set_clause(),
        ))
        .bind(outreach.task_id)
        .execute(&mut *connection)
        .await
        .map_err(AppError::from)?;
        sqlx::query(
            r#"UPDATE human_approvals SET status = 'expired', updated_at = CURRENT_TIMESTAMP
               WHERE task_id = $1 AND action_type = 'quorum_timeout'
                 AND status = 'pending'"#,
        )
        .bind(outreach.task_id)
        .execute(&mut *connection)
        .await
        .map_err(AppError::from)?;
        outreach.status = OutreachStatus::ThresholdMet.as_str().to_string();
    }

    let status = OutreachStatus::from_str(&outreach.status)
        .map_err(|error| AppError::Internal(error.to_string()))?;
    Ok(outreach_progress(
        &outreach,
        status,
        target_count,
        response_count,
        false,
    ))
}

pub(crate) async fn get_task_by_id_on(
    pool: &sqlx::PgPool,
    id: Uuid,
) -> AppResult<Option<BackgroundTask>> {
    let db = sqlx::query_as::<_, BackgroundTaskDb>(
        r#"SELECT id, company_id, channel_id, thread_id, correlation_id, task_type, status, payload,
                  retry_count, max_retries, last_error, owner_principal_id,
                  owner_principal_kind, ownership_version, worker_id, execution_generation,
                  locked_at, lock_expires_at, run_at, created_at, updated_at
           FROM background_tasks WHERE id = $1"#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::from)?;
    db.map(TryInto::try_into).transpose()
}

#[async_trait]
impl TaskPersistence for PostgresPersistence {
    async fn get_collaboration_summary(
        &self,
        scope: CollaborationReadScope<'_>,
        task_id: Uuid,
    ) -> AppResult<Option<CollaborationSummary>> {
        collaboration_summary_on(&self.pool, scope, task_id).await
    }

    async fn list_thread_work_summary(
        &self,
        thread_ids: &[Uuid],
    ) -> AppResult<HashMap<Uuid, ThreadWorkSummary>> {
        if thread_ids.is_empty() {
            return Ok(HashMap::new());
        }

        // `DISTINCT ON` keeps one task per thread: the run still going if there is one, and
        // otherwise whichever of the thread's finished runs ended last.
        //
        // `completed` earns its place in that second group even though it draws no badge. A
        // dead letter is the thread's last word only until a later run answers the question, and
        // asking again is exactly what a reader does with a failure: without the successful run
        // in the comparison, the alert would come back the moment the retry finished and the
        // thread would look broken for good. Ordering by the clock alone cannot express that,
        // hence the boolean: unfinished work outranks any finished run however old it is.
        //
        // `stopped` and `failed` stay out. Neither is an answer, so neither should bury one.
        let rows = sqlx::query_as::<_, ThreadActivityDb>(
            r#"SELECT DISTINCT ON (task.thread_id) task.thread_id, task.id AS task_id,
                      task.status, task.lock_expires_at, task.owner_principal_id,
                      task.owner_principal_kind, task.ownership_version,
                      owner.display_label AS owner_label
               FROM background_tasks AS task
               LEFT JOIN principals AS owner
                 ON owner.company_id = task.company_id AND owner.id = task.owner_principal_id
               WHERE task.thread_id = ANY($1)
                 AND task.status IN ('pending', 'processing', 'pending_approval',
                                'waiting_for_third_party_reply', 'dead_letter', 'completed')
               ORDER BY task.thread_id,
                        task.status IN ('dead_letter', 'completed'),
                        task.updated_at DESC, task.id DESC"#,
        )
        .bind(thread_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        let now = Utc::now();
        rows.into_iter()
            .map(|row| {
                let status = TaskStatus::from_str(&row.status)
                    .map_err(|error| AppError::Internal(error.to_string()))?;
                let owner = task_owner_from_db(
                    row.owner_principal_id,
                    row.owner_principal_kind.as_deref(),
                    &format!("thread work task {}", row.task_id),
                )?;
                let version = u64::try_from(row.ownership_version).map_err(|_| {
                    AppError::Internal(format!(
                        "Invalid ownership version for task {}: {}",
                        row.task_id, row.ownership_version
                    ))
                })?;
                Ok((
                    row.thread_id,
                    ThreadWorkSummary {
                        task_id: row.task_id,
                        ownership: TaskOwnership { owner, version },
                        owner_label: row.owner_label,
                        activity: ThreadActivity::from_task(status, row.lock_expires_at, now),
                    },
                ))
            })
            // `completed` is queried for its position in that ordering, not for a badge: reaching
            // here it means the thread's last word was a run that worked, which is nothing to
            // show. Dropping every `None` also keeps a status added to the query later from
            // turning into a badge nobody chose.
            .filter_map(|entry: AppResult<_>| match entry {
                Ok((thread_id, summary)) if summary.activity.is_some() => {
                    Some(Ok((thread_id, summary)))
                }
                Ok((_, _)) => None,
                Err(error) => Some(Err(error)),
            })
            .collect()
    }

    async fn create_outreach_and_pause(
        &self,
        request: CreateOutreachRequest,
    ) -> AppResult<OutreachProgress> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let outreach = sqlx::query_as::<_, OutreachDb>(
            r#"INSERT INTO task_outreaches (
                    id, task_id, company_id, outreach_key, status, required_threshold_percent,
                    expires_at, subject, body
               )
               SELECT $1, id, company_id, $2, 'waiting', $3, $4, $5, $6
               FROM background_tasks
               WHERE id = $7 AND company_id = $8
                 AND status = 'processing' AND worker_id = $9
                 AND execution_generation = $10
                 AND owner_principal_id = $11 AND ownership_version = $12
                 AND lock_expires_at > CURRENT_TIMESTAMP
               ON CONFLICT (task_id, outreach_key) DO UPDATE
                   SET outreach_key = EXCLUDED.outreach_key
               RETURNING id, task_id, status,
                         required_threshold_percent::double precision AS required_threshold_percent,
                         expires_at"#,
        )
        .bind(request.id)
        .bind(&request.outreach_key)
        .bind(request.required_threshold_percent)
        .bind(request.expires_at)
        .bind(&request.subject)
        .bind(&request.body)
        .bind(request.lease.task_id)
        .bind(request.company_id)
        .bind(request.lease.worker_id)
        .bind(request.lease.execution_generation)
        .bind(
            request
                .lease
                .claimed_owner
                .agent_principal_id()
                .map(PrincipalId::as_uuid),
        )
        .bind(
            i64::try_from(request.lease.ownership_version)
                .map_err(|_| AppError::Conflict("Ownership version exhausted.".into()))?,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::Internal("Outreach task lease was lost before creation".into()))?;

        let created = outreach.id == request.id;
        if created {
            for target in &request.targets {
                // The question, the mail that carries it, and the target row that records both --
                // one transaction. The mark on the target row is what stops the reply guard
                // reading an outreach the agent *sent* as the answer it owes this thread; written
                // separately, a failure between them completed the task without an answer.
                insert_message_on(&mut tx, &target.request).await?;
                insert_delivery_on(&mut tx, &target.delivery).await?;

                let columns = outreach_target_columns(target)?;

                sqlx::query(
                    r#"INSERT INTO task_outreach_targets
                           (outreach_id, company_id, email, target_kind, internal_channel_id,
                            external_transport, external_namespace, external_subject,
                            delivery_id, request_message_id)
                       VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"#,
                )
                .bind(outreach.id)
                .bind(request.company_id)
                .bind(columns.delivery_address)
                .bind(columns.kind)
                .bind(columns.internal_channel_id)
                .bind(columns.external_transport)
                .bind(columns.external_namespace)
                .bind(columns.external_subject)
                .bind(target.delivery.id.as_uuid())
                .bind(target.request.id.as_uuid())
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;
            }
        }

        let status = OutreachStatus::from_str(&outreach.status)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        let suspended = status == OutreachStatus::Waiting;
        if suspended {
            let attribution = TransitionAttribution::new(
                TaskTransitionReason::OutreachStarted,
                TransitionActor::Outreach(outreach.id),
            );
            let paused = sqlx::query(&format!(
                r#"UPDATE background_tasks
                   SET status = 'waiting_for_third_party_reply', wait_expires_at = $1,
                       worker_id = NULL, execution_generation = NULL, locked_at = NULL, lock_expires_at = NULL,
                       updated_at = CURRENT_TIMESTAMP, {attribution}
                   WHERE id = $2 AND company_id = $3
                     AND status = 'processing' AND worker_id = $4
                     AND execution_generation = $5
                     AND owner_principal_id = $6 AND ownership_version = $7
                     AND lock_expires_at > CURRENT_TIMESTAMP"#,
                attribution = attribution.set_clause(),
            ))
            .bind(outreach.expires_at)
            .bind(request.lease.task_id)
            .bind(request.company_id)
            .bind(request.lease.worker_id)
            .bind(request.lease.execution_generation)
            .bind(
                request
                    .lease
                    .claimed_owner
                    .agent_principal_id()
                    .map(PrincipalId::as_uuid),
            )
            .bind(i64::try_from(request.lease.ownership_version).map_err(|_| {
                AppError::Conflict("Ownership version exhausted.".into())
            })?)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
            if paused.rows_affected() != 1 {
                return Err(AppError::Internal(
                    "Outreach task could not be paused".into(),
                ));
            }
        }

        let (target_count, response_count): (i64, i64) = sqlx::query_as(
            r#"SELECT COUNT(*)::bigint,
                      COUNT(*) FILTER (WHERE responded_at IS NOT NULL)::bigint
               FROM task_outreach_targets WHERE outreach_id = $1"#,
        )
        .bind(outreach.id)
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::from)?;
        tx.commit().await.map_err(AppError::from)?;

        Ok(outreach_progress(
            &outreach,
            status,
            target_count,
            response_count,
            suspended,
        ))
    }

    async fn find_correlated_outreach_reply(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
        thread_id: Uuid,
        sender: &str,
        references: &[MessageId],
    ) -> AppResult<Option<OutreachReplyMatch>> {
        if references.is_empty() {
            return Ok(None);
        }
        let reference_strs: Vec<&str> = references.iter().map(MessageId::as_str).collect();
        // Matched on the provider key the outreach mail actually went out under, which lives on
        // the delivery *part* rather than the delivery: one send is one part for mail, and the
        // single `provider_message_id` column this replaces could name only one of a chat
        // provider's several. The delivery must have reached the provider -- a queued question
        // that nobody has been sent yet cannot be what this reply answers.
        let row = sqlx::query_as::<_, (Uuid, Uuid, String)>(
            r#"SELECT outreach.id, task.id, target.email::text
                 FROM task_outreaches AS outreach
                 JOIN background_tasks AS task ON task.id = outreach.task_id
                 JOIN task_outreach_targets AS target ON target.outreach_id = outreach.id
                 JOIN message_delivery_parts AS part ON part.delivery_id = target.delivery_id
                WHERE task.company_id = $1 AND task.channel_id = $2 AND task.thread_id = $3
                  AND target.email = $4
                  AND outreach.status IN (
                      'waiting', 'timeout_pending_approval', 'threshold_met', 'completed'
                  )
                  AND part.status = 'delivered'
                  AND part.provider_message_key = ANY($5)
                ORDER BY outreach.created_at DESC
                LIMIT 1"#,
        )
        .bind(company_id)
        .bind(channel_id)
        .bind(thread_id)
        .bind(sender.trim())
        .bind(&reference_strs)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(
            row.map(|(outreach_id, task_id, target_email)| OutreachReplyMatch {
                outreach_id,
                task_id,
                target_email: target_email.into(),
            }),
        )
    }

    async fn record_outreach_reply(
        &self,
        matched: &OutreachReplyMatch,
        response_association_id: Uuid,
    ) -> AppResult<OutreachProgress> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        // Box the shared transaction body so this broad trait method does not absorb its future;
        // it also runs inside the inbound transaction when a reply arrives.
        let progress = Box::pin(record_outreach_reply_on(
            &mut tx,
            matched,
            response_association_id,
        ))
        .await?;
        tx.commit().await.map_err(AppError::from)?;
        Ok(progress)
    }

    async fn list_due_outreaches(
        &self,
        due_at: DateTime<Utc>,
        limit: i64,
    ) -> AppResult<Vec<DueOutreach>> {
        let rows = sqlx::query_as::<
            _,
            (
                Uuid,
                Uuid,
                Uuid,
                Uuid,
                Option<Uuid>,
                f64,
                i64,
                i64,
                DateTime<Utc>,
            ),
        >(
            r#"SELECT outreach.id, task.id, task.company_id, task.channel_id, task.thread_id,
                      outreach.required_threshold_percent::double precision,
                      COUNT(target.*)::bigint,
                      COUNT(target.*) FILTER (WHERE target.responded_at IS NOT NULL)::bigint,
                      outreach.expires_at
               FROM task_outreaches outreach
               JOIN background_tasks task ON task.id = outreach.task_id
               JOIN task_outreach_targets target ON target.outreach_id = outreach.id
               WHERE outreach.status = 'waiting' AND outreach.expires_at <= $1
                 AND task.status = 'waiting_for_third_party_reply'
               GROUP BY outreach.id, task.id
               ORDER BY outreach.expires_at, outreach.id
               LIMIT $2"#,
        )
        .bind(due_at)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    outreach_id,
                    task_id,
                    company_id,
                    channel_id,
                    thread_id,
                    required_threshold_percent,
                    target_count,
                    response_count,
                    expires_at,
                )| DueOutreach {
                    outreach_id,
                    task_id,
                    company_id,
                    channel_id,
                    thread_id,
                    required_threshold_percent,
                    target_count: target_count as usize,
                    response_count: response_count as usize,
                    expires_at,
                },
            )
            .collect())
    }

    async fn mark_outreach_timeout_pending(&self, outreach_id: Uuid) -> AppResult<bool> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let task_id = sqlx::query_scalar::<_, Uuid>(
            r#"UPDATE task_outreaches
               SET status = 'timeout_pending_approval', updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status = 'waiting' AND expires_at <= CURRENT_TIMESTAMP
               RETURNING task_id"#,
        )
        .bind(outreach_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::from)?;
        let Some(task_id) = task_id else {
            tx.rollback().await.map_err(AppError::from)?;
            return Ok(false);
        };
        let attribution = TransitionAttribution::new(
            TaskTransitionReason::OutreachTimedOut,
            TransitionActor::Outreach(outreach_id),
        );
        let updated = sqlx::query(&format!(
            r#"UPDATE background_tasks
               SET status = 'pending_approval', wait_expires_at = NULL,
                   worker_id = NULL, execution_generation = NULL, locked_at = NULL, lock_expires_at = NULL,
                   updated_at = CURRENT_TIMESTAMP, {attribution}
               WHERE id = $1 AND status = 'waiting_for_third_party_reply'"#,
            attribution = attribution.set_clause(),
        ))
        .bind(task_id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        tx.commit().await.map_err(AppError::from)?;
        Ok(updated.rows_affected() == 1)
    }

    async fn restore_outreach_waiting(&self, outreach_id: Uuid) -> AppResult<()> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let row = sqlx::query_as::<_, (Uuid, DateTime<Utc>)>(
            r#"UPDATE task_outreaches SET status = 'waiting', updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status = 'timeout_pending_approval'
               RETURNING task_id, expires_at"#,
        )
        .bind(outreach_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::from)?;
        if let Some((task_id, expires_at)) = row {
            let attribution = TransitionAttribution::new(
                TaskTransitionReason::OutreachExtended,
                TransitionActor::Outreach(outreach_id),
            );
            sqlx::query(&format!(
                r#"UPDATE background_tasks
                   SET status = 'waiting_for_third_party_reply', wait_expires_at = $2,
                       updated_at = CURRENT_TIMESTAMP, {attribution}
                   WHERE id = $1 AND status = 'pending_approval'"#,
                attribution = attribution.set_clause(),
            ))
            .bind(task_id)
            .bind(expires_at)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
        }
        tx.commit().await.map_err(AppError::from)?;
        Ok(())
    }

    async fn get_outreach_context(&self, task_id: Uuid) -> AppResult<Option<String>> {
        let rows = sqlx::query_as::<_, (String, String, String, f64, String, bool)>(
            r#"SELECT outreach.subject, outreach.body, outreach.status,
                      outreach.required_threshold_percent::double precision,
                      target.email::text, target.responded_at IS NOT NULL
               FROM task_outreaches outreach
               JOIN task_outreach_targets target ON target.outreach_id = outreach.id
               WHERE outreach.id = (
                   SELECT id FROM task_outreaches
                   WHERE task_id = $1 ORDER BY created_at DESC, id DESC LIMIT 1
               )
               ORDER BY outreach.created_at DESC, target.email"#,
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;
        let Some((subject, body, status, threshold, _, _)) = rows.first() else {
            return Ok(None);
        };
        let responded = rows
            .iter()
            .filter(|row| row.5)
            .map(|row| row.4.clone())
            .collect::<Vec<_>>();
        let outstanding = rows
            .iter()
            .filter(|row| !row.5)
            .map(|row| row.4.clone())
            .collect::<Vec<_>>();
        Ok(Some(format!(
            "Outreach status: {status}\nSubject: {subject}\nRequest: {body}\nRequired threshold: {threshold:.1}%\nResponses: {}/{}\nRespondents: {}\nOutstanding: {}",
            responded.len(),
            rows.len(),
            if responded.is_empty() {
                "none".to_string()
            } else {
                responded.join(", ")
            },
            if outstanding.is_empty() {
                "none".to_string()
            } else {
                outstanding.join(", ")
            },
        )))
    }

    async fn complete_outreach(&self, task_id: Uuid) -> AppResult<()> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let completed: Vec<Uuid> = sqlx::query_scalar(
            r#"UPDATE task_outreaches SET status = 'completed', updated_at = CURRENT_TIMESTAMP
               WHERE task_id = $1 AND status IN ('threshold_met', 'proceed_partial')
               RETURNING id"#,
        )
        .bind(task_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(AppError::from)?;
        // The run answered, so any question this outreach had not sent is moot. Retired in the
        // same transaction that closes it, so a worker cannot claim one in between.
        for outreach_id in completed {
            cancel_unsent_outreach_questions(&mut tx, outreach_id).await?;
        }
        tx.commit().await.map_err(AppError::from)?;
        Ok(())
    }

    async fn census_stuck_work(
        &self,
        thresholds: StuckWorkThresholds,
    ) -> AppResult<StuckWorkCensus> {
        // Counted in one pass with FILTER rather than eight statements. Each arm is a bounded
        // index scan: the status arms hit `background_tasks_company_status_created_idx` and
        // `message_deliveries_claimable_idx`, and the lease arm hits
        // `background_tasks_processing_lease_idx`.
        //
        // `wait_expires_at` is what the reply arm compares against rather than the parked
        // threshold: an outreach states its own deadline, and a task waiting inside the window it
        // asked for is not stuck. The threshold is the fallback for a parked row that named no
        // deadline at all.
        let row: (i64, i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
            r#"WITH tasks AS (
                   SELECT
                       count(*) FILTER (WHERE status = 'dead_letter') AS dead_lettered,
                       count(*) FILTER (
                           WHERE status = 'pending'
                             AND run_at < CURRENT_TIMESTAMP - $1::interval
                       ) AS queue_overdue,
                       count(*) FILTER (
                           WHERE status = 'processing'
                             AND lock_expires_at < CURRENT_TIMESTAMP
                       ) AS lease_expired,
                       count(*) FILTER (
                           WHERE status = 'pending_approval'
                             AND updated_at < CURRENT_TIMESTAMP - $2::interval
                       ) AS approval_overdue,
                       count(*) FILTER (
                           WHERE status = 'waiting_for_third_party_reply'
                             AND COALESCE(
                                     wait_expires_at,
                                     updated_at + $2::interval
                                 ) < CURRENT_TIMESTAMP
                       ) AS reply_overdue
                   FROM background_tasks
               ),
               deliveries AS (
                   SELECT
                       count(*) FILTER (WHERE status = 'dead_letter') AS dead_lettered,
                       count(*) FILTER (
                           WHERE status IN ('pending', 'retryable')
                             AND available_at < CURRENT_TIMESTAMP - $1::interval
                       ) AS overdue,
                       -- Nothing retries these: an ambiguous provider outcome is exactly what
                       -- must not be re-sent, so they stay until a reconciler or a human clears
                       -- them and are stuck by definition.
                       count(*) FILTER (WHERE status = 'outcome_unknown') AS unconfirmed
                   FROM message_deliveries
               )
               SELECT tasks.dead_lettered, tasks.queue_overdue, tasks.lease_expired,
                      tasks.approval_overdue, tasks.reply_overdue,
                      deliveries.dead_lettered, deliveries.overdue, deliveries.unconfirmed
               FROM tasks, deliveries"#,
        )
        .bind(
            PgInterval::try_from(thresholds.queue_overdue_after()).map_err(|error| {
                AppError::Internal(format!("Invalid queue-overdue threshold: {error}"))
            })?,
        )
        .bind(
            PgInterval::try_from(thresholds.parked_overdue_after()).map_err(|error| {
                AppError::Internal(format!("Invalid parked-overdue threshold: {error}"))
            })?,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(StuckWorkCensus {
            dead_lettered: row.0,
            queue_overdue: row.1,
            lease_expired: row.2,
            approval_overdue: row.3,
            reply_overdue: row.4,
            delivery_dead_lettered: row.5,
            delivery_overdue: row.6,
            delivery_unconfirmed: row.7,
        })
    }

    async fn reap_expired_task_leases(&self) -> AppResult<u64> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;

        // Hits `background_tasks_processing_lease_idx`. No worker guard: the lease is expired, so
        // by definition no run still holds it.
        // The sweep is one statement over rows held by different workers, so the attribution
        // cannot come from a value the caller knows. `worker_id` on the right-hand side is read
        // from the old row version, which makes each event name the worker that lost *that* lease.
        let reaped = sqlx::query_as::<_, (Uuid, i32)>(
            r#"UPDATE background_tasks
               SET transition_reason = 'lease_lost',
                   transition_actor_kind = 'worker',
                   transition_actor_id = worker_id,
                   transition_approval_id = NULL,
                   transition_outreach_id = NULL,
                   retry_count = retry_count + 1,
                   status = CASE
                       WHEN retry_count + 1 >= max_retries THEN 'dead_letter'
                       ELSE 'pending'
                   END,
                   last_error = $1,
                   -- The same exponential backoff a reported failure gets, with the exponent
                   -- capped so it cannot run away: 30s * 2^attempt.
                   run_at = CURRENT_TIMESTAMP
                       + (30 * POWER(2, LEAST(retry_count + 1, 10))) * INTERVAL '1 second',
                   worker_id = NULL,
                   execution_generation = NULL,
                   locked_at = NULL,
                   lock_expires_at = NULL,
                   updated_at = CURRENT_TIMESTAMP
               WHERE status = 'processing'
                 AND (lock_expires_at IS NULL OR lock_expires_at <= CURRENT_TIMESTAMP)
               RETURNING id, retry_count"#,
        )
        .bind(LEASE_EXPIRED_ERROR)
        .fetch_all(&mut *tx)
        .await
        .map_err(AppError::from)?;

        // Close each reaped run's ledger row. `retry_count` was just incremented, and the attempt
        // that vanished was numbered with the value it now holds -- attempt N is the run made
        // after N-1 failures.
        for (task_id, retry_count) in &reaped {
            sqlx::query(
                r#"UPDATE task_attempts
                   SET status = $3,
                       error = $4,
                       stop_reason = $5,
                       finished_at = CURRENT_TIMESTAMP
                   WHERE task_id = $1 AND attempt_number = $2 AND status = 'processing'"#,
            )
            .bind(task_id)
            .bind(retry_count)
            .bind(TaskAttemptStatus::Failed.as_str())
            .bind(LEASE_EXPIRED_ERROR)
            .bind(TaskStopReason::LeaseLost.as_str())
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
        }

        tx.commit().await.map_err(AppError::from)?;
        Ok(reaped.len() as u64)
    }

    async fn enqueue_delivery(&self, delivery: NewDelivery) -> AppResult<DeliveryCreation> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let created = insert_delivery_on(&mut tx, &delivery).await?;
        tx.commit().await.map_err(AppError::from)?;
        Ok(created)
    }

    async fn get_outreach_thread_for_delivery(
        &self,
        delivery_id: DeliveryId,
    ) -> AppResult<Option<Uuid>> {
        sqlx::query_scalar(
            r#"SELECT task.thread_id
                 FROM task_outreach_targets AS target
                 JOIN task_outreaches AS outreach ON outreach.id = target.outreach_id
                 JOIN background_tasks AS task ON task.id = outreach.task_id
                WHERE target.delivery_id = $1"#,
        )
        .bind(delivery_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)
        .map(Option::flatten)
    }

    async fn record_outreach_request_message(
        &self,
        delivery_id: DeliveryId,
        write: &MessageWrite,
    ) -> AppResult<CanonicalMessageId> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let stored =
            crate::adapters::persistence::thread::insert_message_on(&mut tx, write).await?;
        sqlx::query(
            "UPDATE task_outreach_targets SET request_message_id = $2 WHERE delivery_id = $1",
        )
        .bind(delivery_id.as_uuid())
        .bind(stored.canonical_id.as_uuid())
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        tx.commit().await.map_err(AppError::from)?;
        Ok(stored.canonical_id)
    }

    async fn enqueue_task(&self, new_task: NewTask) -> AppResult<BackgroundTask> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let task = insert_task(&mut tx, new_task).await?;
        tx.commit().await.map_err(AppError::from)?;
        Ok(task)
    }

    async fn get_task_by_id(&self, id: Uuid) -> AppResult<Option<BackgroundTask>> {
        get_task_by_id_on(&self.pool, id).await
    }

    async fn ask_owner_to_act(
        &self,
        command: &crate::entities::internal_note::AskOwnerToAct,
        actor: PrincipalId,
    ) -> AppResult<crate::entities::internal_note::AskOwnerOutcome> {
        super::instructions::ask_owner_to_act(&self.pool, command, actor).await
    }

    async fn start_agent_task(
        &self,
        command: &crate::entities::internal_note::StartAgentTask,
        actor: PrincipalId,
    ) -> AppResult<BackgroundTask> {
        super::instructions::start_agent_task(&self.pool, command, actor).await
    }

    async fn claim_agent_instruction_notes(
        &self,
        company_id: Uuid,
        thread_id: Uuid,
        lease: TaskLeaseRef,
    ) -> AppResult<Vec<crate::entities::internal_note::AgentInstructionNote>> {
        super::instructions::claim_agent_instruction_notes(&self.pool, company_id, thread_id, lease)
            .await
    }

    async fn owned_agent_execution(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
        lease: TaskLeaseRef,
    ) -> AppResult<Option<OwnedAgentExecution>> {
        let Some(owner_id) = lease.claimed_owner.agent_principal_id() else {
            return Ok(None);
        };
        let ownership_version = i64::try_from(lease.ownership_version)
            .map_err(|_| AppError::Conflict("Ownership version exhausted.".into()))?;
        let row = sqlx::query_as::<_, (Uuid, Option<String>)>(
            r#"SELECT principal.agent_id,
                      (SELECT event.handoff_instruction
                       FROM task_ownership_events AS event
                       WHERE event.task_id = task.id
                         AND event.new_owner_principal_id = task.owner_principal_id
                         AND event.operation = 'transfer'
                       ORDER BY event.sequence DESC LIMIT 1) AS handoff_instruction
               FROM background_tasks AS task
               JOIN principals AS principal
                 ON principal.company_id = task.company_id
                AND principal.id = task.owner_principal_id
                AND principal.kind = 'agent'
               JOIN channel_agents AS assignment
                 ON assignment.company_id = task.company_id
                AND assignment.channel_id = task.channel_id
                AND assignment.agent_id = principal.agent_id
               WHERE task.company_id = $1 AND task.channel_id = $2 AND task.id = $3
                 AND task.status = 'processing' AND task.worker_id = $4
                 AND task.execution_generation = $5
                 AND task.owner_principal_id = $6 AND task.ownership_version = $7
                 AND task.lock_expires_at > CURRENT_TIMESTAMP"#,
        )
        .bind(company_id)
        .bind(channel_id)
        .bind(lease.task_id)
        .bind(lease.worker_id)
        .bind(lease.execution_generation)
        .bind(owner_id.as_uuid())
        .bind(ownership_version)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(
            row.map(|(agent_id, handoff_instruction)| OwnedAgentExecution {
                agent_id,
                handoff_instruction,
            }),
        )
    }

    async fn change_task_ownership(
        &self,
        command: TaskOwnershipCommand,
    ) -> AppResult<TaskOwnershipEvent> {
        change_task_ownership_on(&self.pool, command, None).await
    }

    async fn change_task_ownership_with_notification(
        &self,
        command: TaskOwnershipCommand,
        notification: Option<crate::transport::NewStandaloneDelivery>,
    ) -> AppResult<TaskOwnershipEvent> {
        change_task_ownership_on(&self.pool, command, notification).await
    }

    async fn assignment_notification_recipient(
        &self,
        company_id: Uuid,
        principal_id: PrincipalId,
    ) -> AppResult<Option<AssignmentNotificationRecipient>> {
        let recipient: Option<(Uuid, String)> = sqlx::query_as(
            r#"SELECT users.id, users.email
               FROM principals AS principal
               JOIN users ON users.id = principal.user_id
               LEFT JOIN user_notification_preferences AS preference
                 ON preference.user_id = users.id
               WHERE principal.company_id = $1 AND principal.id = $2
                 AND principal.kind = 'person'
                 AND COALESCE(preference.task_assignment_email_enabled, TRUE)"#,
        )
        .bind(company_id)
        .bind(principal_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(
            recipient.map(|(user_id, email)| AssignmentNotificationRecipient {
                user_id,
                email: email.into(),
            }),
        )
    }

    async fn list_task_ownership_events(
        &self,
        company_id: Uuid,
        task_id: Uuid,
    ) -> AppResult<Vec<TaskOwnershipEvent>> {
        list_task_ownership_events_on(&self.pool, company_id, task_id).await
    }

    async fn resolve_task_owner_target(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
        target: TaskOwnerTarget,
    ) -> AppResult<Option<TaskOwner>> {
        let (kind, subject_id) = match target {
            TaskOwnerTarget::HumanUser(id) => ("person", id),
            TaskOwnerTarget::Agent(id) => ("agent", id),
        };
        let principal_id: Option<Uuid> = sqlx::query_scalar(
            r#"SELECT principal.id
               FROM principals AS principal
               WHERE principal.company_id = $1 AND principal.kind = $2
                 AND (($2 = 'person' AND principal.user_id = $3)
                      OR ($2 = 'agent' AND principal.agent_id = $3))
                 AND ($2 <> 'agent' OR EXISTS (
                     SELECT 1 FROM channel_agents AS assignment
                     WHERE assignment.company_id = principal.company_id
                       AND assignment.channel_id = $4
                       AND assignment.agent_id = principal.agent_id
                 ))"#,
        )
        .bind(company_id)
        .bind(kind)
        .bind(subject_id)
        .bind(channel_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(principal_id.map(|id| match target {
            TaskOwnerTarget::HumanUser(_) => TaskOwner::Human(PrincipalId::new(id)),
            TaskOwnerTarget::Agent(_) => TaskOwner::Agent(PrincipalId::new(id)),
        }))
    }

    async fn list_task_owner_candidates(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Vec<TaskOwnerCandidate>> {
        let rows: Vec<(Uuid, String, String)> = sqlx::query_as(
            r#"SELECT principal.id, principal.kind, principal.display_label
               FROM principals AS principal
               JOIN channels AS channel
                 ON channel.company_id = principal.company_id AND channel.id = $2
               LEFT JOIN company_members AS member
                 ON member.company_id = principal.company_id
                AND member.user_id = principal.user_id
               WHERE principal.company_id = $1 AND (
                   (principal.kind = 'agent' AND EXISTS (
                       SELECT 1 FROM channel_agents AS assignment
                       WHERE assignment.company_id = principal.company_id
                         AND assignment.channel_id = $2
                         AND assignment.agent_id = principal.agent_id
                   ))
                   OR
                   (principal.kind = 'person' AND member.user_id IS NOT NULL AND (
                       member.role = 'owner' OR channel.access_mode IN ('team', 'public')
                       OR EXISTS (
                           SELECT 1 FROM channel_principal_grants AS channel_grant
                           WHERE channel_grant.company_id = principal.company_id
                             AND channel_grant.channel_id = $2
                             AND channel_grant.principal_id = principal.id
                             AND channel_grant.capability = 'view'
                       )
                   ))
               )
               ORDER BY principal.kind DESC, lower(principal.display_label), principal.id"#,
        )
        .bind(company_id)
        .bind(channel_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;
        rows.into_iter()
            .map(|(id, kind, label)| {
                let owner = match kind.as_str() {
                    "person" => TaskOwner::Human(PrincipalId::new(id)),
                    "agent" => TaskOwner::Agent(PrincipalId::new(id)),
                    other => {
                        return Err(AppError::Internal(format!(
                            "Invalid eligible owner kind: {other}"
                        )));
                    }
                };
                Ok(TaskOwnerCandidate { owner, label })
            })
            .collect()
    }

    async fn list_task_attempts(
        &self,
        company_id: Uuid,
        task_id: Uuid,
    ) -> AppResult<Vec<TaskAttemptRecord>> {
        let rows = sqlx::query_as::<_, TaskAttemptRecordDb>(
            r#"SELECT attempt.attempt_number, attempt.status, attempt.error,
                      attempt.stop_reason, attempt.prompt_tokens, attempt.completion_tokens,
                      attempt.result, attempt.started_at, attempt.finished_at,
                      attempt.execution_generation, attempt.worker_id, attempt.machine_id,
                      attempt.machine_region
               FROM task_attempts AS attempt
               JOIN background_tasks AS task ON task.id = attempt.task_id
               WHERE task.company_id = $1 AND attempt.task_id = $2
               ORDER BY attempt.attempt_number"#,
        )
        .bind(company_id)
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        rows.into_iter().map(TryInto::try_into).collect()
    }

    /// The board's six columns, projected fresh on every render.
    ///
    /// `eligible` selects at row level — "unfinished, or touched since the cutoff" — before any
    /// aggregate runs. That matters because every predicate in `staged` is an aggregate over a
    /// `GROUP BY correlation_id` that Postgres cannot push below its own grouping, so on its own it
    /// prunes only *after* scanning every task the company has ever run.
    async fn list_task_chain_board(
        &self,
        company_id: Uuid,
        filter: TaskBoardFilter,
        visible_channel_ids: &[Uuid],
    ) -> AppResult<TaskChainBoard> {
        chain_board_on(&self.pool, company_id, filter, visible_channel_ids).await
    }

    async fn list_task_status_events(
        &self,
        company_id: Uuid,
        correlation_id: CorrelationId,
        cursor: Option<TaskStatusEventCursor>,
        limit: usize,
    ) -> AppResult<Vec<TaskStatusEvent>> {
        chain_status_events_on(&self.pool, company_id, correlation_id, cursor, limit).await
    }

    async fn get_task_chain_detail(
        &self,
        scope: CollaborationReadScope<'_>,
        correlation_id: CorrelationId,
    ) -> AppResult<Option<TaskChainDetail>> {
        chain_detail_on(&self.pool, scope, correlation_id).await
    }
    async fn list_task_channel_targets(
        &self,
        company_id: Uuid,
        task_id: Uuid,
    ) -> AppResult<Vec<TaskChannelTarget>> {
        let rows: Vec<(Uuid, Uuid, String)> = sqlx::query_as(
            r#"SELECT target.channel_id, target.thread_id, target.recipient_role
               FROM task_channel_targets AS target
               WHERE target.company_id = $1 AND target.task_id = $2
               ORDER BY target.position, target.channel_id"#,
        )
        .bind(company_id)
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        rows.into_iter()
            .map(|(channel_id, thread_id, recipient_role)| {
                Ok(TaskChannelTarget {
                    channel_id,
                    thread_id,
                    recipient_role: RecipientRole::parse(&recipient_role)?,
                })
            })
            .collect()
    }

    async fn commit_agent_dispatch(
        &self,
        commit: AgentDispatchCommit<'_>,
    ) -> AppResult<DispatchCommit> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;

        // The fence goes first. If this run no longer owns the task the transaction rolls back
        // having written nothing, rather than queueing an email for work someone else has taken
        // over. Every other write below is unguarded precisely because this one guards them all.
        let fenced = sqlx::query(
            r#"UPDATE background_tasks
               SET payload = $1, updated_at = CURRENT_TIMESTAMP
               WHERE id = $2 AND status = 'processing' AND worker_id = $3
                  AND execution_generation = $4
                  AND owner_principal_id = $5 AND ownership_version = $6
                  AND lock_expires_at > CURRENT_TIMESTAMP"#,
        )
        .bind(commit.payload)
        .bind(commit.lease.task_id)
        .bind(commit.lease.worker_id)
        .bind(commit.lease.execution_generation)
        .bind(
            commit
                .lease
                .claimed_owner
                .agent_principal_id()
                .map(PrincipalId::as_uuid),
        )
        .bind(
            i64::try_from(commit.lease.ownership_version)
                .map_err(|_| AppError::Conflict("Ownership version exhausted.".into()))?,
        )
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        if fenced.rows_affected() != 1 {
            return Ok(DispatchCommit::LeaseLost);
        }

        // One canonical row, then one association per further thread the reply answered. Writing
        // the message again per thread is what used to make "the answer" several different rows
        // that had to be kept identical to still read as one.
        let stored =
            crate::adapters::persistence::thread::insert_message_on(&mut tx, &commit.reply.message)
                .await?;
        for &thread_id in &commit.reply.also_in_threads {
            crate::adapters::persistence::thread::associate_message_on(
                &mut tx,
                thread_id,
                stored.canonical_id,
                commit.reply.message.entry_kind,
            )
            .await?;
        }

        // The reply's deliveries, in the same transaction as the reply itself. The unique index
        // on `(destination_binding_id, idempotency_key)` is the lock: a superseded run of this
        // task computes the same keys, so its inserts are absorbed onto the deliveries that exist
        // rather than queueing a second copy of the same answer.
        let mut deliveries = Vec::with_capacity(commit.deliveries.len());
        for delivery in &commit.deliveries {
            deliveries.push(insert_delivery_on(&mut tx, delivery).await?);
        }

        if commit.complete_outreach {
            sqlx::query(
                r#"UPDATE task_outreaches SET status = 'completed', updated_at = CURRENT_TIMESTAMP
                   WHERE task_id = $1 AND status IN ('threshold_met', 'proceed_partial')"#,
            )
            .bind(commit.lease.task_id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
        }

        tx.commit().await.map_err(AppError::from)?;
        Ok(DispatchCommit::Committed { deliveries })
    }

    async fn complete_human_task(
        &self,
        completion: HumanTaskCompletion<'_>,
    ) -> AppResult<HumanTaskCompletionResult> {
        let expected_version = i64::try_from(completion.expected_ownership_version)
            .map_err(|_| AppError::Conflict("Ownership version exhausted.".into()))?;
        let draft_version = i32::try_from(completion.draft_version)
            .map_err(|_| AppError::BadRequest("Draft version is out of range.".into()))?;
        let Some(publish_delivery) = completion.deliveries.first() else {
            return Err(AppError::BadRequest(
                "A published draft requires exactly one delivery.".into(),
            ));
        };
        if completion.deliveries.len() != 1
            || !completion.message.audience.is_externally_deliverable()
            || completion
                .deliveries
                .iter()
                .any(|delivery| !delivery.message_audience.is_externally_deliverable())
        {
            return Err(AppError::BadRequest(
                "Only an external-conversation message can be published.".into(),
            ));
        }
        let recipient_snapshot =
            serde_json::to_value(&completion.recipient_snapshot).map_err(|error| {
                AppError::Internal(format!("Failed to serialize draft recipients: {error}"))
            })?;
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let task: Option<(String, Option<Uuid>, Option<String>, i64, Option<Uuid>)> =
            sqlx::query_as(
                r#"SELECT status, owner_principal_id, owner_principal_kind,
                          ownership_version, thread_id
                   FROM background_tasks
                   WHERE company_id = $1 AND id = $2
                   FOR UPDATE"#,
            )
            .bind(completion.company_id)
            .bind(completion.task_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(AppError::from)?;
        let (status, owner_id, owner_kind, version, thread_id) =
            task.ok_or_else(|| AppError::NotFound("Task not found.".into()))?;

        let prior: Option<(Uuid, Uuid, String)> = sqlx::query_as(
            r#"SELECT message_id, command_id, command_fingerprint
               FROM human_task_completions WHERE task_id = $1"#,
        )
        .bind(completion.task_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::from)?;
        if let Some((message_id, command_id, fingerprint)) = prior {
            if command_id != completion.command_id || fingerprint != completion.command_fingerprint
            {
                return Err(AppError::Conflict(
                    "This task was already completed with another response.".into(),
                ));
            }
            return Ok(HumanTaskCompletionResult {
                message_id: CanonicalMessageId::new(message_id),
                deliveries: Vec::new(),
            });
        }

        if status != "pending" {
            return Err(AppError::Conflict(
                "Only pending work can be completed by its human owner.".into(),
            ));
        }
        if owner_id != Some(completion.owner_principal_id.as_uuid())
            || owner_kind.as_deref() != Some("person")
        {
            return Err(AppError::NotFound("Task not found.".into()));
        }
        if version != expected_version {
            return Err(AppError::Conflict(format!(
                "Ownership changed from version {} to {}; refresh and try again.",
                completion.expected_ownership_version, version
            )));
        }
        if thread_id != Some(completion.message.thread_id)
            || completion.deliveries.iter().any(|delivery| {
                delivery.company_id != completion.company_id
                    || delivery.task_id != Some(completion.task_id)
                    || delivery.message_id != completion.message.id
            })
        {
            return Err(AppError::BadRequest(
                "The completion response does not belong to this task.".into(),
            ));
        }

        let latest = sqlx::query_as::<_, ResponseDraftDb>(
            r#"SELECT version, status, channel_id, thread_id, task_id, author_principal_id,
                      subject, body, recipient_snapshot
                 FROM response_drafts
                WHERE company_id = $1 AND id = $2
                ORDER BY version DESC
                LIMIT 1
                FOR UPDATE"#,
        )
        .bind(completion.company_id)
        .bind(completion.draft_id.as_uuid())
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::from)?;
        match latest {
            Some(draft)
                if draft.version == draft_version
                    && draft.status == "active"
                    && draft.channel_id == publish_delivery.channel_id
                    && draft.thread_id == completion.message.thread_id
                    && draft.task_id == Some(completion.task_id)
                    && draft.author_principal_id == completion.owner_principal_id.as_uuid()
                    && draft.subject == completion.message.subject
                    && draft.body == completion.message.clean_text_body
                    && draft.recipient_snapshot == recipient_snapshot => {}
            Some(_) => {
                return Err(AppError::Conflict(
                    "This draft version is stale, superseded, changed, or already published."
                        .into(),
                ));
            }
            None if draft_version == 1 => {
                sqlx::query(
                    r#"INSERT INTO response_drafts (
                           id, version, company_id, channel_id, thread_id, task_id,
                           author_principal_id, subject, body, recipient_snapshot, status
                       ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'active')"#,
                )
                .bind(completion.draft_id.as_uuid())
                .bind(draft_version)
                .bind(completion.company_id)
                .bind(publish_delivery.channel_id)
                .bind(completion.message.thread_id)
                .bind(completion.task_id)
                .bind(completion.owner_principal_id.as_uuid())
                .bind(&completion.message.subject)
                .bind(&completion.message.clean_text_body)
                .bind(&recipient_snapshot)
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;
            }
            None => {
                return Err(AppError::Conflict(
                    "A draft cannot start at a version other than one.".into(),
                ));
            }
        }

        let stored = insert_message_on(&mut tx, completion.message).await?;
        let mut deliveries = Vec::with_capacity(completion.deliveries.len());
        for delivery in &completion.deliveries {
            deliveries.push(insert_delivery_on(&mut tx, delivery).await?);
        }
        let published_delivery_id = deliveries
            .first()
            .map(|creation| creation.delivery_id())
            .ok_or_else(|| AppError::BadRequest("A published draft requires a delivery.".into()))?;
        let advanced = sqlx::query(
            r#"UPDATE response_drafts
                  SET status = 'published', updated_at = CURRENT_TIMESTAMP
                WHERE company_id = $1 AND id = $2 AND version = $3 AND status = 'active'"#,
        )
        .bind(completion.company_id)
        .bind(completion.draft_id.as_uuid())
        .bind(draft_version)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        if advanced.rows_affected() != 1 {
            return Err(AppError::Conflict(
                "This draft version is stale, superseded, or already published.".into(),
            ));
        }
        sqlx::query(
            r#"INSERT INTO response_draft_publications (
                   company_id, draft_id, draft_version, message_id, delivery_id,
                   published_by_principal_id
               ) VALUES ($1, $2, $3, $4, $5, $6)"#,
        )
        .bind(completion.company_id)
        .bind(completion.draft_id.as_uuid())
        .bind(draft_version)
        .bind(stored.canonical_id.as_uuid())
        .bind(published_delivery_id.as_uuid())
        .bind(completion.owner_principal_id.as_uuid())
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        sqlx::query(
            r#"UPDATE background_tasks
               SET status = 'completed', transition_reason = 'completed',
                   transition_actor_kind = 'human', transition_actor_id = $3,
                   transition_approval_id = NULL, transition_outreach_id = NULL,
                   worker_id = NULL, execution_generation = NULL, locked_at = NULL,
                   lock_expires_at = NULL, wait_expires_at = NULL,
                   updated_at = CURRENT_TIMESTAMP
               WHERE company_id = $1 AND id = $2"#,
        )
        .bind(completion.company_id)
        .bind(completion.task_id)
        .bind(completion.owner_principal_id.as_uuid())
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        sqlx::query(
            r#"INSERT INTO human_task_completions (
                   task_id, company_id, command_id, command_fingerprint,
                   owner_principal_id, ownership_version, message_id
               ) VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
        )
        .bind(completion.task_id)
        .bind(completion.company_id)
        .bind(completion.command_id)
        .bind(completion.command_fingerprint)
        .bind(completion.owner_principal_id.as_uuid())
        .bind(expected_version)
        .bind(stored.canonical_id.as_uuid())
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        tx.commit().await.map_err(AppError::from)?;
        Ok(HumanTaskCompletionResult {
            message_id: stored.canonical_id,
            deliveries,
        })
    }

    async fn renew_task_lease(
        &self,
        lease: TaskLeaseRef,
        lock_expires_at: DateTime<Utc>,
    ) -> AppResult<bool> {
        let result = sqlx::query(
            r#"UPDATE background_tasks
               SET lock_expires_at = $3, updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND status = 'processing' AND worker_id = $2
                 AND execution_generation = $4
                 AND owner_principal_id = $5 AND ownership_version = $6
                 AND lock_expires_at > CURRENT_TIMESTAMP"#,
        )
        .bind(lease.task_id)
        .bind(lease.worker_id)
        .bind(lock_expires_at)
        .bind(lease.execution_generation)
        .bind(
            lease
                .claimed_owner
                .agent_principal_id()
                .map(PrincipalId::as_uuid),
        )
        .bind(
            i64::try_from(lease.ownership_version)
                .map_err(|_| AppError::Conflict("Ownership version exhausted.".into()))?,
        )
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(result.rows_affected() == 1)
    }

    async fn begin_task_attempt(
        &self,
        attempt: TaskAttemptRef,
        machine: &MachineIdentity,
    ) -> AppResult<()> {
        sqlx::query(BEGIN_ATTEMPT_SQL)
            .bind(Uuid::new_v4())
            .bind(attempt.task_id)
            .bind(attempt.attempt_number)
            .bind(attempt.execution_generation)
            .bind(attempt.worker_id)
            .bind(machine.id.as_str())
            .bind(machine.region.as_ref().map(MachineRegion::as_str))
            .execute(&self.pool)
            .await
            .map_err(AppError::from)?;
        Ok(())
    }

    async fn finish_task_attempt(&self, outcome: &TaskAttemptOutcome) -> AppResult<bool> {
        // `usize` tokens into an `INTEGER` column: clamp rather than wrap, so an absurd count is
        // recorded as saturated instead of negative — the CHECK constraint rejects negatives.
        let tokens = |pick: fn(&TokenUsage) -> usize| {
            outcome
                .tokens
                .as_ref()
                .map(|usage| i32::try_from(pick(usage)).unwrap_or(i32::MAX))
        };

        let result = sqlx::query(FINISH_ATTEMPT_SQL)
            .bind(outcome.attempt.task_id)
            .bind(outcome.attempt.attempt_number)
            .bind(outcome.attempt.execution_generation)
            .bind(outcome.status.as_str())
            .bind(outcome.error.as_deref())
            .bind(tokens(|usage| usage.prompt_tokens))
            .bind(tokens(|usage| usage.completion_tokens))
            .bind(outcome.stop_reason.as_str())
            .execute(&self.pool)
            .await
            .map_err(AppError::from)?;

        Ok(result.rows_affected() == 1)
    }

    async fn claim_pending_tasks(
        &self,
        worker_id: Uuid,
        lock_expires_at: DateTime<Utc>,
        limit: i64,
    ) -> AppResult<Vec<BackgroundTask>> {
        let db_list = sqlx::query_as::<_, BackgroundTaskDb>(
            // Pending rows only. An expired `processing` row used to be stolen right here, which
            // re-ran it without spending an attempt, without closing the open attempt and
            // without any backoff -- so a task that reliably outlived its lease looped for ever
            // instead of dead-lettering. `reap_expired_task_leases` now turns those back into
            // pending rows, paying an attempt each time.
            r#"WITH ranked AS (
                   SELECT id, run_at, created_at,
                          ROW_NUMBER() OVER (
                              PARTITION BY company_id
                              ORDER BY run_at ASC, created_at ASC, id ASC
                          ) AS company_round
                   FROM background_tasks
                   WHERE status = 'pending' AND run_at <= CURRENT_TIMESTAMP
                     AND owner_principal_kind = 'agent'
                     AND EXISTS (
                         SELECT 1 FROM principals AS owner
                         JOIN channel_agents AS assignment
                           ON assignment.company_id = background_tasks.company_id
                          AND assignment.channel_id = background_tasks.channel_id
                          AND assignment.agent_id = owner.agent_id
                         WHERE owner.company_id = background_tasks.company_id
                           AND owner.id = background_tasks.owner_principal_id
                           AND owner.kind = 'agent'
                     )
               ), claimable AS (
                   SELECT task.id
                   FROM background_tasks AS task
                   JOIN ranked ON ranked.id = task.id
                   ORDER BY ranked.company_round ASC, ranked.run_at ASC,
                            ranked.created_at ASC, task.id ASC
                   FOR UPDATE SKIP LOCKED
                   LIMIT $1
               )
               UPDATE background_tasks AS task
               SET status = 'processing',
                   worker_id = $2,
                   execution_generation = gen_random_uuid(),
                   locked_at = CURRENT_TIMESTAMP,
                   lock_expires_at = $3,
                   updated_at = CURRENT_TIMESTAMP,
                   transition_reason = 'claimed',
                   transition_actor_kind = 'worker',
                   transition_actor_id = $2,
                   transition_approval_id = NULL,
                   transition_outreach_id = NULL
               FROM claimable
               WHERE task.id = claimable.id
               RETURNING task.id, task.company_id, task.channel_id, task.thread_id,
                         task.correlation_id, task.task_type, task.status, task.payload,
                         task.retry_count,
                         task.max_retries, task.last_error, task.owner_principal_id,
                         task.owner_principal_kind, task.ownership_version, task.worker_id, task.execution_generation, task.locked_at,
                         task.lock_expires_at, task.run_at, task.created_at, task.updated_at"#,
        )
        .bind(limit)
        .bind(worker_id)
        .bind(lock_expires_at)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        let mut tasks = Vec::new();
        for db in db_list {
            tasks.push(db.try_into()?);
        }
        Ok(tasks)
    }

    async fn claim_task(
        &self,
        id: Uuid,
        worker_id: Uuid,
        lock_expires_at: DateTime<Utc>,
    ) -> AppResult<bool> {
        let res = sqlx::query(CLAIM_TASK_SQL)
            .bind(id)
            .bind(worker_id)
            .bind(lock_expires_at)
            .execute(&self.pool)
            .await
            .map_err(AppError::from)?;

        Ok(res.rows_affected() > 0)
    }

    async fn mark_task_completed(&self, lease: TaskLeaseRef) -> AppResult<bool> {
        let result = sqlx::query(
            r#"UPDATE background_tasks
               SET status = 'completed', worker_id = NULL, execution_generation = NULL, locked_at = NULL,
                   lock_expires_at = NULL, updated_at = CURRENT_TIMESTAMP,
                   transition_reason = 'completed', transition_actor_kind = 'worker',
                   transition_actor_id = $2, transition_approval_id = NULL,
                   transition_outreach_id = NULL
               WHERE id = $1 AND status = 'processing' AND worker_id = $2
                 AND execution_generation = $3
                 AND owner_principal_id = $4 AND ownership_version = $5
                 AND lock_expires_at > CURRENT_TIMESTAMP"#,
        )
        .bind(lease.task_id)
        .bind(lease.worker_id)
        .bind(lease.execution_generation)
        .bind(
            lease
                .claimed_owner
                .agent_principal_id()
                .map(PrincipalId::as_uuid),
        )
        .bind(i64::try_from(lease.ownership_version).map_err(|_| {
            AppError::Conflict("Ownership version exhausted.".into())
        })?)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(result.rows_affected() > 0)
    }

    async fn mark_task_failed(&self, failure: TaskFailure<'_>) -> AppResult<bool> {
        mark_task_failed_on(&self.pool, failure).await
    }

    async fn stop_task(&self, id: Uuid, actor: StopActor) -> AppResult<BackgroundTask> {
        stop_task_on(&self.pool, id, actor).await
    }

    async fn resume_task(&self, id: Uuid, actor: ResumeActor) -> AppResult<BackgroundTask> {
        resume_task_on(&self.pool, id, actor).await
    }

    async fn list_company_tasks(
        &self,
        company_id: Uuid,
        channel_id: Option<Uuid>,
        status: Option<TaskStatus>,
        sort_asc: bool,
    ) -> AppResult<Vec<BackgroundTask>> {
        self.list_company_tasks_page(company_id, channel_id, status, sort_asc, 0, 200)
            .await
    }

    async fn list_company_tasks_page(
        &self,
        company_id: Uuid,
        channel_id: Option<Uuid>,
        status: Option<TaskStatus>,
        sort_asc: bool,
        offset: i64,
        limit: i64,
    ) -> AppResult<Vec<BackgroundTask>> {
        let mut query = QueryBuilder::<Postgres>::new(
            r#"SELECT id, company_id, channel_id, thread_id, correlation_id, task_type, status, payload,
                      retry_count, max_retries, last_error, owner_principal_id,
                      owner_principal_kind, ownership_version, worker_id, execution_generation, locked_at, lock_expires_at,
                      run_at, created_at, updated_at
               FROM background_tasks WHERE company_id = "#,
        );
        query.push_bind(company_id);
        if let Some(channel_id) = channel_id {
            query.push(" AND channel_id = ").push_bind(channel_id);
        }
        if let Some(status) = status {
            query.push(" AND status = ").push_bind(status.as_str());
        }
        if sort_asc {
            query.push(" ORDER BY created_at ASC, id ASC");
        } else {
            query.push(" ORDER BY created_at DESC, id DESC");
        }
        query
            .push(" LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);

        let db_list = query
            .build_query_as::<BackgroundTaskDb>()
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::from)?;

        let mut tasks = Vec::new();
        for db in db_list {
            tasks.push(db.try_into()?);
        }
        Ok(tasks)
    }

    async fn list_company_tasks_filtered_page(
        &self,
        company_id: Uuid,
        filter: &TaskFilter,
        visible_channel_ids: &[Uuid],
        offset: i64,
        limit: i64,
    ) -> AppResult<Vec<BackgroundTask>> {
        let mut query = QueryBuilder::<Postgres>::new(
            r#"SELECT id, company_id, channel_id, thread_id, correlation_id, task_type, status, payload,
                      retry_count, max_retries, last_error, owner_principal_id,
                      owner_principal_kind, ownership_version, worker_id, execution_generation,
                      locked_at, lock_expires_at, run_at, created_at, updated_at
               FROM background_tasks WHERE company_id = "#,
        );
        query.push_bind(company_id);
        query
            .push(" AND channel_id = ANY(")
            .push_bind(visible_channel_ids)
            .push(")");
        if let Some(channel_id) = filter.channel_id {
            query.push(" AND channel_id = ").push_bind(channel_id);
        }
        if let Some(status) = filter.status {
            query.push(" AND status = ").push_bind(status.as_str());
        }
        match filter.owner {
            Some(TaskOwnerFilter::Principal(principal_id)) => {
                query
                    .push(" AND owner_principal_id = ")
                    .push_bind(principal_id.as_uuid());
            }
            Some(TaskOwnerFilter::Unassigned) => {
                query.push(" AND owner_principal_id IS NULL");
            }
            None => {}
        }
        if filter.sort_asc {
            query.push(" ORDER BY created_at ASC, id ASC");
        } else {
            query.push(" ORDER BY created_at DESC, id DESC");
        }
        query
            .push(" LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);

        query
            .build_query_as::<BackgroundTaskDb>()
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::from)?
            .into_iter()
            .map(TryInto::try_into)
            .collect()
    }
}
