use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    adapters::persistence::{approval::ApprovalPersistence, task::TaskPersistence},
    app_error::{AppError, AppResult},
    entities::{
        approval::{ApprovalStatus, HumanApproval},
        message::{Message, MessageDirection, MessageRole},
        value_objects::{ChannelSlug, CompanySlug, EmailAddress},
    },
    infra::config::AppConfig,
    services::outbound_dispatcher::OutboundEmail,
    use_cases::thread::ThreadPersistence,
};

pub struct ApprovalUseCases {
    approval_persistence: Arc<dyn ApprovalPersistence>,
    task_persistence: Arc<dyn TaskPersistence>,
    thread_persistence: Arc<dyn ThreadPersistence>,
    config: Arc<AppConfig>,
}

impl ApprovalUseCases {
    pub fn new(
        approval_persistence: Arc<dyn ApprovalPersistence>,
        task_persistence: Arc<dyn TaskPersistence>,
        thread_persistence: Arc<dyn ThreadPersistence>,
        config: Arc<AppConfig>,
    ) -> Self {
        Self {
            approval_persistence,
            task_persistence,
            thread_persistence,
            config,
        }
    }

    pub async fn check_step_approval(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
        thread_id: Option<Uuid>,
        step_key: &str,
    ) -> AppResult<Option<ApprovalStatus>> {
        let approval = self
            .approval_persistence
            .find_approval_by_step_key(company_id, channel_id, thread_id, step_key)
            .await?;

        Ok(approval.map(|a| a.status))
    }

    pub async fn create_and_send_approval_request(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
        channel_name: &str,
        channel_slug: &ChannelSlug,
        company_slug: &CompanySlug,
        thread_id: Option<Uuid>,
        task_id: Option<Uuid>,
        step_key: &str,
        approver_email: &EmailAddress,
        action_type: &str,
        action_title: &str,
        action_summary: &str,
        payload: serde_json::Value,
    ) -> AppResult<HumanApproval> {
        let token = Uuid::new_v4().to_string();
        let expires_at = Utc::now() + chrono::Duration::hours(24);
        let domain = &self.config.app_domain_name;
        let confirm_url = format!("http://{}/approvals/{}?action=confirm", domain, token);
        let reject_url = format!("http://{}/approvals/{}?action=reject", domain, token);
        let body_text = if action_type == "quorum_timeout" {
            let proceed_url = format!(
                "http://{}/approvals/{}?action=proceed_partial",
                domain, token
            );
            let extend_24_url = format!("http://{}/approvals/{}?action=extend_24h", domain, token);
            let extend_48_url = format!("http://{}/approvals/{}?action=extend_48h", domain, token);
            format!(
                "Outreach Timeout Decision for Channel '{}'\n\nAction: {}\nSummary: {}\n\nProceed with available responses:\n{}\n\nExtend 24 hours:\n{}\n\nExtend 48 hours:\n{}\n\nReject and stop the task:\n{}\n\nThis link is valid for 24 hours.",
                channel_name,
                action_title,
                action_summary,
                proceed_url,
                extend_24_url,
                extend_48_url,
                reject_url
            )
        } else {
            format!(
                "Action Approval Requested for Channel '{}'\n\nAction: {}\nSummary: {}\n\nClick to CONFIRM:\n{}\n\nClick to REJECT:\n{}\n\nThis link is valid for 24 hours.",
                channel_name, action_title, action_summary, confirm_url, reject_url
            )
        };
        let notification = serde_json::to_value(OutboundEmail {
            channel_id,
            channel_name: channel_name.to_string(),
            channel_slug: channel_slug.clone(),
            company_slug: company_slug.clone(),
            trigger_message_id: format!("<approval-{}@{}>", token, domain).into(),
            thread_references: vec![],
            recipient_to: approver_email.clone(),
            recipients_cc: vec![],
            subject: format!("[APPROVAL REQUIRED] {}", action_title),
            body_text,
            hop_count: 0,
            trace_channels: vec![channel_id],
        })
        .map_err(|error| {
            AppError::Internal(format!(
                "Failed to serialize approval notification: {error}"
            ))
        })?;

        let (approval, created) = self
            .approval_persistence
            .create_approval(
                company_id,
                channel_id,
                thread_id,
                task_id,
                step_key,
                approver_email,
                action_type,
                action_title,
                action_summary,
                payload,
                notification,
                &token,
                expires_at,
            )
            .await?;

        if !created {
            return Ok(approval);
        }

        Ok(approval)
    }

    /// Apply the decision behind a one-click approval link.
    ///
    /// Every exit either reports what the link did or explains why it no longer applies; a link
    /// that arrives twice is answered, not failed.
    pub async fn process_link_action(
        &self,
        token: &str,
        action: &str,
    ) -> AppResult<(HumanApproval, String)> {
        let approval = self.require_approval(token).await?;

        if approval.status != ApprovalStatus::Pending {
            let message = already_processed_message(&approval);
            return Ok((approval, message));
        }

        let now = Utc::now();
        if approval.expires_at < now {
            return self.report_unavailable(token, now).await;
        }

        let normalized_action = action.to_lowercase();
        let decision =
            LinkAction::parse(&normalized_action, &approval.action_type).ok_or_else(|| {
                AppError::Internal(format!(
                    "Action '{}' is not valid for approval type '{}'.",
                    action, approval.action_type
                ))
            })?;

        let now = Utc::now();
        // Quorum timeouts carry the chosen option through to the paused task; everything else is a
        // plain approve/reject state transition.
        let consumed = if approval.action_type == "quorum_timeout" && approval.task_id.is_some() {
            self.approval_persistence
                .consume_quorum_timeout_action(token, &normalized_action, now)
                .await?
        } else {
            self.approval_persistence
                .consume_pending_approval(token, decision.status(), now)
                .await?
        };
        // Losing the race to consume means someone (or the expiry sweep) got there first.
        let Some(updated) = consumed else {
            return self.report_unavailable(token, now).await;
        };

        let message_text = self.apply_decision(&approval, decision).await;
        Ok((updated, message_text))
    }

    /// Carry out the side effects of a decision and describe them for the human who clicked.
    async fn apply_decision(&self, approval: &HumanApproval, decision: LinkAction) -> String {
        match decision {
            LinkAction::ProceedPartial => {
                self.record_decision_note(
                    approval,
                    "quorum-partial",
                    "HITL Proceed Partial",
                    format!(
                        "Human decision by {}: Proceeding with partial quorum responses.",
                        approval.approver_email
                    ),
                )
                .await;
                format!(
                    "✓ Action '{}' confirmed: Proceeding with partial data. Channels resumed.",
                    approval.action_title
                )
            }
            LinkAction::Extend { hours } => {
                self.record_decision_note(
                    approval,
                    "quorum-extended",
                    "HITL Timeout Extended",
                    format!(
                        "Human decision by {}: Outreach response timeout extended by {} hours.",
                        approval.approver_email, hours
                    ),
                )
                .await;
                format!(
                    "✓ Action '{}' confirmed: Outreach timeout extended by {} hours.",
                    approval.action_title, hours
                )
            }
            LinkAction::Approve => {
                if let Some(task_id) = approval.task_id {
                    let _ = self.task_persistence.resume_task(task_id).await;
                }
                self.record_decision_note(
                    approval,
                    "approval-granted",
                    "HITL Granted",
                    format!(
                        "Human approval GRANTED by {} for action '{}'.",
                        approval.approver_email, approval.action_title
                    ),
                )
                .await;
                format!(
                    "✓ Action '{}' has been CONFIRMED successfully. Associated automated channels have been resumed.",
                    approval.action_title
                )
            }
            LinkAction::Reject => {
                // A rejected quorum timeout leaves the task paused for its own timeout handling.
                if let Some(task_id) = approval.task_id
                    && approval.action_type != "quorum_timeout"
                {
                    let _ = self.task_persistence.stop_task(task_id).await;
                }
                self.record_decision_note(
                    approval,
                    "approval-rejected",
                    "HITL Rejected",
                    format!(
                        "Human approval REJECTED by {} for action '{}'.",
                        approval.approver_email, approval.action_title
                    ),
                )
                .await;
                format!(
                    "✗ Action '{}' has been REJECTED. The automated channel task has been cancelled.",
                    approval.action_title
                )
            }
        }
    }

    /// Append the decision to the originating thread so the conversation records who decided what.
    ///
    /// Best-effort: a thread write failure must not undo a decision that is already persisted.
    async fn record_decision_note(
        &self,
        approval: &HumanApproval,
        message_id_prefix: &str,
        subject_tag: &str,
        body: String,
    ) {
        let Some(thread_id) = approval.thread_id else {
            return;
        };
        let note = Message {
            id: Uuid::new_v4(),
            thread_id,
            message_id: format!(
                "<{}-{}@{}>",
                message_id_prefix, approval.id, self.config.app_domain_name
            )
            .into(),
            in_reply_to: None,
            references_list: vec![],
            sender: approval.approver_email.clone().into(),
            recipients_to: vec![],
            recipients_cc: vec![],
            subject: format!("[{}]: {}", subject_tag, approval.action_title),
            clean_text_body: body,
            raw_text_body: None,
            raw_html_body: None,
            attachments: None,
            direction: MessageDirection::Inbound,
            role: MessageRole::System,
            thread_index: None,
            created_at: Utc::now(),
        };
        let _ = self.thread_persistence.create_message(&note).await;
    }

    async fn require_approval(&self, token: &str) -> AppResult<HumanApproval> {
        self.approval_persistence
            .get_approval_by_token(token)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(
                    "This confirmation link is not valid or has already been used.".into(),
                )
            })
    }

    /// Explain a link that can no longer be acted on: either it just expired, or its decision was
    /// already recorded.
    async fn report_unavailable(
        &self,
        token: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<(HumanApproval, String)> {
        if let Some(expired) = self
            .approval_persistence
            .expire_pending_approval(token, now)
            .await?
        {
            return Ok((
                expired,
                "This confirmation link has expired (24-hour TTL).".into(),
            ));
        }
        let current = self.require_approval(token).await?;
        let message = already_processed_message(&current);
        Ok((current, message))
    }

    pub async fn get_approval_by_token(&self, token: &str) -> AppResult<Option<HumanApproval>> {
        self.approval_persistence.get_approval_by_token(token).await
    }

    pub async fn list_channel_approvals(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Vec<HumanApproval>> {
        self.approval_persistence
            .list_approvals_by_channel(company_id, channel_id)
            .await
    }
}

/// What a one-click approval link asks for, once validated against the approval it belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkAction {
    /// Quorum timeout: continue with whatever responses arrived.
    ProceedPartial,
    /// Quorum timeout: wait longer.
    Extend {
        hours: u32,
    },
    Approve,
    Reject,
}

impl LinkAction {
    /// Parse the query-string action, rejecting verbs that don't belong to this approval type.
    fn parse(normalized_action: &str, approval_action_type: &str) -> Option<Self> {
        let is_quorum_timeout = approval_action_type == "quorum_timeout";
        match normalized_action {
            "proceed_partial" if is_quorum_timeout => Some(Self::ProceedPartial),
            "extend_48h" if is_quorum_timeout => Some(Self::Extend { hours: 48 }),
            "extend_24h" | "extend" if is_quorum_timeout => Some(Self::Extend { hours: 24 }),
            "confirm" | "approved" | "approve" if !is_quorum_timeout => Some(Self::Approve),
            "reject" => Some(Self::Reject),
            "rejected" if !is_quorum_timeout => Some(Self::Reject),
            _ => None,
        }
    }

    fn status(self) -> ApprovalStatus {
        match self {
            Self::Reject => ApprovalStatus::Rejected,
            _ => ApprovalStatus::Approved,
        }
    }
}

fn already_processed_message(approval: &HumanApproval) -> String {
    format!(
        "This request was already processed as '{}'.",
        approval.status.as_str()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{
        cursor::{MessageCursor, ThreadCursor},
        thread::Thread,
    };
    use async_trait::async_trait;
    use std::sync::Mutex;

    /// The one config every test here uses; kept in one place so a new field is added once.
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
        })
    }

    struct MockApprovalPersistence {
        approvals: Mutex<Vec<HumanApproval>>,
    }

    #[async_trait]
    impl ApprovalPersistence for MockApprovalPersistence {
        async fn create_approval(
            &self,
            company_id: Uuid,
            channel_id: Uuid,
            thread_id: Option<Uuid>,
            task_id: Option<Uuid>,
            step_key: &str,
            approver_email: &str,
            action_type: &str,
            action_title: &str,
            action_summary: &str,
            payload: serde_json::Value,
            _notification: serde_json::Value,
            token: &str,
            expires_at: chrono::DateTime<chrono::Utc>,
        ) -> AppResult<(HumanApproval, bool)> {
            let mut approvals = self.approvals.lock().unwrap();
            if let Some(existing) = approvals
                .iter()
                .find(|a| a.thread_id == thread_id && a.step_key == step_key)
            {
                return Ok((existing.clone(), false));
            }

            let approval = HumanApproval {
                id: Uuid::new_v4(),
                company_id,
                channel_id: channel_id,
                thread_id,
                task_id,
                step_key: step_key.to_string(),
                approver_email: approver_email.to_string(),
                action_type: action_type.to_string(),
                action_title: action_title.to_string(),
                action_summary: action_summary.to_string(),
                payload,
                token: token.to_string(),
                status: ApprovalStatus::Pending,
                expires_at,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            approvals.push(approval.clone());
            Ok((approval, true))
        }

        async fn find_approval_by_step_key(
            &self,
            company_id: Uuid,
            channel_id: Uuid,
            thread_id: Option<Uuid>,
            step_key: &str,
        ) -> AppResult<Option<HumanApproval>> {
            Ok(self
                .approvals
                .lock()
                .unwrap()
                .iter()
                .find(|a| {
                    a.company_id == company_id
                        && a.channel_id == channel_id
                        && a.thread_id == thread_id
                        && a.step_key == step_key
                })
                .cloned())
        }

        async fn get_approval_by_token(&self, token: &str) -> AppResult<Option<HumanApproval>> {
            Ok(self
                .approvals
                .lock()
                .unwrap()
                .iter()
                .find(|a| a.token == token)
                .cloned())
        }

        async fn consume_pending_approval(
            &self,
            token: &str,
            status: ApprovalStatus,
            now: chrono::DateTime<chrono::Utc>,
        ) -> AppResult<Option<HumanApproval>> {
            let mut list = self.approvals.lock().unwrap();
            let Some(approval) = list.iter_mut().find(|a| {
                a.token == token && a.status == ApprovalStatus::Pending && a.expires_at >= now
            }) else {
                return Ok(None);
            };
            approval.status = status;
            approval.updated_at = Utc::now();
            Ok(Some(approval.clone()))
        }

        async fn expire_pending_approval(
            &self,
            token: &str,
            now: chrono::DateTime<chrono::Utc>,
        ) -> AppResult<Option<HumanApproval>> {
            let mut list = self.approvals.lock().unwrap();
            let Some(approval) = list.iter_mut().find(|a| {
                a.token == token && a.status == ApprovalStatus::Pending && a.expires_at < now
            }) else {
                return Ok(None);
            };
            approval.status = ApprovalStatus::Expired;
            approval.updated_at = Utc::now();
            Ok(Some(approval.clone()))
        }

        async fn list_approvals_by_channel(
            &self,
            company_id: Uuid,
            channel_id: Uuid,
        ) -> AppResult<Vec<HumanApproval>> {
            Ok(self
                .approvals
                .lock()
                .unwrap()
                .iter()
                .filter(|a| a.company_id == company_id && a.channel_id == channel_id)
                .cloned()
                .collect())
        }
    }

    struct MockTaskPersistence;
    #[async_trait]
    impl TaskPersistence for MockTaskPersistence {
        async fn enqueue_task(
            &self,
            _company_id: Uuid,
            _channel_id: Uuid,
            _thread_id: Option<Uuid>,
            _task_type: &str,
            _payload: serde_json::Value,
        ) -> AppResult<crate::entities::task::BackgroundTask> {
            unimplemented!()
        }
        async fn get_task_by_id(
            &self,
            _id: Uuid,
        ) -> AppResult<Option<crate::entities::task::BackgroundTask>> {
            unimplemented!()
        }
        async fn update_task_payload(
            &self,
            _id: Uuid,
            _payload: serde_json::Value,
        ) -> AppResult<()> {
            Ok(())
        }
        async fn claim_pending_tasks(
            &self,
            _worker_id: Uuid,
            _lock_expires_at: chrono::DateTime<chrono::Utc>,
            _limit: i64,
        ) -> AppResult<Vec<crate::entities::task::BackgroundTask>> {
            unimplemented!()
        }
        async fn claim_task(
            &self,
            _id: Uuid,
            _worker_id: Uuid,
            _lock_expires_at: chrono::DateTime<chrono::Utc>,
        ) -> AppResult<bool> {
            Ok(true)
        }
        async fn mark_task_completed(&self, _id: Uuid, _worker_id: Uuid) -> AppResult<bool> {
            unimplemented!()
        }
        async fn mark_task_failed(
            &self,
            _id: Uuid,
            _worker_id: Uuid,
            _error_msg: &str,
            _next_run_at: chrono::DateTime<chrono::Utc>,
            _is_dead_letter: bool,
        ) -> AppResult<bool> {
            Ok(true)
        }
        async fn stop_task(&self, _id: Uuid) -> AppResult<crate::entities::task::BackgroundTask> {
            unimplemented!()
        }
        async fn resume_task(&self, _id: Uuid) -> AppResult<crate::entities::task::BackgroundTask> {
            unimplemented!()
        }
        async fn update_task_status(
            &self,
            _id: Uuid,
            _status: crate::entities::task::TaskStatus,
        ) -> AppResult<crate::entities::task::BackgroundTask> {
            unimplemented!()
        }
        async fn list_company_tasks(
            &self,
            _company_id: Uuid,
            _channel_id: Option<Uuid>,
            _status: Option<crate::entities::task::TaskStatus>,
            _sort_asc: bool,
        ) -> AppResult<Vec<crate::entities::task::BackgroundTask>> {
            unimplemented!()
        }
    }

    struct MockThreadPersistence;
    #[async_trait]
    impl ThreadPersistence for MockThreadPersistence {
        async fn create_thread(
            &self,
            _channel_id: Uuid,
            _subject: &str,
            _participant_emails: &[crate::entities::value_objects::EmailAddress],
        ) -> AppResult<crate::entities::thread::Thread> {
            unimplemented!()
        }
        async fn get_thread_by_id(
            &self,
            _id: Uuid,
        ) -> AppResult<Option<crate::entities::thread::Thread>> {
            unimplemented!()
        }
        async fn list_threads_by_channel_id(
            &self,
            _channel_id: Uuid,
            _before: Option<ThreadCursor>,
            _limit: usize,
        ) -> AppResult<Vec<crate::entities::thread::Thread>> {
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
            _participant_emails: &[crate::entities::value_objects::EmailAddress],
        ) -> AppResult<crate::entities::thread::Thread> {
            unimplemented!()
        }
        async fn find_thread_by_message_ids(
            &self,
            _channel_id: Uuid,
            _message_ids: &[crate::entities::value_objects::MessageId],
        ) -> AppResult<Option<crate::entities::thread::Thread>> {
            unimplemented!()
        }
        async fn find_thread_by_thread_index(
            &self,
            _channel_id: Uuid,
            _thread_index_prefix: &crate::entities::value_objects::ThreadIndex,
        ) -> AppResult<Option<crate::entities::thread::Thread>> {
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
            Ok(message.clone())
        }
        async fn get_message_by_message_id(
            &self,
            _company_id: Uuid,
            _message_id: &crate::entities::value_objects::MessageId,
        ) -> AppResult<Option<Message>> {
            unimplemented!()
        }
        async fn find_outbound_reply(
            &self,
            _thread_id: Uuid,
            _in_reply_to: &crate::entities::value_objects::MessageId,
        ) -> AppResult<Option<Message>> {
            Ok(None)
        }
        async fn list_messages_by_thread_id(&self, _thread_id: Uuid) -> AppResult<Vec<Message>> {
            unimplemented!()
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

    /// A stale link out of somebody's inbox is routine, not a server fault: it must not answer 500
    /// with the reason swallowed by `AppError::Internal`.
    #[tokio::test]
    async fn an_unknown_approval_token_is_not_found_rather_than_an_internal_error() {
        let use_cases = ApprovalUseCases::new(
            Arc::new(MockApprovalPersistence {
                approvals: Mutex::new(Vec::new()),
            }),
            Arc::new(MockTaskPersistence),
            Arc::new(MockThreadPersistence),
            test_config(),
        );

        let error = use_cases
            .process_link_action("no-such-token", "approve")
            .await
            .unwrap_err();

        assert!(matches!(error, AppError::NotFound(_)), "{error:?}");
        assert!(
            error.to_string().contains("confirmation link"),
            "the reader needs to know which link failed: {error}"
        );
    }

    #[tokio::test]
    async fn approval_lifecycle_confirm_and_reject_flow() {
        let approval_persistence = Arc::new(MockApprovalPersistence {
            approvals: Mutex::new(Vec::new()),
        });
        let task_persistence = Arc::new(MockTaskPersistence);
        let thread_persistence = Arc::new(MockThreadPersistence);
        let config = test_config();

        let use_cases = ApprovalUseCases::new(
            approval_persistence.clone(),
            task_persistence,
            thread_persistence,
            config,
        );

        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let thread_id = Uuid::new_v4();

        // 1. Create approval request
        let approval = use_cases
            .create_and_send_approval_request(
                company_id,
                channel_id,
                "Support Channel",
                &ChannelSlug::from("support"),
                &CompanySlug::from("acme"),
                Some(thread_id),
                None,
                "step_key_hash_123",
                &EmailAddress::from("manager@acme.com"),
                "tool",
                "Tool Execution: command",
                "Execute deploy command",
                serde_json::json!({}),
            )
            .await
            .unwrap();

        assert_eq!(approval.status, ApprovalStatus::Pending);

        let duplicate = use_cases
            .create_and_send_approval_request(
                company_id,
                channel_id,
                "Support Channel",
                &ChannelSlug::from("support"),
                &CompanySlug::from("acme"),
                Some(thread_id),
                None,
                "step_key_hash_123",
                &EmailAddress::from("manager@acme.com"),
                "tool",
                "Tool Execution: command",
                "Execute deploy command",
                serde_json::json!({}),
            )
            .await
            .unwrap();
        assert_eq!(duplicate.id, approval.id);
        assert_eq!(approval_persistence.approvals.lock().unwrap().len(), 1);

        // Check step approval before link click -> Pending
        let check_res = use_cases
            .check_step_approval(company_id, channel_id, Some(thread_id), "step_key_hash_123")
            .await
            .unwrap();
        assert_eq!(check_res, Some(ApprovalStatus::Pending));

        // 2. Concurrent clicks consume the token once.
        let (first, second) = tokio::join!(
            use_cases.process_link_action(&approval.token, "confirm"),
            use_cases.process_link_action(&approval.token, "confirm")
        );
        let results = [first.unwrap(), second.unwrap()];
        assert_eq!(
            results
                .iter()
                .filter(|(_, msg)| msg.contains("CONFIRMED successfully"))
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|(_, msg)| msg.contains("already processed as 'approved'"))
                .count(),
            1
        );
        assert!(
            results
                .iter()
                .all(|(approval, _)| approval.status == ApprovalStatus::Approved)
        );

        // Check step approval after link click -> Approved
        let check_res2 = use_cases
            .check_step_approval(company_id, channel_id, Some(thread_id), "step_key_hash_123")
            .await
            .unwrap();
        assert_eq!(check_res2, Some(ApprovalStatus::Approved));

        // 3. Later clicks remain idempotent.
        let (updated_again, msg_again) = use_cases
            .process_link_action(&approval.token, "confirm")
            .await
            .unwrap();
        assert_eq!(updated_again.status, ApprovalStatus::Approved);
        assert!(msg_again.contains("already processed as 'approved'"));
    }

    #[tokio::test]
    async fn test_simulated_server_restart_with_agent_approval_handler() {
        use crate::services::agent_runner::{
            AgentApprovalHandler, ApprovalContext as AgentApprovalContext,
        };
        use ai_agents::hitl::{ApprovalHandler, ApprovalRequest, ApprovalTrigger};

        // Shared persistent database mock across server instances
        let shared_db = Arc::new(MockApprovalPersistence {
            approvals: Mutex::new(Vec::new()),
        });
        let task_persistence = Arc::new(MockTaskPersistence);
        let thread_persistence = Arc::new(MockThreadPersistence);
        let config = test_config();

        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let thread_id = Uuid::new_v4();

        // --- SERVER INSTANCE 1 ---
        let server1_approval_use_cases = Arc::new(ApprovalUseCases::new(
            shared_db.clone(),
            task_persistence.clone(),
            thread_persistence.clone(),
            config.clone(),
        ));

        let ctx1 = AgentApprovalContext {
            company_id,
            channel_id,
            channel_name: "Deploy Agent".into(),
            channel_slug: "deploy".into(),
            company_slug: "acme".into(),
            thread_id: Some(thread_id),
            task_id: None,
            approver_email: "devops@acme.com".into(),
        };

        let handler1 = AgentApprovalHandler {
            approval_use_cases: server1_approval_use_cases,
            context: ctx1,
            suspended: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };

        // Tool trigger attempt on Server 1
        let req1 = ApprovalRequest::new(
            ApprovalTrigger::tool("command", serde_json::json!({"cmd": "deploy_prod"})),
            "Execute deployment script",
        );

        // Server 1 handles request -> No prior approval -> Pauses task and creates DB token
        let res1 = handler1.request_approval(req1).await;
        assert!(res1.is_rejected()); // Turn paused

        // Verify pending record created in shared DB
        let db_list1 = shared_db.approvals.lock().unwrap();
        assert_eq!(db_list1.len(), 1);
        assert_eq!(db_list1[0].status, ApprovalStatus::Pending);
        let token = db_list1[0].token.clone();
        drop(db_list1);

        // =========================================================================
        // SIMULATED SERVER CRASH / RESTART: Server 1 is completely dropped!
        // =========================================================================
        drop(handler1);

        // --- SERVER INSTANCE 2 (After Restart) ---
        let server2_approval_use_cases = Arc::new(ApprovalUseCases::new(
            shared_db.clone(),
            task_persistence.clone(),
            thread_persistence.clone(),
            config.clone(),
        ));

        // 1. User clicks confirmation link on Server 2
        let (processed_approval, result_msg) = server2_approval_use_cases
            .process_link_action(&token, "confirm")
            .await
            .unwrap();

        assert_eq!(processed_approval.status, ApprovalStatus::Approved);
        assert!(result_msg.contains("CONFIRMED successfully"));

        // 2. Server 2 re-runs Agent task after restart
        let ctx2 = AgentApprovalContext {
            company_id,
            channel_id,
            channel_name: "Deploy Agent".into(),
            channel_slug: "deploy".into(),
            company_slug: "acme".into(),
            thread_id: Some(thread_id),
            task_id: None,
            approver_email: "devops@acme.com".into(),
        };

        let handler2 = AgentApprovalHandler {
            approval_use_cases: server2_approval_use_cases,
            context: ctx2,
            suspended: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };

        let req2 = ApprovalRequest::new(
            ApprovalTrigger::tool("command", serde_json::json!({"cmd": "deploy_prod"})),
            "Execute deployment script",
        );

        // Server 2 handles same trigger -> Checks DB -> Auto-passes as Approved!
        let res2 = handler2.request_approval(req2).await;
        assert!(res2.is_approved()); // Auto-passed! No duplicate email!

        // Ensure no extra approval rows were created in DB
        let db_list2 = shared_db.approvals.lock().unwrap();
        assert_eq!(db_list2.len(), 1);
        assert_eq!(db_list2[0].status, ApprovalStatus::Approved);
    }

    #[tokio::test]
    async fn test_quorum_timeout_hitl_link_actions() {
        let approval_persistence = Arc::new(MockApprovalPersistence {
            approvals: Mutex::new(Vec::new()),
        });
        let task_persistence = Arc::new(MockTaskPersistence);
        let thread_persistence = Arc::new(MockThreadPersistence);
        let config = test_config();

        let use_cases = ApprovalUseCases::new(
            approval_persistence,
            task_persistence,
            thread_persistence,
            config,
        );

        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let thread_id = Uuid::new_v4();

        // Create approval request for partial quorum timeout
        let approval = use_cases
            .create_and_send_approval_request(
                company_id,
                channel_id,
                "Support",
                &ChannelSlug::from("support"),
                &CompanySlug::from("acme"),
                Some(thread_id),
                None,
                "step_quorum_timeout_123",
                &EmailAddress::from("manager@acme.com"),
                "quorum_timeout",
                "Partial Quorum Timeout: Action Required",
                "Received 1/4 responses (25.0%). Required: 50.0%.",
                serde_json::json!({ "threshold_percent": 50.0 }),
            )
            .await
            .unwrap();

        // 1. Test "proceed_partial" action
        let (updated, msg) = use_cases
            .process_link_action(&approval.token, "proceed_partial")
            .await
            .unwrap();
        assert_eq!(updated.status, ApprovalStatus::Approved);
        assert!(msg.contains("Proceeding with partial data"));

        // 2. Create another approval and test "extend_24h" action
        let approval2 = use_cases
            .create_and_send_approval_request(
                company_id,
                channel_id,
                "Support",
                &ChannelSlug::from("support"),
                &CompanySlug::from("acme"),
                Some(thread_id),
                None,
                "step_quorum_timeout_456",
                &EmailAddress::from("manager@acme.com"),
                "quorum_timeout",
                "Partial Quorum Timeout: Action Required",
                "Received 1/4 responses (25.0%). Required: 50.0%.",
                serde_json::json!({ "threshold_percent": 50.0 }),
            )
            .await
            .unwrap();

        let (updated2, msg2) = use_cases
            .process_link_action(&approval2.token, "extend_24h")
            .await
            .unwrap();
        assert_eq!(updated2.status, ApprovalStatus::Approved);
        assert!(msg2.contains("extended by 24 hours"));
    }
}
