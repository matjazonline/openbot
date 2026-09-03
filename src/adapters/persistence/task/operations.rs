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
use serde_json::Value;
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
        correlation::CorrelationId,
        outreach::{DueOutreach, OutreachProgress, OutreachReplyMatch, OutreachStatus},
        stuck_work::{StuckWorkCensus, StuckWorkThresholds},
        task::{
            BackgroundTask, NewTask, ResumeActor, StopActor, TaskAttemptOutcome, TaskAttemptRecord,
            TaskAttemptRef, TaskAttemptStatus, TaskBoardFilter, TaskChainBoard, TaskChainDetail,
            TaskFailure, TaskLeaseRef, TaskStatus, TaskStatusEvent, TaskStatusEventCursor,
            TaskStopReason, TaskTransitionReason, ThreadActivity, TokenUsage, TransitionActor,
        },
        transport::DeliveryId,
        value_objects::MessageId,
    },
    transport::{DeliveryCreation, NewDelivery},
};

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

#[async_trait]
impl TaskPersistence for PostgresPersistence {
    async fn list_thread_activity(
        &self,
        thread_ids: &[Uuid],
    ) -> AppResult<HashMap<Uuid, ThreadActivity>> {
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
            r#"SELECT DISTINCT ON (thread_id) thread_id, status, lock_expires_at
               FROM background_tasks
               WHERE thread_id = ANY($1)
                 AND status IN ('pending', 'processing', 'pending_approval',
                                'waiting_for_third_party_reply', 'dead_letter', 'completed')
               ORDER BY thread_id,
                        status IN ('dead_letter', 'completed'),
                        updated_at DESC, id DESC"#,
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
                Ok((
                    row.thread_id,
                    ThreadActivity::from_task(status, row.lock_expires_at, now),
                ))
            })
            // `completed` is queried for its position in that ordering, not for a badge: reaching
            // here it means the thread's last word was a run that worked, which is nothing to
            // show. Dropping every `None` also keeps a status added to the query later from
            // turning into a badge nobody chose.
            .filter_map(|entry: AppResult<_>| match entry {
                Ok((thread_id, Some(activity))) => Some(Ok((thread_id, activity))),
                Ok((_, None)) => None,
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
                    id, task_id, outreach_key, status, required_threshold_percent,
                    expires_at, subject, body
               )
               SELECT $1, id, $2, 'waiting', $3, $4, $5, $6
               FROM background_tasks
               WHERE id = $7 AND company_id = $8
                 AND status = 'processing' AND worker_id = $9
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
        .bind(request.task_id)
        .bind(request.company_id)
        .bind(request.worker_id)
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

                sqlx::query(
                    r#"INSERT INTO task_outreach_targets
                           (outreach_id, email, delivery_id, request_message_id)
                       VALUES ($1, $2, $3, $4)"#,
                )
                .bind(outreach.id)
                .bind(target.email.as_str())
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
                     AND lock_expires_at > CURRENT_TIMESTAMP"#,
                attribution = attribution.set_clause(),
            ))
            .bind(outreach.expires_at)
            .bind(request.task_id)
            .bind(request.company_id)
            .bind(request.worker_id)
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
        response_message_id: Uuid,
    ) -> AppResult<OutreachProgress> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let mut outreach = sqlx::query_as::<_, OutreachDb>(
            r#"SELECT id, task_id, status,
                      required_threshold_percent::double precision AS required_threshold_percent,
                      expires_at
               FROM task_outreaches WHERE id = $1 FOR UPDATE"#,
        )
        .bind(matched.outreach_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::from)?;

        sqlx::query(
            r#"UPDATE task_outreach_targets
               SET responded_at = CURRENT_TIMESTAMP, response_message_id = $3
               WHERE outreach_id = $1 AND email = $2 AND responded_at IS NULL"#,
        )
        .bind(matched.outreach_id)
        .bind(matched.target_email.as_str())
        .bind(response_message_id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;

        let (target_count, response_count): (i64, i64) = sqlx::query_as(
            r#"SELECT COUNT(*)::bigint,
                      COUNT(*) FILTER (WHERE responded_at IS NOT NULL)::bigint
               FROM task_outreach_targets WHERE outreach_id = $1"#,
        )
        .bind(matched.outreach_id)
        .fetch_one(&mut *tx)
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
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
            cancel_unsent_outreach_questions(&mut tx, matched.outreach_id).await?;
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
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
            sqlx::query(
                r#"UPDATE human_approvals SET status = 'expired', updated_at = CURRENT_TIMESTAMP
                   WHERE task_id = $1 AND action_type = 'quorum_timeout'
                     AND status = 'pending'"#,
            )
            .bind(outreach.task_id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
            outreach.status = OutreachStatus::ThresholdMet.as_str().to_string();
        }
        tx.commit().await.map_err(AppError::from)?;

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
        let db = sqlx::query_as::<_, BackgroundTaskDb>(
            r#"SELECT id, company_id, channel_id, thread_id, correlation_id, task_type, status, payload,
                       retry_count, max_retries, last_error, worker_id, execution_generation, locked_at, lock_expires_at,
                       run_at, created_at, updated_at
               FROM background_tasks WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        match db {
            Some(d) => Ok(Some(d.try_into()?)),
            None => Ok(None),
        }
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
                      attempt.execution_generation
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
    ) -> AppResult<TaskChainBoard> {
        chain_board_on(&self.pool, company_id, filter).await
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
        company_id: Uuid,
        correlation_id: CorrelationId,
    ) -> AppResult<Option<TaskChainDetail>> {
        chain_detail_on(&self.pool, company_id, correlation_id).await
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

    async fn update_task_payload(&self, id: Uuid, payload: Value) -> AppResult<()> {
        sqlx::query(
            r#"UPDATE background_tasks
               SET payload = $1, updated_at = CURRENT_TIMESTAMP
               WHERE id = $2"#,
        )
        .bind(payload)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(())
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
                  AND lock_expires_at > CURRENT_TIMESTAMP"#,
        )
        .bind(commit.payload)
        .bind(commit.lease.task_id)
        .bind(commit.lease.worker_id)
        .bind(commit.lease.execution_generation)
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
                 AND lock_expires_at > CURRENT_TIMESTAMP"#,
        )
        .bind(lease.task_id)
        .bind(lease.worker_id)
        .bind(lock_expires_at)
        .bind(lease.execution_generation)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(result.rows_affected() == 1)
    }

    async fn begin_task_attempt(&self, attempt: TaskAttemptRef) -> AppResult<()> {
        sqlx::query(BEGIN_ATTEMPT_SQL)
            .bind(Uuid::new_v4())
            .bind(attempt.task_id)
            .bind(attempt.attempt_number)
            .bind(attempt.execution_generation)
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
                         task.max_retries, task.last_error, task.worker_id, task.execution_generation, task.locked_at,
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
                 AND lock_expires_at > CURRENT_TIMESTAMP"#,
        )
        .bind(lease.task_id)
        .bind(lease.worker_id)
        .bind(lease.execution_generation)
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
                      retry_count, max_retries, last_error, worker_id, execution_generation, locked_at, lock_expires_at,
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
}
