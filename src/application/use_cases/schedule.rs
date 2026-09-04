use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use tracing::{info, instrument, warn};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        channel::Channel,
        company::Company,
        correlation::CorrelationId,
        message::{MessageDirection, MessageRole},
        participant::{IdentityClaimMetadata, IdentityProvenance},
        schedule::{
            ChannelSchedule, ClaimedScheduleRun, ScheduleDeliveryMode, ScheduleRun, ScheduleRunAs,
            ScheduleRunAsChoices, ScheduleType, ScheduleWrite, ScheduledRunPayload,
        },
        task::{NewTask, TaskSource},
        value_objects::EmailAddress,
    },
    task_queue::TaskPersistence,
    use_cases::{
        channel::ChannelPersistence,
        company::{CompanyPersistence, managed_company},
        participant::IdentityObservation,
        thread::{MessageAuthorWrite, MessageWrite, ThreadPersistence, qualified_email_identity},
    },
};

#[async_trait]
pub trait SchedulePersistence: Send + Sync {
    async fn create(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
        write: ScheduleWrite,
    ) -> AppResult<ChannelSchedule>;
    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<ChannelSchedule>>;
    async fn list_by_channel_id(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Vec<ChannelSchedule>>;
    async fn list_by_company_id(&self, company_id: Uuid) -> AppResult<Vec<ChannelSchedule>>;
    async fn update(
        &self,
        existing: &ChannelSchedule,
        channel_id: Uuid,
        write: ScheduleWrite,
    ) -> AppResult<ChannelSchedule>;
    async fn delete(&self, id: Uuid) -> AppResult<()>;
    async fn set_enabled(&self, id: Uuid, enabled: bool) -> AppResult<bool>;
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
    async fn record_manual_run(&self, id: Uuid) -> AppResult<Option<ChannelSchedule>>;
    async fn release_failed_claim(&self, schedule: &ChannelSchedule, error: &str) -> AppResult<()>;
    async fn clear_last_error(&self, id: Uuid) -> AppResult<()>;
    async fn list_schedule_runs(
        &self,
        schedule_id: Uuid,
        offset: i64,
        limit: i64,
    ) -> AppResult<Vec<ScheduleRun>>;
    async fn schedule_run_contains_thread(
        &self,
        company_id: Uuid,
        schedule_id: Uuid,
        thread_id: Uuid,
    ) -> AppResult<bool>;
}

/// The `background_tasks.task_type` a schedule run is queued under. Named once here because the
/// worker dispatches on it and the runs query filters by it.
pub const SCHEDULED_AGENT_RUN_TASK: &str = "scheduled_agent_run";

/// A submitted change of whom a schedule runs as: what the row holds now, and what the form
/// asked for.
///
/// Two `Option<Uuid>` of opposite meaning, so they travel named rather than as adjacent
/// positional arguments a call site can swap silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RunAsChange {
    stored: Option<Uuid>,
    requested: Option<Uuid>,
}

#[derive(Clone)]
pub struct ScheduleUseCases {
    schedule_persistence: Arc<dyn SchedulePersistence>,
    company_persistence: Arc<dyn CompanyPersistence>,
    channel_persistence: Arc<dyn ChannelPersistence>,
    thread_persistence: Arc<dyn ThreadPersistence>,
    task_persistence: Arc<dyn TaskPersistence>,
}

impl ScheduleUseCases {
    pub fn new(
        schedule_persistence: Arc<dyn SchedulePersistence>,
        company_persistence: Arc<dyn CompanyPersistence>,
        channel_persistence: Arc<dyn ChannelPersistence>,
        thread_persistence: Arc<dyn ThreadPersistence>,
        task_persistence: Arc<dyn TaskPersistence>,
    ) -> Self {
        Self {
            schedule_persistence,
            company_persistence,
            channel_persistence,
            thread_persistence,
            task_persistence,
        }
    }

    pub fn persistence(&self) -> &Arc<dyn SchedulePersistence> {
        &self.schedule_persistence
    }

    /// The company whose automation this caller may manage, which every read and write here
    /// starts from.
    ///
    /// Returns the company rather than asserting access, because who may be named as a run's
    /// member turns on whether the caller owns it.
    async fn managed_company(&self, user_id: Uuid, company_id: Uuid) -> AppResult<Company> {
        managed_company(self.company_persistence.as_ref(), user_id, company_id).await
    }

    /// The one place a schedule is resolved for a caller: the user must own or administer the
    /// company, and the schedule must belong to it. Every read and every write goes through here
    /// *before* touching the row, so a schedule from another tenant is never loaded or mutated.
    async fn managed_schedule(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        id: Uuid,
    ) -> AppResult<ChannelSchedule> {
        self.managed_company(user_id, company_id).await?;

        let schedule = self
            .schedule_persistence
            .get_by_id(id)
            .await?
            .filter(|schedule| schedule.company_id == company_id)
            .ok_or_else(|| AppError::NotFound("Schedule not found.".into()))?;

        Ok(schedule)
    }

    /// The same check for a channel, which owns the schedules listed under it.
    async fn managed_channel_id(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Uuid> {
        self.managed_company(user_id, company_id).await?;

        let channel = self
            .channel_persistence
            .get_by_id(channel_id)
            .await?
            .filter(|channel| channel.company_id == company_id)
            .ok_or_else(|| AppError::NotFound("Channel not found in this company.".into()))?;

        Ok(channel.id)
    }

    fn validate_write(write: &ScheduleWrite) -> AppResult<()> {
        if write.name.trim().is_empty() {
            return Err(AppError::BadRequest(
                "Schedule name cannot be empty.".into(),
            ));
        }
        if write.subject_template.trim().is_empty() {
            return Err(AppError::BadRequest(
                "Thread subject template cannot be empty.".into(),
            ));
        }
        if write.prompt_template.trim().is_empty() {
            return Err(AppError::BadRequest(
                "Agent prompt template cannot be empty.".into(),
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
        if write.delivery_mode == ScheduleDeliveryMode::EmailCustom
            && write.recipient_emails.is_empty()
        {
            return Err(AppError::BadRequest(
                "Custom email delivery requires at least one recipient email address.".into(),
            ));
        }
        Ok(())
    }

    /// Who this caller may make a schedule run as.
    ///
    /// The company's owner may attribute a run to anybody on the team; an admin may attribute one
    /// only to themselves, so an admin cannot make the company's automation act as a colleague --
    /// which would read that colleague's personal memory into a channel and write the channel's
    /// answers back into it. Both the picker and [`Self::authorized_run_as`] are answered from
    /// this one value, so a form cannot offer a choice the write would refuse.
    #[instrument(skip(self))]
    pub async fn run_as_choices(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<ScheduleRunAsChoices> {
        let company = self.managed_company(user_id, company_id).await?;
        let team = self
            .company_persistence
            .list_company_team_accounts(company_id)
            .await?;

        Ok(ScheduleRunAsChoices {
            team,
            restricted_to: (company.user_id != user_id).then_some(user_id),
        })
    }

    /// The run-as member a write may store.
    ///
    /// Leaving a stored attribution alone is not a change, so an admin editing a schedule the
    /// owner attributed to somebody else only has to leave it be -- they do not have to be
    /// allowed to have chosen that person.
    async fn authorized_run_as(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        change: RunAsChange,
    ) -> AppResult<Option<Uuid>> {
        let Some(requested) = change.requested else {
            return Ok(None);
        };
        if change.stored == Some(requested) {
            return Ok(Some(requested));
        }

        let choices = self.run_as_choices(user_id, company_id).await?;
        if !choices.may_choose(requested) {
            return Err(AppError::BadRequest(
                "Only the company owner can have a schedule run as another team member.".into(),
            ));
        }

        Ok(Some(requested))
    }

    #[instrument(skip(self))]
    pub async fn create_schedule(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        channel_id: Uuid,
        mut write: ScheduleWrite,
    ) -> AppResult<ChannelSchedule> {
        let channel_id = self
            .managed_channel_id(user_id, company_id, channel_id)
            .await?;
        Self::validate_write(&write)?;
        write.run_as_user_id = self
            .authorized_run_as(
                user_id,
                company_id,
                RunAsChange {
                    stored: None,
                    requested: write.run_as_user_id,
                },
            )
            .await?;

        info!(
            "Creating schedule '{}' (type={}) for channel {} in company {}",
            write.name, write.schedule_type, channel_id, company_id
        );

        self.schedule_persistence
            .create(company_id, channel_id, write)
            .await
    }

    #[instrument(skip(self))]
    pub async fn get_schedule(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        id: Uuid,
    ) -> AppResult<Option<ChannelSchedule>> {
        match self.managed_schedule(user_id, company_id, id).await {
            Ok(schedule) => Ok(Some(schedule)),
            Err(AppError::NotFound(_)) => Ok(None),
            Err(err) => Err(err),
        }
    }

    #[instrument(skip(self))]
    pub async fn list_channel_schedules(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Vec<ChannelSchedule>> {
        let channel_id = self
            .managed_channel_id(user_id, company_id, channel_id)
            .await?;
        self.schedule_persistence
            .list_by_channel_id(company_id, channel_id)
            .await
    }

    #[instrument(skip(self))]
    pub async fn list_company_schedules(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Vec<ChannelSchedule>> {
        self.managed_company(user_id, company_id).await?;
        self.schedule_persistence
            .list_by_company_id(company_id)
            .await
    }

    #[instrument(skip(self))]
    pub async fn update_schedule(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        id: Uuid,
        channel_id: Uuid,
        mut write: ScheduleWrite,
    ) -> AppResult<ChannelSchedule> {
        let existing = self.managed_schedule(user_id, company_id, id).await?;
        let channel_id = self
            .managed_channel_id(user_id, company_id, channel_id)
            .await?;
        Self::validate_write(&write)?;
        write.run_as_user_id = self
            .authorized_run_as(
                user_id,
                company_id,
                RunAsChange {
                    stored: existing.run_as_user_id,
                    requested: write.run_as_user_id,
                },
            )
            .await?;

        info!(
            "Updating schedule {} for company {}: {}",
            id, company_id, write.name
        );

        self.schedule_persistence
            .update(&existing, channel_id, write)
            .await
    }

    #[instrument(skip(self))]
    pub async fn delete_schedule(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        id: Uuid,
    ) -> AppResult<()> {
        self.managed_schedule(user_id, company_id, id).await?;

        info!("Deleting schedule {} for company {}", id, company_id);
        self.schedule_persistence.delete(id).await
    }

    #[instrument(skip(self))]
    pub async fn toggle_schedule(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        id: Uuid,
        enabled: bool,
    ) -> AppResult<bool> {
        self.managed_schedule(user_id, company_id, id).await?;

        self.schedule_persistence.set_enabled(id, enabled).await
    }

    #[instrument(skip(self))]
    pub async fn list_schedule_runs(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        schedule_id: Uuid,
        offset: i64,
        limit: i64,
    ) -> AppResult<Vec<crate::entities::schedule::ScheduleRun>> {
        self.managed_schedule(user_id, company_id, schedule_id)
            .await?;

        self.schedule_persistence
            .list_schedule_runs(schedule_id, offset, limit)
            .await
    }

    pub async fn authorize_schedule_run_thread(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        schedule_id: Uuid,
        thread_id: Uuid,
    ) -> AppResult<()> {
        self.managed_schedule(user_id, company_id, schedule_id)
            .await?;
        if !self
            .schedule_persistence
            .schedule_run_contains_thread(company_id, schedule_id, thread_id)
            .await?
        {
            return Err(AppError::NotFound("Schedule run not found".into()));
        }
        Ok(())
    }

    /// Manually triggers an immediate execution of this schedule without waiting for the timer.
    #[instrument(skip(self))]
    pub async fn trigger_schedule_now(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        id: Uuid,
    ) -> AppResult<ChannelSchedule> {
        // Management access is settled before `record_manual_run` writes: checking the row it returns
        // would already have stamped another tenant's schedule on the way to the 404.
        let schedule = self.managed_schedule(user_id, company_id, id).await?;

        info!(
            "Manually triggering schedule '{}' ({})",
            schedule.name, schedule.id
        );

        let schedule = self
            .schedule_persistence
            .record_manual_run(id)
            .await?
            .ok_or_else(|| AppError::NotFound("Schedule not found.".into()))?;

        self.execute_schedule_trigger(&schedule).await?;
        Ok(schedule)
    }

    /// The identity a run acts as, resolved fresh each time the schedule fires.
    ///
    /// Re-checked against the team rather than trusted from the row: revoking somebody's
    /// membership has to stop runs that act as them, so a schedule left pointing at a former
    /// colleague fails loudly onto `last_error` instead of quietly running as nobody. An account
    /// that is deleted outright clears the column, which is the unattributed run it was before.
    async fn run_as_identity(
        &self,
        schedule: &ChannelSchedule,
    ) -> AppResult<Option<ScheduleRunAs>> {
        let Some(user_id) = schedule.run_as_user_id else {
            return Ok(None);
        };

        let account = self
            .company_persistence
            .list_company_team_accounts(schedule.company_id)
            .await?
            .into_iter()
            .find(|account| account.user_id == user_id)
            .ok_or_else(|| {
                AppError::BadRequest(
                    "The team member this schedule runs as is no longer on the company team."
                        .into(),
                )
            })?;

        Ok(Some(ScheduleRunAs {
            user_id: account.user_id,
            email: account.email,
            principal_id: account.principal_id,
        }))
    }

    /// Who a run's thread is between: the channel's own participants, or the company's team when
    /// the channel names none -- plus the member the run acts as, who belongs on a thread opened
    /// in their name.
    ///
    /// The team read propagates rather than falling back to an empty list: a participant list
    /// decides who may see the thread, and "nobody" is not the safe reading of a failed query.
    async fn thread_participants(
        &self,
        channel: &Channel,
        company_id: Uuid,
        run_as: Option<&ScheduleRunAs>,
    ) -> AppResult<Vec<EmailAddress>> {
        let mut participants = channel.participant_emails.clone().unwrap_or_default();
        if participants.is_empty() {
            participants = self
                .company_persistence
                .list_company_team_emails(company_id)
                .await?
                .into_iter()
                .map(EmailAddress::from)
                .collect();
        }

        if let Some(run_as) = run_as
            && !participants
                .iter()
                .any(|email| email.eq_ignore_case(&run_as.email))
        {
            participants.push(run_as.email.clone());
        }

        Ok(participants)
    }

    /// Spawns the thread and enqueues the `background_tasks` entry for this schedule run.
    pub async fn execute_schedule_trigger(&self, schedule: &ChannelSchedule) -> AppResult<Uuid> {
        self.execute_schedule_run(schedule, None).await
    }

    async fn execute_schedule_run(
        &self,
        schedule: &ChannelSchedule,
        durable_run: Option<(Uuid, chrono::DateTime<Utc>, Uuid, Uuid)>,
    ) -> AppResult<Uuid> {
        let run_id = durable_run.map(|(id, _, _, _)| id);
        let now = durable_run
            .map(|(_, scheduled_for, _, _)| scheduled_for)
            .unwrap_or_else(Utc::now);
        let rendered_subject = schedule.render_subject(now);
        let rendered_prompt = schedule.render_prompt(now);

        let channel = self
            .channel_persistence
            .get_by_id(schedule.channel_id)
            .await?
            .ok_or_else(|| AppError::Internal("Channel not found for schedule execution".into()))?;

        let company = self
            .company_persistence
            .get_by_id(schedule.company_id)
            .await?
            .ok_or_else(|| AppError::Internal("Company not found for schedule execution".into()))?;

        let run_as = self.run_as_identity(schedule).await?;
        let participants = self
            .thread_participants(&channel, company.id, run_as.as_ref())
            .await?;

        // 1. Create a fresh thread in the channel
        let thread = match run_id {
            Some(run_id) => {
                self.thread_persistence
                    .ensure_schedule_run_thread(
                        run_id,
                        channel.id,
                        &rendered_subject,
                        &participants,
                    )
                    .await?
            }
            None => {
                self.thread_persistence
                    .create_thread(channel.id, &rendered_subject, &participants)
                    .await?
            }
        };

        info!(
            "Created thread {} ('{}') for scheduled run '{}'",
            thread.id, rendered_subject, schedule.name
        );

        // 2. Save the initial prompt message into the thread.
        //
        // Nothing delivered this prompt: the platform asked the question because a slot came due.
        // So it is an internal message with no mail headers and no recipients -- a run that acts
        // as a member is attributed to that member's handle, and an unattributed one to the
        // company's system principal, which is what "the channel talking to itself" really was.
        let correlation_id = CorrelationId::new();
        let (author, role) = match run_as.as_ref() {
            Some(run_as) => (
                MessageAuthorWrite::Observed(IdentityObservation {
                    identity: qualified_email_identity(run_as.email.clone())?,
                    display_label: None,
                    claim_metadata: IdentityClaimMetadata::observation(),
                    provenance: IdentityProvenance::Account,
                }),
                MessageRole::Human,
            ),
            None => (MessageAuthorWrite::Platform, MessageRole::System),
        };
        // This run's stable identity, for anything that has to name the run rather than the
        // message: the durable slot where there is one, and a fresh id otherwise. The mail
        // renderer derives its thread-root header from it -- no mail carried the prompt, so
        // nothing here is a header.
        let run_key = run_id.unwrap_or_else(Uuid::new_v4);

        let prompt_message = self
            .thread_persistence
            .create_message(
                &MessageWrite::internal(
                    thread.id,
                    author,
                    rendered_subject.clone(),
                    rendered_prompt.clone(),
                    MessageDirection::Inbound,
                    role,
                    correlation_id,
                )
                .created_at(now),
            )
            .await?;

        // 3. Enqueue background task for the TaskWorker to execute the agent pipeline
        let task_payload = ScheduledRunPayload {
            schedule_run_id: run_id,
            schedule_id: schedule.id,
            schedule_name: schedule.name.clone(),
            channel_id: channel.id,
            company_id: company.id,
            thread_id: thread.id,
            subject: rendered_subject,
            prompt: rendered_prompt,
            delivery_mode: schedule.delivery_mode,
            recipient_emails: schedule.recipient_emails.clone(),
            run_as,
            run_key,
            prompt_message_id: prompt_message.canonical_id,
        };
        let task_payload = serde_json::to_value(&task_payload).map_err(|err| {
            AppError::Internal(format!("Failed to encode schedule payload: {err}"))
        })?;

        let task = self
            .task_persistence
            .enqueue_task(NewTask {
                targets: Vec::new(),
                company_id: company.id,
                channel_id: channel.id,
                thread_id: Some(thread.id),
                task_type: SCHEDULED_AGENT_RUN_TASK.to_string(),
                payload: task_payload,
                // A second scheduler waking for the same slot finds this task rather than running
                // the agent twice. A run without a durable slot behind it has nothing to dedup on.
                source: match run_id {
                    Some(run_id) => TaskSource::ScheduleRun(run_id),
                    None => TaskSource::Unattributed,
                },
                // A schedule firing is an ingress of its own: nothing outside caused this run, so
                // this is one of the few places a chain legitimately begins. The prompt message
                // above already joined it.
                correlation_id,
            })
            .await?;

        info!(
            "Enqueued background task {} for schedule '{}'",
            task.id, schedule.name
        );

        if let Some((run_id, _, worker_id, generation)) = durable_run
            && !self
                .schedule_persistence
                .record_run_task(run_id, worker_id, generation, task.id)
                .await?
        {
            return Err(AppError::Internal(
                "Schedule-run materialization lease was lost before completion".into(),
            ));
        }

        Ok(task.id)
    }

    /// Process all currently due schedules (called by the background poller loop).
    pub async fn process_due_schedules(
        &self,
        worker_id: Uuid,
        lock_expires_at: chrono::DateTime<Utc>,
        batch_size: i64,
    ) -> AppResult<usize> {
        let due_runs = self
            .schedule_persistence
            .claim_and_advance_due_schedules(worker_id, lock_expires_at, batch_size)
            .await?;

        let count = due_runs.len();
        for run in due_runs {
            let schedule = &run.schedule;
            info!(
                "Triggering due schedule '{}' ({})",
                schedule.name, schedule.id
            );
            match self
                .execute_schedule_run(
                    schedule,
                    Some((
                        run.id,
                        run.scheduled_for,
                        worker_id,
                        run.materialization_generation,
                    )),
                )
                .await
            {
                Ok(_) => {
                    self.schedule_persistence
                        .clear_last_error(schedule.id)
                        .await?;
                }
                Err(err) => {
                    warn!(
                        "Failed to materialize durable schedule run {} for '{}' ({}): {}.",
                        run.id, schedule.name, schedule.id, err
                    );
                    let recorded = self
                        .schedule_persistence
                        .record_run_error(
                            run.id,
                            worker_id,
                            run.materialization_generation,
                            &err.to_string(),
                        )
                        .await?;
                    if !recorded {
                        warn!(run_id = %run.id, "Schedule-run failure ignored after lease loss");
                    }
                }
            }
        }

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::task::{ResumeActor, StopActor, TaskFailure, TaskLeaseRef};
    use crate::entities::transport::{PrincipalId, TransportKind};
    use crate::entities::{
        channel::{Channel, ChannelAccessMode},
        company::{Company, CompanyAccess, CompanyTeamAccount},
        company_member::CompanyMembership,
        schedule::ScheduleTimezone,
        task::{BackgroundTask, TaskStatus},
        value_objects::ChannelSlug,
    };
    use crate::task_queue::{AgentDispatchCommit, DispatchCommit};
    use crate::use_cases::company::CompanyWrite;
    use crate::use_cases::participant::test_support::email_allowlist_grants;
    use crate::use_cases::thread::test_support::InMemoryThreads;
    use async_trait::async_trait;
    use std::sync::Mutex;

    #[test]
    fn validate_write_checks_bad_inputs() {
        let base = ScheduleWrite {
            name: "Test".into(),
            schedule_type: ScheduleType::Interval,
            interval_seconds: Some(3600),
            scheduled_at: None,
            subject_template: "Subject".into(),
            prompt_template: "Prompt".into(),
            delivery_mode: ScheduleDeliveryMode::MailboxOnly,
            recipient_emails: vec![],
            timezone: ScheduleTimezone::utc(),
            run_as_user_id: None,
            enabled: true,
        };

        // Empty name
        let mut bad = base.clone();
        bad.name = "   ".into();
        assert!(ScheduleUseCases::validate_write(&bad).is_err());

        // Empty subject
        let mut bad = base.clone();
        bad.subject_template = "".into();
        assert!(ScheduleUseCases::validate_write(&bad).is_err());

        // Empty prompt
        let mut bad = base.clone();
        bad.prompt_template = "".into();
        assert!(ScheduleUseCases::validate_write(&bad).is_err());

        // Interval < 60s
        let mut bad = base.clone();
        bad.interval_seconds = Some(30);
        assert!(ScheduleUseCases::validate_write(&bad).is_err());

        // One-off missing scheduled_at
        let mut bad = base.clone();
        bad.schedule_type = ScheduleType::OneOff;
        bad.interval_seconds = None;
        bad.scheduled_at = None;
        assert!(ScheduleUseCases::validate_write(&bad).is_err());

        // Custom email missing recipients
        let mut bad = base.clone();
        bad.delivery_mode = ScheduleDeliveryMode::EmailCustom;
        bad.recipient_emails = vec![];
        assert!(ScheduleUseCases::validate_write(&bad).is_err());

        // Valid Custom Email
        let mut good = base.clone();
        good.delivery_mode = ScheduleDeliveryMode::EmailCustom;
        good.recipient_emails = vec![EmailAddress::from("user@example.com")];
        assert!(ScheduleUseCases::validate_write(&good).is_ok());
    }

    struct MockSchedulePersistence {
        schedules: Mutex<Vec<ChannelSchedule>>,
    }

    #[async_trait]
    impl SchedulePersistence for MockSchedulePersistence {
        async fn create(
            &self,
            company_id: Uuid,
            channel_id: Uuid,
            write: ScheduleWrite,
        ) -> AppResult<ChannelSchedule> {
            let schedule = ChannelSchedule {
                id: Uuid::new_v4(),
                company_id,
                channel_id,
                name: write.name,
                schedule_type: write.schedule_type,
                interval_seconds: write.interval_seconds,
                subject_template: write.subject_template,
                prompt_template: write.prompt_template,
                delivery_mode: write.delivery_mode,
                recipient_emails: write.recipient_emails,
                timezone: write.timezone,
                run_as_user_id: write.run_as_user_id,
                enabled: write.enabled,
                last_run_at: None,
                next_run_at: Some(Utc::now()),
                last_error: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            self.schedules.lock().unwrap().push(schedule.clone());
            Ok(schedule)
        }

        async fn get_by_id(&self, id: Uuid) -> AppResult<Option<ChannelSchedule>> {
            Ok(self
                .schedules
                .lock()
                .unwrap()
                .iter()
                .find(|s| s.id == id)
                .cloned())
        }

        async fn list_by_channel_id(
            &self,
            company_id: Uuid,
            channel_id: Uuid,
        ) -> AppResult<Vec<ChannelSchedule>> {
            Ok(self
                .schedules
                .lock()
                .unwrap()
                .iter()
                .filter(|s| s.company_id == company_id && s.channel_id == channel_id)
                .cloned()
                .collect())
        }

        async fn list_by_company_id(&self, company_id: Uuid) -> AppResult<Vec<ChannelSchedule>> {
            Ok(self
                .schedules
                .lock()
                .unwrap()
                .iter()
                .filter(|s| s.company_id == company_id)
                .cloned()
                .collect())
        }

        async fn update(
            &self,
            existing: &ChannelSchedule,
            channel_id: Uuid,
            write: ScheduleWrite,
        ) -> AppResult<ChannelSchedule> {
            let mut list = self.schedules.lock().unwrap();
            let s = list
                .iter_mut()
                .find(|s| s.id == existing.id)
                .ok_or_else(|| AppError::NotFound("Not found".into()))?;
            s.channel_id = channel_id;
            s.name = write.name;
            s.schedule_type = write.schedule_type;
            s.interval_seconds = write.interval_seconds;
            s.subject_template = write.subject_template;
            s.prompt_template = write.prompt_template;
            s.delivery_mode = write.delivery_mode;
            s.recipient_emails = write.recipient_emails;
            s.timezone = write.timezone;
            s.run_as_user_id = write.run_as_user_id;
            s.updated_at = Utc::now();
            Ok(s.clone())
        }

        async fn release_failed_claim(
            &self,
            schedule: &ChannelSchedule,
            error: &str,
        ) -> AppResult<()> {
            let mut list = self.schedules.lock().unwrap();
            if let Some(s) = list.iter_mut().find(|s| s.id == schedule.id) {
                s.enabled = true;
                s.next_run_at = match s.schedule_type {
                    ScheduleType::Interval => Some(Utc::now()),
                    ScheduleType::OneOff => schedule.next_run_at,
                };
                s.last_error = Some(error.to_string());
            }
            Ok(())
        }

        async fn clear_last_error(&self, id: Uuid) -> AppResult<()> {
            let mut list = self.schedules.lock().unwrap();
            if let Some(s) = list.iter_mut().find(|s| s.id == id) {
                s.last_error = None;
            }
            Ok(())
        }

        async fn delete(&self, id: Uuid) -> AppResult<()> {
            self.schedules.lock().unwrap().retain(|s| s.id != id);
            Ok(())
        }

        async fn set_enabled(&self, id: Uuid, enabled: bool) -> AppResult<bool> {
            let mut list = self.schedules.lock().unwrap();
            if let Some(s) = list.iter_mut().find(|s| s.id == id) {
                s.enabled = enabled;
                Ok(true)
            } else {
                Ok(false)
            }
        }

        async fn claim_and_advance_due_schedules(
            &self,
            _worker_id: Uuid,
            _lock_expires_at: chrono::DateTime<Utc>,
            _limit: i64,
        ) -> AppResult<Vec<crate::entities::schedule::ClaimedScheduleRun>> {
            Ok(vec![])
        }

        async fn record_run_task(
            &self,
            _run_id: Uuid,
            _worker_id: Uuid,
            _generation: Uuid,
            _task_id: Uuid,
        ) -> AppResult<bool> {
            Ok(true)
        }

        async fn record_run_error(
            &self,
            _run_id: Uuid,
            _worker_id: Uuid,
            _generation: Uuid,
            _error: &str,
        ) -> AppResult<bool> {
            Ok(true)
        }

        async fn record_manual_run(&self, id: Uuid) -> AppResult<Option<ChannelSchedule>> {
            let mut list = self.schedules.lock().unwrap();
            if let Some(s) = list.iter_mut().find(|s| s.id == id) {
                s.last_run_at = Some(Utc::now());
                Ok(Some(s.clone()))
            } else {
                Ok(None)
            }
        }

        async fn list_schedule_runs(
            &self,
            _schedule_id: Uuid,
            _offset: i64,
            _limit: i64,
        ) -> AppResult<Vec<crate::entities::schedule::ScheduleRun>> {
            Ok(vec![])
        }
        async fn schedule_run_contains_thread(
            &self,
            _company_id: Uuid,
            _schedule_id: Uuid,
            _thread_id: Uuid,
        ) -> AppResult<bool> {
            Ok(false)
        }
    }

    struct MockCompanyPersistence {
        companies: Mutex<Vec<Company>>,
        memberships: Mutex<Vec<(Uuid, Uuid, CompanyMembership)>>,
    }

    #[async_trait]
    impl CompanyPersistence for MockCompanyPersistence {
        async fn create(&self, _user_id: Uuid, _write: CompanyWrite) -> AppResult<Company> {
            unimplemented!()
        }
        async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Company>> {
            Ok(self
                .companies
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.id == id)
                .cloned())
        }
        async fn get_by_slug(&self, slug: &str) -> AppResult<Option<Company>> {
            Ok(self
                .companies
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.slug.eq_ignore_ascii_case(slug))
                .cloned())
        }
        async fn list_by_user_id(&self, _user_id: Uuid) -> AppResult<Vec<Company>> {
            unimplemented!()
        }
        async fn list_accessible_by_user_id(&self, user_id: Uuid) -> AppResult<Vec<CompanyAccess>> {
            let companies = self.companies.lock().unwrap();
            let memberships = self.memberships.lock().unwrap();
            Ok(companies
                .iter()
                .filter_map(|company| {
                    let membership = if company.user_id == user_id {
                        CompanyMembership::Owner
                    } else if let Some((_, _, membership)) =
                        memberships.iter().find(|(member_id, company_id, _)| {
                            *member_id == user_id && *company_id == company.id
                        })
                    {
                        *membership
                    } else {
                        return None;
                    };
                    Some(CompanyAccess {
                        company: company.clone(),
                        membership,
                    })
                })
                .collect())
        }
        async fn update(&self, _id: Uuid, _write: CompanyWrite) -> AppResult<Company> {
            unimplemented!()
        }
        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }
        async fn list_company_team_emails(&self, _company_id: Uuid) -> AppResult<Vec<String>> {
            Ok(vec!["admin@example.com".into()])
        }

        /// The company's owner plus whatever memberships this double was built with, addressed as
        /// `<account id>@example.com` so a test can recognise whom a run was attributed to.
        async fn list_company_team_accounts(
            &self,
            company_id: Uuid,
        ) -> AppResult<Vec<CompanyTeamAccount>> {
            let companies = self.companies.lock().unwrap();
            let memberships = self.memberships.lock().unwrap();
            let owner = companies
                .iter()
                .find(|company| company.id == company_id)
                .map(|company| (company.user_id, CompanyMembership::Owner));

            Ok(owner
                .into_iter()
                .chain(
                    memberships
                        .iter()
                        .filter(|(_, member_company, _)| *member_company == company_id)
                        .map(|(user_id, _, membership)| (*user_id, *membership)),
                )
                .map(|(user_id, membership)| CompanyTeamAccount {
                    user_id,
                    email: EmailAddress::new(format!("{user_id}@example.com")),
                    username: None,
                    membership,
                    // Derived from the account so a fixture's run-as principal is predictable.
                    principal_id: Some(PrincipalId::new(user_id)),
                })
                .collect())
        }

        /// Model connections are not part of what these tests drive; a call here is a wiring mistake
        /// rather than a state worth simulating.
        async fn list_model_connections(
            &self,
            _company_id: Uuid,
        ) -> AppResult<Vec<crate::entities::company::CompanyModelConnection>> {
            unimplemented!("this double is not exercised on the model-connection path")
        }

        async fn model_api_key(
            &self,
            _company_id: Uuid,
            _provider: &crate::entities::value_objects::ModelProvider,
        ) -> AppResult<Option<String>> {
            unimplemented!("this double is not exercised on the model-connection path")
        }

        async fn replace_model_connections_for_user(
            &self,
            _user_id: Uuid,
            _company_id: Uuid,
            _connections: Vec<crate::use_cases::company::CompanyModelConnectionWrite>,
        ) -> AppResult<()> {
            unimplemented!("this double is not exercised on the model-connection path")
        }
    }

    struct MockChannelPersistence {
        channels: Mutex<Vec<Channel>>,
    }

    #[async_trait]
    impl ChannelPersistence for MockChannelPersistence {
        async fn create(
            &self,
            _company_id: Uuid,
            _write: crate::use_cases::channel::ChannelWrite,
        ) -> AppResult<Channel> {
            unimplemented!()
        }
        async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Channel>> {
            Ok(self
                .channels
                .lock()
                .unwrap()
                .iter()
                .find(|ch| ch.id == id)
                .cloned())
        }
        async fn get_by_company_slug_and_channel_slug(
            &self,
            _company_slug: &crate::entities::value_objects::CompanySlug,
            _channel_slug: &ChannelSlug,
        ) -> AppResult<Option<Channel>> {
            unimplemented!()
        }
        async fn list_by_company_id(&self, _company_id: Uuid) -> AppResult<Vec<Channel>> {
            Ok(vec![])
        }
        async fn update(
            &self,
            _id: Uuid,
            _write: crate::use_cases::channel::ChannelWrite,
        ) -> AppResult<Channel> {
            unimplemented!()
        }
        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }
    }

    struct MockTaskPersistence {
        tasks: Mutex<Vec<BackgroundTask>>,
    }

    #[async_trait]
    impl TaskPersistence for MockTaskPersistence {
        /// No fixture here sends an outreach, so nothing ever asks one to be recorded.
        async fn record_outreach_request_message(
            &self,
            _delivery_id: crate::entities::transport::DeliveryId,
            _write: &crate::use_cases::thread::MessageWrite,
        ) -> AppResult<crate::entities::message::CanonicalMessageId> {
            unreachable!("no fixture here sends an outreach")
        }

        /// The task's own channel and thread. These fixtures never enqueue a multi-channel run, so
        /// stating one target is the honest answer rather than an empty list the worker would read
        /// as "answer nowhere".
        async fn list_task_channel_targets(
            &self,
            _company_id: Uuid,
            task_id: Uuid,
        ) -> AppResult<Vec<crate::use_cases::thread::TaskChannelTarget>> {
            Ok(self
                .tasks
                .lock()
                .unwrap()
                .iter()
                .find(|task| task.id == task_id)
                .and_then(|task| {
                    task.thread_id
                        .map(|thread_id| crate::use_cases::thread::TaskChannelTarget {
                            channel_id: task.channel_id,
                            thread_id,
                            recipient_role: crate::transport::RecipientRole::To,
                        })
                })
                .into_iter()
                .collect())
        }

        async fn commit_agent_dispatch(
            &self,
            commit: AgentDispatchCommit<'_>,
        ) -> AppResult<DispatchCommit> {
            let _ = commit;
            Ok(DispatchCommit::Committed {
                deliveries: Vec::new(),
            })
        }

        async fn renew_task_lease(
            &self,
            _lease: TaskLeaseRef,
            _lock_expires_at: chrono::DateTime<chrono::Utc>,
        ) -> AppResult<bool> {
            Ok(true)
        }

        async fn enqueue_task(
            &self,
            NewTask {
                company_id,
                channel_id,
                thread_id,
                targets: _,
                task_type,
                payload,
                source: _,
                correlation_id,
            }: NewTask,
        ) -> AppResult<BackgroundTask> {
            let task = BackgroundTask {
                id: Uuid::new_v4(),
                company_id,
                channel_id,
                thread_id,
                correlation_id,
                task_type,
                status: TaskStatus::Pending,
                payload,
                retry_count: 0,
                max_retries: 3,
                last_error: None,
                worker_id: None,
                execution_generation: None,
                locked_at: None,
                lock_expires_at: None,
                run_at: Utc::now(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            self.tasks.lock().unwrap().push(task.clone());
            Ok(task)
        }
        async fn get_task_by_id(&self, _id: Uuid) -> AppResult<Option<BackgroundTask>> {
            unimplemented!()
        }
        async fn update_task_payload(
            &self,
            _id: Uuid,
            _payload: serde_json::Value,
        ) -> AppResult<()> {
            unimplemented!()
        }
        async fn claim_pending_tasks(
            &self,
            _worker_id: Uuid,
            _lock_expires_at: chrono::DateTime<Utc>,
            _limit: i64,
        ) -> AppResult<Vec<BackgroundTask>> {
            unimplemented!()
        }
        async fn claim_task(
            &self,
            _id: Uuid,
            _worker_id: Uuid,
            _lock_expires_at: chrono::DateTime<Utc>,
        ) -> AppResult<bool> {
            unimplemented!()
        }
        async fn mark_task_completed(&self, _lease: TaskLeaseRef) -> AppResult<bool> {
            unimplemented!()
        }
        async fn mark_task_failed(&self, _failure: TaskFailure<'_>) -> AppResult<bool> {
            unimplemented!()
        }
        async fn stop_task(&self, _id: Uuid, _actor: StopActor) -> AppResult<BackgroundTask> {
            unimplemented!()
        }
        async fn resume_task(&self, _id: Uuid, _actor: ResumeActor) -> AppResult<BackgroundTask> {
            unimplemented!()
        }
        async fn list_company_tasks(
            &self,
            _company_id: Uuid,
            _channel_id: Option<Uuid>,
            _status: Option<TaskStatus>,
            _sort_asc: bool,
        ) -> AppResult<Vec<BackgroundTask>> {
            unimplemented!()
        }
    }

    /// A company owned by `user_id`, named after its slug.
    fn company_of(id: Uuid, user_id: Uuid, slug: &str) -> Company {
        Company {
            channel_defaults: Default::default(),
            id,
            user_id,
            name: slug.into(),
            slug: slug.into(),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            created_at: Utc::now(),
        }
    }

    /// An unrestricted channel: with no participant list of its own, a run's thread falls back to
    /// the company team, which is the path the run-as member is added on top of.
    fn channel_of(id: Uuid, company_id: Uuid) -> Channel {
        Channel {
            owner_agent_id: None,
            id,
            company_id,
            name: "Reports".into(),
            description: None,
            slug: "reports".into(),
            alias_slugs: vec![],
            participant_emails: None,
            access_mode: ChannelAccessMode::Team,
            principal_grants: Vec::new(),
            agent_ids: None,
            enabled: true,
            add_3rd_party: true,
            retrieve_company_memory: false,
            retrieve_agent_memory: false,
            retrieve_user_memory: false,
            persist_company_memory: false,
            persist_agent_memory: false,
            persist_user_memory: false,
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
        }
    }

    /// A schedule belongs to exactly one company. Every entry point has to say so *before* it
    /// reads or writes the row, or a caller who owns one company reaches another's schedules by
    /// pairing their own company id with a borrowed channel or schedule id.
    #[tokio::test]
    async fn a_second_tenant_can_neither_read_nor_stamp_this_tenants_schedules() {
        let mine_user = Uuid::new_v4();
        let theirs_user = Uuid::new_v4();
        let mine_company = Uuid::new_v4();
        let theirs_company = Uuid::new_v4();
        let theirs_channel = Uuid::new_v4();
        let mine_channel = Uuid::new_v4();

        let schedule_persistence = Arc::new(MockSchedulePersistence {
            schedules: Mutex::new(vec![]),
        });
        let use_cases = ScheduleUseCases::new(
            schedule_persistence.clone(),
            Arc::new(MockCompanyPersistence {
                companies: Mutex::new(vec![
                    company_of(mine_company, mine_user, "mine"),
                    company_of(theirs_company, theirs_user, "theirs"),
                ]),
                memberships: Mutex::new(Vec::new()),
            }),
            Arc::new(MockChannelPersistence {
                channels: Mutex::new(vec![
                    channel_of(mine_channel, mine_company),
                    channel_of(theirs_channel, theirs_company),
                ]),
            }),
            Arc::new(InMemoryThreads::new()),
            Arc::new(MockTaskPersistence {
                tasks: Mutex::new(vec![]),
            }),
        );

        let theirs = use_cases
            .create_schedule(
                theirs_user,
                theirs_company,
                theirs_channel,
                ScheduleWrite {
                    name: "Their Private Digest".into(),
                    schedule_type: ScheduleType::Interval,
                    interval_seconds: Some(86400),
                    scheduled_at: None,
                    subject_template: "[Theirs] {{date}}".into(),
                    prompt_template: "Confidential prompt".into(),
                    delivery_mode: ScheduleDeliveryMode::MailboxOnly,
                    recipient_emails: vec![],
                    timezone: ScheduleTimezone::utc(),
                    run_as_user_id: None,
                    enabled: true,
                },
            )
            .await
            .unwrap();
        let untouched = schedule_persistence
            .get_by_id(theirs.id)
            .await
            .unwrap()
            .unwrap();

        // Their channel id, my company id: the channel check has to catch this.
        assert!(
            use_cases
                .list_channel_schedules(mine_user, mine_company, theirs_channel)
                .await
                .is_err(),
            "a channel from another company must not list its schedules"
        );

        // Their schedule id, my company id, on every entry point that takes one.
        assert!(
            use_cases
                .get_schedule(mine_user, mine_company, theirs.id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            use_cases
                .delete_schedule(mine_user, mine_company, theirs.id)
                .await
                .is_err()
        );
        assert!(
            use_cases
                .toggle_schedule(mine_user, mine_company, theirs.id, false)
                .await
                .is_err()
        );
        assert!(
            use_cases
                .trigger_schedule_now(mine_user, mine_company, theirs.id)
                .await
                .is_err()
        );

        // The refused "run now" must not have stamped the row on its way to the error.
        let after = schedule_persistence
            .get_by_id(theirs.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            after.last_run_at, untouched.last_run_at,
            "a refused trigger must not record a run on another tenant's schedule"
        );
        assert_eq!(after.enabled, untouched.enabled);
    }

    #[tokio::test]
    async fn schedule_use_cases_crud_and_manual_trigger_flow_works() {
        let owner_id = Uuid::new_v4();
        let admin_id = Uuid::new_v4();
        let member_id = Uuid::new_v4();
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();

        let company = Company {
            channel_defaults: Default::default(),
            id: company_id,
            user_id: owner_id,
            name: "Acme Corp".into(),
            slug: "acme".into(),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            created_at: Utc::now(),
        };

        let channel = Channel {
            owner_agent_id: None,
            id: channel_id,
            company_id,
            name: "Daily Reports".into(),
            description: None,
            slug: "reports".into(),
            alias_slugs: vec![],
            participant_emails: Some(vec![EmailAddress::from("admin@example.com")]),
            access_mode: ChannelAccessMode::Allowlist,
            principal_grants: email_allowlist_grants(company_id, &["admin@example.com"]),
            agent_ids: None,
            enabled: true,
            add_3rd_party: true,
            retrieve_company_memory: false,
            retrieve_agent_memory: false,
            retrieve_user_memory: false,
            persist_company_memory: false,
            persist_agent_memory: false,
            persist_user_memory: false,
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
        };
        let target_channel = Channel {
            owner_agent_id: None,
            id: Uuid::new_v4(),
            name: "Planning".into(),
            slug: "planning".into(),
            ..channel.clone()
        };

        let schedule_persistence = Arc::new(MockSchedulePersistence {
            schedules: Mutex::new(vec![]),
        });
        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![company.clone()]),
            memberships: Mutex::new(vec![
                (admin_id, company_id, CompanyMembership::Admin),
                (member_id, company_id, CompanyMembership::Member),
            ]),
        });
        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(vec![channel.clone(), target_channel.clone()]),
        });
        let thread_persistence = Arc::new(InMemoryThreads::new());
        let task_persistence = Arc::new(MockTaskPersistence {
            tasks: Mutex::new(vec![]),
        });

        let use_cases = ScheduleUseCases::new(
            schedule_persistence.clone(),
            company_persistence.clone(),
            channel_persistence.clone(),
            thread_persistence.clone(),
            task_persistence.clone(),
        );

        // 1. Create schedule
        let write = ScheduleWrite {
            name: "Morning Digest".into(),
            schedule_type: ScheduleType::Interval,
            interval_seconds: Some(86400),
            scheduled_at: None,
            subject_template: "[Daily] Digest - {{date}}".into(),
            prompt_template: "Analyze tickets for {{date}}".into(),
            delivery_mode: ScheduleDeliveryMode::MailboxOnly,
            recipient_emails: vec![],
            timezone: ScheduleTimezone::utc(),
            run_as_user_id: None,
            enabled: true,
        };

        assert!(
            use_cases
                .create_schedule(member_id, company_id, channel_id, write.clone())
                .await
                .is_err(),
            "an ordinary member must not create schedules"
        );

        let created = use_cases
            .create_schedule(admin_id, company_id, channel_id, write)
            .await
            .unwrap();

        assert_eq!(created.name, "Morning Digest");

        // 2. Fetch and list
        let fetched = use_cases
            .get_schedule(admin_id, company_id, created.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.id, created.id);

        let list = use_cases
            .list_channel_schedules(admin_id, company_id, channel_id)
            .await
            .unwrap();
        assert_eq!(list.len(), 1);

        // Editing can move a schedule to another channel in the same company.
        let moved = use_cases
            .update_schedule(
                admin_id,
                company_id,
                created.id,
                target_channel.id,
                ScheduleWrite {
                    name: created.name.clone(),
                    schedule_type: created.schedule_type,
                    interval_seconds: created.interval_seconds,
                    scheduled_at: None,
                    subject_template: created.subject_template.clone(),
                    prompt_template: created.prompt_template.clone(),
                    delivery_mode: created.delivery_mode,
                    recipient_emails: created.recipient_emails.clone(),
                    timezone: created.timezone,
                    run_as_user_id: None,
                    enabled: created.enabled,
                },
            )
            .await
            .unwrap();
        assert_eq!(moved.channel_id, target_channel.id);

        // 3. Toggle and trigger the schedule now.
        let toggled = use_cases
            .toggle_schedule(admin_id, company_id, created.id, false)
            .await
            .expect("an admin toggles a schedule");
        assert!(toggled);
        assert!(
            !use_cases
                .get_schedule(admin_id, company_id, created.id)
                .await
                .unwrap()
                .unwrap()
                .enabled
        );

        let triggered = use_cases
            .trigger_schedule_now(admin_id, company_id, created.id)
            .await
            .unwrap();
        assert_eq!(triggered.id, created.id);

        // Verify thread, message, and background task were created
        {
            let threads = thread_persistence.threads();
            assert_eq!(threads.len(), 1);
            assert!(threads[0].subject.contains("[Daily] Digest"));

            let messages = thread_persistence.messages();
            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0].thread_id, threads[0].id);
            assert!(messages[0].clean_text_body.contains("Analyze tickets for"));

            let tasks = task_persistence.tasks.lock().unwrap();
            assert_eq!(tasks.len(), 1);
            assert_eq!(tasks[0].task_type, "scheduled_agent_run");
            assert_eq!(tasks[0].thread_id, Some(threads[0].id));
        }

        use_cases
            .delete_schedule(admin_id, company_id, created.id)
            .await
            .expect("an admin deletes a schedule");
    }

    /// The company, its team and one channel, wired to in-memory persistence.
    ///
    /// Every run-as test needs the same three people -- an owner, an admin and a member -- so the
    /// fixture is built once rather than re-declared per test.
    struct RunAsFixture {
        owner_id: Uuid,
        admin_id: Uuid,
        member_id: Uuid,
        company_id: Uuid,
        channel_id: Uuid,
        use_cases: ScheduleUseCases,
        company_persistence: Arc<MockCompanyPersistence>,
        thread_persistence: Arc<InMemoryThreads>,
        task_persistence: Arc<MockTaskPersistence>,
    }

    impl RunAsFixture {
        fn new() -> Self {
            let owner_id = Uuid::new_v4();
            let admin_id = Uuid::new_v4();
            let member_id = Uuid::new_v4();
            let company_id = Uuid::new_v4();
            let channel_id = Uuid::new_v4();

            let company_persistence = Arc::new(MockCompanyPersistence {
                companies: Mutex::new(vec![company_of(company_id, owner_id, "acme")]),
                memberships: Mutex::new(vec![
                    (admin_id, company_id, CompanyMembership::Admin),
                    (member_id, company_id, CompanyMembership::Member),
                ]),
            });
            let thread_persistence = Arc::new(InMemoryThreads::new());
            let task_persistence = Arc::new(MockTaskPersistence {
                tasks: Mutex::new(vec![]),
            });

            let use_cases = ScheduleUseCases::new(
                Arc::new(MockSchedulePersistence {
                    schedules: Mutex::new(vec![]),
                }),
                company_persistence.clone(),
                Arc::new(MockChannelPersistence {
                    channels: Mutex::new(vec![channel_of(channel_id, company_id)]),
                }),
                thread_persistence.clone(),
                task_persistence.clone(),
            );

            Self {
                owner_id,
                admin_id,
                member_id,
                company_id,
                channel_id,
                use_cases,
                company_persistence,
                thread_persistence,
                task_persistence,
            }
        }

        /// A valid write, naming whoever the test wants the runs attributed to.
        fn write(&self, run_as_user_id: Option<Uuid>) -> ScheduleWrite {
            ScheduleWrite {
                name: "Morning Digest".into(),
                schedule_type: ScheduleType::Interval,
                interval_seconds: Some(3600),
                scheduled_at: None,
                subject_template: "[Daily] {{date}}".into(),
                prompt_template: "Summarise yesterday".into(),
                delivery_mode: ScheduleDeliveryMode::MailboxOnly,
                recipient_emails: vec![],
                timezone: ScheduleTimezone::utc(),
                run_as_user_id,
                enabled: true,
            }
        }

        /// The address [`MockCompanyPersistence`] gives an account.
        fn email_of(user_id: Uuid) -> EmailAddress {
            EmailAddress::new(format!("{user_id}@example.com"))
        }
    }

    /// The owner may hand a schedule to anybody on the team; an admin may only take it themselves.
    /// The two halves are one rule, so they are asserted against one picker.
    #[tokio::test]
    async fn who_a_schedule_may_run_as_depends_on_who_is_asking() {
        let fixture = RunAsFixture::new();

        let owners_choices = fixture
            .use_cases
            .run_as_choices(fixture.owner_id, fixture.company_id)
            .await
            .unwrap();
        assert!(owners_choices.may_choose(fixture.member_id));
        assert!(owners_choices.may_choose(fixture.admin_id));
        assert_eq!(owners_choices.choosable().count(), 3);

        let admins_choices = fixture
            .use_cases
            .run_as_choices(fixture.admin_id, fixture.company_id)
            .await
            .unwrap();
        assert!(admins_choices.may_choose(fixture.admin_id));
        assert!(!admins_choices.may_choose(fixture.member_id));
        assert!(!admins_choices.may_choose(fixture.owner_id));
        // The whole team is still named, so an admin can read an attribution they cannot pick.
        assert_eq!(admins_choices.team.len(), 3);
        assert_eq!(admins_choices.choosable().count(), 1);
    }

    /// The picker's rule has to hold at the write, or naming a colleague in a hand-made request
    /// would attribute the company's automation to them anyway.
    #[tokio::test]
    async fn an_admin_cannot_attribute_a_schedule_to_a_colleague() {
        let fixture = RunAsFixture::new();

        let refused = fixture
            .use_cases
            .create_schedule(
                fixture.admin_id,
                fixture.company_id,
                fixture.channel_id,
                fixture.write(Some(fixture.member_id)),
            )
            .await;
        assert!(
            matches!(refused, Err(AppError::BadRequest(_))),
            "an admin naming a colleague must be refused, got {refused:?}"
        );

        let theirs = fixture
            .use_cases
            .create_schedule(
                fixture.admin_id,
                fixture.company_id,
                fixture.channel_id,
                fixture.write(Some(fixture.admin_id)),
            )
            .await
            .expect("an admin may run a schedule as themselves");
        assert_eq!(theirs.run_as_user_id, Some(fixture.admin_id));

        let owners = fixture
            .use_cases
            .create_schedule(
                fixture.owner_id,
                fixture.company_id,
                fixture.channel_id,
                fixture.write(Some(fixture.member_id)),
            )
            .await
            .expect("the owner may run a schedule as any team member");
        assert_eq!(owners.run_as_user_id, Some(fixture.member_id));
    }

    /// Leaving somebody else's attribution alone is not a change, or an admin editing the
    /// schedule's prompt would silently re-point the run at themselves.
    #[tokio::test]
    async fn an_admin_may_keep_an_attribution_they_could_not_have_chosen() {
        let fixture = RunAsFixture::new();

        let created = fixture
            .use_cases
            .create_schedule(
                fixture.owner_id,
                fixture.company_id,
                fixture.channel_id,
                fixture.write(Some(fixture.member_id)),
            )
            .await
            .unwrap();

        let mut edit = fixture.write(Some(fixture.member_id));
        edit.prompt_template = "Summarise last week".into();
        let kept = fixture
            .use_cases
            .update_schedule(
                fixture.admin_id,
                fixture.company_id,
                created.id,
                fixture.channel_id,
                edit,
            )
            .await
            .expect("an unrelated edit must not have to re-choose the member");
        assert_eq!(kept.run_as_user_id, Some(fixture.member_id));
        assert_eq!(kept.prompt_template, "Summarise last week");

        // Changing it to somebody else is still a change, and still theirs alone to make.
        let refused = fixture
            .use_cases
            .update_schedule(
                fixture.admin_id,
                fixture.company_id,
                created.id,
                fixture.channel_id,
                fixture.write(Some(fixture.owner_id)),
            )
            .await;
        assert!(matches!(refused, Err(AppError::BadRequest(_))));
    }

    /// What attributing a run actually buys: the prompt is the member's, and the payload carries
    /// them so the worker scopes their memory rather than nobody's.
    #[tokio::test]
    async fn a_run_acts_as_the_member_it_names() {
        let fixture = RunAsFixture::new();
        let member_email = RunAsFixture::email_of(fixture.member_id);

        let created = fixture
            .use_cases
            .create_schedule(
                fixture.owner_id,
                fixture.company_id,
                fixture.channel_id,
                fixture.write(Some(fixture.member_id)),
            )
            .await
            .unwrap();

        fixture
            .use_cases
            .trigger_schedule_now(fixture.owner_id, fixture.company_id, created.id)
            .await
            .unwrap();

        let messages = fixture.thread_persistence.messages();
        let prompt = messages.first().expect("the run opens with its prompt");
        assert_eq!(
            prompt
                .author
                .identity
                .as_ref()
                .map(|identity| identity.subject().as_str()),
            Some(member_email.as_str())
        );
        assert_eq!(prompt.role, MessageRole::Human);

        let threads = fixture.thread_persistence.threads();
        assert!(
            threads[0]
                .participant_projection
                .subjects_for(TransportKind::Email)
                .contains(&member_email.as_str()),
            "the member a run acts as belongs on its thread"
        );

        let tasks = fixture.task_persistence.tasks.lock().unwrap();
        let payload: ScheduledRunPayload =
            serde_json::from_value(tasks[0].payload.clone()).unwrap();
        assert_eq!(
            payload.run_as,
            Some(ScheduleRunAs {
                user_id: fixture.member_id,
                email: member_email,
                principal_id: Some(PrincipalId::new(fixture.member_id)),
            })
        );
    }

    /// Membership can be revoked after the fact, and a run that still acts as a former colleague
    /// would read and write their personal memory. It has to stop instead.
    #[tokio::test]
    async fn a_run_refuses_once_its_member_has_left_the_team() {
        let fixture = RunAsFixture::new();

        let created = fixture
            .use_cases
            .create_schedule(
                fixture.owner_id,
                fixture.company_id,
                fixture.channel_id,
                fixture.write(Some(fixture.member_id)),
            )
            .await
            .unwrap();

        fixture
            .company_persistence
            .memberships
            .lock()
            .unwrap()
            .retain(|(user_id, _, _)| *user_id != fixture.member_id);

        let refused = fixture
            .use_cases
            .trigger_schedule_now(fixture.owner_id, fixture.company_id, created.id)
            .await;
        assert!(
            matches!(refused, Err(AppError::BadRequest(_))),
            "a run naming a former member must not launch, got {refused:?}"
        );
        assert!(
            fixture.task_persistence.tasks.lock().unwrap().is_empty(),
            "nothing may be queued for a run that cannot name who it acts as"
        );
    }
}
