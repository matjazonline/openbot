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
                            content_type: field_content_type.unwrap_or_else(|| "application/octet-stream".into()),
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

    let result = thread_use_cases
        .process_and_dispatch_email(raw_payload)
        .await
        .map_err(|err| {
            warn!("Error processing inbound email thread: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok((StatusCode::OK, Json(result)))
}

fn extract_from_payload(payload: SendGridPayload, raw: &mut RawInboundPayload) {
    raw.subject = payload.subject;
    raw.text = payload.text;
    raw.html = payload.html;
    raw.headers = payload.headers;
    raw.cc = payload.cc;

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
        entities::{company::Company, message::Message, thread::Thread, workflow::Workflow},
        infra::config::AppConfig,
        use_cases::{
            company::CompanyPersistence,
            thread::ThreadPersistence,
            user::UserPersistence,
            workflow::{WorkflowPersistence, WorkflowUseCases},
            company_invite::CompanyInvitePersistence,
        },
    };

    struct MockCompanyPersistence {
        companies: Mutex<Vec<Company>>,
    }

    #[async_trait]
    impl CompanyPersistence for MockCompanyPersistence {
        async fn create(&self, _user_id: Uuid, _name: &str, _slug: &str) -> AppResult<Company> { unimplemented!() }
        async fn get_by_id(&self, _id: Uuid) -> AppResult<Option<Company>> { unimplemented!() }
        async fn get_by_slug(&self, slug: &str) -> AppResult<Option<Company>> {
            Ok(self.companies.lock().unwrap().iter().find(|c| c.slug == slug).cloned())
        }
        async fn list_by_user_id(&self, _user_id: Uuid) -> AppResult<Vec<Company>> { unimplemented!() }
        async fn update(&self, _id: Uuid, _name: &str, _slug: &str) -> AppResult<Company> { unimplemented!() }
        async fn delete(&self, _id: Uuid) -> AppResult<()> { unimplemented!() }
    }

    struct MockWorkflowPersistence {
        workflows: Mutex<Vec<Workflow>>,
    }

    #[async_trait]
    impl WorkflowPersistence for MockWorkflowPersistence {
        async fn create(&self, _company_id: Uuid, _name: &str, _slug: &str, _participant_emails: Option<Vec<String>>, _workflow_config: Option<serde_json::Value>) -> AppResult<Workflow> { unimplemented!() }
        async fn get_by_id(&self, _id: Uuid) -> AppResult<Option<Workflow>> { unimplemented!() }
        async fn get_by_company_slug_and_workflow_slug(&self, _company_slug: &str, workflow_slug: &str) -> AppResult<Option<Workflow>> {
            Ok(self.workflows.lock().unwrap().iter().find(|w| w.slug == workflow_slug).cloned())
        }
        async fn list_by_company_id(&self, _company_id: Uuid) -> AppResult<Vec<Workflow>> { unimplemented!() }
        async fn update(&self, _id: Uuid, _name: &str, _slug: &str, _participant_emails: Option<Vec<String>>, _workflow_config: Option<serde_json::Value>) -> AppResult<Workflow> { unimplemented!() }
        async fn delete(&self, _id: Uuid) -> AppResult<()> { unimplemented!() }
    }

    struct MockThreadPersistence {
        threads: Mutex<Vec<Thread>>,
        messages: Mutex<Vec<Message>>,
    }

    #[async_trait]
    impl ThreadPersistence for MockThreadPersistence {
        async fn create_thread(&self, workflow_id: Uuid, subject: &str, participant_emails: &[String]) -> AppResult<Thread> {
            let thread = Thread {
                id: Uuid::new_v4(),
                workflow_id,
                subject: subject.to_string(),
                participant_emails: participant_emails.to_vec(),
                created_at: Utc::now().naive_utc(),
                updated_at: Utc::now().naive_utc(),
            };
            self.threads.lock().unwrap().push(thread.clone());
            Ok(thread)
        }

        async fn get_thread_by_id(&self, id: Uuid) -> AppResult<Option<Thread>> {
            Ok(self.threads.lock().unwrap().iter().find(|t| t.id == id).cloned())
        }

        async fn update_thread_participants(&self, id: Uuid, participant_emails: &[String]) -> AppResult<Thread> {
            let mut list = self.threads.lock().unwrap();
            let thread = list.iter_mut().find(|t| t.id == id).unwrap();
            thread.participant_emails = participant_emails.to_vec();
            Ok(thread.clone())
        }

        async fn find_thread_by_message_ids(&self, message_ids: &[String]) -> AppResult<Option<Thread>> {
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

        async fn create_message(&self, message: &Message) -> AppResult<Message> {
            self.messages.lock().unwrap().push(message.clone());
            Ok(message.clone())
        }

        async fn get_message_by_message_id(&self, message_id: &str) -> AppResult<Option<Message>> {
            Ok(self.messages.lock().unwrap().iter().find(|m| m.message_id == message_id).cloned())
        }

        async fn list_messages_by_thread_id(&self, thread_id: Uuid) -> AppResult<Vec<Message>> {
            Ok(self.messages.lock().unwrap().iter().filter(|m| m.thread_id == thread_id).cloned().collect())
        }
    }

    struct MockUserPersistence;
    #[async_trait]
    impl UserPersistence for MockUserPersistence {
        async fn create_user(&self, _username: &str, _email: &str, _password_hash: &str) -> AppResult<()> { unimplemented!() }
        async fn get_by_email(&self, _email: &str) -> AppResult<Option<crate::entities::user::User>> { unimplemented!() }
        async fn get_by_username(&self, _username: &str) -> AppResult<Option<crate::entities::user::User>> { unimplemented!() }
        async fn get_by_id(&self, _id: Uuid) -> AppResult<Option<crate::entities::user::User>> { unimplemented!() }
    }

    struct MockCompanyInvitePersistence;
    #[async_trait]
    impl CompanyInvitePersistence for MockCompanyInvitePersistence {
        async fn create_invite(&self, _company_id: Uuid, _email: &str) -> AppResult<crate::entities::company_invite::CompanyInvite> { unimplemented!() }
        async fn get_invite_by_id(&self, _id: Uuid) -> AppResult<Option<crate::entities::company_invite::CompanyInvite>> { unimplemented!() }
        async fn list_invites_by_company(&self, _company_id: Uuid) -> AppResult<Vec<crate::entities::company_invite::CompanyInvite>> { unimplemented!() }
        async fn update_invite_email(&self, _id: Uuid, _new_email: &str) -> AppResult<crate::entities::company_invite::CompanyInvite> { unimplemented!() }
        async fn update_invite_status(&self, _id: Uuid, _status: &str) -> AppResult<crate::entities::company_invite::CompanyInvite> { unimplemented!() }
        async fn delete_invite(&self, _id: Uuid) -> AppResult<()> { unimplemented!() }
        async fn list_invites_by_email(&self, _email: &str) -> AppResult<Vec<crate::entities::company_invite::CompanyInvite>> { unimplemented!() }
        async fn add_member(&self, _company_id: Uuid, _user_id: Uuid, _role: &str) -> AppResult<crate::entities::company_member::CompanyMember> { unimplemented!() }
        async fn list_members_by_company(&self, _company_id: Uuid) -> AppResult<Vec<crate::entities::company_member::CompanyMember>> { unimplemented!() }
        async fn remove_member(&self, _company_id: Uuid, _user_id: Uuid) -> AppResult<()> { unimplemented!() }
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
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let workflow_persistence = Arc::new(MockWorkflowPersistence {
            workflows: Mutex::new(vec![Workflow {
                id: Uuid::new_v4(),
                company_id,
                name: "Inbound Flow".to_string(),
                slug: "inbound".to_string(),
                participant_emails: None,
                workflow_config: None,
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
        });

        let thread_use_cases = Arc::new(ThreadUseCases::new(
            thread_persistence.clone(),
            workflow_persistence.clone(),
            company_persistence.clone(),
            config.clone(),
        ));

        let app_state = AppState {
            config,
            user_use_cases: Arc::new(crate::use_cases::user::UserUseCases::new(
                Arc::new(crate::infra::argon2_password_hasher()),
                Arc::new(MockUserPersistence {}),
            )),
            company_use_cases: Arc::new(crate::use_cases::company::CompanyUseCases::new(
                Arc::new(MockCompanyPersistence { companies: Mutex::new(vec![]) }),
            )),
            company_invite_use_cases: Arc::new(crate::use_cases::company_invite::CompanyInviteUseCases::new(
                Arc::new(MockCompanyPersistence { companies: Mutex::new(vec![]) }),
                Arc::new(MockCompanyInvitePersistence {}),
            )),
            workflow_use_cases: Arc::new(WorkflowUseCases::new(company_persistence, workflow_persistence)),
            thread_use_cases,
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

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(body_str.contains("\"processed\":true"));
        assert!(body_str.contains("\"inbound_message_id\":\"<MSG123@external.com>\""));

        // Verify message and thread persistence
        let threads = thread_persistence.threads.lock().unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].subject, "Help Needed");

        let messages = thread_persistence.messages.lock().unwrap();
        assert_eq!(messages.len(), 2); // 1 Inbound Human + 1 Outbound Agent

        assert_eq!(messages[0].role, crate::entities::message::MessageRole::Human);
        assert_eq!(messages[0].direction, crate::entities::message::MessageDirection::Inbound);

        assert_eq!(messages[1].role, crate::entities::message::MessageRole::Agent);
        assert_eq!(messages[1].direction, crate::entities::message::MessageDirection::Outbound);
    }
}
