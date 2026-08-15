use std::sync::Arc;

use axum::{
    Form, Json, Router,
    extract::{FromRequest, Multipart, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use serde::{Deserialize, Serialize};
use tracing::{instrument, warn};

use crate::{
    adapters::http::app_state::AppState,
    adapters::protocols::email::EmailIngressAdapter,
    services::email_parser::{RawAttachmentData, RawInboundPayload},
    use_cases::thread::ThreadUseCases,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/webhooks/email/sendgrid", post(sendgrid_inbound_webhook))
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SendGridPayload {
    pub to: Option<String>,
    pub from: Option<String>,
    pub cc: Option<String>,
    pub subject: Option<String>,
    pub text: Option<String>,
    pub html: Option<String>,
    pub headers: Option<String>,
    pub envelope: Option<String>,
    pub spf: Option<String>,
    pub dkim: Option<String>,
    pub spam_score: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
struct SendGridEnvelope {
    pub to: Option<Vec<String>>,
    pub from: Option<String>,
}

#[instrument(skip(thread_use_cases, headers))]
async fn sendgrid_inbound_webhook(
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    headers: HeaderMap,
    req: axum::extract::Request,
) -> Result<impl IntoResponse, StatusCode> {
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_lowercase();

    let mut raw_payload = RawInboundPayload::default();

    if content_type.contains("multipart/form-data") {
        if let Ok(mut multipart) = Multipart::from_request(req, &State(())).await {
            while let Ok(Some(field)) = multipart.next_field().await {
                let name = field.name().unwrap_or_default().to_string();
                let file_name = field.file_name().map(|f| f.to_string());
                let field_content_type = field.content_type().map(|c| c.to_string());

                if let Some(filename) = file_name {
                    if let Ok(bytes) = field.bytes().await {
                        raw_payload.attachments_data.push(RawAttachmentData {
                            filename,
                            content_type: field_content_type
                                .unwrap_or_else(|| "application/octet-stream".into()),
                            content: bytes.to_vec(),
                        });
                    }
                } else if let Ok(value) = field.text().await {
                    match name.as_str() {
                        "to" => {
                            if raw_payload.to.is_empty() {
                                raw_payload.to = value;
                            }
                        }
                        "from" => {
                            if raw_payload.from.is_empty() {
                                raw_payload.from = value;
                            }
                        }
                        "cc" => raw_payload.cc = Some(value),
                        "subject" => raw_payload.subject = Some(value),
                        "text" => raw_payload.text = Some(value),
                        "html" => raw_payload.html = Some(value),
                        "headers" => raw_payload.headers = Some(value),
                        "spf" => raw_payload.spf = Some(value),
                        "dkim" => raw_payload.dkim = Some(value),
                        "spam_score" => raw_payload.spam_score = value.parse().ok(),
                        "envelope" => {
                            if let Ok(env) = serde_json::from_str::<SendGridEnvelope>(&value) {
                                if let Some(recipients) = env.to {
                                    if let Some(first) = recipients.first() {
                                        raw_payload.to = first.clone();
                                    }
                                }
                                if let Some(sender) = env.from {
                                    raw_payload.from = sender;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    } else if content_type.contains("application/json") {
        if let Ok(Json(payload)) = Json::<SendGridPayload>::from_request(req, &State(())).await {
            extract_from_payload(payload, &mut raw_payload);
        }
    } else {
        if let Ok(Form(payload)) = Form::<SendGridPayload>::from_request(req, &State(())).await {
            extract_from_payload(payload, &mut raw_payload);
        }
    }

    // Synchronous Ingestion: Parse MIME into normalized message, resolve thread, verify ACL, and save inbound message
    let norm_payload = EmailIngressAdapter::parse(raw_payload, &thread_use_cases.config());
    let ingest = thread_use_cases
        .ingest_normalized_message(norm_payload)
        .await
        .map_err(|err| {
            warn!("Error ingesting inbound email: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if !ingest.accepted {
        let thread_use_cases_bg = thread_use_cases.clone();
        let ingest_bg = ingest.clone();
        tokio::spawn(async move {
            thread_use_cases_bg.handle_bounce_dispatch(&ingest_bg).await;
        });
    }

    let result = serde_json::json!({
        "processed": ingest.accepted,
        "reason": ingest.reason,
        "thread_id": ingest.thread.as_ref().map(|t| t.id),
        "inbound_message_id": ingest.inbound_message.as_ref().map(|m| m.message_id.clone()),
    });

    // Return HTTP 200 OK immediately to prevent SendGrid webhook timeouts
    Ok((StatusCode::OK, Json(result)))
}

fn extract_from_payload(payload: SendGridPayload, raw: &mut RawInboundPayload) {
    raw.subject = payload.subject;
    raw.text = payload.text;
    raw.html = payload.html;
    raw.headers = payload.headers;
    raw.cc = payload.cc;
    raw.spf = payload.spf;
    raw.dkim = payload.dkim;
    raw.spam_score = payload.spam_score;

    if let Some(ref env_str) = payload.envelope {
        if let Ok(env) = serde_json::from_str::<SendGridEnvelope>(env_str) {
            if let Some(recipients) = env.to {
                if let Some(first) = recipients.first() {
                    raw.to = first.clone();
                }
            }
            if let Some(sender) = env.from {
                raw.from = sender;
            }
        }
    }

    if raw.to.is_empty() {
        if let Some(t) = payload.to {
            raw.to = t;
        }
    }

    if raw.from.is_empty() {
        if let Some(f) = payload.from {
            raw.from = f;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::body::Body;
    use axum::http::Request;
    use chrono::Utc;
    use std::sync::Mutex;
    use tower::ServiceExt;
    use uuid::Uuid;

    use crate::{
        app_error::AppResult,
        entities::{channel::Channel, company::Company, message::Message, thread::Thread},
        infra::config::AppConfig,
        use_cases::{
            channel::{ChannelPersistence, ChannelUseCases},
            company::CompanyPersistence,
            company_invite::CompanyInvitePersistence,
            thread::ThreadPersistence,
            user::UserPersistence,
        },
    };

    struct MockCompanyPersistence {
        companies: Mutex<Vec<Company>>,
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
        async fn get_by_id(&self, _id: Uuid) -> AppResult<Option<Company>> {
            unimplemented!()
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
        async fn is_company_team_member(&self, _company_id: Uuid, _email: &str) -> AppResult<bool> {
            Ok(true)
        }
        async fn list_company_team_emails(&self, _company_id: Uuid) -> AppResult<Vec<String>> {
            Ok(vec![])
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
            _name: &str,
            _slug: &str,
            _api_key: Option<&str>,
            _provider: Option<&str>,
            _model: Option<&str>,
            _participant_emails: Option<Vec<String>>,
            _agent_ids: Option<Vec<Uuid>>,
            _channel_config: Option<serde_json::Value>,
        ) -> AppResult<Channel> {
            unimplemented!()
        }
        async fn get_by_id(&self, _id: Uuid) -> AppResult<Option<Channel>> {
            unimplemented!()
        }
        async fn get_by_company_slug_and_channel_slug(
            &self,
            _company_slug: &str,
            channel_slug: &str,
        ) -> AppResult<Option<Channel>> {
            Ok(self
                .channels
                .lock()
                .unwrap()
                .iter()
                .find(|w| w.slug == channel_slug)
                .cloned())
        }
        async fn list_by_company_id(&self, _company_id: Uuid) -> AppResult<Vec<Channel>> {
            Ok(self.channels.lock().unwrap().clone())
        }
        async fn update(
            &self,
            _id: Uuid,
            _name: &str,
            _slug: &str,
            _api_key: Option<&str>,
            _provider: Option<&str>,
            _model: Option<&str>,
            _participant_emails: Option<Vec<String>>,
            _agent_ids: Option<Vec<Uuid>>,
            _channel_config: Option<serde_json::Value>,
        ) -> AppResult<Channel> {
            unimplemented!()
        }
        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }
    }

    struct MockAgentPersistence;

    #[async_trait]
    impl crate::use_cases::agent::AgentPersistence for MockAgentPersistence {
        async fn create(
            &self,
            _company_id: Uuid,
            _name: &str,
            _slug: &str,
            _provider: Option<&str>,
            _model: Option<&str>,
            _api_key: Option<&str>,
            _system_prompt: Option<&str>,
            _config_json: Option<serde_json::Value>,
        ) -> AppResult<crate::entities::agent::Agent> {
            unimplemented!()
        }
        async fn get_by_id(&self, _id: Uuid) -> AppResult<Option<crate::entities::agent::Agent>> {
            unimplemented!()
        }
        async fn get_by_company_slug_and_agent_slug(
            &self,
            _company_slug: &str,
            _agent_slug: &str,
        ) -> AppResult<Option<crate::entities::agent::Agent>> {
            unimplemented!()
        }
        async fn list_by_company_id(
            &self,
            _company_id: Uuid,
        ) -> AppResult<Vec<crate::entities::agent::Agent>> {
            Ok(vec![])
        }
        async fn update(
            &self,
            _id: Uuid,
            _name: &str,
            _slug: &str,
            _provider: Option<&str>,
            _model: Option<&str>,
            _api_key: Option<&str>,
            _system_prompt: Option<&str>,
            _config_json: Option<serde_json::Value>,
        ) -> AppResult<crate::entities::agent::Agent> {
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
            participant_emails: &[String],
        ) -> AppResult<Thread> {
            let thread = Thread {
                id: Uuid::new_v4(),
                channel_id,
                subject: subject.to_string(),
                participant_emails: participant_emails.to_vec(),
                created_at: Utc::now().naive_utc(),
                updated_at: Utc::now().naive_utc(),
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

        async fn update_thread_participants(
            &self,
            id: Uuid,
            participant_emails: &[String],
        ) -> AppResult<Thread> {
            let mut list = self.threads.lock().unwrap();
            let thread = list.iter_mut().find(|t| t.id == id).unwrap();
            thread.participant_emails = participant_emails.to_vec();
            Ok(thread.clone())
        }

        async fn find_thread_by_message_ids(
            &self,
            _channel_id: Uuid,
            message_ids: &[String],
        ) -> AppResult<Option<Thread>> {
            let thread_id = {
                let msgs = self.messages.lock().unwrap();
                msgs.iter()
                    .find(|m| message_ids.contains(&m.message_id))
                    .map(|m| m.thread_id)
            };
            if let Some(tid) = thread_id {
                return self.get_thread_by_id(tid).await;
            }
            Ok(None)
        }

        async fn find_thread_by_thread_index(
            &self,
            _channel_id: Uuid,
            thread_index_prefix: &str,
        ) -> AppResult<Option<Thread>> {
            let thread_id = {
                let msgs = self.messages.lock().unwrap();
                msgs.iter()
                    .find(|m| {
                        m.thread_index
                            .as_deref()
                            .unwrap_or_default()
                            .starts_with(thread_index_prefix)
                    })
                    .map(|m| m.thread_id)
            };
            if let Some(tid) = thread_id {
                return self.get_thread_by_id(tid).await;
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
            message_id: &str,
        ) -> AppResult<Option<Message>> {
            Ok(self
                .messages
                .lock()
                .unwrap()
                .iter()
                .find(|m| m.message_id == message_id)
                .cloned())
        }

        async fn find_outbound_reply(
            &self,
            thread_id: Uuid,
            in_reply_to: &str,
        ) -> AppResult<Option<Message>> {
            Ok(self
                .messages
                .lock()
                .unwrap()
                .iter()
                .find(|message| {
                    message.thread_id == thread_id
                        && message.direction == crate::entities::message::MessageDirection::Outbound
                        && message.in_reply_to.as_deref() == Some(in_reply_to)
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
    }

    struct MockTaskPersistence {
        tasks: Mutex<Vec<crate::entities::task::BackgroundTask>>,
    }

    #[async_trait]
    impl crate::adapters::persistence::task::TaskPersistence for MockTaskPersistence {
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
                run_at: Utc::now().naive_utc(),
                created_at: Utc::now().naive_utc(),
                updated_at: Utc::now().naive_utc(),
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
            lock_expires_at: chrono::NaiveDateTime,
            limit: i64,
        ) -> AppResult<Vec<crate::entities::task::BackgroundTask>> {
            let now = Utc::now().naive_utc();
            let mut tasks = self.tasks.lock().unwrap();
            let mut claimed = Vec::new();
            for task in tasks
                .iter_mut()
                .filter(|task| {
                    (task.status == crate::entities::task::TaskStatus::Pending
                        && task.run_at <= now)
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
            lock_expires_at: chrono::NaiveDateTime,
        ) -> AppResult<bool> {
            let mut list = self.tasks.lock().unwrap();
            let now = Utc::now().naive_utc();
            if let Some(t) = list.iter_mut().find(|t| {
                t.id == id
                    && t.status == crate::entities::task::TaskStatus::Pending
                    && t.run_at <= now
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
            let now = Utc::now().naive_utc();
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
            next_run_at: chrono::NaiveDateTime,
            is_dead_letter: bool,
        ) -> AppResult<bool> {
            let mut list = self.tasks.lock().unwrap();
            let now = Utc::now().naive_utc();
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
            let t = list.iter_mut().find(|t| t.id == id).unwrap();
            t.status = crate::entities::task::TaskStatus::Stopped;
            Ok(t.clone())
        }

        async fn resume_task(&self, id: Uuid) -> AppResult<crate::entities::task::BackgroundTask> {
            let mut list = self.tasks.lock().unwrap();
            let t = list.iter_mut().find(|t| t.id == id).unwrap();
            t.status = crate::entities::task::TaskStatus::Pending;
            Ok(t.clone())
        }

        async fn update_task_status(
            &self,
            id: Uuid,
            status: crate::entities::task::TaskStatus,
        ) -> AppResult<crate::entities::task::BackgroundTask> {
            let mut list = self.tasks.lock().unwrap();
            let t = list.iter_mut().find(|t| t.id == id).unwrap();
            t.status = status;
            Ok(t.clone())
        }

        async fn list_due_waiting_tasks(
            &self,
            _due_at: chrono::NaiveDateTime,
            _limit: i64,
        ) -> AppResult<Vec<crate::entities::task::BackgroundTask>> {
            Ok(vec![])
        }

        async fn list_company_tasks(
            &self,
            company_id: Uuid,
            _channel_id: Option<Uuid>,
            _status: Option<crate::entities::task::TaskStatus>,
            _sort_asc: bool,
        ) -> AppResult<Vec<crate::entities::task::BackgroundTask>> {
            Ok(self
                .tasks
                .lock()
                .unwrap()
                .iter()
                .filter(|t| t.company_id == company_id)
                .cloned()
                .collect())
        }
    }

    struct MockUserPersistence;
    #[async_trait]
    impl UserPersistence for MockUserPersistence {
        async fn create_user(
            &self,
            _username: &str,
            _email: &str,
            _password_hash: &str,
        ) -> AppResult<()> {
            unimplemented!()
        }
        async fn get_by_email(
            &self,
            _email: &str,
        ) -> AppResult<Option<crate::entities::user::User>> {
            unimplemented!()
        }
        async fn get_by_username(
            &self,
            _username: &str,
        ) -> AppResult<Option<crate::entities::user::User>> {
            unimplemented!()
        }
        async fn get_by_id(&self, _id: Uuid) -> AppResult<Option<crate::entities::user::User>> {
            unimplemented!()
        }
    }

    struct MockCompanyInvitePersistence;
    #[async_trait]
    impl CompanyInvitePersistence for MockCompanyInvitePersistence {
        async fn create_invite(
            &self,
            _company_id: Uuid,
            _email: &str,
        ) -> AppResult<crate::entities::company_invite::CompanyInvite> {
            unimplemented!()
        }
        async fn get_invite_by_id(
            &self,
            _id: Uuid,
        ) -> AppResult<Option<crate::entities::company_invite::CompanyInvite>> {
            unimplemented!()
        }
        async fn list_invites_by_company(
            &self,
            _company_id: Uuid,
        ) -> AppResult<Vec<crate::entities::company_invite::CompanyInvite>> {
            unimplemented!()
        }
        async fn update_invite_email(
            &self,
            _id: Uuid,
            _new_email: &str,
        ) -> AppResult<crate::entities::company_invite::CompanyInvite> {
            unimplemented!()
        }
        async fn accept_pending_invite(
            &self,
            _invite_id: Uuid,
            _user_id: Uuid,
            _user_email: &str,
        ) -> AppResult<Option<crate::entities::company_invite::CompanyInvite>> {
            unimplemented!()
        }
        async fn delete_invite(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }
        async fn list_invites_by_email(
            &self,
            _email: &str,
        ) -> AppResult<Vec<crate::entities::company_invite::CompanyInvite>> {
            unimplemented!()
        }
        async fn decline_pending_invite(
            &self,
            _invite_id: Uuid,
            _user_email: &str,
        ) -> AppResult<Option<crate::entities::company_invite::CompanyInvite>> {
            unimplemented!()
        }
        async fn list_members_by_company(
            &self,
            _company_id: Uuid,
        ) -> AppResult<Vec<crate::entities::company_member::CompanyMember>> {
            unimplemented!()
        }
        async fn remove_member(&self, _company_id: Uuid, _user_id: Uuid) -> AppResult<()> {
            unimplemented!()
        }
    }

    struct MockApprovalPersistence;
    #[async_trait]
    impl crate::adapters::persistence::approval::ApprovalPersistence for MockApprovalPersistence {
        async fn create_approval(
            &self,
            _company_id: Uuid,
            _channel_id: Uuid,
            _thread_id: Option<Uuid>,
            _task_id: Option<Uuid>,
            _step_key: &str,
            _approver_email: &str,
            _action_type: &str,
            _action_title: &str,
            _action_summary: &str,
            _payload: serde_json::Value,
            _notification: serde_json::Value,
            _token: &str,
            _expires_at: chrono::NaiveDateTime,
        ) -> AppResult<(crate::entities::approval::HumanApproval, bool)> {
            unimplemented!()
        }
        async fn find_approval_by_step_key(
            &self,
            _company_id: Uuid,
            _channel_id: Uuid,
            _thread_id: Option<Uuid>,
            _step_key: &str,
        ) -> AppResult<Option<crate::entities::approval::HumanApproval>> {
            Ok(None)
        }
        async fn get_approval_by_token(
            &self,
            _token: &str,
        ) -> AppResult<Option<crate::entities::approval::HumanApproval>> {
            Ok(None)
        }
        async fn consume_pending_approval(
            &self,
            _token: &str,
            _status: crate::entities::approval::ApprovalStatus,
            _now: chrono::NaiveDateTime,
        ) -> AppResult<Option<crate::entities::approval::HumanApproval>> {
            unimplemented!()
        }
        async fn expire_pending_approval(
            &self,
            _token: &str,
            _now: chrono::NaiveDateTime,
        ) -> AppResult<Option<crate::entities::approval::HumanApproval>> {
            unimplemented!()
        }
        async fn list_approvals_by_channel(
            &self,
            _company_id: Uuid,
            _channel_id: Uuid,
        ) -> AppResult<Vec<crate::entities::approval::HumanApproval>> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn sendgrid_webhook_processes_email_creates_thread_and_dispatches() {
        let company_id = Uuid::new_v4();
        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![Company {
                id: company_id,
                user_id: Uuid::new_v4(),
                name: "Acme Corp".to_string(),
                slug: "acme".to_string(),
                api_key: None,
                provider: None,
                model: None,
                enable_llm_spam_guardrail: None,
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(vec![Channel {
                id: Uuid::new_v4(),
                company_id,
                name: "Inbound Flow".to_string(),
                slug: "inbound".to_string(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None,
                agent_ids: None,
                channel_config: None,
                created_at: Utc::now().naive_utc(),
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
        });

        let task_persistence = Arc::new(MockTaskPersistence {
            tasks: Mutex::new(Vec::new()),
        });

        let thread_use_cases = Arc::new(ThreadUseCases::new(
            thread_persistence.clone(),
            channel_persistence.clone(),
            company_persistence.clone(),
            task_persistence.clone(),
            config.clone(),
        ));

        let approval_use_cases = Arc::new(crate::use_cases::approval::ApprovalUseCases::new(
            Arc::new(MockApprovalPersistence),
            task_persistence.clone(),
            thread_persistence.clone(),
            config.clone(),
        ));

        let channel_use_cases = Arc::new(ChannelUseCases::new(
            company_persistence.clone(),
            channel_persistence,
            config.clone(),
        ));
        let app_state = AppState {
            config: config.clone(),
            monitoring: Arc::new(crate::adapters::monitoring::InMemoryMonitor::new()),
            user_use_cases: Arc::new(crate::use_cases::user::UserUseCases::new(
                Arc::new(crate::infra::argon2_password_hasher()),
                Arc::new(MockUserPersistence {}),
            )),
            company_use_cases: Arc::new(crate::use_cases::company::CompanyUseCases::new(Arc::new(
                MockCompanyPersistence {
                    companies: Mutex::new(vec![]),
                },
            ))),
            company_invite_use_cases: Arc::new(
                crate::use_cases::company_invite::CompanyInviteUseCases::new(
                    Arc::new(MockCompanyPersistence {
                        companies: Mutex::new(vec![]),
                    }),
                    Arc::new(MockCompanyInvitePersistence {}),
                ),
            ),
            channel_use_cases,
            agent_use_cases: Arc::new(crate::use_cases::agent::AgentUseCases::new(
                company_persistence,
                Arc::new(MockAgentPersistence),
            )),
            thread_use_cases,
            approval_use_cases,
        };

        let app = router().with_state(app_state);

        let json_body = serde_json::json!({
            "to": "inbound@acme.mailagents.com",
            "from": "user@external.com",
            "subject": "Help Needed",
            "text": "Hello, please assist me with my order.",
            "headers": "Message-ID: <MSG123@external.com>\n"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/webhooks/email/sendgrid")
                    .header("content-type", "application/json")
                    .body(Body::from(json_body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(body_str.contains("\"processed\":true"));
        assert!(body_str.contains("\"inbound_message_id\":\"<MSG123@external.com>\""));

        // Ingestion persists the inbound message; the durable worker sends the reply.
        let threads = thread_persistence.threads.lock().unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].subject, "Help Needed");

        let messages = thread_persistence.messages.lock().unwrap();
        assert_eq!(messages.len(), 1);

        assert_eq!(
            messages[0].role,
            crate::entities::message::MessageRole::Human
        );
        assert_eq!(
            messages[0].direction,
            crate::entities::message::MessageDirection::Inbound
        );
    }
}
