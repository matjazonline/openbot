use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::FromRow;
use std::str::FromStr;
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{
        schedule::{
            ChannelSchedule, ClaimedScheduleRun, ScheduleDeliveryMode, ScheduleRun,
            ScheduleTimezone, ScheduleType, ScheduleWrite,
        },
        task::TaskStatus,
        value_objects::EmailAddress,
    },
};

pub const SCHEDULE_COLUMNS: &str = r#"
    id, company_id, channel_id, name, schedule_type, interval_seconds,
    subject_template, prompt_template, delivery_mode, recipient_emails,
    timezone, enabled, last_run_at, next_run_at, last_error, created_at, updated_at
"#;

#[derive(FromRow, Debug)]
pub struct ScheduleDb {
    pub id: Uuid,
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub name: String,
    pub schedule_type: String,
    pub interval_seconds: Option<i64>,
    pub subject_template: String,
    pub prompt_template: String,
    pub delivery_mode: String,
    pub recipient_emails: Vec<String>,
    pub timezone: String,
    pub enabled: bool,
    pub last_run_at: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TryFrom<ScheduleDb> for ChannelSchedule {
    type Error = AppError;

    fn try_from(db: ScheduleDb) -> AppResult<Self> {
        let schedule_type = ScheduleType::from_str(&db.schedule_type)
            .map_err(|err| AppError::Internal(err.to_string()))?;
        let delivery_mode = ScheduleDeliveryMode::from_str(&db.delivery_mode)
            .map_err(|err| AppError::Internal(err.to_string()))?;
        let timezone = ScheduleTimezone::from_str(&db.timezone)
            .map_err(|err| AppError::Internal(err.to_string()))?;

        Ok(ChannelSchedule {
            id: db.id,
            company_id: db.company_id,
            channel_id: db.channel_id,
            name: db.name,
            schedule_type,
            interval_seconds: db.interval_seconds,
            subject_template: db.subject_template,
            prompt_template: db.prompt_template,
            delivery_mode,
            recipient_emails: db
                .recipient_emails
                .into_iter()
                .map(EmailAddress::from)
                .collect(),
            timezone,
            enabled: db.enabled,
            last_run_at: db.last_run_at,
            next_run_at: db.next_run_at,
            last_error: db.last_error,
            created_at: db.created_at,
            updated_at: db.updated_at,
        })
    }
}

#[derive(FromRow, Debug)]
pub struct ScheduleRunDb {
    pub thread_id: Uuid,
    pub task_id: Uuid,
    pub channel_id: Uuid,
    pub subject: String,
    pub task_status: String,
    pub lock_expires_at: Option<DateTime<Utc>>,
    pub latest_response: Option<String>,
    pub message_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(FromRow, Debug)]
struct ClaimedScheduleRunDb {
    id: Uuid,
    materialization_generation: Uuid,
    scheduled_for: DateTime<Utc>,
    schedule_snapshot: serde_json::Value,
    thread_id: Option<Uuid>,
    task_id: Option<Uuid>,
}

impl TryFrom<ClaimedScheduleRunDb> for ClaimedScheduleRun {
    type Error = AppError;

    fn try_from(db: ClaimedScheduleRunDb) -> AppResult<Self> {
        let schedule = serde_json::from_value(db.schedule_snapshot).map_err(|err| {
            AppError::Internal(format!("Invalid durable schedule-run snapshot: {err}"))
        })?;
        Ok(Self {
            id: db.id,
            materialization_generation: db.materialization_generation,
            scheduled_for: db.scheduled_for,
            schedule,
            thread_id: db.thread_id,
            task_id: db.task_id,
        })
    }
}

impl TryFrom<ScheduleRunDb> for ScheduleRun {
    type Error = AppError;

    fn try_from(db: ScheduleRunDb) -> AppResult<Self> {
        let task_status = TaskStatus::from_str(&db.task_status)
            .map_err(|err| AppError::Internal(err.to_string()))?;

        Ok(ScheduleRun {
            thread_id: db.thread_id,
            task_id: db.task_id,
            channel_id: db.channel_id,
            subject: db.subject,
            task_status,
            lock_expires_at: db.lock_expires_at,
            latest_response: db.latest_response,
            message_count: db.message_count,
            created_at: db.created_at,
            updated_at: db.updated_at,
        })
    }
}

/// Whether this edit moves the schedule's rhythm, and so has to re-anchor the next run.
fn cadence_changed(existing: &ChannelSchedule, write: &ScheduleWrite) -> bool {
    if existing.schedule_type != write.schedule_type {
        return true;
    }
    match write.schedule_type {
        ScheduleType::Interval => existing.interval_seconds != write.interval_seconds,
        // A one-off carries its whole cadence in the chosen instant, so any move is a change. A
        // form that echoed the stored time back unchanged compares equal and stays put.
        ScheduleType::OneOff => write.scheduled_at != existing.next_run_at,
    }
}

/// The first run of a cadence that just changed: one interval out, or the chosen instant.
fn next_run_after_cadence_change(write: &ScheduleWrite) -> Option<DateTime<Utc>> {
    match write.schedule_type {
        ScheduleType::Interval => write
            .interval_seconds
            .map(|secs| Utc::now() + chrono::Duration::seconds(secs)),
        ScheduleType::OneOff => write.scheduled_at,
    }
}

#[async_trait]
pub trait SchedulePersistence: Send + Sync {
    async fn create(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
        write: ScheduleWrite,
    ) -> AppResult<ChannelSchedule>;

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<ChannelSchedule>>;

    /// Scoped by company as well as channel: the company is the tenant boundary, and leaving it
    /// to the caller is how a channel id from another tenant reads this one's schedules.
    async fn list_by_channel_id(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Vec<ChannelSchedule>>;

    async fn list_by_company_id(&self, company_id: Uuid) -> AppResult<Vec<ChannelSchedule>>;

    /// Takes the stored row rather than an id: whether the next run moves depends on what the
    /// cadence *was*, and the caller has already loaded it to authorize the write.
    async fn update(
        &self,
        existing: &ChannelSchedule,
        channel_id: Uuid,
        write: ScheduleWrite,
    ) -> AppResult<ChannelSchedule>;

    async fn delete(&self, id: Uuid) -> AppResult<()>;

    async fn set_enabled(&self, id: Uuid, enabled: bool) -> AppResult<bool>;

    /// Persists each logical slot and advances its schedule in one transaction. Previously
    /// persisted, unfinished slots are returned as well so a process restart resumes them.
    async fn claim_and_advance_due_schedules(
        &self,
        worker_id: Uuid,
        lock_expires_at: DateTime<Utc>,
        limit: i64,
    ) -> AppResult<Vec<ClaimedScheduleRun>>;

    async fn record_run_task(
        &self,
        run_id: Uuid,
        worker_id: Uuid,
        generation: Uuid,
        task_id: Uuid,
    ) -> AppResult<bool>;

    async fn record_run_error(
        &self,
        run_id: Uuid,
        worker_id: Uuid,
        generation: Uuid,
        error: &str,
    ) -> AppResult<bool>;

    /// Updates `last_run_at` on a manual "run now" execution.
    async fn record_manual_run(&self, id: Uuid) -> AppResult<Option<ChannelSchedule>>;

    /// Hands a claimed schedule back when its run could not be launched, so the slot is retried
    /// rather than silently skipped, and records why on the row.
    async fn release_failed_claim(&self, schedule: &ChannelSchedule, error: &str) -> AppResult<()>;

    /// Clears `last_error` once a run launches, so a stale failure does not linger in the UI.
    async fn clear_last_error(&self, id: Uuid) -> AppResult<()>;

    /// Lists paginated execution runs (threads) triggered by this schedule.
    async fn list_schedule_runs(
        &self,
        schedule_id: Uuid,
        offset: i64,
        limit: i64,
    ) -> AppResult<Vec<ScheduleRun>>;

    /// Proves that a caller-selected thread was launched by this tenant's schedule.
    ///
    /// Scheduled runs are represented by their `scheduled_agent_run` task.  Timed runs also have
    /// a durable `schedule_runs` record, but a manual "run now" deliberately launches directly
    /// and therefore has no such record.
    async fn schedule_run_contains_thread(
        &self,
        company_id: Uuid,
        schedule_id: Uuid,
        thread_id: Uuid,
    ) -> AppResult<bool>;
}

#[async_trait]
impl SchedulePersistence for PostgresPersistence {
    async fn create(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
        write: ScheduleWrite,
    ) -> AppResult<ChannelSchedule> {
        let id = Uuid::new_v4();
        let recipient_strs: Vec<String> = write
            .recipient_emails
            .iter()
            .map(|e| e.to_string())
            .collect();

        let next_run_at = match write.schedule_type {
            ScheduleType::Interval => {
                let secs = write.interval_seconds.unwrap_or(3600);
                Some(Utc::now() + chrono::Duration::seconds(secs))
            }
            // `validate_write` already refused a one-off without a time, so this is not a default.
            ScheduleType::OneOff => write.scheduled_at,
        };

        let db = sqlx::query_as::<_, ScheduleDb>(&format!(
            r#"INSERT INTO channel_schedules (
                   id, company_id, channel_id, name, schedule_type, interval_seconds,
                   subject_template, prompt_template, delivery_mode, recipient_emails,
                   timezone, enabled, next_run_at
               ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
               RETURNING {SCHEDULE_COLUMNS}"#
        ))
        .bind(id)
        .bind(company_id)
        .bind(channel_id)
        .bind(&write.name)
        .bind(write.schedule_type.as_str())
        .bind(write.interval_seconds)
        .bind(&write.subject_template)
        .bind(&write.prompt_template)
        .bind(write.delivery_mode.as_str())
        .bind(&recipient_strs)
        .bind(write.timezone.name())
        .bind(write.enabled)
        .bind(next_run_at)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        db.try_into()
    }

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<ChannelSchedule>> {
        let db = sqlx::query_as::<_, ScheduleDb>(&format!(
            "SELECT {SCHEDULE_COLUMNS} FROM channel_schedules WHERE id = $1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        db.map(TryInto::try_into).transpose()
    }

    async fn list_by_channel_id(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Vec<ChannelSchedule>> {
        let db_list = sqlx::query_as::<_, ScheduleDb>(&format!(
            "SELECT {SCHEDULE_COLUMNS} FROM channel_schedules
             WHERE company_id = $1 AND channel_id = $2
             ORDER BY created_at DESC, id DESC"
        ))
        .bind(company_id)
        .bind(channel_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        db_list.into_iter().map(TryInto::try_into).collect()
    }

    async fn list_by_company_id(&self, company_id: Uuid) -> AppResult<Vec<ChannelSchedule>> {
        let db_list = sqlx::query_as::<_, ScheduleDb>(&format!(
            "SELECT {SCHEDULE_COLUMNS} FROM channel_schedules WHERE company_id = $1 ORDER BY created_at DESC, id DESC"
        ))
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        db_list.into_iter().map(TryInto::try_into).collect()
    }

    async fn update(
        &self,
        existing: &ChannelSchedule,
        channel_id: Uuid,
        write: ScheduleWrite,
    ) -> AppResult<ChannelSchedule> {
        let recipient_strs: Vec<String> = write
            .recipient_emails
            .iter()
            .map(|e| e.to_string())
            .collect();

        // Editing a subject or a recipient list is not a reason to move the next run: only a
        // changed cadence re-anchors it, and `enabled` belongs to `set_enabled` alone, so renaming
        // a paused schedule cannot resume it.
        let next_run_at = if cadence_changed(existing, &write) {
            next_run_after_cadence_change(&write)
        } else {
            existing.next_run_at
        };

        let db = sqlx::query_as::<_, ScheduleDb>(&format!(
            r#"UPDATE channel_schedules
               SET channel_id = $1, name = $2, schedule_type = $3, interval_seconds = $4,
                   subject_template = $5, prompt_template = $6, delivery_mode = $7,
                   recipient_emails = $8, timezone = $9, next_run_at = $10,
                   updated_at = CURRENT_TIMESTAMP
               WHERE id = $11
               RETURNING {SCHEDULE_COLUMNS}"#
        ))
        .bind(channel_id)
        .bind(&write.name)
        .bind(write.schedule_type.as_str())
        .bind(write.interval_seconds)
        .bind(&write.subject_template)
        .bind(&write.prompt_template)
        .bind(write.delivery_mode.as_str())
        .bind(&recipient_strs)
        .bind(write.timezone.name())
        .bind(next_run_at)
        .bind(existing.id)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        db.try_into()
    }

    async fn delete(&self, id: Uuid) -> AppResult<()> {
        sqlx::query("DELETE FROM channel_schedules WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(AppError::from)?;

        Ok(())
    }

    async fn set_enabled(&self, id: Uuid, enabled: bool) -> AppResult<bool> {
        let result = sqlx::query(
            r#"UPDATE channel_schedules
               SET enabled = $1, updated_at = CURRENT_TIMESTAMP
               WHERE id = $2"#,
        )
        .bind(enabled)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(result.rows_affected() == 1)
    }

    async fn claim_and_advance_due_schedules(
        &self,
        worker_id: Uuid,
        lock_expires_at: DateTime<Utc>,
        limit: i64,
    ) -> AppResult<Vec<ClaimedScheduleRun>> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let due = sqlx::query_as::<_, ScheduleDb>(&format!(
            r#"SELECT {SCHEDULE_COLUMNS}
                   FROM channel_schedules
                   WHERE enabled = true AND next_run_at IS NOT NULL AND next_run_at <= CURRENT_TIMESTAMP
                   ORDER BY next_run_at, id
                   FOR UPDATE SKIP LOCKED
                   LIMIT $1"#
        ))
        .bind(limit.max(1))
        .fetch_all(&mut *tx)
        .await
        .map_err(AppError::from)?;

        for db in due {
            let schedule: ChannelSchedule = db.try_into()?;
            let scheduled_for = schedule
                .next_run_at
                .ok_or_else(|| AppError::Internal("Claimed schedule had no due slot".into()))?;
            let snapshot = serde_json::to_value(&schedule).map_err(|err| {
                AppError::Internal(format!("Failed to encode schedule-run snapshot: {err}"))
            })?;

            sqlx::query(
                r#"INSERT INTO schedule_runs (id, schedule_id, scheduled_for, schedule_snapshot)
                   VALUES ($1, $2, $3, $4)
                   ON CONFLICT (schedule_id, scheduled_for) DO NOTHING"#,
            )
            .bind(Uuid::new_v4())
            .bind(schedule.id)
            .bind(scheduled_for)
            .bind(snapshot)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;

            sqlx::query(
                r#"UPDATE channel_schedules AS schedule
               SET last_run_at = CURRENT_TIMESTAMP,
                   -- Step from the slot that was due, not from now, so a run never drifts later
                   -- than its slot. The arithmetic happens in the schedule's own zone so a daily
                   -- cadence holds its local time across a DST shift, and `ceil` skips whole
                   -- missed slots in one go rather than replaying a backlog after an outage.
                   next_run_at = CASE
                       WHEN schedule.schedule_type <> 'interval' THEN NULL
                       ELSE (
                           (schedule.next_run_at AT TIME ZONE schedule.timezone)
                           + make_interval(secs => schedule.interval_seconds::double precision)
                             * (
                                 floor(
                                     EXTRACT(EPOCH FROM (CURRENT_TIMESTAMP - schedule.next_run_at))
                                     / schedule.interval_seconds
                                 ) + 1
                             )::double precision
                       ) AT TIME ZONE schedule.timezone
                   END,
                   enabled = (schedule.schedule_type = 'interval'),
                   updated_at = CURRENT_TIMESTAMP
               WHERE schedule.id = $1"#,
            )
            .bind(schedule.id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
        }

        let rows = sqlx::query_as::<_, ClaimedScheduleRunDb>(
            r#"WITH terminal AS (
                   UPDATE schedule_runs
                   SET materialization_status = 'failed',
                       last_error = COALESCE(last_error, 'Materialization lease expired at retry limit'),
                       materialization_worker_id = NULL, materialization_generation = NULL,
                       materialization_locked_at = NULL, materialization_lock_expires_at = NULL,
                       updated_at = CURRENT_TIMESTAMP
                   WHERE materialization_status = 'materializing'
                     AND materialization_attempts >= 5
                     AND materialization_lock_expires_at <= CURRENT_TIMESTAMP
                   RETURNING id
               ), candidates AS (
                   SELECT id
                   FROM schedule_runs
                   WHERE (materialization_status = 'pending'
                          AND materialization_available_at <= CURRENT_TIMESTAMP
                          AND materialization_attempts < 5)
                      OR (materialization_status = 'materializing'
                          AND materialization_lock_expires_at <= CURRENT_TIMESTAMP
                          AND materialization_attempts < 5)
                   ORDER BY materialization_available_at, created_at, id
                   FOR UPDATE SKIP LOCKED
                   LIMIT $1
               )
               UPDATE schedule_runs AS run
               SET materialization_status = 'materializing',
                   materialization_attempts = materialization_attempts + 1,
                   materialization_worker_id = $2,
                   materialization_generation = gen_random_uuid(),
                   materialization_locked_at = CURRENT_TIMESTAMP,
                   materialization_lock_expires_at = $3,
                   updated_at = CURRENT_TIMESTAMP
               FROM candidates
               WHERE run.id = candidates.id
                 AND run.materialization_attempts < 5
               RETURNING run.id, run.materialization_generation, run.scheduled_for,
                         run.schedule_snapshot, run.thread_id, run.task_id"#,
        )
        .bind(limit.clamp(1, 100))
        .bind(worker_id)
        .bind(lock_expires_at)
        .fetch_all(&mut *tx)
        .await
        .map_err(AppError::from)?;

        tx.commit().await.map_err(AppError::from)?;

        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn record_run_task(
        &self,
        run_id: Uuid,
        worker_id: Uuid,
        generation: Uuid,
        task_id: Uuid,
    ) -> AppResult<bool> {
        let result = sqlx::query(
            r#"UPDATE schedule_runs
               SET task_id = $4, last_error = NULL, materialization_status = 'materialized',
                   materialization_worker_id = NULL, materialization_generation = NULL,
                   materialization_locked_at = NULL, materialization_lock_expires_at = NULL,
                   updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND thread_id IS NOT NULL AND task_id IS NULL
                 AND materialization_status = 'materializing'
                 AND materialization_worker_id = $2 AND materialization_generation = $3
                 AND materialization_lock_expires_at > CURRENT_TIMESTAMP"#,
        )
        .bind(run_id)
        .bind(worker_id)
        .bind(generation)
        .bind(task_id)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(result.rows_affected() == 1)
    }

    async fn record_run_error(
        &self,
        run_id: Uuid,
        worker_id: Uuid,
        generation: Uuid,
        error: &str,
    ) -> AppResult<bool> {
        let result = sqlx::query(
            r#"UPDATE schedule_runs
               SET last_error = $4,
                   materialization_status = CASE WHEN materialization_attempts >= 5
                       THEN 'failed' ELSE 'pending' END,
                   materialization_available_at = CURRENT_TIMESTAMP + make_interval(secs =>
                       CASE materialization_attempts WHEN 1 THEN 5 WHEN 2 THEN 15
                            WHEN 3 THEN 60 WHEN 4 THEN 300 ELSE 900 END),
                   materialization_worker_id = NULL, materialization_generation = NULL,
                   materialization_locked_at = NULL, materialization_lock_expires_at = NULL,
                   updated_at = CURRENT_TIMESTAMP
               WHERE id = $1 AND task_id IS NULL
                 AND materialization_status = 'materializing'
                 AND materialization_worker_id = $2 AND materialization_generation = $3"#,
        )
        .bind(run_id)
        .bind(worker_id)
        .bind(generation)
        .bind(error)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(result.rows_affected() == 1)
    }

    async fn record_manual_run(&self, id: Uuid) -> AppResult<Option<ChannelSchedule>> {
        let db = sqlx::query_as::<_, ScheduleDb>(&format!(
            r#"UPDATE channel_schedules
               SET last_run_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP
               WHERE id = $1
               RETURNING {SCHEDULE_COLUMNS}"#
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        db.map(TryInto::try_into).transpose()
    }

    async fn release_failed_claim(&self, schedule: &ChannelSchedule, error: &str) -> AppResult<()> {
        // Put the slot back where the claim found it. An interval schedule retries on the next
        // tick; a one-off, which the claim disabled, is re-armed at its original time.
        let retry_at = match schedule.schedule_type {
            ScheduleType::Interval => Some(Utc::now()),
            ScheduleType::OneOff => schedule.next_run_at,
        };

        sqlx::query(
            r#"UPDATE channel_schedules
               SET next_run_at = $1, enabled = true, last_error = $2,
                   updated_at = CURRENT_TIMESTAMP
               WHERE id = $3"#,
        )
        .bind(retry_at)
        .bind(error)
        .bind(schedule.id)
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(())
    }

    async fn clear_last_error(&self, id: Uuid) -> AppResult<()> {
        sqlx::query("UPDATE channel_schedules SET last_error = NULL WHERE id = $1 AND last_error IS NOT NULL")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(AppError::from)?;

        Ok(())
    }

    async fn list_schedule_runs(
        &self,
        schedule_id: Uuid,
        offset: i64,
        limit: i64,
    ) -> AppResult<Vec<ScheduleRun>> {
        let schedule_id_str = schedule_id.to_string();
        let rows = sqlx::query_as::<_, ScheduleRunDb>(
            r#"SELECT t.id AS thread_id, task.id AS task_id, t.channel_id, t.subject,
                      task.status AS task_status, task.lock_expires_at,
                      (SELECT clean_text_body FROM thread_messages tm WHERE tm.thread_id = t.id AND tm.direction = 'outbound' ORDER BY tm.created_at DESC, tm.id DESC LIMIT 1) AS latest_response,
                      (SELECT COUNT(*)::bigint FROM thread_messages tm WHERE tm.thread_id = t.id) AS message_count,
                      t.created_at, t.updated_at
               FROM background_tasks AS task
               JOIN threads AS t ON t.id = task.thread_id
               WHERE task.task_type = 'scheduled_agent_run'
                 AND task.payload->>'schedule_id' = $1
               ORDER BY t.created_at DESC, t.id DESC
               OFFSET $2 LIMIT $3"#,
        )
        .bind(&schedule_id_str)
        .bind(offset.max(0))
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn schedule_run_contains_thread(
        &self,
        company_id: Uuid,
        schedule_id: Uuid,
        thread_id: Uuid,
    ) -> AppResult<bool> {
        sqlx::query_scalar::<_, bool>(
            r#"SELECT EXISTS (
                   SELECT 1
                   FROM background_tasks AS task
                   JOIN channel_schedules AS schedule ON schedule.id = $2
                   WHERE schedule.company_id = $1
                     AND task.company_id = schedule.company_id
                     AND task.channel_id = schedule.channel_id
                     AND task.thread_id = $3
                     AND task.task_type = 'scheduled_agent_run'
                     AND task.payload->>'schedule_id' = $2::text
               )"#,
        )
        .bind(company_id)
        .bind(schedule_id)
        .bind(thread_id)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::persistence::task::TaskPersistence;
    use crate::adapters::persistence::test_support::{UNSCOPED_CLAIM, test_pool};
    use crate::entities::task::NewTask;
    use crate::use_cases::{
        channel::{ChannelPersistence, ChannelWrite},
        company::{CompanyPersistence, CompanyWrite},
        thread::ThreadPersistence,
        user::UserPersistence,
    };

    /// The claim steps from the slot that was due, so a run cannot drift later than its slot and
    /// a backlog after downtime collapses into the next slot instead of replaying.
    #[tokio::test]
    async fn claiming_anchors_the_next_run_on_the_slot_not_the_clock() {
        let Some(pool) = test_pool().await else {
            return;
        };
        // Held for the whole test: `claim_and_advance_due_schedules` is unscoped, and the sibling
        // test below queues a due row of its own. See [`UNSCOPED_CLAIM`].
        let _claim_guard = UNSCOPED_CLAIM.lock().await;

        let persistence = PostgresPersistence::new(pool.clone());

        let owner_username = format!("anchor_owner_{}", Uuid::new_v4().simple());
        let owner_email = format!("{}@example.com", owner_username);
        let _ = persistence
            .create_user(&owner_username, &owner_email, "hash")
            .await;
        let owner = persistence
            .get_by_email(&owner_email)
            .await
            .unwrap()
            .unwrap();

        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            CompanyWrite {
                name: "Anchor Corp".to_string(),
                slug: format!("anchor-corp-{}", Uuid::new_v4().simple()),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();

        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Ops".into(),
                slug: "ops".into(),
                enabled: true,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        let foreign_company = CompanyPersistence::create(
            &persistence,
            owner.id,
            CompanyWrite {
                name: "Foreign Schedule Corp".to_string(),
                slug: format!("foreign-sched-{}", Uuid::new_v4().simple()),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let cross_tenant = SchedulePersistence::create(
            &persistence,
            foreign_company.id,
            channel.id,
            ScheduleWrite {
                name: "Invalid tenant pairing".into(),
                schedule_type: ScheduleType::Interval,
                interval_seconds: Some(3600),
                scheduled_at: None,
                subject_template: "Invalid".into(),
                prompt_template: "Invalid".into(),
                delivery_mode: ScheduleDeliveryMode::MailboxOnly,
                recipient_emails: vec![],
                timezone: ScheduleTimezone::utc(),
                enabled: true,
            },
        )
        .await;
        assert!(cross_tenant.is_err());

        let hourly = SchedulePersistence::create(
            &persistence,
            company.id,
            channel.id,
            ScheduleWrite {
                name: "Hourly".into(),
                schedule_type: ScheduleType::Interval,
                interval_seconds: Some(3600),
                scheduled_at: None,
                subject_template: "s".into(),
                prompt_template: "p".into(),
                delivery_mode: ScheduleDeliveryMode::MailboxOnly,
                recipient_emails: vec![],
                timezone: ScheduleTimezone::utc(),
                enabled: true,
            },
        )
        .await
        .unwrap();

        // Backdate the slot by three and a half hours: three runs were missed.
        let missed_slot = Utc::now() - chrono::Duration::minutes(210);
        sqlx::query("UPDATE channel_schedules SET next_run_at = $1 WHERE id = $2")
            .bind(missed_slot)
            .bind(hourly.id)
            .execute(&pool)
            .await
            .unwrap();

        let claimed = SchedulePersistence::claim_and_advance_due_schedules(
            &persistence,
            Uuid::new_v4(),
            Utc::now() + chrono::Duration::minutes(1),
            10,
        )
        .await
        .unwrap();
        assert!(claimed.iter().any(|run| run.schedule.id == hourly.id));

        let advanced = SchedulePersistence::get_by_id(&persistence, hourly.id)
            .await
            .unwrap()
            .unwrap();
        let next = advanced
            .next_run_at
            .expect("an interval schedule stays armed");

        // Anchored on the slot: 4 hours past a slot 3.5 hours ago, so half an hour out -- not the
        // full hour that anchoring on the claim time would have given.
        assert!(
            next > Utc::now(),
            "the next run must be strictly in the future"
        );
        let lead = next - Utc::now();
        assert!(
            lead < chrono::Duration::minutes(35),
            "expected the next slot in about 30 minutes, got {lead}"
        );
        assert_eq!(
            (next - missed_slot).num_seconds() % 3600,
            0,
            "the next run must stay on the original slot's rhythm"
        );

        let _ = SchedulePersistence::delete(&persistence, hourly.id).await;
        let _ = ChannelPersistence::delete(&persistence, channel.id).await;
        let _ = CompanyPersistence::delete(&persistence, company.id).await;
    }

    #[tokio::test]
    async fn postgres_schedule_persistence_crud_and_claim_works() {
        let Some(pool) = test_pool().await else {
            return;
        };
        // Held for the whole test: `claim_and_advance_due_schedules` is unscoped, and the sibling
        // test above queues a due row of its own. See [`UNSCOPED_CLAIM`].
        let _claim_guard = UNSCOPED_CLAIM.lock().await;

        let persistence = PostgresPersistence::new(pool);

        let owner_username = format!("sched_owner_{}", Uuid::new_v4().simple());
        let owner_email = format!("{}@example.com", owner_username);
        let _ = persistence
            .create_user(&owner_username, &owner_email, "hash")
            .await;
        let owner = persistence
            .get_by_email(&owner_email)
            .await
            .unwrap()
            .unwrap();

        let company_slug = format!("sched-corp-{}", Uuid::new_v4().simple());
        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            CompanyWrite {
                name: "Schedule Corp".to_string(),
                slug: company_slug.to_string(),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();

        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Ops".into(),
                slug: "ops".into(),
                enabled: true,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        let moved_channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Planning".into(),
                slug: "planning".into(),
                enabled: true,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();

        // 1. Create an interval schedule
        let write = ScheduleWrite {
            name: "Hourly Triage".into(),
            schedule_type: ScheduleType::Interval,
            interval_seconds: Some(3600),
            scheduled_at: None,
            subject_template: "[Hourly] Triage - {{date}}".into(),
            prompt_template: "Summarize recent tickets".into(),
            delivery_mode: ScheduleDeliveryMode::MailboxOnly,
            recipient_emails: vec![],
            timezone: ScheduleTimezone::utc(),
            enabled: true,
        };

        let schedule = SchedulePersistence::create(&persistence, company.id, channel.id, write)
            .await
            .unwrap();

        assert_eq!(schedule.name, "Hourly Triage");
        assert_eq!(schedule.schedule_type, ScheduleType::Interval);
        assert_eq!(schedule.interval_seconds, Some(3600));
        assert_eq!(schedule.delivery_mode, ScheduleDeliveryMode::MailboxOnly);
        assert!(schedule.enabled);
        assert!(schedule.next_run_at.is_some());

        // 2. Fetch by id & list
        let fetched = SchedulePersistence::get_by_id(&persistence, schedule.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.id, schedule.id);

        let list_ch = SchedulePersistence::list_by_channel_id(&persistence, company.id, channel.id)
            .await
            .unwrap();
        assert_eq!(list_ch.len(), 1);

        let list_co = SchedulePersistence::list_by_company_id(&persistence, company.id)
            .await
            .unwrap();
        assert_eq!(list_co.len(), 1);

        // 3. Create a due one-off schedule
        let one_off_write = ScheduleWrite {
            name: "One-Off Audit".into(),
            schedule_type: ScheduleType::OneOff,
            interval_seconds: None,
            scheduled_at: Some(Utc::now() - chrono::Duration::minutes(5)), // Due in the past
            subject_template: "[Audit] Point-in-time".into(),
            prompt_template: "Run audit".into(),
            delivery_mode: ScheduleDeliveryMode::EmailCustom,
            recipient_emails: vec![EmailAddress::from("auditor@example.com")],
            timezone: ScheduleTimezone::utc(),
            enabled: true,
        };

        let one_off =
            SchedulePersistence::create(&persistence, company.id, channel.id, one_off_write)
                .await
                .unwrap();

        // 4. Claim and advance due schedules
        let first_worker = Uuid::new_v4();
        let second_worker = Uuid::new_v4();
        let lease_expires_at = Utc::now() + chrono::Duration::minutes(1);
        let (first_claim, second_claim) = tokio::join!(
            SchedulePersistence::claim_and_advance_due_schedules(
                &persistence,
                first_worker,
                lease_expires_at,
                10,
            ),
            SchedulePersistence::claim_and_advance_due_schedules(
                &persistence,
                second_worker,
                lease_expires_at,
                10,
            )
        );
        let first_claim = first_claim.unwrap();
        let second_claim = second_claim.unwrap();
        let owners: Vec<_> = [(&first_claim, first_worker), (&second_claim, second_worker)]
            .into_iter()
            .filter(|(runs, _)| runs.iter().any(|run| run.schedule.id == one_off.id))
            .collect();
        assert_eq!(owners.len(), 1, "only one worker may claim a durable run");
        let (claimed, claiming_worker) = owners[0];
        let claimed_one_off = claimed.iter().find(|run| run.schedule.id == one_off.id);
        assert!(claimed_one_off.is_some());
        let durable_run = claimed_one_off.unwrap();

        let immediate_retry = SchedulePersistence::claim_and_advance_due_schedules(
            &persistence,
            Uuid::new_v4(),
            lease_expires_at,
            10,
        )
        .await
        .unwrap();
        assert!(
            immediate_retry
                .iter()
                .all(|run| run.schedule.id != one_off.id),
            "a live materialization lease must exclude the run from another claim"
        );

        let first_thread = ThreadPersistence::ensure_schedule_run_thread(
            &persistence,
            durable_run.id,
            channel.id,
            "Durable run",
            &[],
        )
        .await
        .unwrap();
        let resumed_thread = ThreadPersistence::ensure_schedule_run_thread(
            &persistence,
            durable_run.id,
            channel.id,
            "Durable run",
            &[],
        )
        .await
        .unwrap();
        assert_eq!(first_thread.id, resumed_thread.id);

        let payload = serde_json::json!({
            "schedule_run_id": durable_run.id,
            "schedule_id": one_off.id,
        });
        let first_task = TaskPersistence::enqueue_task(
            &persistence,
            NewTask::starting_new_chain(
                company.id,
                channel.id,
                Some(first_thread.id),
                "scheduled_agent_run",
                payload.clone(),
            ),
        )
        .await
        .unwrap();
        let resumed_task = TaskPersistence::enqueue_task(
            &persistence,
            NewTask::starting_new_chain(
                company.id,
                channel.id,
                Some(first_thread.id),
                "scheduled_agent_run",
                payload,
            ),
        )
        .await
        .unwrap();
        assert_eq!(first_task.id, resumed_task.id);
        assert!(
            SchedulePersistence::record_run_task(
                &persistence,
                durable_run.id,
                claiming_worker,
                durable_run.materialization_generation,
                first_task.id,
            )
            .await
            .unwrap()
        );

        // Manual runs are intentionally not materialized through `schedule_runs`, but they are
        // still visible in the schedule workspace and must be authorized when selected there.
        let manual_thread =
            ThreadPersistence::create_thread(&persistence, channel.id, "Manual schedule run", &[])
                .await
                .unwrap();
        TaskPersistence::enqueue_task(
            &persistence,
            NewTask::starting_new_chain(
                company.id,
                channel.id,
                Some(manual_thread.id),
                "scheduled_agent_run",
                serde_json::json!({ "schedule_id": one_off.id }),
            ),
        )
        .await
        .unwrap();
        assert!(
            SchedulePersistence::schedule_run_contains_thread(
                &persistence,
                company.id,
                one_off.id,
                manual_thread.id,
            )
            .await
            .unwrap(),
            "a manual run should be authorized from its scheduled task"
        );
        assert!(
            !SchedulePersistence::schedule_run_contains_thread(
                &persistence,
                company.id,
                schedule.id,
                manual_thread.id,
            )
            .await
            .unwrap(),
            "a task cannot authorize the thread under a different schedule"
        );

        // A poison materialization is handed back with persisted backoff, rather than filling the
        // next batch and driving the worker's zero-delay backlog path forever.
        let poison_run_id = Uuid::new_v4();
        sqlx::query(
            r#"INSERT INTO schedule_runs (id, schedule_id, scheduled_for, schedule_snapshot)
               VALUES ($1, $2, $3, $4)"#,
        )
        .bind(poison_run_id)
        .bind(schedule.id)
        .bind(Utc::now())
        .bind(serde_json::to_value(&schedule).unwrap())
        .execute(&persistence.pool)
        .await
        .unwrap();
        let poison_worker = Uuid::new_v4();
        let poison_claim = SchedulePersistence::claim_and_advance_due_schedules(
            &persistence,
            poison_worker,
            lease_expires_at,
            10,
        )
        .await
        .unwrap();
        let poison = poison_claim
            .iter()
            .find(|run| run.id == poison_run_id)
            .expect("the pending poison run is claimed once");
        assert!(
            !SchedulePersistence::record_run_error(
                &persistence,
                poison.id,
                poison_worker,
                Uuid::new_v4(),
                "stale worker",
            )
            .await
            .unwrap(),
            "a stale generation cannot fail the replacement claim"
        );
        assert!(
            SchedulePersistence::record_run_error(
                &persistence,
                poison.id,
                poison_worker,
                poison.materialization_generation,
                "materialization failed",
            )
            .await
            .unwrap()
        );
        let immediate_retry = SchedulePersistence::claim_and_advance_due_schedules(
            &persistence,
            Uuid::new_v4(),
            lease_expires_at,
            10,
        )
        .await
        .unwrap();
        assert!(
            immediate_retry.iter().all(|run| run.id != poison_run_id),
            "a failed run must stay out of the queue until its backoff expires"
        );

        // One-off schedule should now be disabled with next_run_at = None
        let re_read_one_off = SchedulePersistence::get_by_id(&persistence, one_off.id)
            .await
            .unwrap()
            .unwrap();
        assert!(!re_read_one_off.enabled);
        assert!(re_read_one_off.next_run_at.is_none());
        assert!(re_read_one_off.last_run_at.is_some());

        // 5. Update schedule
        let updated = SchedulePersistence::update(
            &persistence,
            &schedule,
            moved_channel.id,
            ScheduleWrite {
                name: "Updated Triage".into(),
                schedule_type: ScheduleType::Interval,
                interval_seconds: Some(1800),
                scheduled_at: None,
                subject_template: "[Updated] {{time}}".into(),
                prompt_template: "Updated prompt".into(),
                delivery_mode: ScheduleDeliveryMode::EmailParticipants,
                recipient_emails: vec![],
                timezone: ScheduleTimezone::utc(),
                enabled: true,
            },
        )
        .await
        .unwrap();

        assert_eq!(updated.name, "Updated Triage");
        assert_eq!(updated.channel_id, moved_channel.id);
        assert_eq!(updated.interval_seconds, Some(1800));
        assert_eq!(
            updated.delivery_mode,
            ScheduleDeliveryMode::EmailParticipants
        );

        // 5b. An edit that leaves the cadence alone must not move the next run, and must not
        // resume a paused schedule -- pausing is `set_enabled`'s job alone.
        SchedulePersistence::set_enabled(&persistence, updated.id, false)
            .await
            .unwrap();
        let paused = SchedulePersistence::get_by_id(&persistence, updated.id)
            .await
            .unwrap()
            .unwrap();

        let renamed = SchedulePersistence::update(
            &persistence,
            &paused,
            paused.channel_id,
            ScheduleWrite {
                name: "Renamed Only".into(),
                schedule_type: ScheduleType::Interval,
                interval_seconds: paused.interval_seconds,
                scheduled_at: None,
                subject_template: paused.subject_template.clone(),
                prompt_template: paused.prompt_template.clone(),
                delivery_mode: paused.delivery_mode,
                recipient_emails: vec![],
                timezone: paused.timezone,
                enabled: true,
            },
        )
        .await
        .unwrap();

        assert_eq!(renamed.name, "Renamed Only");
        assert!(
            !renamed.enabled,
            "a rename must not resume a paused schedule"
        );
        assert_eq!(
            renamed.next_run_at, paused.next_run_at,
            "a rename must not push the next run out"
        );

        // 6. Delete schedule
        SchedulePersistence::delete(&persistence, schedule.id)
            .await
            .unwrap();
        assert!(
            SchedulePersistence::get_by_id(&persistence, schedule.id)
                .await
                .unwrap()
                .is_none()
        );

        let _ = SchedulePersistence::delete(&persistence, one_off.id).await;
        let _ = ChannelPersistence::delete(&persistence, moved_channel.id).await;
        let _ = ChannelPersistence::delete(&persistence, channel.id).await;
        let _ = CompanyPersistence::delete(&persistence, company.id).await;
    }
}
