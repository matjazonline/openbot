//! Schedules on workflow bindings (p08 D1.9). A schedule is a cadence on one channel that admits
//! a run of one binding per occurrence; the run acts as the binding's run-as member. Managing a
//! schedule takes what configuring that binding takes: a current owner or admin of the company
//! who can see the schedule's channel. Persistence rechecks that authority, and the binding's
//! compatibility with the channel, under the company lock every workflow lifecycle write takes
//! first; the preflight here only answers reads and gives writes their early refusal.

use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::Arc;
use tracing::{info, instrument, warn};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    application::workflow::{
        RelatedAssociation, RelatedChannelId, WorkflowActor, WorkflowAuthorization,
        WorkflowOperation,
        schedule_admission::{Materialized, ScheduleRunNow},
    },
    domain::workflow::CompanyId,
    entities::schedule::{ChannelSchedule, ScheduleRun, ScheduleType, ScheduleWrite},
};

/// A binding a schedule on a channel may name: associated with the company or that channel.
/// Its input schema and run-as are checked when the schedule is saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleBindingChoice {
    pub binding_id: Uuid,
    /// `None` for a company binding.
    pub channel_id: Option<Uuid>,
    /// The bound version's title.
    pub title: String,
    pub active: bool,
}

#[async_trait]
pub trait SchedulePersistence: Send + Sync {
    /// Each write authorizes `actor` as a current owner or admin who can see the schedule's
    /// channel, under the company lock, and refuses a binding incompatible with that channel.
    async fn create(
        &self,
        actor: WorkflowActor,
        company: CompanyId,
        channel_id: Uuid,
        write: ScheduleWrite,
    ) -> AppResult<ChannelSchedule>;
    /// Edits within the existing channel; `channel_id` is scope, never a destination.
    async fn update(
        &self,
        actor: WorkflowActor,
        company: CompanyId,
        id: Uuid,
        channel_id: Uuid,
        write: ScheduleWrite,
    ) -> AppResult<ChannelSchedule>;
    async fn delete(&self, actor: WorkflowActor, company: CompanyId, id: Uuid) -> AppResult<()>;
    async fn set_enabled(
        &self,
        actor: WorkflowActor,
        company: CompanyId,
        id: Uuid,
        enabled: bool,
    ) -> AppResult<()>;
    /// Capture a run-now occurrence of `id`, or return the one `command_key` already captured.
    async fn capture_now(
        &self,
        actor: WorkflowActor,
        company: CompanyId,
        id: Uuid,
        command_key: &str,
    ) -> AppResult<Uuid>;
    async fn get(&self, company: CompanyId, id: Uuid) -> AppResult<Option<ChannelSchedule>>;
    async fn list_by_channel(
        &self,
        company: CompanyId,
        channel_id: Uuid,
    ) -> AppResult<Vec<ChannelSchedule>>;
    async fn list_by_company(&self, company: CompanyId) -> AppResult<Vec<ChannelSchedule>>;
    async fn list_runs(
        &self,
        company: CompanyId,
        schedule_id: Uuid,
        offset: i64,
        limit: i64,
    ) -> AppResult<Vec<ScheduleRun>>;
    async fn run(
        &self,
        company: CompanyId,
        schedule_id: Uuid,
        occurrence: Uuid,
    ) -> AppResult<Option<ScheduleRun>>;
    /// The company's bindings and those of `channel_id`, or of every channel for `None`.
    async fn binding_choices(
        &self,
        company: CompanyId,
        channel_id: Option<Uuid>,
    ) -> AppResult<Vec<ScheduleBindingChoice>>;
}

/// Where a caller addresses one schedule: its company, and the channel a route nested under a
/// channel names. A schedule of any other company or channel is not found (p08 F031). A bare
/// company id is the company-wide scope of the schedules workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduleScope {
    pub company_id: Uuid,
    pub channel_id: Option<Uuid>,
}

impl ScheduleScope {
    pub fn channel(company_id: Uuid, channel_id: Uuid) -> Self {
        Self {
            company_id,
            channel_id: Some(channel_id),
        }
    }
}

impl From<Uuid> for ScheduleScope {
    fn from(company_id: Uuid) -> Self {
        Self {
            company_id,
            channel_id: None,
        }
    }
}

/// The longest run-now command key a client may send.
pub const MAX_RUN_NOW_KEY_BYTES: usize = 200;

#[derive(Clone)]
pub struct ScheduleUseCases {
    persistence: Arc<dyn SchedulePersistence>,
    authorization: Arc<dyn WorkflowAuthorization>,
    run_now: Arc<dyn ScheduleRunNow>,
}

impl ScheduleUseCases {
    pub fn new(
        persistence: Arc<dyn SchedulePersistence>,
        authorization: Arc<dyn WorkflowAuthorization>,
        run_now: Arc<dyn ScheduleRunNow>,
    ) -> Self {
        Self {
            persistence,
            authorization,
            run_now,
        }
    }

    pub fn persistence(&self) -> &Arc<dyn SchedulePersistence> {
        &self.persistence
    }

    /// Owner or admin of the company who can see `channel` (or any channel, for `None`).
    async fn authorize(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        channel: Option<Uuid>,
    ) -> AppResult<(WorkflowActor, CompanyId)> {
        let actor = WorkflowActor::authenticated(user_id)?;
        let company = CompanyId::new(company_id);
        let association = channel.map_or(RelatedAssociation::Company, |channel| {
            RelatedAssociation::Channel(RelatedChannelId::new(channel))
        });
        self.authorization
            .authorize(
                company,
                actor,
                association,
                WorkflowOperation::ManageBinding,
            )
            .await?;
        Ok((actor, company))
    }

    /// The one place a schedule is resolved for a caller: it must belong to the scope's company
    /// (and channel, when the scope names one), and the caller must be allowed to manage
    /// schedules on its channel. A schedule's channel never changes, so the check holds.
    async fn managed_schedule(
        &self,
        user_id: Uuid,
        scope: impl Into<ScheduleScope>,
        id: Uuid,
    ) -> AppResult<(WorkflowActor, CompanyId, ChannelSchedule)> {
        let ScheduleScope {
            company_id,
            channel_id,
        } = scope.into();
        let (_, company) = self.authorize(user_id, company_id, None).await?;
        let schedule = self
            .persistence
            .get(company, id)
            .await?
            .filter(|schedule| channel_id.is_none_or(|channel| channel == schedule.channel_id))
            .ok_or_else(not_found)?;
        let (actor, company) = self
            .authorize(user_id, company_id, Some(schedule.channel_id))
            .await?;
        Ok((actor, company, schedule))
    }

    fn validate_write(write: &ScheduleWrite) -> AppResult<()> {
        if write.name.trim().is_empty() {
            return Err(AppError::BadRequest(
                "Schedule name cannot be empty.".into(),
            ));
        }
        match write.schedule_type {
            ScheduleType::Interval => {
                let secs = write.interval_seconds.unwrap_or(0);
                if secs < 60 {
                    return Err(AppError::BadRequest(
                        "Interval must be at least 60 seconds (1 minute).".into(),
                    ));
                }
            }
            ScheduleType::OneOff => {
                if write.scheduled_at.is_none() {
                    return Err(AppError::BadRequest(
                        "One-off schedule must have a scheduled execution time.".into(),
                    ));
                }
            }
        }
        Ok(())
    }

    /// The bindings a schedule on `channel_id` may name, or on any channel this caller can see.
    #[instrument(skip_all)]
    pub async fn binding_choices(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        channel_id: Option<Uuid>,
    ) -> AppResult<Vec<ScheduleBindingChoice>> {
        let (_, company) = self.authorize(user_id, company_id, channel_id).await?;
        let choices = self
            .persistence
            .binding_choices(company, channel_id)
            .await?;
        if channel_id.is_some() {
            return Ok(choices);
        }
        let channels = choices.iter().filter_map(|choice| choice.channel_id);
        let visible = self.visible_channels(user_id, company_id, channels).await?;
        Ok(choices
            .into_iter()
            .filter(|choice| choice.channel_id.is_none_or(|channel| visible[&channel]))
            .collect())
    }

    /// Which of `channels` this caller may manage schedules on.
    async fn visible_channels(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        channels: impl Iterator<Item = Uuid>,
    ) -> AppResult<BTreeMap<Uuid, bool>> {
        let mut visible = BTreeMap::new();
        for channel in channels {
            if visible.contains_key(&channel) {
                continue;
            }
            let seen = match self.authorize(user_id, company_id, Some(channel)).await {
                Ok(_) => true,
                Err(AppError::NotFound(_)) => false,
                Err(error) => return Err(error),
            };
            visible.insert(channel, seen);
        }
        Ok(visible)
    }

    pub async fn create_schedule(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        channel_id: Uuid,
        write: ScheduleWrite,
    ) -> AppResult<ChannelSchedule> {
        let (actor, company) = self
            .authorize(user_id, company_id, Some(channel_id))
            .await?;
        Self::validate_write(&write)?;
        info!(%channel_id, %company_id, schedule_type = %write.schedule_type, "Creating schedule");
        self.persistence
            .create(actor, company, channel_id, write)
            .await
    }

    #[instrument(skip_all)]
    pub async fn get_schedule(
        &self,
        user_id: Uuid,
        scope: impl Into<ScheduleScope> + std::fmt::Debug,
        id: Uuid,
    ) -> AppResult<Option<ChannelSchedule>> {
        match self.managed_schedule(user_id, scope, id).await {
            Ok((_, _, schedule)) => Ok(Some(schedule)),
            Err(AppError::NotFound(_)) => Ok(None),
            Err(err) => Err(err),
        }
    }

    #[instrument(skip_all)]
    pub async fn list_channel_schedules(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Vec<ChannelSchedule>> {
        let (_, company) = self
            .authorize(user_id, company_id, Some(channel_id))
            .await?;
        self.persistence.list_by_channel(company, channel_id).await
    }

    /// Every schedule on a channel this caller can see.
    #[instrument(skip_all)]
    pub async fn list_company_schedules(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Vec<ChannelSchedule>> {
        let (_, company) = self.authorize(user_id, company_id, None).await?;
        let schedules = self.persistence.list_by_company(company).await?;
        let channels = schedules.iter().map(|schedule| schedule.channel_id);
        let visible = self.visible_channels(user_id, company_id, channels).await?;
        Ok(schedules
            .into_iter()
            .filter(|schedule| visible[&schedule.channel_id])
            .collect())
    }

    #[instrument(skip_all)]
    pub async fn update_schedule(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        id: Uuid,
        channel_id: Uuid,
        write: ScheduleWrite,
    ) -> AppResult<ChannelSchedule> {
        let (actor, company, existing) = self.managed_schedule(user_id, company_id, id).await?;
        if existing.channel_id != channel_id {
            return Err(AppError::BadRequest(
                "A schedule cannot change its channel".into(),
            ));
        }
        Self::validate_write(&write)?;
        info!(schedule_id = %id, %company_id, "Updating schedule");
        self.persistence
            .update(actor, company, id, channel_id, write)
            .await
    }

    #[instrument(skip_all)]
    pub async fn delete_schedule(
        &self,
        user_id: Uuid,
        scope: impl Into<ScheduleScope> + std::fmt::Debug,
        id: Uuid,
    ) -> AppResult<()> {
        let (actor, company, _) = self.managed_schedule(user_id, scope, id).await?;
        info!("Deleting schedule {} for company {}", id, company.as_uuid());
        self.persistence.delete(actor, company, id).await
    }

    #[instrument(skip_all)]
    pub async fn toggle_schedule(
        &self,
        user_id: Uuid,
        scope: impl Into<ScheduleScope> + std::fmt::Debug,
        id: Uuid,
        enabled: bool,
    ) -> AppResult<()> {
        let (actor, company, _) = self.managed_schedule(user_id, scope, id).await?;
        self.persistence
            .set_enabled(actor, company, id, enabled)
            .await
    }

    #[instrument(skip_all)]
    pub async fn list_schedule_runs(
        &self,
        user_id: Uuid,
        scope: impl Into<ScheduleScope> + std::fmt::Debug,
        schedule_id: Uuid,
        offset: i64,
        limit: i64,
    ) -> AppResult<Vec<ScheduleRun>> {
        let (_, company, _) = self.managed_schedule(user_id, scope, schedule_id).await?;
        self.persistence
            .list_runs(company, schedule_id, offset, limit)
            .await
    }

    /// Run the schedule now (p08 D1.9): capture one occurrence for `command_key` and materialize
    /// it through the materializer's own path. A repeated key returns the same occurrence and
    /// run. An occurrence that could not be admitted yet stays for the materializer's retry.
    #[instrument(skip_all)]
    pub async fn trigger_schedule_now(
        &self,
        user_id: Uuid,
        scope: impl Into<ScheduleScope> + std::fmt::Debug,
        id: Uuid,
        command_key: &str,
    ) -> AppResult<ScheduleRun> {
        if command_key.is_empty() || command_key.len() > MAX_RUN_NOW_KEY_BYTES {
            return Err(AppError::BadRequest(
                "A run-now request needs a command key of at most 200 bytes.".into(),
            ));
        }
        let (actor, company, schedule) = self.managed_schedule(user_id, scope, id).await?;
        info!(schedule_id = %schedule.id, "Manually triggering schedule");
        let occurrence = self
            .persistence
            .capture_now(actor, company, id, command_key)
            .await?;
        match self.run_now.materialize_now(company, occurrence).await {
            Ok(Some(Materialized::Admitted(_)) | None) => {}
            Ok(Some(outcome)) => {
                warn!(occurrence_id = %occurrence, ?outcome, "A run-now occurrence was not admitted")
            }
            Err(_) => {
                warn!(occurrence_id = %occurrence,
                    "A run-now occurrence is left for the schedule materializer")
            }
        }
        self.persistence
            .run(company, id, occurrence)
            .await?
            .ok_or_else(not_found)
    }
}

fn not_found() -> AppError {
    AppError::NotFound("Schedule not found.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::schedule::ScheduleTimezone;

    #[test]
    fn validate_write_checks_bad_inputs() {
        let base = ScheduleWrite {
            name: "Test".into(),
            schedule_type: ScheduleType::Interval,
            interval_seconds: Some(3600),
            scheduled_at: None,
            timezone: ScheduleTimezone::utc(),
            binding_id: Uuid::new_v4(),
            enabled: true,
        };
        assert!(ScheduleUseCases::validate_write(&base).is_ok());
        let mut blank = base.clone();
        blank.name = "  ".into();
        assert!(ScheduleUseCases::validate_write(&blank).is_err());
        let mut fast = base.clone();
        fast.interval_seconds = Some(59);
        assert!(ScheduleUseCases::validate_write(&fast).is_err());
        let mut one_off = base;
        one_off.schedule_type = ScheduleType::OneOff;
        one_off.interval_seconds = None;
        assert!(ScheduleUseCases::validate_write(&one_off).is_err());
    }
}
