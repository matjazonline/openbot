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
    infra::config::AppConfig,
    use_cases::workflow::{InboundEmail, WorkflowUseCases},
};

pub fn router() -> Router<AppState> {
    Router::new().route("/webhooks/email/sendgrid", post(sendgrid_inbound_webhook))
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SendGridPayload {
    pub to: Option<String>,
    pub from: Option<String>,
    pub subject: Option<String>,
    pub text: Option<String>,
    pub html: Option<String>,
    pub envelope: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SendGridEnvelope {
    pub to: Option<Vec<String>>,
    pub from: Option<String>,
}

#[instrument(skip(workflow_use_cases, config, headers))]
async fn sendgrid_inbound_webhook(
    State(workflow_use_cases): State<Arc<WorkflowUseCases>>,
    State(config): State<Arc<AppConfig>>,
    headers: HeaderMap,
    req: axum::extract::Request,
) -> Result<impl IntoResponse, StatusCode> {
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_lowercase();

    let mut to = String::new();
    let mut from = String::new();
    let mut subject = None;
    let mut text_body = None;
    let mut html_body = None;

    if content_type.contains("multipart/form-data") {
        if let Ok(mut multipart) = Multipart::from_request(req, &State(())).await {
            while let Ok(Some(field)) = multipart.next_field().await {
                let name = field.name().unwrap_or_default().to_string();
                if let Ok(value) = field.text().await {
                    match name.as_str() {
                        "to" => {
                            if to.is_empty() {
                                to = value;
                            }
                        }
                        "from" => {
                            if from.is_empty() {
                                from = value;
                            }
                        }
                        "subject" => subject = Some(value),
                        "text" => text_body = Some(value),
                        "html" => html_body = Some(value),
                        "envelope" => {
                            if let Ok(env) = serde_json::from_str::<SendGridEnvelope>(&value) {
                                if let Some(recipients) = env.to {
                                    if let Some(first) = recipients.first() {
                                        to = first.clone();
                                    }
                                }
                                if let Some(sender) = env.from {
                                    from = sender;
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
            extract_from_payload(payload, &mut to, &mut from, &mut subject, &mut text_body, &mut html_body);
        }
    } else {
        if let Ok(Form(payload)) = Form::<SendGridPayload>::from_request(req, &State(())).await {
            extract_from_payload(payload, &mut to, &mut from, &mut subject, &mut text_body, &mut html_body);
        }
    }

    let inbound_email = InboundEmail {
        to,
        from,
        subject,
        text_body,
        html_body,
        raw_payload: None,
    };

    let result = workflow_use_cases
        .process_inbound_email("sendgrid", inbound_email, &config.app_domain_name)
        .await
        .map_err(|err| {
            warn!("Error processing inbound email: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok((StatusCode::OK, Json(result)))
}

fn extract_from_payload(
    payload: SendGridPayload,
    to: &mut String,
    from: &mut String,
    subject: &mut Option<String>,
    text_body: &mut Option<String>,
    html_body: &mut Option<String>,
) {
    *subject = payload.subject;
    *text_body = payload.text;
    *html_body = payload.html;

    if let Some(ref env_str) = payload.envelope {
        if let Ok(env) = serde_json::from_str::<SendGridEnvelope>(env_str) {
            if let Some(recipients) = env.to {
                if let Some(first) = recipients.first() {
                    *to = first.clone();
                }
            }
            if let Some(sender) = env.from {
                *from = sender;
            }
        }
    }

    if to.is_empty() {
        if let Some(t) = payload.to {
            *to = t;
        }
    }

    if from.is_empty() {
        if let Some(f) = payload.from {
            *from = f;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_error::AppResult;
    use crate::entities::company::Company;
    use crate::entities::workflow::Workflow;
    use crate::use_cases::company::CompanyPersistence;
    use crate::use_cases::workflow::WorkflowPersistence;
    use async_trait::async_trait;
    use axum::body::Body;
    use axum::http::Request;
    use chrono::Utc;
    use std::sync::Mutex;
    use tower::ServiceExt;
    use uuid::Uuid;

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

    #[tokio::test]
    async fn sendgrid_webhook_endpoint_resolves_workflow() {
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

        let workflow_use_cases = Arc::new(WorkflowUseCases::new(company_persistence, workflow_persistence));
        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
        });

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
            workflow_use_cases,
        };

        let app = router().with_state(app_state);

        let json_body = serde_json::json!({
            "to": "inbound@acme.mailagents.com",
            "from": "user@external.com",
            "subject": "Test Email",
            "text": "Hello world"
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
        assert!(body_str.contains("\"resolved\":true"));
        assert!(body_str.contains("\"company_slug\":\"acme\""));
        assert!(body_str.contains("\"workflow_slug\":\"inbound\""));
    }

    struct MockUserPersistence;
    #[async_trait]
    impl crate::use_cases::user::UserPersistence for MockUserPersistence {
        async fn create_user(&self, _username: &str, _email: &str, _password_hash: &str) -> AppResult<()> { unimplemented!() }
        async fn get_by_email(&self, _email: &str) -> AppResult<Option<crate::entities::user::User>> { unimplemented!() }
        async fn get_by_username(&self, _username: &str) -> AppResult<Option<crate::entities::user::User>> { unimplemented!() }
        async fn get_by_id(&self, _id: Uuid) -> AppResult<Option<crate::entities::user::User>> { unimplemented!() }
    }

    struct MockCompanyInvitePersistence;
    #[async_trait]
    impl crate::use_cases::company_invite::CompanyInvitePersistence for MockCompanyInvitePersistence {
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
}
