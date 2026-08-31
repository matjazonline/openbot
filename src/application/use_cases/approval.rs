use chrono::Utc;
use std::sync::Arc;
use tracing::{error, info};
use uuid::Uuid;

use crate::{
    adapters::persistence::{
        approval::{ApprovalPersistence, NewApproval},
        task::TaskPersistence,
    },
    app_error::{AppError, AppResult},
    entities::{
        approval::{
            ApprovalAction, ApprovalStatus, ApprovalSubject, HumanApproval, QUORUM_TIMEOUT_ACTION,
            QuorumTimeoutAction,
        },
        message::{Message, MessageDirection, MessageRole},
        task::{ResumeActor, StopActor},
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
        subject: &ApprovalSubject,
        action: ApprovalAction,
    ) -> AppResult<HumanApproval> {
        let token = Uuid::new_v4();
        let expires_at = Utc::now() + chrono::Duration::hours(24);
        let domain = &self.config.app_domain_name;
        let link = |decision: &str| format!("http://{domain}/approvals/{token}?action={decision}");

        let body_text = if action.is_quorum_timeout() {
            format!(
                "Outreach Timeout Decision for Channel '{}'\n\nAction: {}\nSummary: {}\n\nProceed with available responses:\n{}\n\nExtend 24 hours:\n{}\n\nExtend 48 hours:\n{}\n\nReject and stop the task:\n{}\n\nThis link is valid for 24 hours.",
                subject.channel_name,
                action.title,
                action.summary,
                link("proceed_partial"),
                link("extend_24h"),
                link("extend_48h"),
                link("reject"),
            )
        } else {
            format!(
                "Action Approval Requested for Channel '{}'\n\nAction: {}\nSummary: {}\n\nClick to CONFIRM:\n{}\n\nClick to REJECT:\n{}\n\nThis link is valid for 24 hours.",
                subject.channel_name,
                action.title,
                action.summary,
                link("confirm"),
                link("reject"),
            )
        };

        let notification = serde_json::to_value(OutboundEmail {
            channel_id: subject.channel_id,
            channel_name: subject.channel_name.clone(),
            channel_slug: subject.channel_slug.clone(),
            company_slug: subject.company_slug.clone(),
            trigger_message_id: format!("<approval-{token}@{domain}>").into(),
            thread_references: vec![],
            recipient_to: subject.approver_email.clone(),
            recipients_cc: vec![],
            subject: format!("[APPROVAL REQUIRED] {}", action.title),
            body_text,
            hop_count: 0,
            trace_channels: vec![subject.channel_id],
            correlation_id: subject.correlation_id,
        })
        .map_err(|error| {
            AppError::Internal(format!(
                "Failed to serialize approval notification: {error}"
            ))
        })?;

        let (approval, created) = self
            .approval_persistence
            .create_approval(NewApproval {
                subject,
                action: &action,
                notification,
                token,
                expires_at,
            })
            .await?;

        if created {
            info!(
                approval_id = %approval.id,
                correlation_id = %subject.correlation_id,
                channel_id = %subject.channel_id,
                action_type = %action.action_type,
                "Queued an approval request for a human"
            );
        } else {
            info!(
                approval_id = %approval.id,
                correlation_id = %subject.correlation_id,
                step_key = %action.step_key,
                "Reused the approval already standing for this step"
            );
        }

        Ok(approval)
    }

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
        // The verb comes off a link someone clicked, so an unrecognised one is bad input rather
        // than a fault on this side. It is also the last place a string is inspected: everything
        // below matches on the parsed value.
        let decision =
            LinkAction::parse(&normalized_action, &approval.action_type).ok_or_else(|| {
                AppError::BadRequest(format!(
                    "Action '{}' is not valid for approval type '{}'.",
                    action, approval.action_type
                ))
            })?;

        let now = Utc::now();
        // Quorum timeouts carry the chosen option through to the paused task; everything else is a
        // plain approve/reject state transition.
        let consumed = match decision {
            LinkAction::QuorumTimeout(quorum_action) if approval.task_id.is_some() => {
                self.approval_persistence
                    .consume_quorum_timeout_action(token, quorum_action, now)
                    .await?
            }
            _ => {
                self.approval_persistence
                    .consume_pending_approval(token, decision.status(), now)
                    .await?
            }
        };
        // Losing the race to consume means someone (or the expiry sweep) got there first.
        let Some(updated) = consumed else {
            return self.report_unavailable(token, now).await;
        };

        let message_text = self.apply_decision(&approval, decision).await?;
        Ok((updated, message_text))
    }

    /// Carry out the side effects of a decision and describe them for the human who clicked.
    ///
    /// The approval row is already consumed by the time this runs, so a failed task transition
    /// cannot be undone by rolling anything back -- but it must not be reported as success either.
    /// It propagates with both ids logged, which is what a reconciliation starts from.
    async fn apply_decision(
        &self,
        approval: &HumanApproval,
        decision: LinkAction,
    ) -> AppResult<String> {
        match decision {
            LinkAction::QuorumTimeout(QuorumTimeoutAction::ProceedPartial) => {
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
                Ok(format!(
                    "✓ Action '{}' confirmed: Proceeding with partial data. Channels resumed.",
                    approval.action_title
                ))
            }
            LinkAction::QuorumTimeout(QuorumTimeoutAction::Extend { hours }) => {
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
                Ok(format!(
                    "✓ Action '{}' confirmed: Outreach timeout extended by {} hours.",
                    approval.action_title, hours
                ))
            }
            LinkAction::Approve => {
                if let Some(task_id) = approval.task_id {
                    self.task_persistence
                        .resume_task(task_id, ResumeActor::Approval(approval.id))
                        .await
                        .inspect_err(|error| {
                            error!(
                                approval_id = %approval.id,
                                task_id = %task_id,
                                %error,
                                "Approval was consumed but its task could not be resumed"
                            )
                        })?;
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
                Ok(format!(
                    "✓ Action '{}' has been CONFIRMED successfully. Associated automated channels have been resumed.",
                    approval.action_title
                ))
            }
            LinkAction::QuorumTimeout(QuorumTimeoutAction::Reject) => {
                // The transaction that consumed this approval also cancelled the outreach and
                // stopped the task, so there is no transition left to order here. The old
                // `action_type != "quorum_timeout"` guard on the shared arm below said the same
                // thing by re-reading a string; separate arms say it by construction.
                Ok(self.record_rejection(approval).await)
            }
            LinkAction::Reject => {
                if let Some(task_id) = approval.task_id {
                    self.task_persistence
                        .stop_task(task_id, StopActor::Approval(approval.id))
                        .await
                        .inspect_err(|error| {
                            error!(
                                approval_id = %approval.id,
                                task_id = %task_id,
                                %error,
                                "Approval was consumed but its task could not be stopped"
                            )
                        })?;
                }
                Ok(self.record_rejection(approval).await)
            }
        }
    }

    /// Record a rejection on the originating thread and describe it for the human who clicked.
    ///
    /// Both rejection paths end here; they differ only in who stopped the task.
    async fn record_rejection(&self, approval: &HumanApproval) -> String {
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
///
/// The two kinds of approval offer disjoint verbs, and this is where that stops being a runtime
/// question: a quorum timeout carries [`QuorumTimeoutAction`], and everything else is the plain
/// confirm/reject pair. Nothing downstream re-reads the query string to work out which it got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkAction {
    /// A quorum timeout's own decision, parsed once and passed on as a value.
    QuorumTimeout(QuorumTimeoutAction),
    Approve,
    Reject,
}

impl LinkAction {
    /// Parse the query-string action, rejecting verbs that don't belong to this approval type.
    fn parse(normalized_action: &str, approval_action_type: &str) -> Option<Self> {
        if approval_action_type == QUORUM_TIMEOUT_ACTION {
            return normalized_action.parse().ok().map(Self::QuorumTimeout);
        }
        match normalized_action {
            "confirm" | "approved" | "approve" => Some(Self::Approve),
            "reject" | "rejected" => Some(Self::Reject),
            _ => None,
        }
    }

    fn status(self) -> ApprovalStatus {
        match self {
            Self::Reject | Self::QuorumTimeout(QuorumTimeoutAction::Reject) => {
                ApprovalStatus::Rejected
            }
            Self::Approve | Self::QuorumTimeout(_) => ApprovalStatus::Approved,
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

    /// The two approval kinds offer disjoint verbs, and a verb borrowed from the other kind is
    /// refused rather than reinterpreted. `reject` is the only one both accept, and it resolves to
    /// a different decision on each side: a quorum rejection is settled inside the transaction
    /// that consumes the approval, a plain one still has to stop its task.
    #[test]
    fn link_verbs_are_accepted_only_for_the_approval_kind_that_offers_them() {
        use LinkAction::{Approve, QuorumTimeout, Reject};
        use QuorumTimeoutAction::{Extend, ProceedPartial};

        let quorum = |verb| LinkAction::parse(verb, QUORUM_TIMEOUT_ACTION);
        assert_eq!(
            quorum("proceed_partial"),
            Some(QuorumTimeout(ProceedPartial))
        );
        assert_eq!(
            quorum("extend_48h"),
            Some(QuorumTimeout(Extend { hours: 48 }))
        );
        assert_eq!(
            quorum("extend_24h"),
            Some(QuorumTimeout(Extend { hours: 24 }))
        );
        assert_eq!(quorum("extend"), Some(QuorumTimeout(Extend { hours: 24 })));
        assert_eq!(
            quorum("reject"),
            Some(QuorumTimeout(QuorumTimeoutAction::Reject))
        );
        for borrowed in ["confirm", "approve", "approved", "rejected"] {
            assert_eq!(quorum(borrowed), None, "verb {borrowed}");
        }

        let plain = |verb| LinkAction::parse(verb, "tool");
        for accepted in ["confirm", "approve", "approved"] {
            assert_eq!(plain(accepted), Some(Approve), "verb {accepted}");
        }
        for accepted in ["reject", "rejected"] {
            assert_eq!(plain(accepted), Some(Reject), "verb {accepted}");
        }
        for borrowed in ["proceed_partial", "extend_24h", "extend"] {
            assert_eq!(plain(borrowed), None, "verb {borrowed}");
        }
    }

    /// A quorum rejection consumes its approval as rejected like any other. It reaches
    /// `consume_quorum_timeout_action` rather than `consume_pending_approval`, but the status it
    /// records is not the thing that differs.
    #[test]
    fn a_rejection_records_the_rejected_status_whichever_kind_it_came_from() {
        assert_eq!(LinkAction::Reject.status(), ApprovalStatus::Rejected);
        assert_eq!(
            LinkAction::QuorumTimeout(QuorumTimeoutAction::Reject).status(),
            ApprovalStatus::Rejected
        );
        assert_eq!(LinkAction::Approve.status(), ApprovalStatus::Approved);
        assert_eq!(
            LinkAction::QuorumTimeout(QuorumTimeoutAction::ProceedPartial).status(),
            ApprovalStatus::Approved
        );
    }
    use crate::adapters::persistence::task::{AgentDispatchCommit, DispatchCommit};
    use crate::entities::correlation::CorrelationId;
    use crate::entities::task::NewTask;
    use crate::entities::task::{
        ResumeActor, StopActor, TaskFailure, TaskLeaseRef, TaskSuspension,
    };
    use crate::entities::value_objects::{ChannelSlug, CompanySlug, EmailAddress};
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
            sendgrid_inbound: None,
            hydradb: None,
            hindsight: None,
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

    struct MockApprovalPersistence {
        approvals: Mutex<Vec<HumanApproval>>,
    }

    #[async_trait]
    impl ApprovalPersistence for MockApprovalPersistence {
        async fn create_approval(
            &self,
            new_approval: NewApproval<'_>,
        ) -> AppResult<(HumanApproval, bool)> {
            let NewApproval {
                subject,
                action,
                token,
                expires_at,
                ..
            } = new_approval;
            let mut approvals = self.approvals.lock().unwrap();
            if let Some(existing) = approvals
                .iter()
                .find(|a| a.thread_id == subject.thread_id && a.step_key == action.step_key)
            {
                return Ok((existing.clone(), false));
            }

            let approval = HumanApproval {
                id: Uuid::new_v4(),
                company_id: subject.company_id,
                channel_id: subject.channel_id,
                thread_id: subject.thread_id,
                task_id: subject.suspension.map(TaskSuspension::task_id),
                step_key: action.step_key.clone(),
                approver_email: subject.approver_email.to_string(),
                action_type: action.action_type.clone(),
                action_title: action.title.clone(),
                action_summary: action.summary.clone(),
                payload: action.payload.clone(),
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
        async fn commit_agent_dispatch(
            &self,
            commit: AgentDispatchCommit<'_>,
        ) -> AppResult<DispatchCommit> {
            let _ = commit;
            Ok(DispatchCommit::Committed { outbox_id: None })
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
            _new_task: NewTask,
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
        async fn mark_task_completed(&self, _lease: TaskLeaseRef) -> AppResult<bool> {
            unimplemented!()
        }
        async fn mark_task_failed(&self, _failure: TaskFailure<'_>) -> AppResult<bool> {
            Ok(true)
        }
        async fn stop_task(
            &self,
            _id: Uuid,
            _actor: StopActor,
        ) -> AppResult<crate::entities::task::BackgroundTask> {
            unimplemented!()
        }
        async fn resume_task(
            &self,
            _id: Uuid,
            _actor: ResumeActor,
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
        // One subject, reused across every request this test raises -- which is the point of
        // splitting it out: only the action differs between them.
        let subject = ApprovalSubject {
            company_id,
            channel_id,
            channel_name: "Support Channel".to_string(),
            channel_slug: ChannelSlug::from("support"),
            company_slug: CompanySlug::from("acme"),
            thread_id: Some(thread_id),
            suspension: None,
            correlation_id: CorrelationId::new(),
            approver_email: EmailAddress::from("manager@acme.com"),
        };

        // 1. Create approval request
        let approval = use_cases
            .create_and_send_approval_request(
                &subject,
                ApprovalAction {
                    step_key: "step_key_hash_123".to_string(),
                    action_type: "tool".to_string(),
                    title: "Tool Execution: command".to_string(),
                    summary: "Execute deploy command".to_string(),
                    payload: serde_json::json!({}),
                },
            )
            .await
            .unwrap();

        assert_eq!(approval.status, ApprovalStatus::Pending);

        let duplicate = use_cases
            .create_and_send_approval_request(
                &subject,
                ApprovalAction {
                    step_key: "step_key_hash_123".to_string(),
                    action_type: "tool".to_string(),
                    title: "Tool Execution: command".to_string(),
                    summary: "Execute deploy command".to_string(),
                    payload: serde_json::json!({}),
                },
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
        use crate::services::agent_runner::AgentApprovalHandler;
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

        let ctx1 = ApprovalSubject {
            correlation_id: CorrelationId::new(),
            company_id,
            channel_id,
            channel_name: "Deploy Agent".into(),
            channel_slug: "deploy".into(),
            company_slug: "acme".into(),
            thread_id: Some(thread_id),
            suspension: None,
            approver_email: "devops@acme.com".into(),
        };

        let handler1 = AgentApprovalHandler {
            approval_use_cases: server1_approval_use_cases,
            context: ctx1,
            suspended: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            delegation: None,
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
        let token = {
            let db_list1 = shared_db.approvals.lock().unwrap();
            assert_eq!(db_list1.len(), 1);
            assert_eq!(db_list1[0].status, ApprovalStatus::Pending);
            db_list1[0].token.clone()
        };

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
        let ctx2 = ApprovalSubject {
            correlation_id: CorrelationId::new(),
            company_id,
            channel_id,
            channel_name: "Deploy Agent".into(),
            channel_slug: "deploy".into(),
            company_slug: "acme".into(),
            thread_id: Some(thread_id),
            suspension: None,
            approver_email: "devops@acme.com".into(),
        };

        let handler2 = AgentApprovalHandler {
            approval_use_cases: server2_approval_use_cases,
            context: ctx2,
            suspended: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            delegation: None,
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
        // One subject, two requests: only the step key and the decision differ.
        let subject = ApprovalSubject {
            company_id,
            channel_id,
            channel_name: "Support".to_string(),
            channel_slug: ChannelSlug::from("support"),
            company_slug: CompanySlug::from("acme"),
            thread_id: Some(thread_id),
            suspension: None,
            correlation_id: CorrelationId::new(),
            approver_email: EmailAddress::from("manager@acme.com"),
        };

        // Create approval request for partial quorum timeout
        let approval = use_cases
            .create_and_send_approval_request(
                &subject,
                ApprovalAction {
                    step_key: "step_quorum_timeout_123".to_string(),
                    action_type: "quorum_timeout".to_string(),
                    title: "Partial Quorum Timeout: Action Required".to_string(),
                    summary: "Received 1/4 responses (25.0%). Required: 50.0%.".to_string(),
                    payload: serde_json::json!({ "threshold_percent": 50.0 }),
                },
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
                &subject,
                ApprovalAction {
                    step_key: "step_quorum_timeout_456".to_string(),
                    action_type: "quorum_timeout".to_string(),
                    title: "Partial Quorum Timeout: Action Required".to_string(),
                    summary: "Received 1/4 responses (25.0%). Required: 50.0%.".to_string(),
                    payload: serde_json::json!({ "threshold_percent": 50.0 }),
                },
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
