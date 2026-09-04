use std::sync::Arc;

use axum::{
    Form, Json, Router,
    body::to_bytes,
    extract::{FromRequest, Multipart, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use base64::Engine;
use p256::ecdsa::{Signature, VerifyingKey, signature::Verifier};
use serde::{Deserialize, Serialize};
use tracing::{instrument, warn};

use crate::{
    adapters::{
        http::app_state::AppState,
        protocols::email::{
            EmailIngressAdapter, EmailIngressTrust, VerifiedEmailAuth, parse_raw_mime_to_payload,
            parser::{MAX_INBOUND_MESSAGE_BYTES, RawAttachmentData, RawInboundPayload},
            verify_email_authentication,
        },
        storage::FileStorage,
    },
    use_cases::thread::{InboundPreflight, IngressOrigin, ReplyDelivery, ThreadUseCases},
};

/// The largest request body this endpoint reads.
///
/// The mail itself is bounded by [`MAX_INBOUND_MESSAGE_BYTES`], the same limit the SMTP listener
/// enforces; the extra megabyte is the multipart framing the provider wraps it in. Bounding the
/// request and the message with two unrelated numbers is how one of them stops being enforced.
const MAX_REQUEST_BODY_BYTES: usize = MAX_INBOUND_MESSAGE_BYTES + 1024 * 1024;

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

#[instrument(skip_all, fields(provider = "sendgrid"))]
async fn sendgrid_inbound_webhook(
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(file_storage): State<Option<Arc<dyn FileStorage>>>,
    headers: HeaderMap,
    req: axum::extract::Request,
) -> Result<impl IntoResponse, StatusCode> {
    let config = thread_use_cases.config();
    let Some(sendgrid_config) = config.sendgrid_inbound.as_ref() else {
        return Err(StatusCode::NOT_FOUND);
    };
    let (parts, body) = req.into_parts();
    let body = to_bytes(body, MAX_REQUEST_BODY_BYTES)
        .await
        .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
    verify_sendgrid_signature(&headers, &body, sendgrid_config)?;
    let req = axum::extract::Request::from_parts(parts, axum::body::Body::from(body));

    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_lowercase();

    let mut raw_payload = RawInboundPayload::default();
    let mut raw_mime = None;
    let mut sender_ip = None;

    if content_type.contains("multipart/form-data") {
        if let Ok(mut multipart) = Multipart::from_request(req, &State(())).await {
            while let Ok(Some(field)) = multipart.next_field().await {
                let name = field.name().unwrap_or_default().to_string();
                let file_name = field.file_name().map(|f| f.to_string());
                let field_content_type = field.content_type().map(|c| c.to_string());

                if name == "email" {
                    raw_mime = field.bytes().await.ok().map(|bytes| bytes.to_vec());
                    continue;
                }
                if name == "sender_ip" {
                    sender_ip = field.text().await.ok().and_then(|value| value.parse().ok());
                    continue;
                }

                if let Some(filename) = file_name {
                    if let Ok(bytes) = field.bytes().await {
                        raw_payload.attachments_data.push(RawAttachmentData {
                            filename,
                            content_type: field_content_type
                                .unwrap_or_else(|| "application/octet-stream".into()),
                            content: bytes.to_vec(),
                            stored_key: None,
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
                        // Provider-supplied authentication verdicts are deliberately ignored.
                        "spf" | "dkim" | "dmarc" => {}
                        "spam_score" => raw_payload.spam_score = value.parse().ok(),
                        "envelope" => {
                            if let Ok(env) = serde_json::from_str::<SendGridEnvelope>(&value) {
                                if let Some(recipients) = env.to
                                    && let Some(first) = recipients.first()
                                {
                                    raw_payload.to = first.clone();
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

    let raw_mime = raw_mime.ok_or(StatusCode::UNPROCESSABLE_ENTITY)?;
    if raw_mime.len() > MAX_INBOUND_MESSAGE_BYTES {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let sender_ip = sender_ip.ok_or(StatusCode::UNPROCESSABLE_ENTITY)?;
    let envelope_from = raw_payload.from.clone();
    let envelope_to = raw_payload.to.clone();
    let auth = verify_email_authentication(&raw_mime, Some(&envelope_from), sender_ip).await;
    raw_payload = parse_raw_mime_to_payload(
        &raw_mime,
        Some(&envelope_from),
        Some(&envelope_to),
        std::slice::from_ref(&envelope_to),
        auth.spf,
        auth.dkim,
        auth.dmarc,
    );

    // The verdicts are this boundary's: they come from verifying the raw MIME against the sending
    // IP above, never from the provider's own `spf`/`dkim` form fields, which anyone who can reach
    // this route could set.
    let trust = EmailIngressTrust::Verified(VerifiedEmailAuth {
        spf: raw_payload.spf,
        dkim: raw_payload.dkim,
        dmarc: raw_payload.dmarc,
        spam_score: raw_payload.spam_score,
    });
    let accepted = EmailIngressAdapter::for_config(config)
        .accept(raw_payload, trust)
        .map_err(|error| {
            warn!(%error, "Refusing an inbound message this adapter could not read");
            StatusCode::UNPROCESSABLE_ENTITY
        })?;

    let (inbound, attachments) =
        accepted.into_preflight_parts(IngressOrigin::ExternalTransport, ReplyDelivery::Send);
    let ingest = match thread_use_cases
        .preflight_inbound(inbound)
        .await
        .map_err(|error| {
            warn!(%error, "Could not preflight an inbound message");
            StatusCode::INTERNAL_SERVER_ERROR
        })? {
        InboundPreflight::Rejected(result) => *result,
        InboundPreflight::Accepted(mut prepared) => {
            let persisted = attachments
                .persist(config, file_storage.as_deref())
                .await
                .map_err(|error| {
                    warn!(%error, "Could not prepare inbound attachment metadata");
                    StatusCode::UNPROCESSABLE_ENTITY
                })?;
            prepared.replace_attachments(
                persisted.metadata,
                persisted.stored_count,
                persisted.failed_count,
            );
            thread_use_cases
                .commit_prepared_inbound(*prepared)
                .await
                .map_err(|error| {
                    warn!(%error, "Could not ingest an inbound message");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?
        }
    };

    if !ingest.accepted {
        // The HTTP server supervises this request task. Awaiting avoids a detached task whose
        // delivery can be silently cancelled when the runtime shuts down.
        thread_use_cases
            .handle_bounce_dispatch(&ingest)
            .await
            .map_err(|error| {
                warn!(%error, "Could not queue an inbound rejection bounce");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
    }

    let result = serde_json::json!({
        "processed": ingest.accepted,
        "reason": ingest.reason(),
        "thread_id": ingest.thread.as_ref().map(|t| t.id),
        "inbound_message_id": ingest
            .inbound_message
            .as_ref()
            .map(|message| message.canonical_id),
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
    raw.spam_score = payload.spam_score;

    if let Some(ref env_str) = payload.envelope
        && let Ok(env) = serde_json::from_str::<SendGridEnvelope>(env_str)
    {
        if let Some(recipients) = env.to
            && let Some(first) = recipients.first()
        {
            raw.to = first.clone();
        }
        if let Some(sender) = env.from {
            raw.from = sender;
        }
    }

    if raw.to.is_empty()
        && let Some(t) = payload.to
    {
        raw.to = t;
    }

    if raw.from.is_empty()
        && let Some(f) = payload.from
    {
        raw.from = f;
    }
}

fn verify_sendgrid_signature(
    headers: &HeaderMap,
    body: &[u8],
    config: &crate::infra::config::SendGridInboundConfig,
) -> Result<(), StatusCode> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| StatusCode::UNAUTHORIZED)?
        .as_secs();
    verify_sendgrid_signature_at(
        headers,
        body,
        &config.verifying_key,
        config.webhook_max_age_secs,
        now,
    )
}

fn verify_sendgrid_signature_at(
    headers: &HeaderMap,
    body: &[u8],
    verifying_key: &VerifyingKey,
    max_age_secs: u64,
    now: u64,
) -> Result<(), StatusCode> {
    const SIGNATURE: &str = "x-twilio-email-event-webhook-signature";
    const TIMESTAMP: &str = "x-twilio-email-event-webhook-timestamp";
    let timestamp = headers
        .get(TIMESTAMP)
        .and_then(|value| value.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let timestamp_secs: u64 = timestamp.parse().map_err(|_| StatusCode::UNAUTHORIZED)?;
    if timestamp_secs > now || now - timestamp_secs > max_age_secs {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let signature = headers
        .get(SIGNATURE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| base64::engine::general_purpose::STANDARD.decode(value).ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let signature = Signature::from_der(&signature)
        .or_else(|_| Signature::from_slice(&signature))
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    let mut signed = timestamp.as_bytes().to_vec();
    signed.extend_from_slice(body);
    verifying_key
        .verify(&signed, &signature)
        .map_err(|_| StatusCode::UNAUTHORIZED)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::company::CompanyAccess;
    use crate::entities::company_member::CompanyMembership;
    use crate::entities::task::NewTask;
    use crate::entities::task::{ResumeActor, StopActor, TaskFailure, TaskLeaseRef};
    use crate::task_queue::{AgentDispatchCommit, DispatchCommit};
    use crate::use_cases::approval::NewApproval;
    use crate::use_cases::participant::test_support::{InMemoryParticipantDirectory, TeamFixture};
    use crate::use_cases::thread::test_support::InMemoryThreads;
    use async_trait::async_trait;
    use axum::body::Body;
    use axum::http::Request;
    use chrono::Utc;
    use p256::ecdsa::{SigningKey, signature::Signer};
    use std::sync::Mutex;
    use tower::ServiceExt;
    use uuid::Uuid;

    #[test]
    fn signature_covers_timestamp_and_untouched_body_and_expires() {
        let signing_key = SigningKey::from_bytes((&[7_u8; 32]).into()).unwrap();
        let verifying_key = signing_key.verifying_key();
        let body = b"multipart bytes must stay exactly like this\r\n";
        let timestamp = "1000";
        let mut signed = timestamp.as_bytes().to_vec();
        signed.extend_from_slice(body);
        let signature: Signature = signing_key.sign(&signed);
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-twilio-email-event-webhook-timestamp",
            timestamp.parse().unwrap(),
        );
        headers.insert(
            "x-twilio-email-event-webhook-signature",
            base64::engine::general_purpose::STANDARD
                .encode(signature.to_der().as_bytes())
                .parse()
                .unwrap(),
        );

        assert!(verify_sendgrid_signature_at(&headers, body, verifying_key, 300, 1100).is_ok());
        assert_eq!(
            verify_sendgrid_signature_at(&headers, b"changed", verifying_key, 300, 1100),
            Err(StatusCode::UNAUTHORIZED)
        );
        assert_eq!(
            verify_sendgrid_signature_at(&headers, body, verifying_key, 300, 1301),
            Err(StatusCode::UNAUTHORIZED)
        );
    }

    use crate::{
        app_error::AppResult,
        entities::{channel::Channel, company::Company},
        infra::config::AppConfig,
        use_cases::{
            channel::{ChannelPersistence, ChannelUseCases, ChannelWrite},
            company::{CompanyPersistence, CompanyWrite},
            company_invite::CompanyInvitePersistence,
            user::UserPersistence,
        },
    };

    struct MockCompanyPersistence {
        companies: Mutex<Vec<Company>>,
    }

    /// These tests are about webhook ingestion, not team management: every sender is a colleague.
    #[async_trait]
    impl TeamFixture for MockCompanyPersistence {
        async fn membership_for_email(
            &self,
            _company_id: Uuid,
            _email: &str,
        ) -> AppResult<CompanyMembership> {
            Ok(CompanyMembership::Member)
        }

        async fn company_access(
            &self,
            _user_id: Uuid,
            _company_id: Uuid,
        ) -> AppResult<Option<CompanyAccess>> {
            unimplemented!("this double is not exercised on the signed-in path")
        }
    }

    #[async_trait]
    impl CompanyPersistence for MockCompanyPersistence {
        async fn create(&self, _user_id: Uuid, _write: CompanyWrite) -> AppResult<Company> {
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
        async fn update(&self, _id: Uuid, _write: CompanyWrite) -> AppResult<Company> {
            unimplemented!()
        }
        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }
        async fn list_company_team_emails(&self, _company_id: Uuid) -> AppResult<Vec<String>> {
            Ok(vec![])
        }

        async fn list_company_team_accounts(
            &self,
            _company_id: Uuid,
        ) -> AppResult<Vec<crate::entities::company::CompanyTeamAccount>> {
            unimplemented!("this double is not exercised on the team-account path")
        }

        /// This double never reaches a provider call: the tests around it assert on ingestion and
        /// threading, and an agent run that gets this far fails at parameter resolution by design.
        async fn list_model_connections(
            &self,
            _company_id: Uuid,
        ) -> AppResult<Vec<crate::entities::company::CompanyModelConnection>> {
            Ok(Vec::new())
        }

        async fn model_api_key(
            &self,
            _company_id: Uuid,
            _provider: &crate::entities::value_objects::ModelProvider,
        ) -> AppResult<Option<String>> {
            Ok(None)
        }

        async fn replace_model_connections_for_user(
            &self,
            _user_id: Uuid,
            _company_id: Uuid,
            _connections: Vec<crate::use_cases::company::CompanyModelConnectionWrite>,
        ) -> AppResult<()> {
            unimplemented!("this double is not exercised on the model-connection write path")
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
        async fn get_by_id(&self, _id: Uuid) -> AppResult<Option<Channel>> {
            unimplemented!()
        }
        async fn get_by_company_slug_and_channel_slug(
            &self,
            _company_slug: &crate::entities::value_objects::CompanySlug,
            channel_slug: &crate::entities::value_objects::ChannelSlug,
        ) -> AppResult<Option<Channel>> {
            Ok(self
                .channels
                .lock()
                .unwrap()
                .iter()
                .find(|w| w.matches_slug(channel_slug))
                .cloned())
        }
        async fn list_by_company_id(&self, _company_id: Uuid) -> AppResult<Vec<Channel>> {
            Ok(self.channels.lock().unwrap().clone())
        }
        async fn update(&self, _id: Uuid, _write: ChannelWrite) -> AppResult<Channel> {
            unimplemented!()
        }
        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }
    }

    use crate::use_cases::agent::AgentWrite;

    struct MockAgentPersistence;

    struct UnusedOwnedAgentPersistence;

    #[async_trait]
    impl crate::use_cases::agent::OwnedAgentChannelPersistence for UnusedOwnedAgentPersistence {
        async fn create_owned_agent_channel(
            &self,
            _company_id: Uuid,
            _agent: crate::use_cases::agent::AgentWrite,
            _channel: crate::use_cases::channel::ChannelWrite,
        ) -> AppResult<(
            crate::entities::agent::Agent,
            crate::entities::channel::Channel,
        )> {
            Err(crate::app_error::AppError::Internal(
                "unused owned-agent persistence".into(),
            ))
        }

        async fn update_agent_and_owned_address(
            &self,
            _agent_id: Uuid,
            _write: crate::use_cases::agent::AgentWrite,
        ) -> AppResult<crate::entities::agent::Agent> {
            Err(crate::app_error::AppError::Internal(
                "unused owned-agent persistence".into(),
            ))
        }
    }

    #[async_trait]
    impl crate::use_cases::agent::AgentPersistence for MockAgentPersistence {
        async fn create(
            &self,
            _company_id: Uuid,
            _write: AgentWrite,
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
            _write: AgentWrite,
        ) -> AppResult<crate::entities::agent::Agent> {
            unimplemented!()
        }
        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }
    }

    struct MockTaskPersistence {
        tasks: Mutex<Vec<crate::entities::task::BackgroundTask>>,
    }

    #[async_trait]
    impl crate::task_queue::TaskPersistence for MockTaskPersistence {
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
        ) -> AppResult<crate::entities::task::BackgroundTask> {
            let task = crate::entities::task::BackgroundTask {
                id: Uuid::new_v4(),
                company_id,
                channel_id,
                thread_id,
                correlation_id,
                task_type,
                status: crate::entities::task::TaskStatus::Pending,
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
            lock_expires_at: chrono::DateTime<chrono::Utc>,
        ) -> AppResult<bool> {
            let mut list = self.tasks.lock().unwrap();
            let now = Utc::now();
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

        async fn mark_task_completed(&self, lease: TaskLeaseRef) -> AppResult<bool> {
            let mut list = self.tasks.lock().unwrap();
            let now = Utc::now();
            if let Some(t) = list.iter_mut().find(|t| {
                t.id == lease.task_id
                    && t.status == crate::entities::task::TaskStatus::Processing
                    && t.worker_id == Some(lease.worker_id)
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

        async fn mark_task_failed(&self, failure: TaskFailure<'_>) -> AppResult<bool> {
            let mut list = self.tasks.lock().unwrap();
            let now = Utc::now();
            if let Some(t) = list.iter_mut().find(|t| {
                t.id == failure.lease.task_id
                    && t.status == crate::entities::task::TaskStatus::Processing
                    && t.worker_id == Some(failure.lease.worker_id)
                    && t.lock_expires_at.is_some_and(|expires| expires > now)
            }) {
                t.last_error = Some(failure.error.to_string());
                t.retry_count += 1;
                t.run_at = failure.next_run_at;
                t.status = failure.outcome.status();
                t.worker_id = None;
                t.locked_at = None;
                t.lock_expires_at = None;
                Ok(true)
            } else {
                Ok(false)
            }
        }

        async fn stop_task(
            &self,
            id: Uuid,
            _actor: StopActor,
        ) -> AppResult<crate::entities::task::BackgroundTask> {
            let mut list = self.tasks.lock().unwrap();
            let t = list.iter_mut().find(|t| t.id == id).unwrap();
            t.status = crate::entities::task::TaskStatus::Stopped;
            Ok(t.clone())
        }

        async fn resume_task(
            &self,
            id: Uuid,
            _actor: ResumeActor,
        ) -> AppResult<crate::entities::task::BackgroundTask> {
            let mut list = self.tasks.lock().unwrap();
            let t = list.iter_mut().find(|t| t.id == id).unwrap();
            t.status = crate::entities::task::TaskStatus::Pending;
            Ok(t.clone())
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
        ) -> AppResult<crate::entities::user::User> {
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
        async fn update_avatar_url(
            &self,
            _id: Uuid,
            _avatar_url: Option<&crate::entities::value_objects::AvatarUrl>,
        ) -> AppResult<Option<crate::entities::user::User>> {
            unimplemented!()
        }
        async fn update_profile(
            &self,
            _id: Uuid,
            _profile: crate::use_cases::user::ProfileUpdate<'_>,
        ) -> AppResult<Option<crate::entities::user::User>> {
            unimplemented!()
        }
        async fn update_password_hash(
            &self,
            _id: Uuid,
            _password_hash: &str,
        ) -> AppResult<Option<crate::entities::user::User>> {
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
            _role: crate::entities::company_member::CompanyAccessRole,
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
        async fn update_invite(
            &self,
            _id: Uuid,
            _new_email: &str,
            _role: crate::entities::company_member::CompanyAccessRole,
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
        async fn update_member_role(
            &self,
            _company_id: Uuid,
            _user_id: Uuid,
            _role: crate::entities::company_member::CompanyAccessRole,
        ) -> AppResult<Option<crate::entities::company_member::CompanyMember>> {
            unimplemented!()
        }
        async fn remove_member(&self, _company_id: Uuid, _user_id: Uuid) -> AppResult<()> {
            unimplemented!()
        }
    }

    struct MockApprovalPersistence;
    #[async_trait]
    impl crate::use_cases::approval::ApprovalPersistence for MockApprovalPersistence {
        async fn create_approval(
            &self,
            _new_approval: NewApproval<'_>,
        ) -> AppResult<(crate::entities::approval::HumanApproval, bool)> {
            unimplemented!()
        }
        async fn find_approval_by_step_key(
            &self,
            _company_id: Uuid,
            _channel_id: Uuid,
            _thread_id: Uuid,
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
            _now: chrono::DateTime<chrono::Utc>,
        ) -> AppResult<Option<crate::entities::approval::HumanApproval>> {
            unimplemented!()
        }
        async fn expire_pending_approval(
            &self,
            _token: &str,
            _now: chrono::DateTime<chrono::Utc>,
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

    struct MockSchedulePersistence;
    #[async_trait]
    impl crate::use_cases::schedule::SchedulePersistence for MockSchedulePersistence {
        async fn create(
            &self,
            _company_id: Uuid,
            _channel_id: Uuid,
            _write: crate::entities::schedule::ScheduleWrite,
        ) -> AppResult<crate::entities::schedule::ChannelSchedule> {
            unimplemented!()
        }
        async fn get_by_id(
            &self,
            _id: Uuid,
        ) -> AppResult<Option<crate::entities::schedule::ChannelSchedule>> {
            Ok(None)
        }
        async fn list_by_channel_id(
            &self,
            _company_id: Uuid,
            _channel_id: Uuid,
        ) -> AppResult<Vec<crate::entities::schedule::ChannelSchedule>> {
            Ok(vec![])
        }
        async fn list_by_company_id(
            &self,
            _company_id: Uuid,
        ) -> AppResult<Vec<crate::entities::schedule::ChannelSchedule>> {
            Ok(vec![])
        }
        async fn update(
            &self,
            _existing: &crate::entities::schedule::ChannelSchedule,
            _channel_id: Uuid,
            _write: crate::entities::schedule::ScheduleWrite,
        ) -> AppResult<crate::entities::schedule::ChannelSchedule> {
            unimplemented!()
        }
        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            Ok(())
        }
        async fn set_enabled(&self, _id: Uuid, _enabled: bool) -> AppResult<bool> {
            Ok(true)
        }
        async fn claim_and_advance_due_schedules(
            &self,
            _worker_id: Uuid,
            _lock_expires_at: chrono::DateTime<chrono::Utc>,
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
        async fn record_manual_run(
            &self,
            _id: Uuid,
        ) -> AppResult<Option<crate::entities::schedule::ChannelSchedule>> {
            Ok(None)
        }
        async fn release_failed_claim(
            &self,
            _schedule: &crate::entities::schedule::ChannelSchedule,
            _error: &str,
        ) -> AppResult<()> {
            Ok(())
        }
        async fn clear_last_error(&self, _id: Uuid) -> AppResult<()> {
            Ok(())
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

    #[tokio::test]
    async fn sendgrid_webhook_is_absent_when_disabled() {
        let company_id = Uuid::new_v4();
        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![Company {
                channel_defaults: Default::default(),
                id: company_id,
                user_id: Uuid::new_v4(),
                name: "Acme Corp".to_string(),
                slug: "acme".into(),
                enable_llm_spam_guardrail: None,
                avatar_url: None,
                memory_provider: None,
                created_at: Utc::now(),
            }]),
        });

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(vec![Channel {
                owner_agent_id: None,
                enabled: true,
                add_3rd_party: true,
                id: Uuid::new_v4(),
                company_id,
                name: "Inbound Flow".to_string(),
                description: None,
                slug: "inbound".into(),
                alias_slugs: Vec::new(),
                participant_emails: None,
                access_mode: crate::entities::channel::ChannelAccessMode::Team,
                principal_grants: Vec::new(),
                agent_ids: None,
                retrieve_company_memory: false,
                retrieve_agent_memory: false,
                retrieve_user_memory: false,
                persist_company_memory: false,
                persist_agent_memory: false,
                persist_user_memory: false,
                created_by: crate::entities::creation::CreationProvenance::system(),
                created_at: Utc::now(),
            }]),
        });

        let thread_persistence = Arc::new(InMemoryThreads::new());

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            sendgrid_inbound: None,
            resend_inbound: None,
            resend_outbound: None,
            hydradb: None,
            hindsight: None,
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

        let thread_use_cases = Arc::new(ThreadUseCases::for_test(
            thread_persistence.clone(),
            channel_persistence.clone(),
            company_persistence.clone(),
            Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence.clone())),
            task_persistence.clone(),
            config.clone(),
        ));

        // The same renderer and the same synthetic email bindings the thread doubles use, so an
        // approval composed here freezes exactly what production would.
        let renderers = Arc::new(
            crate::transport::ports::TransportRenderers::new()
                .register(Arc::new(
                    crate::adapters::protocols::email::EmailRenderer::new(&config.app_domain_name),
                ))
                .expect("one renderer registers"),
        );
        let delivery_composer = crate::transport::DeliveryComposer::new(
            renderers.clone(),
            crate::use_cases::thread::test_support::InMemoryIngress::ports(
                thread_persistence.clone(),
                task_persistence.clone(),
            )
            .bindings,
        );

        let approval_use_cases = Arc::new(crate::use_cases::approval::ApprovalUseCases::new(
            Arc::new(MockApprovalPersistence),
            task_persistence.clone(),
            thread_persistence.clone(),
            delivery_composer,
            config.clone(),
        ));

        let channel_use_cases = Arc::new(ChannelUseCases::new(
            company_persistence.clone(),
            channel_persistence.clone(),
            Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence.clone())),
            config.clone(),
        ));
        let memory_persistence = Arc::new(crate::adapters::persistence::PostgresPersistence::new(
            sqlx::PgPool::connect_lazy("postgres://localhost/mail_agents_test")
                .expect("valid lazy pool url"),
        ));
        let memory_providers =
            Arc::new(crate::services::memory_provider::MemoryProviderRegistry::default());
        let monitoring = Arc::new(crate::adapters::monitoring::InMemoryMonitor::new());
        let inbound_persistence = Arc::new(crate::adapters::persistence::PostgresPersistence::new(
            sqlx::PgPool::connect_lazy("postgres://localhost/mail_agents_test")
                .expect("valid lazy pool url"),
        ));
        let inbound_event_wakeups =
            crate::services::inbound_event_worker::InboundEventWakeups::new();
        let inbound_event_worker = Arc::new(
            crate::services::inbound_event_worker::InboundEventWorker::new(
                inbound_persistence.clone(),
                inbound_persistence.clone(),
                Arc::new(crate::transport::InboundEventDecoderRegistry::new()),
                inbound_event_wakeups.clone(),
            ),
        );
        let app_state = AppState {
            memory_provider_activity: Default::default(),
            // Lazy: this test drives mocked persistence and never opens a connection.
            db: sqlx::PgPool::connect_lazy("postgres://localhost/mail_agents_test")
                .expect("valid lazy pool url"),
            config: config.clone(),
            monitoring: monitoring.clone(),
            sessions: Arc::new(crate::adapters::http::session::SessionAuthority::new(
                &config,
            )),
            // Inbound mail never uploads anything.
            file_storage: None,
            // Same lazy pool: this test never renders a dashboard, so it never connects.
            dashboard_persistence: Arc::new(
                crate::adapters::persistence::PostgresPersistence::new(
                    sqlx::PgPool::connect_lazy("postgres://localhost/mail_agents_test")
                        .expect("valid lazy pool url"),
                ),
            ),
            database_query_health: Arc::new(
                crate::services::database_query_health::DatabaseQueryHealthService::new(Arc::new(
                    crate::adapters::persistence::PostgresPersistence::new(
                        sqlx::PgPool::connect_lazy("postgres://localhost/mail_agents_test")
                            .expect("valid lazy pool url"),
                    ),
                )),
            ),
            dashboard_sse_connections: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            transports: Arc::new(crate::transport::TransportRegistry::new()),
            deliveries: Arc::new(crate::adapters::persistence::PostgresPersistence::new(
                sqlx::PgPool::connect_lazy("postgres://localhost/mail_agents_test")
                    .expect("valid lazy pool url"),
            )),
            delivery_queue: Arc::new(crate::adapters::persistence::PostgresPersistence::new(
                sqlx::PgPool::connect_lazy("postgres://localhost/mail_agents_test")
                    .expect("valid lazy pool url"),
            )),
            runtime_metrics: Arc::new(crate::adapters::persistence::PostgresPersistence::new(
                sqlx::PgPool::connect_lazy("postgres://localhost/mail_agents_test")
                    .expect("valid lazy pool url"),
            )),
            runtime_identity: crate::entities::runtime_metrics::MachineIdentity {
                id: crate::entities::runtime_metrics::MachineId::new("webhook-test"),
                region: None,
            },
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
            channel_use_cases: channel_use_cases.clone(),
            schedule_use_cases: Arc::new(crate::use_cases::schedule::ScheduleUseCases::new(
                Arc::new(MockSchedulePersistence),
                company_persistence.clone(),
                channel_persistence.clone(),
                thread_persistence.clone(),
                task_persistence.clone(),
            )),
            agent_use_cases: Arc::new(crate::use_cases::agent::AgentUseCases::new(
                company_persistence.clone(),
                Arc::new(MockAgentPersistence),
                Arc::new(UnusedOwnedAgentPersistence),
                crate::use_cases::agent::SpamScanning::Available,
            )),
            thread_use_cases,
            approval_use_cases,
            memory_use_cases: Arc::new(crate::use_cases::memory::MemoryUseCases::new(
                company_persistence.clone(),
                memory_persistence.clone(),
                memory_persistence.clone(),
                Default::default(),
            )),
            memory_worker: Arc::new(crate::services::memory_worker::MemoryWorker::new(
                memory_persistence,
                memory_providers,
                monitoring,
            )),
            inbound_event_worker,
            inbound_event_inbox: inbound_persistence,
            inbound_event_wakeups,
            events: crate::infra::events::MailboxEvents::new(),
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

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let threads = thread_persistence.threads();
        assert!(threads.is_empty());
        let messages = thread_persistence.messages();
        assert!(messages.is_empty());
    }
}
