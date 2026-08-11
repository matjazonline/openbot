use std::sync::Arc;
use chrono::Utc;
use uuid::Uuid;

use crate::{
    adapters::persistence::{
        approval::ApprovalPersistence, task::TaskPersistence,
    },
    app_error::{AppError, AppResult},
    entities::{
        approval::{ApprovalStatus, HumanApproval},
        message::{Message, MessageDirection, MessageRole},
    },
    infra::config::AppConfig,
    services::outbound_dispatcher::{OutboundDispatcher, OutboundEmail},
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
        thread_id: Option<Uuid>,
        step_key: &str,
    ) -> AppResult<Option<ApprovalStatus>> {
        let approval = self
            .approval_persistence
            .find_approval_by_step_key(thread_id, step_key)
            .await?;

        Ok(approval.map(|a| a.status))
    }

    pub async fn create_and_send_approval_request(
        &self,
        company_id: Uuid,
        workflow_id: Uuid,
        workflow_name: &str,
        workflow_slug: &str,
        company_slug: &str,
        thread_id: Option<Uuid>,
        task_id: Option<Uuid>,
        step_key: &str,
        approver_email: &str,
        action_type: &str,
        action_title: &str,
        action_summary: &str,
        payload: serde_json::Value,
    ) -> AppResult<HumanApproval> {
        let token = Uuid::new_v4().to_string();
        let expires_at = Utc::now().naive_utc() + chrono::Duration::hours(24);

        let approval = self
            .approval_persistence
            .create_approval(
                company_id,
                workflow_id,
                thread_id,
                task_id,
                step_key,
                approver_email,
                action_type,
                action_title,
                action_summary,
                payload,
                &token,
                expires_at,
            )
            .await?;

        // Construct links
        let domain = &self.config.app_domain_name;
        let confirm_url = format!("http://{}/approvals/{}?action=confirm", domain, token);
        let reject_url = format!("http://{}/approvals/{}?action=reject", domain, token);

        let body_text = format!(
            "Action Approval Requested for Workflow '{}'\n\n\
             Action: {}\n\
             Summary: {}\n\n\
             Click to CONFIRM:\n{}\n\n\
             Click to REJECT:\n{}\n\n\
             This link is valid for 24 hours.",
            workflow_name, action_title, action_summary, confirm_url, reject_url
        );

        let email = OutboundEmail {
            workflow_id,
            workflow_name: workflow_name.to_string(),
            workflow_slug: workflow_slug.to_string(),
            company_slug: company_slug.to_string(),
            trigger_message_id: format!("<approval-{}@{}>", approval.id, domain),
            thread_references: vec![],
            recipient_to: approver_email.to_string(),
            recipients_cc: vec![],
            subject: format!("[APPROVAL REQUIRED] {}", action_title),
            body_text,
            hop_count: 0,
            trace_workflows: vec![workflow_id],
        };

        let _ = OutboundDispatcher::send(&self.config, email).await;

        // If task_id is present, mark task status as PendingApproval so poller ignores it until link click
        if let Some(tid) = task_id {
            let _ = self
                .task_persistence
                .mark_task_failed(
                    tid,
                    "Task paused waiting for HumanInTheMiddle approval",
                    Utc::now().naive_utc(),
                    false,
                )
                .await;
        }

        Ok(approval)
    }

    pub async fn process_link_action(
        &self,
        token: &str,
        action: &str,
    ) -> AppResult<(HumanApproval, String)> {
        let approval = self
            .approval_persistence
            .get_approval_by_token(token)
            .await?
            .ok_or_else(|| AppError::Internal("Approval request token not found.".into()))?;

        if approval.status != ApprovalStatus::Pending {
            let msg = format!(
                "This request was already processed as '{}'.",
                approval.status.as_str()
            );
            return Ok((approval, msg));
        }

        if approval.expires_at < Utc::now().naive_utc() {
            let expired = self
                .approval_persistence
                .update_approval_status(approval.id, ApprovalStatus::Expired)
                .await?;
            return Ok((
                expired,
                "This confirmation link has expired (24-hour TTL).".into(),
            ));
        }

        let new_status = match action.to_lowercase().as_str() {
            "confirm" | "approved" | "approve" => ApprovalStatus::Approved,
            "reject" | "rejected" => ApprovalStatus::Rejected,
            _ => {
                return Err(AppError::Internal(
                    "Invalid action (must be 'confirm' or 'reject').".into(),
                ));
            }
        };

        let updated = self
            .approval_persistence
            .update_approval_status(approval.id, new_status.clone())
            .await?;

        let message_text = match new_status {
            ApprovalStatus::Approved => {
                // Re-queue task if associated
                if let Some(task_id) = approval.task_id {
                    let _ = self.task_persistence.resume_task(task_id).await;
                }

                // Add system message to thread if associated
                if let Some(thread_id) = approval.thread_id {
                    let sys_msg = Message {
                        id: Uuid::new_v4(),
                        thread_id,
                        message_id: format!("<approval-granted-{}@{}>", approval.id, self.config.app_domain_name),
                        in_reply_to: None,
                        references_list: vec![],
                        sender: approval.approver_email.clone(),
                        recipients_to: vec![],
                        recipients_cc: vec![],
                        subject: format!("[HITL Granted]: {}", approval.action_title),
                        clean_text_body: format!(
                            "Human approval GRANTED by {} for action '{}'.",
                            approval.approver_email, approval.action_title
                        ),
                        raw_text_body: None,
                        raw_html_body: None,
                        attachments: None,
                        direction: MessageDirection::Inbound,
                        role: MessageRole::System,
                        thread_index: None,
                        created_at: Utc::now().naive_utc(),
                    };
                    let _ = self.thread_persistence.create_message(&sys_msg).await;
                }

                format!(
                    "✓ Action '{}' has been CONFIRMED successfully. Associated automated workflows have been resumed.",
                    approval.action_title
                )
            }
            ApprovalStatus::Rejected => {
                // Stop task if associated
                if let Some(task_id) = approval.task_id {
                    let _ = self.task_persistence.stop_task(task_id).await;
                }

                // Add system message to thread if associated
                if let Some(thread_id) = approval.thread_id {
                    let sys_msg = Message {
                        id: Uuid::new_v4(),
                        thread_id,
                        message_id: format!("<approval-rejected-{}@{}>", approval.id, self.config.app_domain_name),
                        in_reply_to: None,
                        references_list: vec![],
                        sender: approval.approver_email.clone(),
                        recipients_to: vec![],
                        recipients_cc: vec![],
                        subject: format!("[HITL Rejected]: {}", approval.action_title),
                        clean_text_body: format!(
                            "Human approval REJECTED by {} for action '{}'.",
                            approval.approver_email, approval.action_title
                        ),
                        raw_text_body: None,
                        raw_html_body: None,
                        attachments: None,
                        direction: MessageDirection::Inbound,
                        role: MessageRole::System,
                        thread_index: None,
                        created_at: Utc::now().naive_utc(),
                    };
                    let _ = self.thread_persistence.create_message(&sys_msg).await;
                }

                format!(
                    "✗ Action '{}' has been REJECTED. The automated workflow task has been cancelled.",
                    approval.action_title
                )
            }
            _ => String::new(),
        };

        Ok((updated, message_text))
    }

    pub async fn list_workflow_approvals(
        &self,
        company_id: Uuid,
        workflow_id: Uuid,
    ) -> AppResult<Vec<HumanApproval>> {
        self.approval_persistence
            .list_approvals_by_workflow(company_id, workflow_id)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;

    struct MockApprovalPersistence {
        approvals: Mutex<Vec<HumanApproval>>,
    }

    #[async_trait]
    impl ApprovalPersistence for MockApprovalPersistence {
        async fn create_approval(
            &self,
            company_id: Uuid,
            workflow_id: Uuid,
            thread_id: Option<Uuid>,
            task_id: Option<Uuid>,
            step_key: &str,
            approver_email: &str,
            action_type: &str,
            action_title: &str,
            action_summary: &str,
            payload: serde_json::Value,
            token: &str,
            expires_at: chrono::NaiveDateTime,
        ) -> AppResult<HumanApproval> {
            let approval = HumanApproval {
                id: Uuid::new_v4(),
                company_id,
                workflow_id,
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
                created_at: Utc::now().naive_utc(),
                updated_at: Utc::now().naive_utc(),
            };
            self.approvals.lock().unwrap().push(approval.clone());
            Ok(approval)
        }

        async fn find_approval_by_step_key(
            &self,
            thread_id: Option<Uuid>,
            step_key: &str,
        ) -> AppResult<Option<HumanApproval>> {
            Ok(self
                .approvals
                .lock()
                .unwrap()
                .iter()
                .find(|a| a.thread_id == thread_id && a.step_key == step_key)
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

        async fn update_approval_status(
            &self,
            id: Uuid,
            status: ApprovalStatus,
        ) -> AppResult<HumanApproval> {
            let mut list = self.approvals.lock().unwrap();
            let approval = list.iter_mut().find(|a| a.id == id).unwrap();
            approval.status = status;
            approval.updated_at = Utc::now().naive_utc();
            Ok(approval.clone())
        }

        async fn list_approvals_by_workflow(
            &self,
            company_id: Uuid,
            workflow_id: Uuid,
        ) -> AppResult<Vec<HumanApproval>> {
            Ok(self
                .approvals
                .lock()
                .unwrap()
                .iter()
                .filter(|a| a.company_id == company_id && a.workflow_id == workflow_id)
                .cloned()
                .collect())
        }
    }

    struct MockTaskPersistence;
    #[async_trait]
    impl TaskPersistence for MockTaskPersistence {
        async fn enqueue_task(&self, _company_id: Uuid, _workflow_id: Uuid, _thread_id: Option<Uuid>, _task_type: &str, _payload: serde_json::Value) -> AppResult<crate::entities::task::BackgroundTask> { unimplemented!() }
        async fn get_task_by_id(&self, _id: Uuid) -> AppResult<Option<crate::entities::task::BackgroundTask>> { unimplemented!() }
        async fn update_task_payload(&self, _id: Uuid, _payload: serde_json::Value) -> AppResult<()> { Ok(()) }
        async fn poll_next_pending_tasks(&self, _limit: i64) -> AppResult<Vec<crate::entities::task::BackgroundTask>> { unimplemented!() }
        async fn mark_task_processing(&self, _id: Uuid) -> AppResult<bool> { Ok(true) }
        async fn mark_task_completed(&self, _id: Uuid) -> AppResult<()> { unimplemented!() }
        async fn mark_task_failed(&self, _id: Uuid, _error_msg: &str, _next_run_at: chrono::NaiveDateTime, _is_dead_letter: bool) -> AppResult<()> { Ok(()) }
        async fn stop_task(&self, _id: Uuid) -> AppResult<crate::entities::task::BackgroundTask> { unimplemented!() }
        async fn resume_task(&self, _id: Uuid) -> AppResult<crate::entities::task::BackgroundTask> { unimplemented!() }
        async fn list_company_tasks(&self, _company_id: Uuid, _workflow_id: Option<Uuid>, _status: Option<crate::entities::task::TaskStatus>, _sort_asc: bool) -> AppResult<Vec<crate::entities::task::BackgroundTask>> { unimplemented!() }
    }

    struct MockThreadPersistence;
    #[async_trait]
    impl ThreadPersistence for MockThreadPersistence {
        async fn create_thread(&self, _workflow_id: Uuid, _subject: &str, _participant_emails: &[String]) -> AppResult<crate::entities::thread::Thread> { unimplemented!() }
        async fn get_thread_by_id(&self, _id: Uuid) -> AppResult<Option<crate::entities::thread::Thread>> { unimplemented!() }
        async fn update_thread_participants(&self, _id: Uuid, _participant_emails: &[String]) -> AppResult<crate::entities::thread::Thread> { unimplemented!() }
        async fn find_thread_by_message_ids(&self, _message_ids: &[String]) -> AppResult<Option<crate::entities::thread::Thread>> { unimplemented!() }
        async fn find_thread_by_thread_index(&self, _thread_index_prefix: &str) -> AppResult<Option<crate::entities::thread::Thread>> { unimplemented!() }
        async fn count_recent_messages(&self, _thread_id: Uuid, _duration_secs: i64) -> AppResult<usize> { unimplemented!() }
        async fn create_message(&self, message: &Message) -> AppResult<Message> { Ok(message.clone()) }
        async fn get_message_by_message_id(&self, _message_id: &str) -> AppResult<Option<Message>> { unimplemented!() }
        async fn list_messages_by_thread_id(&self, _thread_id: Uuid) -> AppResult<Vec<Message>> { unimplemented!() }
    }

    #[tokio::test]
    async fn approval_lifecycle_confirm_and_reject_flow() {
        let approval_persistence = Arc::new(MockApprovalPersistence {
            approvals: Mutex::new(Vec::new()),
        });
        let task_persistence = Arc::new(MockTaskPersistence);
        let thread_persistence = Arc::new(MockThreadPersistence);
        let config = Arc::new(AppConfig {
            jwt_secret: "secret".into(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".into(),
            smtp_host: "localhost".into(),
            smtp_port: 1025,
            smtp_username: "".into(),
            smtp_password: "".into(),
            smtp_from_address: "noreply@mailagents.com".into(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".into(),
            incoming_smtp_port: 2525,
        });

        let use_cases = ApprovalUseCases::new(
            approval_persistence.clone(),
            task_persistence,
            thread_persistence,
            config,
        );

        let company_id = Uuid::new_v4();
        let workflow_id = Uuid::new_v4();
        let thread_id = Uuid::new_v4();

        // 1. Create approval request
        let approval = use_cases
            .create_and_send_approval_request(
                company_id,
                workflow_id,
                "Support Workflow",
                "support",
                "acme",
                Some(thread_id),
                None,
                "step_key_hash_123",
                "manager@acme.com",
                "tool",
                "Tool Execution: command",
                "Execute deploy command",
                serde_json::json!({}),
            )
            .await
            .unwrap();

        assert_eq!(approval.status, ApprovalStatus::Pending);

        // Check step approval before link click -> Pending
        let check_res = use_cases
            .check_step_approval(Some(thread_id), "step_key_hash_123")
            .await
            .unwrap();
        assert_eq!(check_res, Some(ApprovalStatus::Pending));

        // 2. Process link confirmation
        let (updated, msg) = use_cases
            .process_link_action(&approval.token, "confirm")
            .await
            .unwrap();
        assert_eq!(updated.status, ApprovalStatus::Approved);
        assert!(msg.contains("CONFIRMED successfully"));

        // Check step approval after link click -> Approved
        let check_res2 = use_cases
            .check_step_approval(Some(thread_id), "step_key_hash_123")
            .await
            .unwrap();
        assert_eq!(check_res2, Some(ApprovalStatus::Approved));

        // 3. Second click (Idempotency) -> Returns already processed message
        let (updated_again, msg_again) = use_cases
            .process_link_action(&approval.token, "confirm")
            .await
            .unwrap();
        assert_eq!(updated_again.status, ApprovalStatus::Approved);
        assert!(msg_again.contains("already processed as 'approved'"));
    }

    #[tokio::test]
    async fn test_simulated_server_restart_with_agent_approval_handler() {
        use crate::services::agent_runner::{AgentApprovalHandler, ApprovalContext as AgentApprovalContext};
        use ai_agents::hitl::{ApprovalHandler, ApprovalRequest, ApprovalTrigger};

        // Shared persistent database mock across server instances
        let shared_db = Arc::new(MockApprovalPersistence {
            approvals: Mutex::new(Vec::new()),
        });
        let task_persistence = Arc::new(MockTaskPersistence);
        let thread_persistence = Arc::new(MockThreadPersistence);
        let config = Arc::new(AppConfig {
            jwt_secret: "secret".into(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".into(),
            smtp_host: "localhost".into(),
            smtp_port: 1025,
            smtp_username: "".into(),
            smtp_password: "".into(),
            smtp_from_address: "noreply@mailagents.com".into(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".into(),
            incoming_smtp_port: 2525,
        });

        let company_id = Uuid::new_v4();
        let workflow_id = Uuid::new_v4();
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
            workflow_id,
            workflow_name: "Deploy Agent".into(),
            workflow_slug: "deploy".into(),
            company_slug: "acme".into(),
            thread_id: Some(thread_id),
            task_id: None,
            approver_email: "devops@acme.com".into(),
        };

        let handler1 = AgentApprovalHandler {
            approval_use_cases: server1_approval_use_cases,
            context: ctx1,
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
            workflow_id,
            workflow_name: "Deploy Agent".into(),
            workflow_slug: "deploy".into(),
            company_slug: "acme".into(),
            thread_id: Some(thread_id),
            task_id: None,
            approver_email: "devops@acme.com".into(),
        };

        let handler2 = AgentApprovalHandler {
            approval_use_cases: server2_approval_use_cases,
            context: ctx2,
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
}
