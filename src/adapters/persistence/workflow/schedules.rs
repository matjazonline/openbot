//! Schedules on workflow bindings (p08 D1.9). Writes take the company lock first, as binding
//! configuration and admission do, then the schedule's channel visibility and the binding's
//! compatibility with that channel; so a schedule and the binding it names never change past
//! each other. Occurrences are captured by the cadence claim or by run-now and materialized by
//! the schedule materializer under a worker/generation fence.

use super::*;
use crate::application::use_cases::schedule::{ScheduleBindingChoice, SchedulePersistence};
use crate::application::workflow::binding::ConfiguredBinding;
use crate::application::workflow::schedule_admission::{
    ErrorRecorded, FailedOccurrence, OccurrenceClaim, ScheduleBinding, ScheduleOccurrences,
    ScheduleWorkerId,
};
use crate::application::workflow::triggers::ScheduleTriggerInput;
use crate::domain::entities::schedule::{
    ChannelSchedule, ClaimedScheduleRun, OccurrenceStatus, ScheduleRun, ScheduleTimezone,
    ScheduleType, ScheduleWrite,
};
use chrono::{DateTime, Utc};
use std::str::FromStr;

const COLUMNS: &str = "id, company_id, channel_id, binding_id, name, schedule_type, \
    interval_seconds, timezone, enabled, last_run_at, next_run_at, last_error, created_at, updated_at";

#[derive(sqlx::FromRow)]
struct ScheduleRow {
    id: Uuid,
    company_id: Uuid,
    channel_id: Uuid,
    binding_id: Uuid,
    name: String,
    schedule_type: String,
    interval_seconds: Option<i64>,
    timezone: String,
    enabled: bool,
    last_run_at: Option<DateTime<Utc>>,
    next_run_at: Option<DateTime<Utc>>,
    last_error: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl ScheduleRow {
    fn schedule(self) -> AppResult<ChannelSchedule> {
        Ok(ChannelSchedule {
            id: self.id,
            company_id: self.company_id,
            channel_id: self.channel_id,
            binding_id: self.binding_id,
            name: self.name,
            schedule_type: ScheduleType::from_str(&self.schedule_type).map_err(|_| invalid())?,
            interval_seconds: self.interval_seconds,
            timezone: ScheduleTimezone::from_str(&self.timezone).map_err(|_| invalid())?,
            enabled: self.enabled,
            last_run_at: self.last_run_at,
            next_run_at: self.next_run_at,
            last_error: self.last_error,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

#[derive(sqlx::FromRow)]
struct RunRow {
    id: Uuid,
    scheduled_for: DateTime<Utc>,
    manual: bool,
    materialization_status: String,
    run_id: Option<Uuid>,
    run_state: Option<String>,
    refusal: Option<String>,
    last_error: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl RunRow {
    fn run(self) -> AppResult<ScheduleRun> {
        Ok(ScheduleRun {
            id: self.id,
            scheduled_for: self.scheduled_for,
            manual: self.manual,
            status: OccurrenceStatus::from_str(&self.materialization_status)
                .map_err(|_| invalid())?,
            run_id: self.run_id,
            run_state: self.run_state,
            refusal: self.refusal,
            last_error: self.last_error,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

const RUNS: &str = "SELECT occurrence.id, occurrence.scheduled_for, \
       occurrence.command_key IS NOT NULL AS manual, occurrence.materialization_status, \
       occurrence.run_id, run.state AS run_state, occurrence.refusal, occurrence.last_error, \
       occurrence.created_at, occurrence.updated_at \
     FROM schedule_runs AS occurrence \
     LEFT JOIN workflow_runs AS run ON run.company_id = occurrence.company_id AND run.id = occurrence.run_id \
     WHERE occurrence.company_id = $1 AND occurrence.schedule_id = $2";

#[derive(sqlx::FromRow)]
struct ClaimedRow {
    id: Uuid,
    materialization_generation: Uuid,
    scheduled_for: DateTime<Utc>,
    binding_id: Uuid,
    schedule_snapshot: serde_json::Value,
}

impl ClaimedRow {
    fn claimed(self) -> AppResult<ClaimedScheduleRun> {
        Ok(ClaimedScheduleRun {
            id: self.id,
            materialization_generation: self.materialization_generation,
            scheduled_for: self.scheduled_for,
            binding_id: self.binding_id,
            schedule: serde_json::from_value(self.schedule_snapshot).map_err(|_| invalid())?,
        })
    }
}

/// The claim's returned shape, shared by the cadence and run-now claims.
const CLAIMED: &str = "RETURNING run.id, run.materialization_generation, run.scheduled_for, \
    run.binding_id, run.schedule_snapshot";

fn channel_association(channel: Uuid) -> RelatedAssociation {
    RelatedAssociation::Channel(RelatedChannelId::new(channel))
}

/// A current owner or admin who can see `channel`, under the company lock.
async fn authorize_channel(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    actor: WorkflowActor,
    channel: Uuid,
) -> AppResult<()> {
    authority::authorize_company(tx, company, actor).await?;
    association::authorize_association(tx, company, actor, channel_association(channel)).await
}

/// The schedule `id`, locked, for an actor who may manage it.
async fn managed(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    actor: WorkflowActor,
    id: Uuid,
) -> AppResult<ChannelSchedule> {
    authority::authorize_company(tx, company, actor).await?;
    // A schedule's channel is immutable, so it is safe to authorize before locking the row.
    let channel: Uuid = sqlx::query_scalar(
        "SELECT channel_id FROM channel_schedules WHERE company_id = $1 AND id = $2",
    )
    .bind(company.as_uuid())
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(missing)?;
    association::authorize_association(tx, company, actor, channel_association(channel)).await?;
    sqlx::query_as::<_, ScheduleRow>(&format!(
        "SELECT {COLUMNS} FROM channel_schedules WHERE company_id = $1 AND id = $2 FOR UPDATE"
    ))
    .bind(company.as_uuid())
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(missing)?
    .schedule()
}

/// Why `configuration` (with `association`) cannot drive a schedule on `channel`, if it cannot
/// (p08 D1.9): the binding must be the company's or this channel's, its pinned input must accept
/// the schedule input, and its run-as must see the channel.
async fn incompatibility(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    configuration: &ConfiguredBinding,
    binding_association: RelatedAssociation,
    channel: Uuid,
) -> AppResult<Option<&'static str>> {
    match binding_association {
        RelatedAssociation::Company => {}
        RelatedAssociation::Channel(bound) if bound.as_uuid() == channel => {}
        _ => {
            return Ok(Some(
                "the binding is associated with neither the company nor the schedule's channel",
            ));
        }
    }
    if configuration
        .bundle()
        .compiled()
        .validate_input(&ScheduleTriggerInput::representative())
        .is_err()
    {
        return Ok(Some(
            "the workflow input schema does not accept the schedule input (scheduled_for)",
        ));
    }
    match association::authorize_association(
        tx,
        company,
        configuration.run_as(),
        channel_association(channel),
    )
    .await
    {
        Ok(()) => Ok(None),
        Err(AppError::NotFound(_)) => Ok(Some(
            "the binding's run-as member cannot see the schedule's channel",
        )),
        Err(error) => Err(error),
    }
}

/// Refuse a schedule write naming a binding that cannot drive it.
async fn check_binding(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    binding: Uuid,
    channel: Uuid,
) -> AppResult<()> {
    let state = binding_rows::read_binding(tx, company, WorkflowBindingId::new(binding))
        .await?
        .ok_or_else(|| {
            AppError::BadRequest("The schedule's workflow binding does not exist".into())
        })?;
    if let Some(reason) = incompatibility(
        tx,
        company,
        &state.configuration,
        state.association,
        channel,
    )
    .await?
    {
        return Err(AppError::BadRequest(format!(
            "This workflow binding cannot run on this schedule: {reason}"
        )));
    }
    Ok(())
}

/// A binding's new configuration must still drive every schedule that names it; the save is
/// refused, naming them, otherwise (p08 D1.9). Called under the company lock.
pub(super) async fn revalidate_schedules(
    tx: &mut Transaction<'_, Postgres>,
    configuration: &ConfiguredBinding,
    binding_association: RelatedAssociation,
) -> AppResult<()> {
    let company = configuration.company_id();
    let schedules: Vec<(String, Uuid)> = sqlx::query_as(
        "SELECT name, channel_id FROM channel_schedules \
         WHERE company_id = $1 AND binding_id = $2 ORDER BY name, id",
    )
    .bind(company.as_uuid())
    .bind(configuration.id().as_uuid())
    .fetch_all(&mut **tx)
    .await?;
    let mut refused = Vec::new();
    for (name, channel) in schedules {
        if let Some(reason) =
            incompatibility(tx, company, configuration, binding_association, channel).await?
        {
            refused.push(format!("“{name}” ({reason})"));
        }
    }
    if refused.is_empty() {
        return Ok(());
    }
    Err(AppError::BadRequest(format!(
        "This configuration cannot run the schedules that use the binding: {}",
        refused.join("; ")
    )))
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

/// The first run of a cadence that just started or changed: one interval out, or the instant.
fn first_run(write: &ScheduleWrite) -> Option<DateTime<Utc>> {
    match write.schedule_type {
        ScheduleType::Interval => write
            .interval_seconds
            .map(|secs| Utc::now() + chrono::Duration::seconds(secs)),
        ScheduleType::OneOff => write.scheduled_at,
    }
}

#[async_trait]
impl SchedulePersistence for PostgresPersistence {
    async fn create(
        &self,
        actor: WorkflowActor,
        company: CompanyId,
        channel_id: Uuid,
        write: ScheduleWrite,
    ) -> AppResult<ChannelSchedule> {
        let mut tx = self.pool.begin().await?;
        authorize_channel(&mut tx, company, actor, channel_id).await?;
        check_binding(&mut tx, company, write.binding_id, channel_id).await?;
        let schedule = sqlx::query_as::<_, ScheduleRow>(&format!(
            "INSERT INTO channel_schedules (id, company_id, channel_id, binding_id, name, \
               schedule_type, interval_seconds, timezone, enabled, next_run_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING {COLUMNS}"
        ))
        .bind(Uuid::new_v4())
        .bind(company.as_uuid())
        .bind(channel_id)
        .bind(write.binding_id)
        .bind(&write.name)
        .bind(write.schedule_type.as_str())
        .bind(write.interval_seconds)
        .bind(write.timezone.name())
        .bind(write.enabled)
        .bind(first_run(&write))
        .fetch_one(&mut *tx)
        .await?
        .schedule()?;
        tx.commit().await?;
        Ok(schedule)
    }

    async fn update(
        &self,
        actor: WorkflowActor,
        company: CompanyId,
        id: Uuid,
        channel_id: Uuid,
        write: ScheduleWrite,
    ) -> AppResult<ChannelSchedule> {
        let mut tx = self.pool.begin().await?;
        let existing = managed(&mut tx, company, actor, id).await?;
        if existing.channel_id != channel_id {
            return Err(AppError::BadRequest(
                "A schedule cannot change its channel".into(),
            ));
        }
        check_binding(&mut tx, company, write.binding_id, channel_id).await?;
        // Only a changed cadence re-anchors the next run, and `enabled` belongs to
        // `set_enabled` alone, so renaming a paused schedule cannot resume it.
        let next_run_at = if cadence_changed(&existing, &write) {
            first_run(&write)
        } else {
            existing.next_run_at
        };
        let schedule = sqlx::query_as::<_, ScheduleRow>(&format!(
            "UPDATE channel_schedules SET binding_id = $3, name = $4, schedule_type = $5, \
               interval_seconds = $6, timezone = $7, next_run_at = $8, updated_at = CURRENT_TIMESTAMP \
             WHERE company_id = $1 AND id = $2 RETURNING {COLUMNS}"
        ))
        .bind(company.as_uuid())
        .bind(id)
        .bind(write.binding_id)
        .bind(&write.name)
        .bind(write.schedule_type.as_str())
        .bind(write.interval_seconds)
        .bind(write.timezone.name())
        .bind(next_run_at)
        .fetch_one(&mut *tx)
        .await?
        .schedule()?;
        tx.commit().await?;
        Ok(schedule)
    }

    async fn delete(&self, actor: WorkflowActor, company: CompanyId, id: Uuid) -> AppResult<()> {
        let mut tx = self.pool.begin().await?;
        managed(&mut tx, company, actor, id).await?;
        sqlx::query("DELETE FROM channel_schedules WHERE company_id = $1 AND id = $2")
            .bind(company.as_uuid())
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn set_enabled(
        &self,
        actor: WorkflowActor,
        company: CompanyId,
        id: Uuid,
        enabled: bool,
    ) -> AppResult<()> {
        let mut tx = self.pool.begin().await?;
        managed(&mut tx, company, actor, id).await?;
        sqlx::query(
            "UPDATE channel_schedules SET enabled = $3, updated_at = CURRENT_TIMESTAMP \
             WHERE company_id = $1 AND id = $2",
        )
        .bind(company.as_uuid())
        .bind(id)
        .bind(enabled)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn capture_now(
        &self,
        actor: WorkflowActor,
        company: CompanyId,
        id: Uuid,
        command_key: &str,
    ) -> AppResult<Uuid> {
        let mut tx = self.pool.begin().await?;
        let schedule = managed(&mut tx, company, actor, id).await?;
        let snapshot = serde_json::to_value(&schedule).map_err(|_| invalid())?;
        let captured = sqlx::query(
            "INSERT INTO schedule_runs (id, schedule_id, scheduled_for, schedule_snapshot, \
               company_id, channel_id, binding_id, command_key) \
             VALUES ($1, $2, clock_timestamp(), $3, $4, $5, $6, $7) \
             ON CONFLICT (company_id, schedule_id, command_key) DO NOTHING",
        )
        .bind(Uuid::new_v4())
        .bind(id)
        .bind(snapshot)
        .bind(company.as_uuid())
        .bind(schedule.channel_id)
        .bind(schedule.binding_id)
        .bind(command_key)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            == 1;
        if captured {
            sqlx::query(
                "UPDATE channel_schedules SET last_run_at = CURRENT_TIMESTAMP, \
                   updated_at = CURRENT_TIMESTAMP WHERE company_id = $1 AND id = $2",
            )
            .bind(company.as_uuid())
            .bind(id)
            .execute(&mut *tx)
            .await?;
        }
        let occurrence: Uuid = sqlx::query_scalar(
            "SELECT id FROM schedule_runs \
             WHERE company_id = $1 AND schedule_id = $2 AND command_key = $3",
        )
        .bind(company.as_uuid())
        .bind(id)
        .bind(command_key)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(occurrence)
    }

    async fn get(&self, company: CompanyId, id: Uuid) -> AppResult<Option<ChannelSchedule>> {
        sqlx::query_as::<_, ScheduleRow>(&format!(
            "SELECT {COLUMNS} FROM channel_schedules WHERE company_id = $1 AND id = $2"
        ))
        .bind(company.as_uuid())
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .map(ScheduleRow::schedule)
        .transpose()
    }

    async fn list_by_channel(
        &self,
        company: CompanyId,
        channel_id: Uuid,
    ) -> AppResult<Vec<ChannelSchedule>> {
        sqlx::query_as::<_, ScheduleRow>(&format!(
            "SELECT {COLUMNS} FROM channel_schedules WHERE company_id = $1 AND channel_id = $2 \
             ORDER BY created_at DESC, id DESC"
        ))
        .bind(company.as_uuid())
        .bind(channel_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(ScheduleRow::schedule)
        .collect()
    }

    async fn list_by_company(&self, company: CompanyId) -> AppResult<Vec<ChannelSchedule>> {
        sqlx::query_as::<_, ScheduleRow>(&format!(
            "SELECT {COLUMNS} FROM channel_schedules WHERE company_id = $1 \
             ORDER BY created_at DESC, id DESC"
        ))
        .bind(company.as_uuid())
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(ScheduleRow::schedule)
        .collect()
    }

    async fn list_runs(
        &self,
        company: CompanyId,
        schedule_id: Uuid,
        offset: i64,
        limit: i64,
    ) -> AppResult<Vec<ScheduleRun>> {
        sqlx::query_as::<_, RunRow>(&format!(
            "{RUNS} ORDER BY occurrence.created_at DESC, occurrence.id DESC OFFSET $3 LIMIT $4"
        ))
        .bind(company.as_uuid())
        .bind(schedule_id)
        .bind(offset.max(0))
        .bind(limit.clamp(1, 100))
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(RunRow::run)
        .collect()
    }

    async fn run(
        &self,
        company: CompanyId,
        schedule_id: Uuid,
        occurrence: Uuid,
    ) -> AppResult<Option<ScheduleRun>> {
        sqlx::query_as::<_, RunRow>(&format!("{RUNS} AND occurrence.id = $3"))
            .bind(company.as_uuid())
            .bind(schedule_id)
            .bind(occurrence)
            .fetch_optional(&self.pool)
            .await?
            .map(RunRow::run)
            .transpose()
    }

    async fn binding_choices(
        &self,
        company: CompanyId,
        channel_id: Option<Uuid>,
    ) -> AppResult<Vec<ScheduleBindingChoice>> {
        let rows: Vec<(Uuid, Option<Uuid>, String, bool)> = sqlx::query_as(
            "SELECT binding.id, binding.channel_id, version.title, binding.active \
             FROM workflow_bindings AS binding \
             JOIN workflow_binding_revisions AS configuration ON configuration.company_id = binding.company_id \
               AND configuration.binding_id = binding.id AND configuration.revision = binding.configuration_revision \
             JOIN workflow_versions AS version ON version.company_id = binding.company_id \
               AND version.id = configuration.version_id \
             WHERE binding.company_id = $1 AND binding.thread_id IS NULL \
               AND ($2::uuid IS NULL OR binding.channel_id IS NULL OR binding.channel_id = $2) \
             ORDER BY version.title, binding.id",
        )
        .bind(company.as_uuid())
        .bind(channel_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(binding_id, channel_id, title, active)| ScheduleBindingChoice {
                    binding_id,
                    channel_id,
                    title,
                    active,
                },
            )
            .collect())
    }
}

#[async_trait]
impl ScheduleOccurrences for PostgresPersistence {
    async fn fail_expired_occurrences(&self, page: i64) -> AppResult<Vec<FailedOccurrence>> {
        // Two statements, neither holding the other's lock: the occurrence before its schedule
        // here would invert the schedule-then-occurrence order of every other writer. The
        // occurrences are failed once the first commits, so the second is best effort: they are
        // reported even when their schedules cannot name the failure.
        let failed: Vec<(Uuid, Uuid, Uuid)> = sqlx::query_as(
            "UPDATE schedule_runs SET materialization_status = 'failed', \
               last_error = COALESCE(last_error, $2), materialization_worker_id = NULL, \
               materialization_generation = NULL, materialization_locked_at = NULL, \
               materialization_lock_expires_at = NULL, updated_at = CURRENT_TIMESTAMP \
             WHERE id IN (SELECT id FROM schedule_runs \
               WHERE materialization_status = 'materializing' AND materialization_attempts >= 5 \
                 AND materialization_lock_expires_at <= CURRENT_TIMESTAMP \
               ORDER BY materialization_lock_expires_at, id LIMIT $1 FOR UPDATE SKIP LOCKED) \
             RETURNING company_id, id, schedule_id",
        )
        .bind(page.clamp(1, 100))
        .bind(LEASE_EXPIRED_AT_LIMIT)
        .fetch_all(&self.pool)
        .await?;
        for (company, _, schedule) in &failed {
            if let Err(error) = sqlx::query(
                "UPDATE channel_schedules SET last_error = $3 WHERE company_id = $1 AND id = $2",
            )
            .bind(company)
            .bind(schedule)
            .bind(LEASE_EXPIRED_AT_LIMIT)
            .execute(&self.pool)
            .await
            {
                tracing::warn!(company_id = %company, schedule_id = %schedule, %error,
                    "could not name an expired occurrence's failure on its schedule");
            }
        }
        Ok(failed
            .into_iter()
            .map(|(company, occurrence, _)| FailedOccurrence {
                company: CompanyId::new(company),
                occurrence,
            })
            .collect())
    }

    async fn claim_due(
        &self,
        worker: ScheduleWorkerId,
        lease_until: DateTime<Utc>,
        page: i64,
    ) -> AppResult<Vec<ClaimedScheduleRun>> {
        let mut tx = self.pool.begin().await?;
        let due = sqlx::query_as::<_, ScheduleRow>(&format!(
            "SELECT {COLUMNS} FROM channel_schedules \
             WHERE enabled AND next_run_at IS NOT NULL AND next_run_at <= CURRENT_TIMESTAMP \
             ORDER BY next_run_at, id FOR UPDATE SKIP LOCKED LIMIT $1"
        ))
        .bind(page.max(1))
        .fetch_all(&mut *tx)
        .await?;
        for row in due {
            let schedule = row.schedule()?;
            let scheduled_for = schedule.next_run_at.ok_or_else(invalid)?;
            let snapshot = serde_json::to_value(&schedule).map_err(|_| invalid())?;
            sqlx::query(
                "INSERT INTO schedule_runs (id, schedule_id, scheduled_for, schedule_snapshot, \
                   company_id, channel_id, binding_id) VALUES ($1, $2, $3, $4, $5, $6, $7) \
                 ON CONFLICT (schedule_id, scheduled_for) WHERE command_key IS NULL DO NOTHING",
            )
            .bind(Uuid::new_v4())
            .bind(schedule.id)
            .bind(scheduled_for)
            .bind(snapshot)
            .bind(schedule.company_id)
            .bind(schedule.channel_id)
            .bind(schedule.binding_id)
            .execute(&mut *tx)
            .await?;
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
            .await?;
        }
        let claimed = sqlx::query_as::<_, ClaimedRow>(&format!(
            "WITH candidates AS ( \
               SELECT id FROM schedule_runs \
               WHERE (materialization_status = 'pending' \
                      AND materialization_available_at <= CURRENT_TIMESTAMP \
                      AND materialization_attempts < 5) \
                  OR (materialization_status = 'materializing' \
                      AND materialization_lock_expires_at <= CURRENT_TIMESTAMP \
                      AND materialization_attempts < 5) \
               ORDER BY materialization_available_at, created_at, id \
               FOR UPDATE SKIP LOCKED LIMIT $1 \
             ) \
             UPDATE schedule_runs AS run \
             SET materialization_status = 'materializing', \
                 materialization_attempts = materialization_attempts + 1, \
                 materialization_worker_id = $2, materialization_generation = gen_random_uuid(), \
                 materialization_locked_at = CURRENT_TIMESTAMP, materialization_lock_expires_at = $3, \
                 updated_at = CURRENT_TIMESTAMP \
             FROM candidates WHERE run.id = candidates.id AND run.materialization_attempts < 5 \
             {CLAIMED}"
        ))
        .bind(page.clamp(1, 100))
        .bind(worker.0)
        .bind(lease_until)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        claimed.into_iter().map(ClaimedRow::claimed).collect()
    }

    async fn claim_occurrence(
        &self,
        company: CompanyId,
        occurrence: Uuid,
        worker: ScheduleWorkerId,
        lease_until: DateTime<Utc>,
    ) -> AppResult<Option<ClaimedScheduleRun>> {
        sqlx::query_as::<_, ClaimedRow>(&format!(
            "UPDATE schedule_runs AS run \
             SET materialization_status = 'materializing', \
                 materialization_attempts = materialization_attempts + 1, \
                 materialization_worker_id = $3, materialization_generation = gen_random_uuid(), \
                 materialization_locked_at = CURRENT_TIMESTAMP, materialization_lock_expires_at = $4, \
                 updated_at = CURRENT_TIMESTAMP \
             WHERE run.company_id = $1 AND run.id = $2 AND run.materialization_attempts < 5 \
               AND ((run.materialization_status = 'pending' \
                     AND run.materialization_available_at <= CURRENT_TIMESTAMP) \
                 OR (run.materialization_status = 'materializing' \
                     AND run.materialization_lock_expires_at <= CURRENT_TIMESTAMP)) \
             {CLAIMED}"
        ))
        .bind(company.as_uuid())
        .bind(occurrence)
        .bind(worker.0)
        .bind(lease_until)
        .fetch_optional(&self.pool)
        .await?
        .map(ClaimedRow::claimed)
        .transpose()
    }

    async fn schedule_binding(
        &self,
        company: CompanyId,
        binding: WorkflowBindingId,
    ) -> AppResult<Option<ScheduleBinding>> {
        let row: Option<(i64, Uuid)> = sqlx::query_as(
            "SELECT binding.configuration_revision, configuration.run_as_user_id \
             FROM workflow_bindings AS binding \
             JOIN workflow_binding_revisions AS configuration ON configuration.company_id = binding.company_id \
               AND configuration.binding_id = binding.id AND configuration.revision = binding.configuration_revision \
             WHERE binding.company_id = $1 AND binding.id = $2 AND binding.active",
        )
        .bind(company.as_uuid())
        .bind(binding.as_uuid())
        .fetch_optional(&self.pool)
        .await?;
        row.map(|(revision, run_as)| {
            Ok(ScheduleBinding {
                id: binding,
                revision: BindingRevision::new(u64::try_from(revision).map_err(|_| invalid())?)
                    .ok_or_else(invalid)?,
                run_as: WorkflowActor::authenticated(run_as).map_err(|_| invalid())?,
            })
        })
        .transpose()
    }

    async fn record_run(&self, claim: OccurrenceClaim, run: RunId) -> AppResult<bool> {
        Ok(self
            .record(
                claim,
                "run_id = $5, last_error = NULL, materialization_status = 'materialized'",
                Outcome::Run(run.as_uuid()),
                None,
            )
            .await?
            .is_some())
    }

    async fn record_refusal(
        &self,
        claim: OccurrenceClaim,
        refusal: &'static str,
    ) -> AppResult<bool> {
        let error = format!("The workflow binding refused this occurrence ({refusal})");
        Ok(self
            .record(
                claim,
                "refusal = $5, last_error = $6, materialization_status = 'failed'",
                Outcome::Refusal(refusal, error.clone()),
                Some(&error),
            )
            .await?
            .is_some())
    }

    async fn record_error(&self, claim: OccurrenceClaim, error: &str) -> AppResult<ErrorRecorded> {
        let status = self
            .record(
                claim,
                "last_error = $5, \
                 materialization_status = CASE WHEN materialization_attempts >= 5 \
                   THEN 'failed' ELSE 'pending' END, \
                 materialization_available_at = CURRENT_TIMESTAMP + make_interval(secs => \
                   CASE materialization_attempts WHEN 1 THEN 5 WHEN 2 THEN 15 \
                        WHEN 3 THEN 60 WHEN 4 THEN 300 ELSE 900 END)",
                Outcome::Error(error.to_owned()),
                Some(error),
            )
            .await?;
        Ok(match status.as_deref() {
            None => ErrorRecorded::Lost,
            Some("failed") => ErrorRecorded::Failed,
            Some(_) => ErrorRecorded::Retrying,
        })
    }
}

/// Why an occurrence whose last attempt's lease lapsed failed, on it and on its schedule.
const LEASE_EXPIRED_AT_LIMIT: &str = "Materialization lease expired at retry limit";

/// The values a fenced outcome write binds after the claim (`$5`, `$6`).
enum Outcome {
    Run(Uuid),
    Refusal(&'static str, String),
    Error(String),
}

impl PostgresPersistence {
    /// One fenced outcome write, returning the occurrence's new status, or `None` when the claim
    /// was lost. The schedule row is locked before its occurrence, the order schedule edits,
    /// deletes, captures and admissions take, and its `last_error` follows the outcome: cleared by
    /// a run (`schedule_error = None`), set otherwise.
    ///
    /// The fence is the claim's worker and generation, not its lease expiry: a reclaim mints a
    /// new generation, so a lapsed but unreclaimed claim is still the latest one and may record
    /// what it did (p08 F032).
    async fn record(
        &self,
        claim: OccurrenceClaim,
        set: &str,
        outcome: Outcome,
        schedule_error: Option<&str>,
    ) -> AppResult<Option<String>> {
        let mut tx = self.pool.begin().await?;
        let schedule: Option<Uuid> = sqlx::query_scalar(
            "SELECT schedule.id FROM channel_schedules AS schedule \
             JOIN schedule_runs AS occurrence ON occurrence.company_id = schedule.company_id \
               AND occurrence.schedule_id = schedule.id \
             WHERE occurrence.company_id = $1 AND occurrence.id = $2 \
             FOR NO KEY UPDATE OF schedule",
        )
        .bind(claim.company.as_uuid())
        .bind(claim.occurrence)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(schedule) = schedule else {
            return Ok(None);
        };
        let sql = format!(
            "UPDATE schedule_runs SET {set}, materialization_worker_id = NULL, \
               materialization_generation = NULL, materialization_locked_at = NULL, \
               materialization_lock_expires_at = NULL, updated_at = CURRENT_TIMESTAMP \
             WHERE company_id = $1 AND id = $2 AND materialization_status = 'materializing' \
               AND materialization_worker_id = $3 AND materialization_generation = $4 \
             RETURNING materialization_status"
        );
        let query = sqlx::query_scalar::<_, String>(&sql)
            .bind(claim.company.as_uuid())
            .bind(claim.occurrence)
            .bind(claim.worker.0)
            .bind(claim.generation);
        let query = match outcome {
            Outcome::Run(run) => query.bind(run),
            Outcome::Refusal(refusal, error) => query.bind(refusal).bind(error),
            Outcome::Error(error) => query.bind(error),
        };
        let status = query.fetch_optional(&mut *tx).await?;
        if status.is_some() {
            sqlx::query(
                "UPDATE channel_schedules SET last_error = $3 \
                 WHERE company_id = $1 AND id = $2 AND last_error IS DISTINCT FROM $3",
            )
            .bind(claim.company.as_uuid())
            .bind(schedule)
            .bind(schedule_error)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(status)
    }
}
