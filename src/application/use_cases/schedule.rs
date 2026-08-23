use chrono::Utc;
use std::sync::Arc;
use tracing::{info, instrument, warn};
use uuid::Uuid;

use crate::{
    adapters::persistence::{schedule::SchedulePersistence, task::TaskPersistence},
    app_error::{AppError, AppResult},
    entities::{
        message::{Message, MessageDirection, MessageRole},
        schedule::{
            ChannelSchedule, ScheduleDeliveryMode, ScheduleType, ScheduleWrite, ScheduledRunPayload,
        },
        value_objects::{EmailAddress, MessageId},
    },
    infra::config::AppConfig,
    use_cases::{
        channel::ChannelPersistence,
        company::{CompanyPersistence, owned_company},
        thread::ThreadPersistence,
    },
};

/// The `background_tasks.task_type` a schedule run is queued under. Named once here because the
/// worker dispatches on it and the runs query filters by it.
pub const SCHEDULED_AGENT_RUN_TASK: &str = "scheduled_agent_run";

#[derive(Clone)]
pub struct ScheduleUseCases {
    schedule_persistence: Arc<dyn SchedulePersistence>,
    company_persistence: Arc<dyn CompanyPersistence>,
    channel_persistence: Arc<dyn ChannelPersistence>,
    thread_persistence: Arc<dyn ThreadPersistence>,
    task_persistence: Arc<dyn TaskPersistence>,
    config: Arc<AppConfig>,
}

impl ScheduleUseCases {
    pub fn new(
        schedule_persistence: Arc<dyn SchedulePersistence>,
        company_persistence: Arc<dyn CompanyPersistence>,
        channel_persistence: Arc<dyn ChannelPersistence>,
        thread_persistence: Arc<dyn ThreadPersistence>,
        task_persistence: Arc<dyn TaskPersistence>,
        config: Arc<AppConfig>,
    ) -> Self {
        Self {
            schedule_persistence,
            company_persistence,
            channel_persistence,
            thread_persistence,
            task_persistence,
            config,
        }
    }

    pub fn persistence(&self) -> &Arc<dyn SchedulePersistence> {
        &self.schedule_persistence
    }

    async fn verify_company_owner(&self, user_id: Uuid, company_id: Uuid) -> AppResult<()> {
        owned_company(self.company_persistence.as_ref(), user_id, company_id).await?;
        Ok(())
    }

    /// The one place a schedule is resolved for a caller: the user must own the company, and the
    /// schedule must belong to it. Every read and every write goes through here *before* touching
    /// the row, so a schedule from another tenant is never loaded, mutated or returned.
    async fn owned_schedule(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        id: Uuid,
    ) -> AppResult<ChannelSchedule> {
        self.verify_company_owner(user_id, company_id).await?;

        let schedule = self
            .schedule_persistence
            .get_by_id(id)
            .await?
            .filter(|schedule| schedule.company_id == company_id)
            .ok_or_else(|| AppError::NotFound("Schedule not found.".into()))?;

        Ok(schedule)
    }

    /// The same check for a channel, which owns the schedules listed under it.
    async fn owned_channel_id(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Uuid> {
        self.verify_company_owner(user_id, company_id).await?;

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

    #[instrument(skip(self))]
    pub async fn create_schedule(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        channel_id: Uuid,
        write: ScheduleWrite,
    ) -> AppResult<ChannelSchedule> {
        let channel_id = self
            .owned_channel_id(user_id, company_id, channel_id)
            .await?;
        Self::validate_write(&write)?;

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
        match self.owned_schedule(user_id, company_id, id).await {
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
            .owned_channel_id(user_id, company_id, channel_id)
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
        self.verify_company_owner(user_id, company_id).await?;
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
        write: ScheduleWrite,
    ) -> AppResult<ChannelSchedule> {
        let existing = self.owned_schedule(user_id, company_id, id).await?;
        Self::validate_write(&write)?;

        info!(
            "Updating schedule {} for company {}: {}",
            id, company_id, write.name
        );

        self.schedule_persistence.update(&existing, write).await
    }

    #[instrument(skip(self))]
    pub async fn delete_schedule(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        id: Uuid,
    ) -> AppResult<()> {
        self.owned_schedule(user_id, company_id, id).await?;

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
        self.owned_schedule(user_id, company_id, id).await?;

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
        self.owned_schedule(user_id, company_id, schedule_id)
            .await?;

        self.schedule_persistence
            .list_schedule_runs(schedule_id, offset, limit)
            .await
    }

    /// Manually triggers an immediate execution of this schedule without waiting for the timer.
    #[instrument(skip(self))]
    pub async fn trigger_schedule_now(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        id: Uuid,
    ) -> AppResult<ChannelSchedule> {
        // Ownership is settled before `record_manual_run` writes: checking the row it returns
        // would already have stamped another tenant's schedule on the way to the 404.
        let schedule = self.owned_schedule(user_id, company_id, id).await?;

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

    /// Spawns the thread and enqueues the `background_tasks` entry for this schedule run.
    pub async fn execute_schedule_trigger(&self, schedule: &ChannelSchedule) -> AppResult<Uuid> {
        self.execute_schedule_run(schedule, None).await
    }

    async fn execute_schedule_run(
        &self,
        schedule: &ChannelSchedule,
        durable_run: Option<(Uuid, chrono::DateTime<Utc>)>,
    ) -> AppResult<Uuid> {
        let run_id = durable_run.map(|(id, _)| id);
        let now = durable_run
            .map(|(_, scheduled_for)| scheduled_for)
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

        // Thread participants: default to channel participants or company team
        let mut participants = channel.participant_emails.clone().unwrap_or_default();
        if participants.is_empty() {
            let team = self
                .company_persistence
                .list_company_team_emails(company.id)
                .await
                .unwrap_or_default();
            participants = team.into_iter().map(EmailAddress::from).collect();
        }

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

        // 2. Save the initial prompt message into the thread
        let sender = channel.inbound_address(&company.slug, &self.config.app_domain_name);
        let prompt_message_id = MessageId::new(match run_id {
            Some(run_id) => format!("<schedule-run-{run_id}@{}>", self.config.app_domain_name),
            None => format!(
                "<schedule-{}-{}@{}>",
                schedule.id,
                now.timestamp(),
                self.config.app_domain_name
            ),
        });

        let prompt_message = self
            .thread_persistence
            .create_message(&Message {
                id: Uuid::new_v4(),
                thread_id: thread.id,
                message_id: prompt_message_id.clone(),
                in_reply_to: None,
                references_list: vec![],
                sender: sender.clone(),
                recipients_to: vec![sender.clone()],
                recipients_cc: vec![],
                subject: rendered_subject.clone(),
                clean_text_body: rendered_prompt.clone(),
                raw_text_body: Some(rendered_prompt.clone()),
                raw_html_body: None,
                attachments: None,
                direction: MessageDirection::Inbound,
                role: MessageRole::System,
                thread_index: None,
                created_at: now,
            })
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
            trigger_message_id: prompt_message.message_id.clone(),
        };
        let task_payload = serde_json::to_value(&task_payload).map_err(|err| {
            AppError::Internal(format!("Failed to encode schedule payload: {err}"))
        })?;

        let task = self
            .task_persistence
            .enqueue_task(
                company.id,
                channel.id,
                Some(thread.id),
                SCHEDULED_AGENT_RUN_TASK,
                task_payload,
            )
            .await?;

        info!(
            "Enqueued background task {} for schedule '{}'",
            task.id, schedule.name
        );

        if let Some(run_id) = run_id {
            self.schedule_persistence
                .record_run_task(run_id, task.id)
                .await?;
        }

        Ok(task.id)
    }

    /// Process all currently due schedules (called by the background poller loop).
    pub async fn process_due_schedules(&self, batch_size: i64) -> AppResult<usize> {
        let due_runs = self
            .schedule_persistence
            .claim_and_advance_due_schedules(batch_size)
            .await?;

        let count = due_runs.len();
        for run in due_runs {
            let schedule = &run.schedule;
            info!(
                "Triggering due schedule '{}' ({})",
                schedule.name, schedule.id
            );
            match self
                .execute_schedule_run(schedule, Some((run.id, run.scheduled_for)))
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
                    self.schedule_persistence
                        .record_run_error(run.id, &err.to_string())
                        .await?;
                }
            }
        }

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{
        channel::Channel,
        company::Company,
        company_member::CompanyMembership,
        cursor::{MessageCursor, ThreadCursor},
        schedule::ScheduleTimezone,
        task::{BackgroundTask, TaskStatus},
        thread::Thread,
        value_objects::ChannelSlug,
    };
    use crate::use_cases::company::CompanyWrite;
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
            write: ScheduleWrite,
        ) -> AppResult<ChannelSchedule> {
            let mut list = self.schedules.lock().unwrap();
            let s = list
                .iter_mut()
                .find(|s| s.id == existing.id)
                .ok_or_else(|| AppError::NotFound("Not found".into()))?;
            s.name = write.name;
            s.schedule_type = write.schedule_type;
            s.interval_seconds = write.interval_seconds;
            s.subject_template = write.subject_template;
            s.prompt_template = write.prompt_template;
            s.delivery_mode = write.delivery_mode;
            s.recipient_emails = write.recipient_emails;
            s.timezone = write.timezone;
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
            _limit: i64,
        ) -> AppResult<Vec<crate::entities::schedule::ClaimedScheduleRun>> {
            Ok(vec![])
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
    }

    struct MockCompanyPersistence {
        companies: Mutex<Vec<Company>>,
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
        async fn update(&self, _id: Uuid, _write: CompanyWrite) -> AppResult<Company> {
            unimplemented!()
        }
        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }
        async fn membership_for_email(
            &self,
            _company_id: Uuid,
            _email: &str,
        ) -> AppResult<CompanyMembership> {
            Ok(CompanyMembership::Member)
        }
        async fn list_company_team_emails(&self, _company_id: Uuid) -> AppResult<Vec<String>> {
            Ok(vec!["admin@example.com".into()])
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

    struct MockThreadPersistence {
        threads: Mutex<Vec<Thread>>,
        messages: Mutex<Vec<Message>>,
    }

    #[async_trait]
    impl ThreadPersistence for MockThreadPersistence {
        async fn create_thread(
            &self,
            channel_id: Uuid,
            subject: &str,
            participant_emails: &[EmailAddress],
        ) -> AppResult<Thread> {
            let thread = Thread {
                id: Uuid::new_v4(),
                channel_id,
                subject: subject.to_string(),
                participant_emails: participant_emails.to_vec(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            self.threads.lock().unwrap().push(thread.clone());
            Ok(thread)
        }
        async fn get_thread_by_id(&self, _id: Uuid) -> AppResult<Option<Thread>> {
            unimplemented!()
        }
        async fn list_threads_by_channel_id(
            &self,
            _channel_id: Uuid,
            _before: Option<ThreadCursor>,
            _limit: usize,
        ) -> AppResult<Vec<Thread>> {
            unimplemented!()
        }
        async fn list_threads_updated_after(
            &self,
            _channel_id: Uuid,
            _after: Option<ThreadCursor>,
            _limit: usize,
        ) -> AppResult<Vec<Thread>> {
            unimplemented!()
        }
        async fn update_thread_participants(
            &self,
            _id: Uuid,
            _participant_emails: &[EmailAddress],
        ) -> AppResult<Thread> {
            unimplemented!()
        }
        async fn find_thread_by_message_ids(
            &self,
            _channel_id: Uuid,
            _message_ids: &[MessageId],
        ) -> AppResult<Option<Thread>> {
            unimplemented!()
        }
        async fn find_thread_by_thread_index(
            &self,
            _channel_id: Uuid,
            _thread_index_prefix: &crate::entities::value_objects::ThreadIndex,
        ) -> AppResult<Option<Thread>> {
            unimplemented!()
        }
        async fn count_recent_messages(
            &self,
            _thread_id: Uuid,
            _duration_secs: i64,
        ) -> AppResult<usize> {
            unimplemented!()
        }
        async fn create_message(&self, message: &Message) -> AppResult<Message> {
            self.messages.lock().unwrap().push(message.clone());
            Ok(message.clone())
        }
        async fn get_message_by_message_id(
            &self,
            _company_id: Uuid,
            _message_id: &MessageId,
        ) -> AppResult<Option<Message>> {
            unimplemented!()
        }
        async fn find_outbound_reply(
            &self,
            _thread_id: Uuid,
            _in_reply_to: &MessageId,
        ) -> AppResult<Option<Message>> {
            unimplemented!()
        }
        async fn list_messages_by_thread_id(&self, thread_id: Uuid) -> AppResult<Vec<Message>> {
            Ok(self
                .messages
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.thread_id == thread_id)
                .cloned()
                .collect())
        }
        async fn list_messages_after(
            &self,
            _thread_id: Uuid,
            _after: Option<MessageCursor>,
            _limit: usize,
        ) -> AppResult<Vec<Message>> {
            unimplemented!()
        }
    }

    struct MockTaskPersistence {
        tasks: Mutex<Vec<BackgroundTask>>,
    }

    #[async_trait]
    impl TaskPersistence for MockTaskPersistence {
        async fn enqueue_task(
            &self,
            company_id: Uuid,
            channel_id: Uuid,
            thread_id: Option<Uuid>,
            task_type: &str,
            payload: serde_json::Value,
        ) -> AppResult<BackgroundTask> {
            let task = BackgroundTask {
                id: Uuid::new_v4(),
                company_id,
                channel_id,
                thread_id,
                task_type: task_type.to_string(),
                status: TaskStatus::Pending,
                payload,
                retry_count: 0,
                max_retries: 3,
                last_error: None,
                worker_id: None,
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
        async fn mark_task_completed(&self, _id: Uuid, _worker_id: Uuid) -> AppResult<bool> {
            unimplemented!()
        }
        async fn mark_task_failed(
            &self,
            _id: Uuid,
            _worker_id: Uuid,
            _error_msg: &str,
            _next_run_at: chrono::DateTime<Utc>,
            _is_dead_letter: bool,
        ) -> AppResult<bool> {
            unimplemented!()
        }
        async fn stop_task(&self, _id: Uuid) -> AppResult<BackgroundTask> {
            unimplemented!()
        }
        async fn resume_task(&self, _id: Uuid) -> AppResult<BackgroundTask> {
            unimplemented!()
        }
        async fn update_task_status(
            &self,
            _id: Uuid,
            _status: TaskStatus,
        ) -> AppResult<BackgroundTask> {
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

    fn test_config() -> Arc<AppConfig> {
        Arc::new(AppConfig {
            jwt_secret: "secret".into(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".into(),
            cors_allowed_origins: vec![],
            smtp_host: "localhost".into(),
            smtp_port: 1025,
            smtp_username: "".into(),
            smtp_password: "".into(),
            smtp_from_address: "noreply@mailagents.com".into(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".into(),
            incoming_smtp_port: 2525,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
            secure_cookies: false,
            gcs: None,
            operator_emails: Vec::new(),
        })
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

        let company_of = |id: Uuid, user_id: Uuid, slug: &str| Company {
            id,
            user_id,
            name: slug.into(),
            slug: slug.into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            created_at: Utc::now(),
        };
        let channel_of = |id: Uuid, company_id: Uuid| Channel {
            id,
            company_id,
            name: "Reports".into(),
            description: None,
            slug: "reports".into(),
            alias_slugs: vec![],
            api_key: None,
            provider: None,
            model: None,
            participant_emails: None,
            agent_ids: None,
            channel_config: None,
            enabled: true,
            add_3rd_party: true,
            retrieve_company_memory: false,
            retrieve_agent_memory: false,
            retrieve_user_memory: false,
            persist_company_memory: false,
            persist_agent_memory: false,
            persist_user_memory: false,
            memory_recall_mode: crate::entities::memory::MemoryRecallMode::Fast,
            memory_max_results: 5,
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
        };

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
            }),
            Arc::new(MockChannelPersistence {
                channels: Mutex::new(vec![
                    channel_of(mine_channel, mine_company),
                    channel_of(theirs_channel, theirs_company),
                ]),
            }),
            Arc::new(MockThreadPersistence {
                threads: Mutex::new(vec![]),
                messages: Mutex::new(vec![]),
            }),
            Arc::new(MockTaskPersistence {
                tasks: Mutex::new(vec![]),
            }),
            test_config(),
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
        let user_id = Uuid::new_v4();
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();

        let company = Company {
            id: company_id,
            user_id,
            name: "Acme Corp".into(),
            slug: "acme".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            created_at: Utc::now(),
        };

        let channel = Channel {
            id: channel_id,
            company_id,
            name: "Daily Reports".into(),
            description: None,
            slug: "reports".into(),
            alias_slugs: vec![],
            api_key: None,
            provider: None,
            model: None,
            participant_emails: Some(vec![EmailAddress::from("admin@example.com")]),
            agent_ids: None,
            channel_config: None,
            enabled: true,
            add_3rd_party: true,
            retrieve_company_memory: false,
            retrieve_agent_memory: false,
            retrieve_user_memory: false,
            persist_company_memory: false,
            persist_agent_memory: false,
            persist_user_memory: false,
            memory_recall_mode: crate::entities::memory::MemoryRecallMode::Fast,
            memory_max_results: 5,
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
        };

        let schedule_persistence = Arc::new(MockSchedulePersistence {
            schedules: Mutex::new(vec![]),
        });
        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![company.clone()]),
        });
        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(vec![channel.clone()]),
        });
        let thread_persistence = Arc::new(MockThreadPersistence {
            threads: Mutex::new(vec![]),
            messages: Mutex::new(vec![]),
        });
        let task_persistence = Arc::new(MockTaskPersistence {
            tasks: Mutex::new(vec![]),
        });

        let use_cases = ScheduleUseCases::new(
            schedule_persistence.clone(),
            company_persistence.clone(),
            channel_persistence.clone(),
            thread_persistence.clone(),
            task_persistence.clone(),
            test_config(),
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
            enabled: true,
        };

        let created = use_cases
            .create_schedule(user_id, company_id, channel_id, write)
            .await
            .unwrap();

        assert_eq!(created.name, "Morning Digest");

        // 2. Fetch and list
        let fetched = use_cases
            .get_schedule(user_id, company_id, created.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.id, created.id);

        let list = use_cases
            .list_channel_schedules(user_id, company_id, channel_id)
            .await
            .unwrap();
        assert_eq!(list.len(), 1);

        // 3. Trigger schedule now
        let triggered = use_cases
            .trigger_schedule_now(user_id, company_id, created.id)
            .await
            .unwrap();
        assert_eq!(triggered.id, created.id);

        // Verify thread, message, and background task were created
        let threads = thread_persistence.threads.lock().unwrap();
        assert_eq!(threads.len(), 1);
        assert!(threads[0].subject.contains("[Daily] Digest"));

        let messages = thread_persistence.messages.lock().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].thread_id, threads[0].id);
        assert!(messages[0].clean_text_body.contains("Analyze tickets for"));

        let tasks = task_persistence.tasks.lock().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].task_type, "scheduled_agent_run");
        assert_eq!(tasks[0].thread_id, Some(threads[0].id));
    }
}
