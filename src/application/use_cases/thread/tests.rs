use super::*;
use crate::entities::channel::Channel;
use crate::entities::company_member::CompanyMembership;
use crate::services::email_parser::MAX_CHANNEL_HOPS;
use crate::use_cases::channel::{ChannelPersistence, ChannelWrite};
use chrono::Utc;
use std::sync::Mutex;

fn internal_test_config() -> Arc<AppConfig> {
    Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        access_token_ttl: time::Duration::days(1),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "smtp.invalid".to_string(),
        smtp_port: 2525,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
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

struct MockCompanyPersistence {
    companies: Mutex<Vec<Company>>,
    team_members: Mutex<Vec<(Uuid, String)>>,
}

impl MockCompanyPersistence {
    fn new(companies: Vec<Company>) -> Self {
        Self {
            companies: Mutex::new(companies),
            team_members: Mutex::new(Vec::new()),
        }
    }

    fn with_team_members(companies: Vec<Company>, members: Vec<(Uuid, String)>) -> Self {
        Self {
            companies: Mutex::new(companies),
            team_members: Mutex::new(members),
        }
    }
}

#[async_trait]
impl CompanyPersistence for MockCompanyPersistence {
    async fn create(
        &self,
        _user_id: Uuid,
        _name: &str,
        _slug: &str,
        _api_key: Option<&str>,
        _provider: Option<&str>,
        _model: Option<&str>,
        _enable_llm_spam_guardrail: Option<bool>,
    ) -> AppResult<Company> {
        unimplemented!()
    }
    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Company>> {
        Ok(self
            .companies
            .lock()
            .unwrap()
            .iter()
            .find(|company| company.id == id)
            .cloned())
    }
    async fn get_by_slug(&self, slug: &str) -> AppResult<Option<Company>> {
        Ok(self
            .companies
            .lock()
            .unwrap()
            .iter()
            .find(|c| c.slug == slug)
            .cloned())
    }
    async fn list_by_user_id(&self, _user_id: Uuid) -> AppResult<Vec<Company>> {
        unimplemented!()
    }
    async fn update(
        &self,
        _id: Uuid,
        _name: &str,
        _slug: &str,
        _api_key: Option<&str>,
        _provider: Option<&str>,
        _model: Option<&str>,
        _enable_llm_spam_guardrail: Option<bool>,
    ) -> AppResult<Company> {
        unimplemented!()
    }
    async fn delete(&self, _id: Uuid) -> AppResult<()> {
        unimplemented!()
    }
    /// A flat team: this mock knows who belongs to a company, not who owns it, so it never
    /// answers `Owner`. The owner exemption is the entity's rule and is tested there.
    async fn membership_for_email(
        &self,
        company_id: Uuid,
        email: &str,
    ) -> AppResult<CompanyMembership> {
        let members = self.team_members.lock().unwrap();
        let clean = email.trim().to_lowercase();
        let on_the_team = if members.is_empty() {
            !(clean.contains("spammer")
                || clean.contains("unauthorized")
                || clean.contains("evil")
                || clean.contains("notallowed"))
        } else {
            members
                .iter()
                .any(|(cid, e)| *cid == company_id && e.eq_ignore_ascii_case(&clean))
        };

        Ok(if on_the_team {
            CompanyMembership::Member
        } else {
            CompanyMembership::None
        })
    }
    async fn list_company_team_emails(&self, company_id: Uuid) -> AppResult<Vec<String>> {
        let members = self.team_members.lock().unwrap();
        Ok(members
            .iter()
            .filter(|(cid, _)| *cid == company_id)
            .map(|(_, e)| e.clone())
            .collect())
    }
}

struct MockChannelPersistence {
    channels: Mutex<Vec<Channel>>,
}

#[async_trait]
impl ChannelPersistence for MockChannelPersistence {
    async fn create(&self, _company_id: Uuid, _write: ChannelWrite) -> AppResult<Channel> {
        unimplemented!()
    }
    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Channel>> {
        Ok(self
            .channels
            .lock()
            .unwrap()
            .iter()
            .find(|channel| channel.id == id)
            .cloned())
    }
    async fn get_by_company_slug_and_channel_slug(
        &self,
        _company_slug: &CompanySlug,
        channel_slug: &ChannelSlug,
    ) -> AppResult<Option<Channel>> {
        Ok(self
            .channels
            .lock()
            .unwrap()
            .iter()
            .find(|w| w.matches_slug(channel_slug))
            .cloned())
    }
    async fn list_by_company_id(&self, company_id: Uuid) -> AppResult<Vec<Channel>> {
        Ok(self
            .channels
            .lock()
            .unwrap()
            .iter()
            .filter(|w| w.company_id == company_id)
            .cloned()
            .collect())
    }
    async fn update(&self, _id: Uuid, _write: ChannelWrite) -> AppResult<Channel> {
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

    async fn get_thread_by_id(&self, id: Uuid) -> AppResult<Option<Thread>> {
        Ok(self
            .threads
            .lock()
            .unwrap()
            .iter()
            .find(|t| t.id == id)
            .cloned())
    }

    async fn list_threads_by_channel_id(
        &self,
        channel_id: Uuid,
        before: Option<ThreadCursor>,
        limit: usize,
    ) -> AppResult<Vec<Thread>> {
        let mut threads: Vec<_> = self
            .threads
            .lock()
            .unwrap()
            .iter()
            .filter(|thread| thread.channel_id == channel_id)
            .filter(|thread| {
                before.is_none_or(|cursor| {
                    (thread.updated_at, thread.id) < (cursor.updated_at, cursor.id)
                })
            })
            .cloned()
            .collect();
        threads.sort_by_key(|thread| std::cmp::Reverse((thread.updated_at, thread.id)));
        threads.truncate(limit);
        Ok(threads)
    }

    async fn list_threads_updated_after(
        &self,
        channel_id: Uuid,
        after: Option<ThreadCursor>,
        limit: usize,
    ) -> AppResult<Vec<Thread>> {
        let mut threads: Vec<Thread> = self
            .threads
            .lock()
            .unwrap()
            .iter()
            .filter(|t| t.channel_id == channel_id)
            .filter(|t| after.is_none_or(|cursor| t.cursor() > cursor))
            .cloned()
            .collect();
        threads.sort_by_key(|t| t.cursor());
        threads.truncate(limit);
        Ok(threads)
    }

    async fn update_thread_participants(
        &self,
        id: Uuid,
        participant_emails: &[EmailAddress],
    ) -> AppResult<Thread> {
        let mut list = self.threads.lock().unwrap();
        let thread = list.iter_mut().find(|t| t.id == id).unwrap();
        thread.participant_emails = participant_emails.to_vec();
        Ok(thread.clone())
    }

    async fn find_thread_by_message_ids(
        &self,
        channel_id: Uuid,
        message_ids: &[MessageId],
    ) -> AppResult<Option<Thread>> {
        let thread_id = {
            let msgs = self.messages.lock().unwrap();
            msgs.iter()
                .find(|m| message_ids.contains(&m.message_id))
                .map(|m| m.thread_id)
        };
        if let Some(tid) = thread_id {
            return Ok(self
                .get_thread_by_id(tid)
                .await?
                .filter(|thread| thread.channel_id == channel_id));
        }
        Ok(None)
    }

    async fn find_thread_by_thread_index(
        &self,
        channel_id: Uuid,
        thread_index_prefix: &ThreadIndex,
    ) -> AppResult<Option<Thread>> {
        let thread_id = {
            let msgs = self.messages.lock().unwrap();
            msgs.iter()
                .find(|m| {
                    m.thread_index
                        .as_deref()
                        .unwrap_or_default()
                        .starts_with(thread_index_prefix.as_str())
                })
                .map(|m| m.thread_id)
        };
        if let Some(tid) = thread_id {
            return Ok(self
                .get_thread_by_id(tid)
                .await?
                .filter(|thread| thread.channel_id == channel_id));
        }
        Ok(None)
    }

    async fn count_recent_messages(
        &self,
        thread_id: Uuid,
        _duration_secs: i64,
    ) -> AppResult<usize> {
        let msgs = self.messages.lock().unwrap();
        Ok(msgs.iter().filter(|m| m.thread_id == thread_id).count())
    }

    async fn create_message(&self, message: &Message) -> AppResult<Message> {
        self.messages.lock().unwrap().push(message.clone());
        Ok(message.clone())
    }

    async fn get_message_by_message_id(
        &self,
        _company_id: Uuid,
        message_id: &MessageId,
    ) -> AppResult<Option<Message>> {
        Ok(self
            .messages
            .lock()
            .unwrap()
            .iter()
            .find(|m| &m.message_id == message_id)
            .cloned())
    }

    async fn find_outbound_reply(
        &self,
        thread_id: Uuid,
        in_reply_to: &MessageId,
    ) -> AppResult<Option<Message>> {
        Ok(self
            .messages
            .lock()
            .unwrap()
            .iter()
            .find(|message| {
                message.thread_id == thread_id
                    && message.direction == MessageDirection::Outbound
                    && message.in_reply_to.as_ref() == Some(in_reply_to)
            })
            .cloned())
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
        thread_id: Uuid,
        after: Option<MessageCursor>,
        limit: usize,
    ) -> AppResult<Vec<Message>> {
        let mut messages: Vec<Message> = self
            .messages
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.thread_id == thread_id)
            .filter(|m| after.is_none_or(|cursor| m.cursor() > cursor))
            .cloned()
            .collect();
        messages.sort_by_key(|m| m.cursor());
        messages.truncate(limit);
        Ok(messages)
    }
}

struct MockTaskPersistence {
    tasks: Mutex<Vec<crate::entities::task::BackgroundTask>>,
}

#[async_trait]
impl TaskPersistence for MockTaskPersistence {
    async fn find_correlated_outreach_reply(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
        thread_id: Uuid,
        sender: &str,
        references: &[MessageId],
    ) -> AppResult<Option<crate::entities::outreach::OutreachReplyMatch>> {
        let tasks = self.tasks.lock().unwrap();
        Ok(tasks.iter().find_map(|task| {
            let outreach = task.payload.get("test_outreach")?;
            let target = outreach.get("target_email")?.as_str()?;
            let outbound_message_id = outreach.get("outbound_message_id")?.as_str()?;
            (task.company_id == company_id
                && task.channel_id == channel_id
                && task.thread_id == Some(thread_id)
                && target.eq_ignore_ascii_case(sender)
                && references.iter().any(|value| value == outbound_message_id))
            .then(|| crate::entities::outreach::OutreachReplyMatch {
                outreach_id: outreach
                    .get("outreach_id")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|value| Uuid::parse_str(value).ok())
                    .unwrap(),
                task_id: task.id,
                target_email: target.into(),
            })
        }))
    }

    async fn record_outreach_reply(
        &self,
        matched: &crate::entities::outreach::OutreachReplyMatch,
        _response_message_id: Uuid,
    ) -> AppResult<crate::entities::outreach::OutreachProgress> {
        let mut tasks = self.tasks.lock().unwrap();
        let task = tasks
            .iter_mut()
            .find(|task| task.id == matched.task_id)
            .unwrap();
        task.status = crate::entities::task::TaskStatus::Pending;
        Ok(crate::entities::outreach::OutreachProgress {
            id: matched.outreach_id,
            task_id: matched.task_id,
            status: crate::entities::outreach::OutreachStatus::ThresholdMet,
            required_threshold_percent: 100.0,
            target_count: 1,
            response_count: 1,
            required_response_count: 1,
            expires_at: Utc::now() + chrono::Duration::hours(1),
            suspended: false,
        })
    }

    async fn enqueue_task(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
        thread_id: Option<Uuid>,
        task_type: &str,
        payload: serde_json::Value,
    ) -> AppResult<crate::entities::task::BackgroundTask> {
        let task = crate::entities::task::BackgroundTask {
            id: Uuid::new_v4(),
            company_id,
            channel_id,
            thread_id,
            task_type: task_type.to_string(),
            status: crate::entities::task::TaskStatus::Pending,
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

    async fn get_task_by_id(
        &self,
        id: Uuid,
    ) -> AppResult<Option<crate::entities::task::BackgroundTask>> {
        Ok(self
            .tasks
            .lock()
            .unwrap()
            .iter()
            .find(|t| t.id == id)
            .cloned())
    }

    async fn update_task_payload(&self, id: Uuid, payload: serde_json::Value) -> AppResult<()> {
        let mut list = self.tasks.lock().unwrap();
        if let Some(t) = list.iter_mut().find(|t| t.id == id) {
            t.payload = payload;
        }
        Ok(())
    }

    async fn claim_pending_tasks(
        &self,
        worker_id: Uuid,
        lock_expires_at: chrono::DateTime<chrono::Utc>,
        limit: i64,
    ) -> AppResult<Vec<crate::entities::task::BackgroundTask>> {
        let now = Utc::now();
        let mut tasks = self.tasks.lock().unwrap();
        let mut claimed = Vec::new();
        for task in tasks
            .iter_mut()
            .filter(|task| {
                (task.status == crate::entities::task::TaskStatus::Pending && task.run_at <= now)
                    || (task.status == crate::entities::task::TaskStatus::Processing
                        && task.lock_expires_at.is_none_or(|expires| expires <= now))
            })
            .take(limit as usize)
        {
            task.status = crate::entities::task::TaskStatus::Processing;
            task.worker_id = Some(worker_id);
            task.locked_at = Some(now);
            task.lock_expires_at = Some(lock_expires_at);
            claimed.push(task.clone());
        }
        Ok(claimed)
    }

    async fn claim_task(
        &self,
        id: Uuid,
        worker_id: Uuid,
        lock_expires_at: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<bool> {
        let mut list = self.tasks.lock().unwrap();
        let now = Utc::now();
        if let Some(t) = list.iter_mut().find(|t| {
            t.id == id && t.status == crate::entities::task::TaskStatus::Pending && t.run_at <= now
        }) {
            t.status = crate::entities::task::TaskStatus::Processing;
            t.worker_id = Some(worker_id);
            t.locked_at = Some(now);
            t.lock_expires_at = Some(lock_expires_at);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn mark_task_completed(&self, id: Uuid, worker_id: Uuid) -> AppResult<bool> {
        let mut list = self.tasks.lock().unwrap();
        let now = Utc::now();
        if let Some(t) = list.iter_mut().find(|t| {
            t.id == id
                && t.status == crate::entities::task::TaskStatus::Processing
                && t.worker_id == Some(worker_id)
                && t.lock_expires_at.is_some_and(|expires| expires > now)
        }) {
            t.status = crate::entities::task::TaskStatus::Completed;
            t.worker_id = None;
            t.locked_at = None;
            t.lock_expires_at = None;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn mark_task_failed(
        &self,
        id: Uuid,
        worker_id: Uuid,
        error_msg: &str,
        next_run_at: chrono::DateTime<chrono::Utc>,
        is_dead_letter: bool,
    ) -> AppResult<bool> {
        let mut list = self.tasks.lock().unwrap();
        let now = Utc::now();
        if let Some(t) = list.iter_mut().find(|t| {
            t.id == id
                && t.status == crate::entities::task::TaskStatus::Processing
                && t.worker_id == Some(worker_id)
                && t.lock_expires_at.is_some_and(|expires| expires > now)
        }) {
            t.last_error = Some(error_msg.to_string());
            t.retry_count += 1;
            t.run_at = next_run_at;
            t.status = if is_dead_letter {
                crate::entities::task::TaskStatus::DeadLetter
            } else {
                crate::entities::task::TaskStatus::Pending
            };
            t.worker_id = None;
            t.locked_at = None;
            t.lock_expires_at = None;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn stop_task(&self, id: Uuid) -> AppResult<crate::entities::task::BackgroundTask> {
        let mut list = self.tasks.lock().unwrap();
        let t = list
            .iter_mut()
            .find(|t| {
                t.id == id
                    && matches!(
                        t.status,
                        crate::entities::task::TaskStatus::Pending
                            | crate::entities::task::TaskStatus::Processing
                            | crate::entities::task::TaskStatus::PendingApproval
                            | crate::entities::task::TaskStatus::WaitingForThirdPartyReply
                            | crate::entities::task::TaskStatus::Failed
                    )
            })
            .unwrap();
        t.status = crate::entities::task::TaskStatus::Stopped;
        t.worker_id = None;
        t.locked_at = None;
        t.lock_expires_at = None;
        Ok(t.clone())
    }

    async fn resume_task(&self, id: Uuid) -> AppResult<crate::entities::task::BackgroundTask> {
        let mut list = self.tasks.lock().unwrap();
        let t = list
            .iter_mut()
            .find(|t| {
                t.id == id
                    && matches!(
                        t.status,
                        crate::entities::task::TaskStatus::Stopped
                            | crate::entities::task::TaskStatus::PendingApproval
                            | crate::entities::task::TaskStatus::WaitingForThirdPartyReply
                            | crate::entities::task::TaskStatus::Failed
                    )
            })
            .unwrap();
        t.status = crate::entities::task::TaskStatus::Pending;
        t.run_at = Utc::now();
        t.worker_id = None;
        t.locked_at = None;
        t.lock_expires_at = None;
        Ok(t.clone())
    }

    async fn update_task_status(
        &self,
        id: Uuid,
        status: crate::entities::task::TaskStatus,
    ) -> AppResult<crate::entities::task::BackgroundTask> {
        let mut list = self.tasks.lock().unwrap();
        let t = list
            .iter_mut()
            .find(|t| {
                t.id == id
                    && match status {
                        crate::entities::task::TaskStatus::PendingApproval => matches!(
                            t.status,
                            crate::entities::task::TaskStatus::Processing
                                | crate::entities::task::TaskStatus::WaitingForThirdPartyReply
                        ),
                        crate::entities::task::TaskStatus::WaitingForThirdPartyReply => {
                            matches!(
                                t.status,
                                crate::entities::task::TaskStatus::Processing
                                    | crate::entities::task::TaskStatus::PendingApproval
                            )
                        }
                        _ => false,
                    }
            })
            .unwrap();
        t.status = status;
        t.worker_id = None;
        t.locked_at = None;
        t.lock_expires_at = None;
        Ok(t.clone())
    }

    async fn list_company_tasks(
        &self,
        company_id: Uuid,
        channel_id: Option<Uuid>,
        status: Option<crate::entities::task::TaskStatus>,
        _sort_asc: bool,
    ) -> AppResult<Vec<crate::entities::task::BackgroundTask>> {
        Ok(self
            .tasks
            .lock()
            .unwrap()
            .iter()
            .filter(|t| t.company_id == company_id)
            .filter(|t| channel_id.map_or(true, |w_id| t.channel_id == w_id))
            .filter(|t| status.as_ref().map_or(true, |s| t.status == *s))
            .cloned()
            .collect())
    }
}

/// A task row written before the delivery choice was persisted has no `deliver` key, and the
/// worker that picks it up must answer it for real. Defaulting the other way would silently
/// swallow every reply still sitting in the queue across a deploy.
#[test]
fn a_payload_without_a_delivery_choice_still_delivers() {
    let legacy: InboundIngestResult = serde_json::from_value(serde_json::json!({
        "accepted": true,
        "reason": null,
        "thread": null,
        "inbound_message": null,
        "company": null,
        "channel": null,
        "parsed_email": null,
        "normalized_message": null,
        "task_id": null,
    }))
    .expect("a payload predating the field must still deserialize");
    assert!(legacy.deliver);

    let in_app: InboundIngestResult = serde_json::from_value(serde_json::json!({
        "accepted": true,
        "reason": null,
        "thread": null,
        "inbound_message": null,
        "company": null,
        "channel": null,
        "parsed_email": null,
        "normalized_message": null,
        "task_id": null,
        "deliver": false,
    }))
    .expect("an in-app-only payload must deserialize");
    assert!(!in_app.deliver);
}

#[tokio::test]
async fn test_inter_channel_hop_limit_rejection() {
    let company_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();
    let source_channel_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        api_key: None,
        provider: None,
        model: None,
        enable_llm_spam_guardrail: None,
        created_at: Utc::now(),
    }]));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: channel_id,
                company_id,
                name: "Inbound Flow".to_string(),
                slug: "inbound".into(),
                alias_slugs: Vec::new(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None,
                agent_ids: Some(vec![Uuid::new_v4()]),
                channel_config: None,
                created_at: Utc::now(),
            },
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: source_channel_id,
                company_id,
                name: "Source Flow".to_string(),
                slug: "source".into(),
                alias_slugs: Vec::new(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None,
                agent_ids: Some(vec![Uuid::new_v4()]),
                channel_config: None,
                created_at: Utc::now(),
            },
        ]),
    });

    let thread_persistence = Arc::new(MockThreadPersistence {
        threads: Mutex::new(Vec::new()),
        messages: Mutex::new(Vec::new()),
    });

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        access_token_ttl: time::Duration::days(1),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
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
    });

    let task_persistence = Arc::new(MockTaskPersistence {
        tasks: Mutex::new(Vec::new()),
    });

    let thread_use_cases = ThreadUseCases::new(
        thread_persistence,
        channel_persistence,
        company_persistence,
        task_persistence,
        config,
    );

    let prepared = thread_use_cases
        .prepare_internal_channel_delivery(
            OutboundEmail {
                channel_id: source_channel_id,
                channel_name: "Source Flow".to_string(),
                channel_slug: "source".into(),
                company_slug: "acme".into(),
                trigger_message_id: "<source@example.com>".into(),
                thread_references: Vec::new(),
                recipient_to: "inbound@acme.mailagents.com".into(),
                recipients_cc: Vec::new(),
                subject: "Test Inter Channel".to_string(),
                body_text: "Hello".to_string(),
                hop_count: MAX_CHANNEL_HOPS - 1,
                trace_channels: Vec::new(),
            },
            Some("hop-limit-test"),
        )
        .await
        .unwrap()
        .unwrap();
    let result = thread_use_cases
        .ingest_prepared_internal_message(&prepared)
        .await
        .unwrap();
    assert!(!result.accepted);
    assert_eq!(
        result.reason.as_deref(),
        Some("Max inter-channel hop count reached")
    );
}

#[tokio::test]
async fn test_spf_authentication_failure_rejection() {
    let company_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        api_key: None,
        provider: None,
        model: None,
        enable_llm_spam_guardrail: None,
        created_at: Utc::now(),
    }]));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Inbound Flow".to_string(),
            slug: "inbound".into(),
            alias_slugs: Vec::new(),
            api_key: None,
            provider: None,
            model: None,
            participant_emails: Some(vec!["@public".into()]),
            agent_ids: None,
            channel_config: None,
            created_at: Utc::now(),
        }]),
    });

    let thread_persistence = Arc::new(MockThreadPersistence {
        threads: Mutex::new(Vec::new()),
        messages: Mutex::new(Vec::new()),
    });

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        access_token_ttl: time::Duration::days(1),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
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
    });

    let task_persistence = Arc::new(MockTaskPersistence {
        tasks: Mutex::new(Vec::new()),
    });

    let thread_use_cases = ThreadUseCases::new(
        thread_persistence,
        channel_persistence,
        company_persistence,
        task_persistence,
        config,
    );

    let raw_payload = RawInboundPayload {
        to: "restricted@acme.mailagents.com".to_string(),
        from: "forged@acme.mailagents.com".to_string(),
        subject: Some("Spoofed email".to_string()),
        text: Some("Hello".to_string()),
        headers: Some(format!("X-MailAgents-Channel-ID: {channel_id}\n")),
        spf: Some("fail".to_string()),
        ..Default::default()
    };

    let result = thread_use_cases
        .ingest_and_save_inbound_message(raw_payload)
        .await
        .unwrap();
    assert!(!result.accepted);
    assert_eq!(result.reason.as_deref(), Some("SPF authentication failed"));
}

#[tokio::test]
async fn test_high_spam_score_rejection() {
    let company_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        api_key: None,
        provider: None,
        model: None,
        enable_llm_spam_guardrail: None,
        created_at: Utc::now(),
    }]));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Inbound Flow".to_string(),
            slug: "inbound".into(),
            alias_slugs: Vec::new(),
            api_key: None,
            provider: None,
            model: None,
            participant_emails: Some(vec!["@public".into()]),
            agent_ids: None,
            channel_config: None,
            created_at: Utc::now(),
        }]),
    });

    let thread_persistence = Arc::new(MockThreadPersistence {
        threads: Mutex::new(Vec::new()),
        messages: Mutex::new(Vec::new()),
    });

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        access_token_ttl: time::Duration::days(1),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
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
    });

    let task_persistence = Arc::new(MockTaskPersistence {
        tasks: Mutex::new(Vec::new()),
    });

    let thread_use_cases = ThreadUseCases::new(
        thread_persistence,
        channel_persistence,
        company_persistence,
        task_persistence,
        config,
    );

    let raw_payload = RawInboundPayload {
        to: "inbound@acme.mailagents.com".to_string(),
        from: "spammer@external.com".to_string(),
        subject: Some("Buy Cheap Rolex".to_string()),
        text: Some("Spam message body".to_string()),
        spam_score: Some(8.5),
        ..Default::default()
    };

    let result = thread_use_cases
        .ingest_and_save_inbound_message(raw_payload)
        .await
        .unwrap();
    assert!(!result.accepted);
    assert_eq!(
        result.reason.as_deref(),
        Some("Spam score threshold exceeded")
    );
}

#[tokio::test]
async fn test_dmarc_authentication_failure_rejection() {
    let company_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        api_key: None,
        provider: None,
        model: None,
        enable_llm_spam_guardrail: None,
        created_at: Utc::now(),
    }]));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Inbound Flow".to_string(),
            slug: "inbound".into(),
            alias_slugs: Vec::new(),
            api_key: None,
            provider: None,
            model: None,
            participant_emails: None,
            agent_ids: None,
            channel_config: None,
            created_at: Utc::now(),
        }]),
    });

    let thread_persistence = Arc::new(MockThreadPersistence {
        threads: Mutex::new(Vec::new()),
        messages: Mutex::new(Vec::new()),
    });

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        access_token_ttl: time::Duration::days(1),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
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
    });

    let task_persistence = Arc::new(MockTaskPersistence {
        tasks: Mutex::new(Vec::new()),
    });

    let thread_use_cases = ThreadUseCases::new(
        thread_persistence,
        channel_persistence,
        company_persistence,
        task_persistence,
        config,
    );

    let raw_payload = RawInboundPayload {
        to: "inbound@acme.mailagents.com".to_string(),
        from: "spoofed@external.com".to_string(),
        subject: Some("Spoofed email".to_string()),
        text: Some("Hello".to_string()),
        dmarc: Some("fail".to_string()),
        ..Default::default()
    };

    let result = thread_use_cases
        .ingest_and_save_inbound_message(raw_payload)
        .await
        .unwrap();
    assert!(!result.accepted);
    assert_eq!(
        result.reason.as_deref(),
        Some("DMARC authentication failed")
    );
}

#[tokio::test]
async fn test_unauthorized_sender_blocked_before_spam_checks() {
    let company_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        api_key: None,
        provider: None,
        model: None,
        enable_llm_spam_guardrail: None,
        created_at: Utc::now(),
    }]));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Restricted Flow".to_string(),
            slug: "restricted".into(),
            alias_slugs: Vec::new(),
            api_key: None,
            provider: None,
            model: None,
            participant_emails: Some(vec!["alice@example.com".into()]),
            agent_ids: None,
            channel_config: None,
            created_at: Utc::now(),
        }]),
    });

    let thread_persistence = Arc::new(MockThreadPersistence {
        threads: Mutex::new(Vec::new()),
        messages: Mutex::new(Vec::new()),
    });

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        access_token_ttl: time::Duration::days(1),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
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
    });

    let task_persistence = Arc::new(MockTaskPersistence {
        tasks: Mutex::new(Vec::new()),
    });

    let thread_use_cases = ThreadUseCases::new(
        thread_persistence,
        channel_persistence,
        company_persistence,
        task_persistence,
        config,
    );

    let raw_payload = RawInboundPayload {
        to: "restricted@acme.mailagents.com".to_string(),
        from: "unauthorized@evil.com".to_string(),
        subject: Some("Hello".to_string()),
        text: Some("Test message".to_string()),
        spam_score: Some(10.0), // high spam score
        ..Default::default()
    };

    let result = thread_use_cases
        .ingest_and_save_inbound_message(raw_payload)
        .await
        .unwrap();

    assert!(!result.accepted);
    assert_eq!(
        result.reason.as_deref(),
        Some("Sender unauthorized for channel")
    );
}

#[tokio::test]
async fn test_participant_sender_bypasses_spam_checks() {
    let company_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        api_key: None,
        provider: None,
        model: None,
        enable_llm_spam_guardrail: None,
        created_at: Utc::now(),
    }]));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Restricted Flow".to_string(),
            slug: "restricted".into(),
            alias_slugs: Vec::new(),
            api_key: None,
            provider: None,
            model: None,
            participant_emails: Some(vec!["alice@example.com".into()]),
            agent_ids: None,
            channel_config: None,
            created_at: Utc::now(),
        }]),
    });

    let thread_persistence = Arc::new(MockThreadPersistence {
        threads: Mutex::new(Vec::new()),
        messages: Mutex::new(Vec::new()),
    });

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        access_token_ttl: time::Duration::days(1),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
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
    });

    let task_persistence = Arc::new(MockTaskPersistence {
        tasks: Mutex::new(Vec::new()),
    });

    let thread_use_cases = ThreadUseCases::new(
        thread_persistence,
        channel_persistence,
        company_persistence,
        task_persistence,
        config,
    );

    // High spam score payload from a registered participant
    let raw_payload = RawInboundPayload {
        to: "restricted@acme.mailagents.com".to_string(),
        from: "alice@example.com".to_string(),
        subject: Some("Urgent update".to_string()),
        text: Some("Meeting notes".to_string()),
        spam_score: Some(99.0), // extreme spam score that would normally be rejected
        ..Default::default()
    };

    let result = thread_use_cases
        .ingest_and_save_inbound_message(raw_payload)
        .await
        .unwrap();

    assert!(result.accepted);
    assert!(result.thread.is_some());
}

#[tokio::test]
async fn test_channel_in_cc_resolves_properly() {
    let company_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        api_key: None,
        provider: None,
        model: None,
        enable_llm_spam_guardrail: None,
        created_at: Utc::now(),
    }]));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Support Flow".to_string(),
            slug: "support".into(),
            alias_slugs: Vec::new(),
            api_key: None,
            provider: None,
            model: None,
            participant_emails: None,
            agent_ids: None,
            channel_config: None,
            created_at: Utc::now(),
        }]),
    });

    let thread_persistence = Arc::new(MockThreadPersistence {
        threads: Mutex::new(Vec::new()),
        messages: Mutex::new(Vec::new()),
    });

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        access_token_ttl: time::Duration::days(1),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
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
    });

    let task_persistence = Arc::new(MockTaskPersistence {
        tasks: Mutex::new(Vec::new()),
    });

    let thread_use_cases = ThreadUseCases::new(
        thread_persistence,
        channel_persistence,
        company_persistence,
        task_persistence,
        config,
    );

    let raw_payload = RawInboundPayload {
        to: "user@example.com".to_string(),
        cc: Some("support@acme.mailagents.com".to_string()),
        from: "customer@client.com".to_string(),
        subject: Some("Need help".to_string()),
        text: Some("Can someone assist?".to_string()),
        ..Default::default()
    };

    let result = thread_use_cases
        .ingest_and_save_inbound_message(raw_payload)
        .await
        .unwrap();

    assert!(result.accepted);
    assert_eq!(result.channel_matches.len(), 1);
    assert_eq!(result.channel_matches[0].recipient_role, RecipientRole::Cc);
}

#[tokio::test]
async fn test_multi_channel_to_and_cc_execution() {
    let company_id = Uuid::new_v4();
    let wf1_id = Uuid::new_v4();
    let wf2_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        api_key: None,
        provider: None,
        model: None,
        enable_llm_spam_guardrail: None,
        created_at: Utc::now(),
    }]));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: wf1_id,
                company_id,
                name: "Support".to_string(),
                slug: "support".into(),
                alias_slugs: Vec::new(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None,
                agent_ids: None,
                channel_config: None,
                created_at: Utc::now(),
            },
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: wf2_id,
                company_id,
                name: "Billing".to_string(),
                slug: "billing".into(),
                alias_slugs: Vec::new(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None,
                agent_ids: None,
                channel_config: None,
                created_at: Utc::now(),
            },
        ]),
    });

    let thread_persistence = Arc::new(MockThreadPersistence {
        threads: Mutex::new(Vec::new()),
        messages: Mutex::new(Vec::new()),
    });

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        access_token_ttl: time::Duration::days(1),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
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
    });

    let task_persistence = Arc::new(MockTaskPersistence {
        tasks: Mutex::new(Vec::new()),
    });

    let thread_use_cases = ThreadUseCases::new(
        thread_persistence.clone(),
        channel_persistence,
        company_persistence,
        task_persistence,
        config,
    );

    let raw_payload = RawInboundPayload {
        to: "support@acme.mailagents.com".to_string(),
        cc: Some("billing@acme.mailagents.com".to_string()),
        from: "customer@client.com".to_string(),
        subject: Some("Invoice and account question".to_string()),
        text: Some("Please help with my account and invoice.".to_string()),
        ..Default::default()
    };

    let ingest = thread_use_cases
        .ingest_and_save_inbound_message(raw_payload)
        .await
        .unwrap();

    assert!(ingest.accepted);
    assert_eq!(ingest.channel_matches.len(), 2);
    assert_eq!(ingest.channel_matches[0].channel.slug, "support");
    assert_eq!(ingest.channel_matches[0].recipient_role, RecipientRole::To);
    assert_eq!(ingest.channel_matches[1].channel.slug, "billing");
    assert_eq!(ingest.channel_matches[1].recipient_role, RecipientRole::Cc);

    // Verify thread creation for both channels
    let threads = thread_persistence.threads.lock().unwrap();
    assert_eq!(threads.len(), 2);
}

#[tokio::test]
async fn test_pipeline_address_chaining_execution() {
    let company_id = Uuid::new_v4();
    let wf1_id = Uuid::new_v4();
    let wf2_id = Uuid::new_v4();
    let wf3_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        api_key: None,
        provider: None,
        model: None,
        enable_llm_spam_guardrail: None,
        created_at: Utc::now(),
    }]));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: wf1_id,
                company_id,
                name: "Support".to_string(),
                slug: "support".into(),
                alias_slugs: Vec::new(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None,
                agent_ids: None,
                channel_config: None,
                created_at: Utc::now(),
            },
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: wf2_id,
                company_id,
                name: "Billing".to_string(),
                slug: "billing".into(),
                alias_slugs: Vec::new(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None,
                agent_ids: None,
                channel_config: None,
                created_at: Utc::now(),
            },
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: wf3_id,
                company_id,
                name: "Legal".to_string(),
                slug: "legal".into(),
                alias_slugs: Vec::new(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None,
                agent_ids: None,
                channel_config: None,
                created_at: Utc::now(),
            },
        ]),
    });

    let thread_persistence = Arc::new(MockThreadPersistence {
        threads: Mutex::new(Vec::new()),
        messages: Mutex::new(Vec::new()),
    });

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        access_token_ttl: time::Duration::days(1),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
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
    });

    let task_persistence = Arc::new(MockTaskPersistence {
        tasks: Mutex::new(Vec::new()),
    });

    let thread_use_cases = ThreadUseCases::new(
        thread_persistence.clone(),
        channel_persistence,
        company_persistence,
        task_persistence,
        config,
    );

    let raw_payload = RawInboundPayload {
        to: "support+billing+legal@acme.mailagents.com".to_string(),
        from: "customer@client.com".to_string(),
        subject: Some("Pipeline Request".to_string()),
        text: Some("Please process via support, billing, and legal.".to_string()),
        ..Default::default()
    };

    let ingest = thread_use_cases
        .ingest_and_save_inbound_message(raw_payload)
        .await
        .unwrap();

    assert!(ingest.accepted);
    assert_eq!(ingest.channel_matches.len(), 3);
    assert_eq!(ingest.channel_matches[0].channel.slug, "support");
    assert_eq!(ingest.channel_matches[0].step_index, 0);
    assert_eq!(ingest.channel_matches[0].total_steps, 3);
    assert_eq!(ingest.channel_matches[1].channel.slug, "billing");
    assert_eq!(ingest.channel_matches[1].step_index, 1);
    assert_eq!(ingest.channel_matches[2].channel.slug, "legal");
    assert_eq!(ingest.channel_matches[2].step_index, 2);

    // Verify threads were created for all 3 step channels
    let threads = thread_persistence.threads.lock().unwrap();
    assert_eq!(threads.len(), 3);
}

#[tokio::test]
async fn test_misspelled_channel_bounce_and_strict_pipeline_validation() {
    let company_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        api_key: None,
        provider: None,
        model: None,
        enable_llm_spam_guardrail: None,
        created_at: Utc::now(),
    }]));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: Uuid::new_v4(),
                company_id,
                name: "Support".to_string(),
                slug: "support".into(),
                alias_slugs: Vec::new(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None,
                agent_ids: None,
                channel_config: None,
                created_at: Utc::now(),
            },
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: Uuid::new_v4(),
                company_id,
                name: "Billing".to_string(),
                slug: "billing".into(),
                alias_slugs: Vec::new(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None,
                agent_ids: None,
                channel_config: None,
                created_at: Utc::now(),
            },
        ]),
    });

    let thread_persistence = Arc::new(MockThreadPersistence {
        threads: Mutex::new(Vec::new()),
        messages: Mutex::new(Vec::new()),
    });

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        access_token_ttl: time::Duration::days(1),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "".to_string(), // skip external SMTP in test
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
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
    });

    let task_persistence = Arc::new(MockTaskPersistence {
        tasks: Mutex::new(Vec::new()),
    });

    let thread_use_cases = ThreadUseCases::new(
        thread_persistence.clone(),
        channel_persistence,
        company_persistence,
        task_persistence,
        config,
    );

    // 1. Single misspelled address
    let raw_payload_single = RawInboundPayload {
        to: "suppport@acme.mailagents.com".to_string(),
        from: "customer@client.com".to_string(),
        subject: Some("Help".to_string()),
        text: Some("Help please".to_string()),
        ..Default::default()
    };

    let ingest_single = thread_use_cases
        .ingest_and_save_inbound_message(raw_payload_single)
        .await
        .unwrap();

    assert!(!ingest_single.accepted);
    assert_eq!(
        ingest_single.reason.as_deref(),
        Some("Channel address not found or misspelled")
    );
    let bounce_single = ingest_single.bounce_info.unwrap();
    assert_eq!(bounce_single.invalid_slugs, vec!["suppport"]);
    assert_eq!(bounce_single.suggestions[0].suggestions, vec!["support"]);

    // Verify bounce email body formatting
    let bounce_body = format_bounce_email_body(&bounce_single, "mailagents.com");
    assert!(bounce_body.contains("suppport@acme.mailagents.com"));
    assert!(bounce_body.contains("support@acme.mailagents.com"));

    // 2. Strict pipeline validation with misspelled step 'biling'
    let raw_payload_pipeline = RawInboundPayload {
        to: "support+biling@acme.mailagents.com".to_string(),
        from: "customer@client.com".to_string(),
        subject: Some("Pipeline Help".to_string()),
        text: Some("Help please".to_string()),
        ..Default::default()
    };

    let ingest_pipeline = thread_use_cases
        .ingest_and_save_inbound_message(raw_payload_pipeline)
        .await
        .unwrap();

    assert!(!ingest_pipeline.accepted);
    let bounce_pipeline = ingest_pipeline.bounce_info.unwrap();
    assert_eq!(bounce_pipeline.invalid_slugs, vec!["biling"]);
    assert_eq!(bounce_pipeline.suggestions[0].suggestions, vec!["billing"]);
}

#[tokio::test]
async fn test_quote_stripping_rules_for_first_in_thread_and_forwarded_emails() {
    let company_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        api_key: None,
        provider: None,
        model: None,
        enable_llm_spam_guardrail: None,
        created_at: Utc::now(),
    }]));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Support".to_string(),
            slug: "support".into(),
            alias_slugs: Vec::new(),
            api_key: None,
            provider: None,
            model: None,
            participant_emails: None,
            agent_ids: None,
            channel_config: None,
            created_at: Utc::now(),
        }]),
    });

    let thread_persistence = Arc::new(MockThreadPersistence {
        threads: Mutex::new(Vec::new()),
        messages: Mutex::new(Vec::new()),
    });

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        access_token_ttl: time::Duration::days(1),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
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
    });

    let task_persistence = Arc::new(MockTaskPersistence {
        tasks: Mutex::new(Vec::new()),
    });

    let thread_use_cases = ThreadUseCases::new(
        thread_persistence.clone(),
        channel_persistence,
        company_persistence,
        task_persistence,
        config,
    );

    // 1. Initial email in a NEW thread containing blockquotes/header markers.
    // Quotes MUST NOT be stripped because it is the first email in a new thread.
    let msg1_text = "Please check this user issue:\n\nOn Mon, Aug 10, 2026 at 10:00 AM User wrote:\n> Blockquoted text here";
    let raw_first_email = RawInboundPayload {
        to: "support@acme.mailagents.com".to_string(),
        from: "customer@client.com".to_string(),
        subject: Some("New Ticket with Quotes".to_string()),
        text: Some(msg1_text.to_string()),
        headers: Some("Message-ID: <msg1@client.com>\n".to_string()),
        ..Default::default()
    };

    let res1 = thread_use_cases
        .ingest_and_save_inbound_message(raw_first_email)
        .await
        .unwrap();

    assert!(res1.accepted);
    let msg1 = res1.inbound_message.unwrap();
    assert_eq!(msg1.clean_text_body, msg1_text); // Preserved full text!

    // 2. Reply in the EXISTING thread containing repeated quotes from msg1.
    // Quotes MUST be stripped because this is a subsequent message in an existing thread.
    let msg2_text = "Thanks for looking into this.\n\nPlease check this user issue:\n\nOn Mon, Aug 10, 2026 at 10:00 AM User wrote:\n> Blockquoted text here";
    let raw_reply_email = RawInboundPayload {
        to: "support@acme.mailagents.com".to_string(),
        from: "customer@client.com".to_string(),
        subject: Some("Re: New Ticket with Quotes".to_string()),
        text: Some(msg2_text.to_string()),
        headers: Some(
            "Message-ID: <msg2@client.com>\nIn-Reply-To: <msg1@client.com>\n".to_string(),
        ),
        ..Default::default()
    };

    let res2 = thread_use_cases
        .ingest_and_save_inbound_message(raw_reply_email)
        .await
        .unwrap();

    assert!(res2.accepted);
    let msg2 = res2.inbound_message.unwrap();
    assert_eq!(msg2.clean_text_body, "Thanks for looking into this."); // Stripped quotes!

    // 3. Forwarded email into existing or new thread.
    // Quotes MUST NOT be stripped because it is a forwarded email.
    let fwd_text = "FYI forwarding this:\n\n---------- Forwarded message ---------\nFrom: Alice <alice@other.com>\nDate: Mon, Aug 10, 2026\nSubject: Forwarded Issue\n\nOriginal forwarded body";
    let raw_fwd_email = RawInboundPayload {
        to: "support@acme.mailagents.com".to_string(),
        from: "manager@client.com".to_string(),
        subject: Some("Fwd: External Report".to_string()),
        text: Some(fwd_text.to_string()),
        headers: Some("Message-ID: <msg3@client.com>\n".to_string()),
        ..Default::default()
    };

    let res3 = thread_use_cases
        .ingest_and_save_inbound_message(raw_fwd_email)
        .await
        .unwrap();

    assert!(res3.accepted);
    let msg3 = res3.inbound_message.unwrap();
    assert_eq!(msg3.clean_text_body, fwd_text); // Preserved full forwarded text!
}

#[tokio::test]
async fn test_participant_modes_company_team_public_and_explicit() {
    let company_id = Uuid::new_v4();
    let company_persistence = Arc::new(MockCompanyPersistence::with_team_members(
        vec![Company {
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            created_at: Utc::now(),
        }],
        vec![(company_id, "team_member@acme.com".to_string())],
    ));

    let flow_team_only = Uuid::new_v4();
    let flow_public = Uuid::new_v4();
    let flow_explicit = Uuid::new_v4();

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: flow_team_only,
                company_id,
                name: "Team Only".to_string(),
                slug: "team-only".into(),
                alias_slugs: Vec::new(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None, // Default = Company Team
                agent_ids: None,
                channel_config: None,
                created_at: Utc::now(),
            },
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: flow_public,
                company_id,
                name: "Public Flow".to_string(),
                slug: "public-flow".into(),
                alias_slugs: Vec::new(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: Some(vec!["@public".into()]), // Public
                agent_ids: None,
                channel_config: None,
                created_at: Utc::now(),
            },
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: flow_explicit,
                company_id,
                name: "Explicit Flow".to_string(),
                slug: "explicit-flow".into(),
                alias_slugs: Vec::new(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: Some(vec!["allowed@external.com".into()]), // Explicit list
                agent_ids: None,
                channel_config: None,
                created_at: Utc::now(),
            },
        ]),
    });

    let thread_persistence = Arc::new(MockThreadPersistence {
        threads: Mutex::new(Vec::new()),
        messages: Mutex::new(Vec::new()),
    });

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        access_token_ttl: time::Duration::days(1),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
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
    });

    let task_persistence = Arc::new(MockTaskPersistence {
        tasks: Mutex::new(Vec::new()),
    });

    let thread_use_cases = ThreadUseCases::new(
        thread_persistence,
        channel_persistence,
        company_persistence,
        task_persistence,
        config,
    );

    // 1. Team-only flow: Team member accepted
    let res1 = thread_use_cases
        .ingest_and_save_inbound_message(RawInboundPayload {
            to: "team-only@acme.mailagents.com".to_string(),
            from: "team_member@acme.com".to_string(),
            subject: Some("Team msg".to_string()),
            text: Some("Hello team".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(res1.accepted);

    // 2. Team-only flow: External sender rejected
    let res2 = thread_use_cases
        .ingest_and_save_inbound_message(RawInboundPayload {
            to: "team-only@acme.mailagents.com".to_string(),
            from: "external@other.com".to_string(),
            subject: Some("External msg".to_string()),
            text: Some("Hello team".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(!res2.accepted);
    assert_eq!(
        res2.reason.as_deref(),
        Some("Sender unauthorized for channel")
    );

    // 3. Public flow: External sender accepted
    let res3 = thread_use_cases
        .ingest_and_save_inbound_message(RawInboundPayload {
            to: "public-flow@acme.mailagents.com".to_string(),
            from: "external@other.com".to_string(),
            subject: Some("Public msg".to_string()),
            text: Some("Hello public".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(res3.accepted);

    // 4. Explicit list flow: Allowed sender accepted, non-allowed rejected
    let res4 = thread_use_cases
        .ingest_and_save_inbound_message(RawInboundPayload {
            to: "explicit-flow@acme.mailagents.com".to_string(),
            from: "allowed@external.com".to_string(),
            subject: Some("Explicit allowed".to_string()),
            text: Some("Hello allowed".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(res4.accepted);

    let res5 = thread_use_cases
        .ingest_and_save_inbound_message(RawInboundPayload {
            to: "explicit-flow@acme.mailagents.com".to_string(),
            from: "notallowed@external.com".to_string(),
            subject: Some("Explicit blocked".to_string()),
            text: Some("Hello blocked".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(!res5.accepted);
    assert_eq!(
        res5.reason.as_deref(),
        Some("Sender unauthorized for channel")
    );
}

#[tokio::test]
async fn test_sender_verification_and_delegation_target_check() {
    let company_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        api_key: None,
        provider: None,
        model: None,
        enable_llm_spam_guardrail: None,
        created_at: Utc::now(),
    }]));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Support".to_string(),
            slug: "support".into(),
            alias_slugs: Vec::new(),
            api_key: None,
            provider: None,
            model: None,
            participant_emails: Some(vec!["@public".into()]),
            agent_ids: None,
            channel_config: None,
            created_at: Utc::now(),
        }]),
    });

    let thread_persistence = Arc::new(MockThreadPersistence {
        threads: Mutex::new(Vec::new()),
        messages: Mutex::new(Vec::new()),
    });

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        access_token_ttl: time::Duration::days(1),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
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
    });

    let task_persistence = Arc::new(MockTaskPersistence {
        tasks: Mutex::new(Vec::new()),
    });

    let thread_use_cases = ThreadUseCases::new(
        thread_persistence,
        channel_persistence,
        company_persistence,
        task_persistence.clone(),
        config,
    );

    // 1. Initial email from client creates thread
    let res1 = thread_use_cases
        .ingest_and_save_inbound_message(RawInboundPayload {
            headers: Some("Message-ID: <msg-client-1@external.com>\n".to_string()),
            to: "support@acme.mailagents.com".to_string(),
            from: "client@external.com".to_string(),
            subject: Some("Need quote".to_string()),
            text: Some("Can I get a quote?".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(res1.accepted);
    let thread_id = res1.thread.unwrap().id;

    // 2. Enqueue a waiting outreach with one target and a sent outbound Message-ID.
    let outreach_id = Uuid::new_v4();
    let delegation_payload = serde_json::json!({
        "test_outreach": {
            "outreach_id": outreach_id,
            "target_email": "vendor@supplier.com",
            "outbound_message_id": "<outreach-vendor@mailagents.com>"
        }
    });
    let task = task_persistence
        .enqueue_task(
            company_id,
            channel_id,
            Some(thread_id),
            "email_agent_dispatch",
            delegation_payload,
        )
        .await
        .unwrap();

    let mut list = task_persistence.tasks.lock().unwrap();
    if let Some(t) = list.iter_mut().find(|t| t.id == task.id) {
        t.status = crate::entities::task::TaskStatus::WaitingForThirdPartyReply;
    }
    drop(list);

    // 3. Unauthorized third-party attacker tries to inject message into thread using In-Reply-To
    let res_attacker = thread_use_cases
        .ingest_and_save_inbound_message(RawInboundPayload {
            headers: Some(
                "Message-ID: <msg-attacker-1@evil.com>\nIn-Reply-To: <msg-client-1@external.com>\n"
                    .to_string(),
            ),
            to: "support@acme.mailagents.com".to_string(),
            from: "attacker@evil.com".to_string(),
            subject: Some("Re: Need quote".to_string()),
            text: Some("Fake reply".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(!res_attacker.accepted);
    assert!(res_attacker.bounce_info.is_some());
    assert_eq!(
        res_attacker.reason.as_deref(),
        Some("Sender is not an authorized participant or delegation target for this thread")
    );

    // 4. Authorized vendor replies to their exact outreach Message-ID.
    let res_vendor = thread_use_cases.ingest_and_save_inbound_message(RawInboundPayload {
            headers: Some("Message-ID: <msg-vendor-1@supplier.com>\nIn-Reply-To: <outreach-vendor@mailagents.com>\nReferences: <msg-client-1@external.com>\n".to_string()),
            to: "support@acme.mailagents.com".to_string(),
            from: "vendor@supplier.com".to_string(),
            subject: Some("Re: Need quote".to_string()),
            text: Some("Quote is $500".to_string()),
            ..Default::default()
        }).await.unwrap();
    assert!(res_vendor.accepted);

    // Verify task was resumed to Pending
    let resumed_task = task_persistence
        .get_task_by_id(task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        resumed_task.status,
        crate::entities::task::TaskStatus::Pending
    );

    // Outreach authorization is scoped to the correlated outbound message and does not
    // permanently promote the target to a thread participant.
    let res_uncorrelated = thread_use_cases
            .ingest_and_save_inbound_message(RawInboundPayload {
                headers: Some(
                    "Message-ID: <msg-vendor-2@supplier.com>\nIn-Reply-To: <msg-client-1@external.com>\n"
                        .to_string(),
                ),
                to: "support@acme.mailagents.com".to_string(),
                from: "vendor@supplier.com".to_string(),
                subject: Some("Re: Need quote".to_string()),
                text: Some("Uncorrelated follow-up".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
    assert!(!res_uncorrelated.accepted);
}

#[tokio::test]
async fn internal_channel_callback_resumes_original_task_without_new_task() {
    let company_id = Uuid::new_v4();
    let channel_a_id = Uuid::new_v4();
    let channel_b_id = Uuid::new_v4();
    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        api_key: None,
        provider: None,
        model: None,
        enable_llm_spam_guardrail: None,
        created_at: Utc::now(),
    }]));
    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: channel_a_id,
                company_id,
                name: "Agent A".to_string(),
                slug: "agent-a".into(),
                alias_slugs: Vec::new(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: Some(vec!["@public".into()]),
                agent_ids: Some(vec![Uuid::new_v4()]),
                channel_config: None,
                created_at: Utc::now(),
            },
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: channel_b_id,
                company_id,
                name: "Agent B".to_string(),
                slug: "agent-b".into(),
                alias_slugs: Vec::new(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None,
                agent_ids: Some(vec![Uuid::new_v4()]),
                channel_config: None,
                created_at: Utc::now(),
            },
        ]),
    });
    let thread_persistence = Arc::new(MockThreadPersistence {
        threads: Mutex::new(Vec::new()),
        messages: Mutex::new(Vec::new()),
    });
    let task_persistence = Arc::new(MockTaskPersistence {
        tasks: Mutex::new(Vec::new()),
    });
    let use_cases = ThreadUseCases::new(
        thread_persistence.clone(),
        channel_persistence,
        company_persistence,
        task_persistence.clone(),
        internal_test_config(),
    );

    let initial = use_cases
        .ingest_and_save_inbound_message(RawInboundPayload {
            headers: Some("Message-ID: <human-request@example.com>\n".to_string()),
            to: "agent-a@acme.mailagents.com".to_string(),
            from: "human@example.com".to_string(),
            subject: Some("Acquire data".to_string()),
            text: Some("Please obtain supplier data".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    let parent_task_id = initial.task_id.unwrap();
    let thread_a = initial.thread.unwrap();
    let call = use_cases
        .prepare_internal_channel_delivery(
            OutboundEmail {
                channel_id: channel_a_id,
                channel_name: "Agent A".to_string(),
                channel_slug: "agent-a".into(),
                company_slug: "acme".into(),
                trigger_message_id: "<human-request@example.com>".into(),
                thread_references: Vec::new(),
                recipient_to: "agent-b@acme.mailagents.com".into(),
                recipients_cc: Vec::new(),
                subject: "Acquire data".to_string(),
                body_text: "Obtain supplier data".to_string(),
                hop_count: 0,
                trace_channels: Vec::new(),
            },
            Some("agent-a-call"),
        )
        .await
        .unwrap()
        .unwrap();
    let call_message_id = call.outbound_message_id.clone();
    thread_persistence
        .create_message(&Message {
            id: Uuid::new_v4(),
            thread_id: thread_a.id,
            message_id: call_message_id.clone(),
            in_reply_to: Some(call.in_reply_to.clone()),
            references_list: call.references.clone(),
            sender: call.from_address.clone(),
            recipients_to: call.recipients_to.clone(),
            recipients_cc: call.recipients_cc.clone(),
            subject: call.subject.clone(),
            clean_text_body: call.body_text.clone(),
            raw_text_body: None,
            raw_html_body: None,
            attachments: None,
            direction: MessageDirection::Outbound,
            role: MessageRole::Agent,
            thread_index: None,
            created_at: Utc::now(),
        })
        .await
        .unwrap();
    let outreach_id = Uuid::new_v4();
    {
        let mut tasks = task_persistence.tasks.lock().unwrap();
        let parent_task = tasks
            .iter_mut()
            .find(|task| task.id == parent_task_id)
            .unwrap();
        parent_task.payload = serde_json::json!({
            "test_outreach": {
                "outreach_id": outreach_id,
                "target_email": "agent-b@acme.mailagents.com",
                "outbound_message_id": call_message_id.clone()
            }
        });
        parent_task.status = crate::entities::task::TaskStatus::WaitingForThirdPartyReply;
    }
    let delegated = use_cases
        .ingest_prepared_internal_message(&call)
        .await
        .unwrap();
    assert!(delegated.accepted);
    assert!(delegated.task_id.is_some());
    assert_ne!(delegated.thread.as_ref().unwrap().id, thread_a.id);
    assert_eq!(delegated.thread.as_ref().unwrap().channel_id, channel_b_id);

    let prepared = use_cases
        .prepare_internal_channel_delivery(
            OutboundEmail {
                channel_id: channel_b_id,
                channel_name: "Agent B".to_string(),
                channel_slug: "agent-b".into(),
                company_slug: "acme".into(),
                trigger_message_id: call_message_id.clone(),
                thread_references: vec![
                    "<human-request@example.com>".into(),
                    call_message_id.clone(),
                ],
                recipient_to: "agent-a@acme.mailagents.com".into(),
                recipients_cc: Vec::new(),
                subject: "Acquire data".to_string(),
                body_text: "Supplier confirmed 2,000 units".to_string(),
                hop_count: 1,
                trace_channels: vec![channel_a_id],
            },
            Some("agent-b-result"),
        )
        .await
        .unwrap()
        .unwrap();
    let callback = use_cases
        .ingest_prepared_internal_message(&prepared)
        .await
        .unwrap();

    assert!(callback.accepted);
    assert!(callback.task_id.is_none());
    assert_eq!(callback.thread.unwrap().id, thread_a.id);
    assert_eq!(callback.inbound_message.unwrap().role, MessageRole::Agent);
    assert_eq!(
        task_persistence
            .get_task_by_id(parent_task_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        crate::entities::task::TaskStatus::Pending
    );
    assert_eq!(task_persistence.tasks.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn uncorrelated_inter_channel_cycle_is_rejected() {
    let company_id = Uuid::new_v4();
    let channel_a_id = Uuid::new_v4();
    let channel_b_id = Uuid::new_v4();
    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        api_key: None,
        provider: None,
        model: None,
        enable_llm_spam_guardrail: None,
        created_at: Utc::now(),
    }]));
    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: channel_a_id,
                company_id,
                name: "Agent A".to_string(),
                slug: "agent-a".into(),
                alias_slugs: Vec::new(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None,
                agent_ids: Some(vec![Uuid::new_v4()]),
                channel_config: None,
                created_at: Utc::now(),
            },
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: channel_b_id,
                company_id,
                name: "Agent B".to_string(),
                slug: "agent-b".into(),
                alias_slugs: Vec::new(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None,
                agent_ids: Some(vec![Uuid::new_v4()]),
                channel_config: None,
                created_at: Utc::now(),
            },
        ]),
    });
    let thread_persistence = Arc::new(MockThreadPersistence {
        threads: Mutex::new(Vec::new()),
        messages: Mutex::new(Vec::new()),
    });
    let task_persistence = Arc::new(MockTaskPersistence {
        tasks: Mutex::new(Vec::new()),
    });
    let use_cases = ThreadUseCases::new(
        thread_persistence,
        channel_persistence,
        company_persistence,
        task_persistence,
        internal_test_config(),
    );

    let unsolicited_prepared = use_cases
        .prepare_internal_channel_delivery(
            OutboundEmail {
                channel_id: channel_b_id,
                channel_name: "Agent B".to_string(),
                channel_slug: "agent-b".into(),
                company_slug: "acme".into(),
                trigger_message_id: "<unsolicited@example.com>".into(),
                thread_references: vec!["<unsolicited@example.com>".into()],
                recipient_to: "agent-a@acme.mailagents.com".into(),
                recipients_cc: Vec::new(),
                subject: "Unsolicited message".to_string(),
                body_text: "Spontaneous message".to_string(),
                hop_count: 1,
                trace_channels: vec![channel_a_id],
            },
            None,
        )
        .await
        .unwrap()
        .unwrap();

    let result = use_cases
        .ingest_prepared_internal_message(&unsolicited_prepared)
        .await
        .unwrap();

    assert!(!result.accepted);
    assert_eq!(
        result.reason.as_deref(),
        Some("Inter-channel loop cycle detected")
    );
}

#[tokio::test]
async fn inter_channel_max_hops_exceeded_is_rejected() {
    let company_id = Uuid::new_v4();
    let channel_a_id = Uuid::new_v4();
    let channel_b_id = Uuid::new_v4();
    let company_persistence = Arc::new(MockCompanyPersistence::new(vec![Company {
        id: company_id,
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: "acme".into(),
        api_key: None,
        provider: None,
        model: None,
        enable_llm_spam_guardrail: None,
        created_at: Utc::now(),
    }]));
    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: channel_a_id,
                company_id,
                name: "Agent A".to_string(),
                slug: "agent-a".into(),
                alias_slugs: Vec::new(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None,
                agent_ids: Some(vec![Uuid::new_v4()]),
                channel_config: None,
                created_at: Utc::now(),
            },
            Channel {
                enabled: true,
                add_3rd_party: true,
                id: channel_b_id,
                company_id,
                name: "Agent B".to_string(),
                slug: "agent-b".into(),
                alias_slugs: Vec::new(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None,
                agent_ids: Some(vec![Uuid::new_v4()]),
                channel_config: None,
                created_at: Utc::now(),
            },
        ]),
    });
    let thread_persistence = Arc::new(MockThreadPersistence {
        threads: Mutex::new(Vec::new()),
        messages: Mutex::new(Vec::new()),
    });
    let task_persistence = Arc::new(MockTaskPersistence {
        tasks: Mutex::new(Vec::new()),
    });
    let use_cases = ThreadUseCases::new(
        thread_persistence,
        channel_persistence,
        company_persistence,
        task_persistence,
        internal_test_config(),
    );

    let max_hop_prepared = use_cases
        .prepare_internal_channel_delivery(
            OutboundEmail {
                channel_id: channel_a_id,
                channel_name: "Agent A".to_string(),
                channel_slug: "agent-a".into(),
                company_slug: "acme".into(),
                trigger_message_id: "<msg@example.com>".into(),
                thread_references: Vec::new(),
                recipient_to: "agent-b@acme.mailagents.com".into(),
                recipients_cc: Vec::new(),
                subject: "Deep loop".to_string(),
                body_text: "Exceeding max hops".to_string(),
                hop_count: MAX_CHANNEL_HOPS,
                trace_channels: Vec::new(),
            },
            None,
        )
        .await
        .unwrap()
        .unwrap();

    let result = use_cases
        .ingest_prepared_internal_message(&max_hop_prepared)
        .await
        .unwrap();

    assert!(!result.accepted);
    assert_eq!(
        result.reason.as_deref(),
        Some("Max inter-channel hop count reached")
    );
}

#[tokio::test]
async fn test_third_party_thread_participants_addition_and_authorization() {
    let company_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::with_team_members(
        vec![Company {
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            created_at: Utc::now(),
        }],
        vec![(company_id, "team@acme.com".to_string())],
    ));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Support Channel".to_string(),
            slug: "support".into(),
            alias_slugs: Vec::new(),
            api_key: None,
            provider: None,
            model: None,
            participant_emails: None, // Default = company team members
            agent_ids: None,
            channel_config: None,
            created_at: Utc::now(),
        }]),
    });

    let thread_persistence = Arc::new(MockThreadPersistence {
        threads: Mutex::new(Vec::new()),
        messages: Mutex::new(Vec::new()),
    });

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        access_token_ttl: time::Duration::days(1),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
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
    });

    let task_persistence = Arc::new(MockTaskPersistence {
        tasks: Mutex::new(Vec::new()),
    });

    let thread_use_cases = ThreadUseCases::new(
        thread_persistence.clone(),
        channel_persistence,
        company_persistence,
        task_persistence,
        config,
    );

    // 1. Team member (workflow participant) sends email to channel with third-party in CC
    let res1 = thread_use_cases
        .ingest_and_save_inbound_message(RawInboundPayload {
            headers: Some("Message-ID: <msg-team-1@acme.com>\n".to_string()),
            to: "support@acme.mailagents.com".to_string(),
            cc: Some("client@external.com".to_string()),
            from: "team@acme.com".to_string(),
            subject: Some("Client Inquiry".to_string()),
            text: Some("Adding third-party client to thread".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert!(res1.accepted);
    let thread = res1.thread.unwrap();
    assert!(
        thread
            .participant_emails
            .iter()
            .any(|p| p.eq_ignore_ascii_case("client@external.com"))
    );

    // 2. Third-party client replies to thread -> ACCEPTED because they were added to thread participants
    let res_reply = thread_use_cases
            .ingest_and_save_inbound_message(RawInboundPayload {
                headers: Some(
                    "Message-ID: <msg-client-reply-1@external.com>\nIn-Reply-To: <msg-team-1@acme.com>\n"
                        .to_string(),
                ),
                to: "support@acme.mailagents.com".to_string(),
                from: "client@external.com".to_string(),
                subject: Some("Re: Client Inquiry".to_string()),
                text: Some("Thanks for adding me".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();

    assert!(res_reply.accepted);

    // 3. Third-party client tries to send a NEW email to channel without thread reference -> REJECTED
    let res_unauth = thread_use_cases
        .ingest_and_save_inbound_message(RawInboundPayload {
            headers: Some("Message-ID: <msg-client-new-1@external.com>\n".to_string()),
            to: "support@acme.mailagents.com".to_string(),
            from: "client@external.com".to_string(),
            subject: Some("Unrelated Subject".to_string()),
            text: Some("Starting a new conversation".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert!(!res_unauth.accepted);
    assert_eq!(
        res_unauth.reason.as_deref(),
        Some("Sender unauthorized for channel")
    );

    // 4. Team member replies to existing thread, adding another third party (vendor@supplier.com) in To
    let res_expand = thread_use_cases
        .ingest_and_save_inbound_message(RawInboundPayload {
            headers: Some(
                "Message-ID: <msg-team-2@acme.com>\nIn-Reply-To: <msg-team-1@acme.com>\n"
                    .to_string(),
            ),
            to: "support@acme.mailagents.com, vendor@supplier.com".to_string(),
            from: "team@acme.com".to_string(),
            subject: Some("Re: Client Inquiry".to_string()),
            text: Some("Looping in supplier".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert!(res_expand.accepted);
    let updated_thread = res_expand.thread.unwrap();
    assert!(
        updated_thread
            .participant_emails
            .iter()
            .any(|p| p.eq_ignore_ascii_case("vendor@supplier.com"))
    );

    // 5. vendor@supplier.com replies to thread -> ACCEPTED
    let res_vendor_reply = thread_use_cases
            .ingest_and_save_inbound_message(RawInboundPayload {
                headers: Some(
                    "Message-ID: <msg-vendor-reply-1@supplier.com>\nIn-Reply-To: <msg-team-2@acme.com>\n"
                        .to_string(),
                ),
                to: "support@acme.mailagents.com".to_string(),
                from: "vendor@supplier.com".to_string(),
                subject: Some("Re: Client Inquiry".to_string()),
                text: Some("Vendor here".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();

    assert!(res_vendor_reply.accepted);

    // 6. External non-team-member (client@external.com) replies and tries to CC unauthorized@other.com -> unauthorized@other.com is NOT added
    let res_external_cc = thread_use_cases
            .ingest_and_save_inbound_message(RawInboundPayload {
                headers: Some(
                    "Message-ID: <msg-client-reply-2@external.com>\nIn-Reply-To: <msg-team-1@acme.com>\n"
                        .to_string(),
                ),
                to: "support@acme.mailagents.com".to_string(),
                cc: Some("unauthorized@other.com".to_string()),
                from: "client@external.com".to_string(),
                subject: Some("Re: Client Inquiry".to_string()),
                text: Some("Adding random CC".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();

    assert!(res_external_cc.accepted);
    let current_thread = res_external_cc.thread.unwrap();
    assert!(
        !current_thread
            .participant_emails
            .iter()
            .any(|p| p.eq_ignore_ascii_case("unauthorized@other.com"))
    );
}

#[tokio::test]
async fn test_context_only_quiet_mode_ingestion() {
    let company_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::with_team_members(
        vec![Company {
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            created_at: Utc::now(),
        }],
        vec![(company_id, "team@acme.com".to_string())],
    ));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Support Channel".to_string(),
            slug: "support".into(),
            alias_slugs: Vec::new(),
            api_key: None,
            provider: None,
            model: None,
            participant_emails: None,
            agent_ids: None,
            channel_config: None,
            created_at: Utc::now(),
        }]),
    });

    let thread_persistence = Arc::new(MockThreadPersistence {
        threads: Mutex::new(Vec::new()),
        messages: Mutex::new(Vec::new()),
    });

    let config = Arc::new(AppConfig {
        jwt_secret: "secret".to_string(),
        access_token_ttl: time::Duration::days(1),
        refresh_token_ttl: time::Duration::days(30),
        app_domain_name: "mailagents.com".to_string(),
        cors_allowed_origins: vec![],
        smtp_host: "localhost".to_string(),
        smtp_port: 1025,
        smtp_username: "".to_string(),
        smtp_password: "".to_string(),
        smtp_from_address: "noreply@mailagents.com".to_string(),
        incoming_smtp_enabled: true,
        incoming_smtp_host: "0.0.0.0".to_string(),
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
    });

    let task_persistence = Arc::new(MockTaskPersistence {
        tasks: Mutex::new(Vec::new()),
    });

    let thread_use_cases = ThreadUseCases::new(
        thread_persistence.clone(),
        channel_persistence,
        company_persistence,
        task_persistence,
        config,
    );

    // 1. Ingest email with .quiet suffix in address -> accepted, task_id is None (agent execution skipped)
    let res_quiet_addr = thread_use_cases
        .ingest_and_save_inbound_message(RawInboundPayload {
            headers: Some("Message-ID: <msg-quiet-1@acme.com>\n".to_string()),
            to: "support.quiet@acme.mailagents.com".to_string(),
            from: "team@acme.com".to_string(),
            subject: Some("Quiet Note".to_string()),
            text: Some("Background context note".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert!(res_quiet_addr.accepted);
    assert!(res_quiet_addr.task_id.is_none());
    assert!(res_quiet_addr.parsed_email.unwrap().is_context_only);

    // 2. Ingest email with [[quiet]] body tag -> accepted, task_id is None, tag stripped from text
    let res_quiet_body = thread_use_cases
        .ingest_and_save_inbound_message(RawInboundPayload {
            headers: Some(
                "Message-ID: <msg-quiet-2@acme.com>\nIn-Reply-To: <msg-quiet-1@acme.com>\n"
                    .to_string(),
            ),
            to: "support@acme.mailagents.com".to_string(),
            from: "team@acme.com".to_string(),
            subject: Some("Re: Quiet Note".to_string()),
            text: Some("[[quiet]] Additional background note".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert!(res_quiet_body.accepted);
    assert!(res_quiet_body.task_id.is_none());
    let parsed_body = res_quiet_body.parsed_email.unwrap();
    assert!(parsed_body.is_context_only);
    assert_eq!(parsed_body.clean_text_body, "Additional background note");
}

/// The `support` channel one of these fixtures builds.
///
/// Named fields rather than positional arguments: `enabled` and `add_3rd_party` are same-typed
/// switches that a positional call could silently transpose.
struct TestChannel {
    enabled: bool,
    add_3rd_party: bool,
    alias_slugs: Vec<ChannelSlug>,
    participant_emails: Option<Vec<EmailAddress>>,
}

impl Default for TestChannel {
    /// A plain team-only channel, taking traffic and pulling CC'd outsiders onto its threads —
    /// what every channel is until someone changes it.
    fn default() -> Self {
        Self {
            enabled: true,
            add_3rd_party: true,
            alias_slugs: Vec::new(),
            participant_emails: None,
        }
    }
}

/// One company with a `support` channel whose on/off state the caller picks, wired for ingest.
fn use_cases_with_channel_enabled(enabled: bool) -> (ThreadUseCases, Uuid) {
    use_cases_with_channel(TestChannel {
        enabled,
        ..TestChannel::default()
    })
}

fn use_cases_with_channel_aliases(alias_slugs: Vec<ChannelSlug>) -> (ThreadUseCases, Uuid) {
    use_cases_with_channel(TestChannel {
        alias_slugs,
        ..TestChannel::default()
    })
}

fn use_cases_with_channel(spec: TestChannel) -> (ThreadUseCases, Uuid) {
    let TestChannel {
        enabled,
        add_3rd_party,
        alias_slugs,
        participant_emails,
    } = spec;
    let company_id = Uuid::new_v4();
    let channel_id = Uuid::new_v4();

    let company_persistence = Arc::new(MockCompanyPersistence::with_team_members(
        vec![Company {
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            created_at: Utc::now(),
        }],
        vec![(company_id, "team@acme.com".to_string())],
    ));

    let channel_persistence = Arc::new(MockChannelPersistence {
        channels: Mutex::new(vec![Channel {
            enabled,
            add_3rd_party,
            id: channel_id,
            company_id,
            name: "Support Channel".to_string(),
            slug: "support".into(),
            alias_slugs,
            api_key: None,
            provider: None,
            model: None,
            participant_emails,
            agent_ids: None,
            channel_config: None,
            created_at: Utc::now(),
        }]),
    });

    let thread_use_cases = ThreadUseCases::new(
        Arc::new(MockThreadPersistence {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        }),
        channel_persistence,
        company_persistence,
        Arc::new(MockTaskPersistence {
            tasks: Mutex::new(Vec::new()),
        }),
        internal_test_config(),
    );

    (thread_use_cases, channel_id)
}

/// A message from a team member that copies someone outside the platform.
fn message_to_support_cc_outsider() -> RawInboundPayload {
    RawInboundPayload {
        headers: Some("Message-ID: <cc-outsider@acme.com>\n".to_string()),
        to: "support@acme.mailagents.com".to_string(),
        cc: Some("client@external.com".to_string()),
        from: "team@acme.com".to_string(),
        subject: Some("Client inquiry".to_string()),
        text: Some("Looping the client in".to_string()),
        ..Default::default()
    }
}

fn participants_of(result: &InboundIngestResult) -> Vec<String> {
    result
        .thread
        .as_ref()
        .unwrap()
        .participant_emails
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// The flag off makes the channel internal: the CC'd outsider is not recorded on the thread, and
/// so does not inherit the thread's standing invitation to post into it.
#[tokio::test]
async fn a_closed_channel_keeps_cc_d_outsiders_off_the_thread() {
    let (thread_use_cases, _) = use_cases_with_channel(TestChannel {
        add_3rd_party: false,
        ..TestChannel::default()
    });

    let result = thread_use_cases
        .ingest_and_save_inbound_message(message_to_support_cc_outsider())
        .await
        .unwrap();

    assert!(result.accepted);
    assert_eq!(participants_of(&result), vec!["team@acme.com".to_string()]);
}

/// The same message on the default channel, so the test above is pinned to the flag rather than to
/// something else about the fixture.
#[tokio::test]
async fn an_open_channel_still_adds_cc_d_outsiders_to_the_thread() {
    let (thread_use_cases, _) = use_cases_with_channel(TestChannel::default());

    let result = thread_use_cases
        .ingest_and_save_inbound_message(message_to_support_cc_outsider())
        .await
        .unwrap();

    assert!(result.accepted);
    assert!(
        participants_of(&result)
            .iter()
            .any(|p| p.eq_ignore_ascii_case("client@external.com"))
    );
}

/// The flag narrows, it never widens. A `@public` channel admits any sender, but an admitted
/// stranger is still not *trusted*, so turning the flag on does not let them attach whoever they
/// like to the thread — and every such address would otherwise gain the right to post into it.
#[tokio::test]
async fn an_untrusted_sender_cannot_add_outsiders_even_with_the_flag_on() {
    let (thread_use_cases, _) = use_cases_with_channel(TestChannel {
        participant_emails: Some(vec!["@public".into()]),
        ..TestChannel::default()
    });

    let result = thread_use_cases
        .ingest_and_save_inbound_message(RawInboundPayload {
            headers: Some("Message-ID: <stranger@external.com>\n".to_string()),
            to: "support@acme.mailagents.com".to_string(),
            cc: Some("accomplice@external.com".to_string()),
            from: "stranger@external.com".to_string(),
            subject: Some("Hello".to_string()),
            text: Some("Bringing a friend".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert!(result.accepted);
    assert_eq!(
        participants_of(&result),
        vec!["stranger@external.com".to_string()]
    );
}

/// The outsider's own reply-all. Never having joined the thread, they have no way into it — the
/// thread membership that `add_3rd_party` withholds is exactly what would have let them in.
///
/// On a team-only channel they are turned away by the channel ACL, before the thread is consulted
/// at all, which is a silent drop rather than a bounce. That is the pre-existing shape of an
/// unauthorized sender, and this test pins it so the difference from the `@public` case below is
/// deliberate rather than accidental.
#[tokio::test]
async fn a_closed_channel_refuses_the_outsider_s_own_reply() {
    let (thread_use_cases, _) = use_cases_with_channel(TestChannel {
        add_3rd_party: false,
        ..TestChannel::default()
    });

    thread_use_cases
        .ingest_and_save_inbound_message(message_to_support_cc_outsider())
        .await
        .unwrap();

    let reply = thread_use_cases
        .ingest_and_save_inbound_message(outsider_reply())
        .await
        .unwrap();

    assert!(!reply.accepted);
    assert_eq!(
        reply.reason.as_deref(),
        Some("Sender unauthorized for channel")
    );
}

/// Where the channel ACL does admit the sender, the thread is consulted, and refusing them there
/// is a bounce — they get told, rather than having their mail disappear.
#[tokio::test]
async fn a_closed_public_channel_bounces_the_outsider_s_own_reply() {
    let (thread_use_cases, _) = use_cases_with_channel(TestChannel {
        add_3rd_party: false,
        participant_emails: Some(vec!["@public".into(), "team@acme.com".into()]),
        ..TestChannel::default()
    });

    thread_use_cases
        .ingest_and_save_inbound_message(message_to_support_cc_outsider())
        .await
        .unwrap();

    let reply = thread_use_cases
        .ingest_and_save_inbound_message(outsider_reply())
        .await
        .unwrap();

    assert!(!reply.accepted);
    assert!(reply.bounce_info.is_some(), "the sender must be told why");
}

/// The CC'd outsider answering the message that copied them.
fn outsider_reply() -> RawInboundPayload {
    RawInboundPayload {
        headers: Some(
            "Message-ID: <outsider-reply@external.com>\nIn-Reply-To: <cc-outsider@acme.com>\n"
                .to_string(),
        ),
        to: "support@acme.mailagents.com".to_string(),
        from: "client@external.com".to_string(),
        subject: Some("Re: Client inquiry".to_string()),
        text: Some("Replying to all".to_string()),
        ..Default::default()
    }
}

/// The reply's Cc is where "never copied on the agent's reply" is actually enforced: the inbound
/// Cc line is the only thing that decides who a reply reaches, and thread membership never enters
/// into it. Platform addresses survive the filter — a pipeline's later steps ride the Cc line.
#[tokio::test]
async fn a_closed_channel_drops_outsiders_from_the_reply_cc() {
    let (thread_use_cases, _) = use_cases_with_channel(TestChannel {
        add_3rd_party: false,
        ..TestChannel::default()
    });

    let result = thread_use_cases
        .ingest_and_save_inbound_message(message_to_support_cc_outsider())
        .await
        .unwrap();

    let mut parsed = result.parsed_email.clone().unwrap();
    parsed.recipients_cc = vec![
        "client@external.com".to_string(),
        "support@acme.mailagents.com".to_string(),
    ];

    let cc = thread_use_cases
        .outbound_cc_for(&result.channel_matches[0], &parsed)
        .await
        .unwrap();

    assert_eq!(cc, vec!["support@acme.mailagents.com".to_string()]);
}

/// The default channel copies everyone the inbound message copied, as it always has.
#[tokio::test]
async fn an_open_channel_keeps_outsiders_on_the_reply_cc() {
    let (thread_use_cases, _) = use_cases_with_channel(TestChannel::default());

    let result = thread_use_cases
        .ingest_and_save_inbound_message(message_to_support_cc_outsider())
        .await
        .unwrap();

    let parsed = result.parsed_email.clone().unwrap();
    let cc = thread_use_cases
        .outbound_cc_for(&result.channel_matches[0], &parsed)
        .await
        .unwrap();

    assert!(cc.iter().any(|to| to == "client@external.com"));
}

fn message_to_support() -> RawInboundPayload {
    RawInboundPayload {
        to: "support@acme.mailagents.com".to_string(),
        from: "team@acme.com".to_string(),
        subject: Some("Hello".to_string()),
        text: Some("Anyone there?".to_string()),
        ..Default::default()
    }
}

#[tokio::test]
async fn disabled_channel_bounces_inbound_mail() {
    let (thread_use_cases, _) = use_cases_with_channel_enabled(false);

    let result = thread_use_cases
        .ingest_and_save_inbound_message(message_to_support())
        .await
        .unwrap();

    assert!(!result.accepted);
    assert_eq!(result.reason.as_deref(), Some("Channel is disabled"));

    let bounce = result
        .bounce_info
        .expect("a disabled channel bounces rather than dropping the message");
    assert_eq!(bounce.disabled_slugs, vec![ChannelSlug::from("support")]);
    assert!(
        bounce.invalid_slugs.is_empty(),
        "a disabled channel is not a misspelling"
    );
    assert_eq!(bounce.recipient_to, EmailAddress::from("team@acme.com"));

    let body = format_bounce_email_body(&bounce, "mailagents.com");
    assert!(body.contains("support@acme.mailagents.com"));
    assert!(body.contains("switched off"));
    assert!(!body.contains("Did you mean"));
}

#[tokio::test]
async fn enabled_channel_still_accepts_the_same_message() {
    let (thread_use_cases, channel_id) = use_cases_with_channel_enabled(true);

    let result = thread_use_cases
        .ingest_and_save_inbound_message(message_to_support())
        .await
        .unwrap();

    assert!(result.accepted, "reason: {:?}", result.reason);
    assert_eq!(result.thread.unwrap().channel_id, channel_id);
}

#[tokio::test]
async fn alias_address_reaches_the_channel_and_is_replied_from() {
    let (thread_use_cases, channel_id) = use_cases_with_channel_aliases(vec!["sales".into()]);

    let result = thread_use_cases
        .ingest_and_save_inbound_message(RawInboundPayload {
            to: "sales@acme.mailagents.com".to_string(),
            ..message_to_support()
        })
        .await
        .unwrap();

    assert!(result.accepted, "reason: {:?}", result.reason);
    assert_eq!(result.channel_matches.len(), 1);

    let matched = &result.channel_matches[0];
    assert_eq!(matched.channel.id, channel_id);
    assert_eq!(matched.channel.slug, "support");
    // The reply must go back out on the address the sender wrote to, not the canonical slug.
    assert_eq!(matched.reply_slug(), ChannelSlug::from("sales"));
}

#[tokio::test]
async fn alias_and_canonical_slug_in_one_pipeline_ingest_once() {
    let (thread_use_cases, channel_id) = use_cases_with_channel_aliases(vec!["sales".into()]);

    let result = thread_use_cases
        .ingest_and_save_inbound_message(RawInboundPayload {
            to: "sales+support@acme.mailagents.com".to_string(),
            ..message_to_support()
        })
        .await
        .unwrap();

    assert!(result.accepted, "reason: {:?}", result.reason);
    assert_eq!(
        result.channel_matches.len(),
        1,
        "two names for one channel are still one delivery"
    );
    assert_eq!(result.channel_matches[0].channel.id, channel_id);
    // The first address on the envelope wins, so the reply stays on the alias.
    assert_eq!(
        result.channel_matches[0].reply_slug(),
        ChannelSlug::from("sales")
    );
}

#[tokio::test]
async fn unknown_slug_is_still_a_bounce_when_the_channel_has_aliases() {
    let (thread_use_cases, _) = use_cases_with_channel_aliases(vec!["sales".into()]);

    let result = thread_use_cases
        .ingest_and_save_inbound_message(RawInboundPayload {
            to: "salez@acme.mailagents.com".to_string(),
            ..message_to_support()
        })
        .await
        .unwrap();

    assert!(!result.accepted);
    let bounce = result.bounce_info.expect("a misspelled alias bounces");
    assert_eq!(bounce.invalid_slugs, vec![ChannelSlug::from("salez")]);
    assert!(
        bounce
            .suggestions
            .iter()
            .any(|s| s.suggestions.contains(&ChannelSlug::from("sales"))),
        "aliases are offered as did-you-mean suggestions: {:?}",
        bounce.suggestions
    );
}
