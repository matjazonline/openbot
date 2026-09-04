use std::sync::Mutex;

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use base64::Engine;
use ring::hmac;
use secrecy::{ExposeSecret, SecretString};
use tower::ServiceExt;
use uuid::Uuid;

use super::*;
use crate::{
    adapters::{monitoring::InMemoryMonitor, resend::signature::decode_signing_secret},
    app_error::AppResult,
    entities::{
        company::{Company, CompanyAccess},
        transport::InboundEventId,
    },
    infra::config::ResendInboundConfig,
    transport::InboundEventStoreOutcome,
    use_cases::company::{CompanyPersistence, CompanyWrite},
};

const SECRET: &str = "whsec_MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw";
const COMPANY_SLUG: &str = "acme";

/// Records what a verified webhook decided to store.
#[derive(Default)]
struct RecordingInbox {
    stored: Mutex<Vec<AuthenticatedInboundEvent>>,
    /// Keys already seen, so a redelivery answers `Duplicate` the way Postgres would.
    seen: Mutex<Vec<String>>,
}

#[async_trait]
impl InboundEventInbox for RecordingInbox {
    async fn store_authenticated(
        &self,
        event: AuthenticatedInboundEvent,
    ) -> AppResult<InboundEventStoreOutcome> {
        let key = event.external_event_key.as_str().to_string();
        let mut seen = self.seen.lock().unwrap();
        let id = InboundEventId::from(Uuid::new_v4());
        if seen.contains(&key) {
            return Ok(InboundEventStoreOutcome::Duplicate(id));
        }
        seen.push(key);
        self.stored.lock().unwrap().push(event);
        Ok(InboundEventStoreOutcome::Stored(id))
    }
}

/// One company, found by slug. Every other method is a path these tests do not reach.
struct OneCompany {
    company: Option<Company>,
}

#[async_trait]
impl CompanyPersistence for OneCompany {
    async fn get_by_slug(&self, slug: &str) -> AppResult<Option<Company>> {
        Ok(self
            .company
            .as_ref()
            .filter(|company| company.slug.as_str() == slug)
            .cloned())
    }
    async fn create(&self, _user_id: Uuid, _write: CompanyWrite) -> AppResult<Company> {
        unimplemented!("the webhook never creates a company")
    }
    async fn get_by_id(&self, _id: Uuid) -> AppResult<Option<Company>> {
        unimplemented!("the webhook resolves by slug")
    }
    async fn list_by_user_id(&self, _user_id: Uuid) -> AppResult<Vec<Company>> {
        unimplemented!("the webhook has no user")
    }
    async fn update(&self, _id: Uuid, _write: CompanyWrite) -> AppResult<Company> {
        unimplemented!("the webhook never writes a company")
    }
    async fn delete(&self, _id: Uuid) -> AppResult<()> {
        unimplemented!("the webhook never deletes a company")
    }
    async fn list_company_team_emails(&self, _company_id: Uuid) -> AppResult<Vec<String>> {
        unimplemented!("the webhook resolves no team")
    }
    async fn list_company_team_accounts(
        &self,
        _company_id: Uuid,
    ) -> AppResult<Vec<crate::entities::company::CompanyTeamAccount>> {
        unimplemented!("the webhook resolves no team")
    }
    async fn list_model_connections(
        &self,
        _company_id: Uuid,
    ) -> AppResult<Vec<crate::entities::company::CompanyModelConnection>> {
        unimplemented!("the webhook runs no agent")
    }
    async fn model_api_key(
        &self,
        _company_id: Uuid,
        _provider: &crate::entities::value_objects::ModelProvider,
    ) -> AppResult<Option<String>> {
        unimplemented!("the webhook runs no agent")
    }
    async fn replace_model_connections_for_user(
        &self,
        _user_id: Uuid,
        _company_id: Uuid,
        _connections: Vec<crate::use_cases::company::CompanyModelConnectionWrite>,
    ) -> AppResult<()> {
        unimplemented!("the webhook never writes a connection")
    }
    async fn company_access(
        &self,
        _user_id: Uuid,
        _company_id: Uuid,
    ) -> AppResult<Option<CompanyAccess>> {
        unimplemented!("the webhook is not a signed-in route")
    }
}

fn company() -> Company {
    Company {
        channel_defaults: Default::default(),
        id: Uuid::new_v4(),
        user_id: Uuid::new_v4(),
        name: "Acme Corp".to_string(),
        slug: COMPANY_SLUG.into(),
        enable_llm_spam_guardrail: None,
        avatar_url: None,
        memory_provider: None,
        created_at: chrono::Utc::now(),
    }
}

struct Harness {
    state: ResendWebhookState,
    inbox: Arc<RecordingInbox>,
}

impl Harness {
    /// A deployment with Resend inbound switched on and one company that owns `acme.localhost`.
    fn enabled() -> Self {
        Self::new(
            Some(ResendInboundConfig {
                signing_secret: SecretString::from(SECRET),
                webhook_max_age_secs: 300,
                authserv_id: "resend.com".to_string(),
            }),
            Some(company()),
        )
    }

    fn new(inbound: Option<ResendInboundConfig>, company: Option<Company>) -> Self {
        let inbox = Arc::new(RecordingInbox::default());
        let config = Arc::new(AppConfig {
            app_domain_name: "localhost".to_string(),
            resend_inbound: inbound,
            ..AppConfig::for_test()
        });
        Self {
            state: ResendWebhookState {
                config,
                companies: Arc::new(CompanyUseCases::new(Arc::new(OneCompany { company }))),
                inbox: inbox.clone(),
                wakeups: InboundEventWakeups::new(),
                monitoring: Arc::new(InMemoryMonitor::new()),
            },
            inbox,
        }
    }

    async fn post(&self, body: &str, headers: Vec<(&str, String)>) -> StatusCode {
        let mut request = Request::builder()
            .method("POST")
            .uri("/webhooks/email/resend")
            .header("content-type", "application/json");
        for (name, value) in headers {
            request = request.header(name, value);
        }
        Router::new()
            .route(
                "/webhooks/email/resend",
                axum::routing::post(resend_inbound_webhook),
            )
            .with_state(self.state.clone())
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap()
            .status()
    }

    /// A correctly signed delivery of `body`.
    async fn post_signed(&self, body: &str) -> StatusCode {
        self.post(body, signed_headers("msg_1", body, SECRET)).await
    }

    fn stored(&self) -> Vec<AuthenticatedInboundEvent> {
        self.inbox
            .stored
            .lock()
            .unwrap()
            .iter()
            .map(|event| AuthenticatedInboundEvent {
                transport: event.transport,
                company_id: event.company_id,
                installation_id: event.installation_id,
                external_event_key: event.external_event_key.clone(),
                correlation_id: event.correlation_id,
                payload: event.payload.clone(),
                content_type: event.content_type.clone(),
                safe_header_facts: event.safe_header_facts.clone(),
                received_at: event.received_at,
            })
            .collect()
    }
}

fn signed_headers(id: &str, body: &str, secret: &str) -> Vec<(&'static str, String)> {
    let timestamp = chrono::Utc::now().timestamp().to_string();
    let key = hmac::Key::new(
        hmac::HMAC_SHA256,
        &decode_signing_secret(secret).expect("a test secret decodes"),
    );
    let signed = format!("{id}.{timestamp}.{body}");
    let signature = base64::engine::general_purpose::STANDARD
        .encode(hmac::sign(&key, signed.as_bytes()).as_ref());
    vec![
        ("svix-id", id.to_string()),
        ("svix-timestamp", timestamp),
        ("svix-signature", format!("v1,{signature}")),
    ]
}

fn received_event(email_id: &str, received_for: &str) -> String {
    serde_json::json!({
        "type": "email.received",
        "created_at": "2026-02-22T23:41:12.126Z",
        "data": {
            "email_id": email_id,
            "from": "someone@example.com",
            "to": ["forwarded@example.com"],
            "received_for": [received_for],
            "subject": "hello",
            "attachments": []
        }
    })
    .to_string()
}

#[tokio::test]
async fn the_route_does_not_exist_when_resend_inbound_is_not_configured() {
    let harness = Harness::new(None, Some(company()));
    let body = received_event("email-1", "support@acme.localhost");

    assert_eq!(harness.post_signed(&body).await, StatusCode::NOT_FOUND);
    assert!(harness.stored().is_empty());
}

#[tokio::test]
async fn an_unsigned_or_wrongly_signed_request_stores_nothing() {
    let harness = Harness::enabled();
    let body = received_event("email-1", "support@acme.localhost");

    assert_eq!(
        harness.post(&body, Vec::new()).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        harness
            .post(
                &body,
                signed_headers("msg_1", &body, "whsec_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA")
            )
            .await,
        StatusCode::UNAUTHORIZED
    );
    // A signature made over a different body is the same failure: this is what stops anyone who
    // can reach the route from injecting mail into a tenant.
    let tampered = received_event("email-2", "support@acme.localhost");
    assert_eq!(
        harness
            .post(&tampered, signed_headers("msg_1", &body, SECRET))
            .await,
        StatusCode::UNAUTHORIZED
    );
    assert!(harness.stored().is_empty());
}

#[tokio::test]
async fn a_signed_received_event_is_stored_under_its_tenant_and_keyed_by_the_mail() {
    let harness = Harness::enabled();
    let body = received_event("email-1", "support@acme.localhost");

    assert_eq!(harness.post_signed(&body).await, StatusCode::OK);

    let stored = harness.stored();
    assert_eq!(stored.len(), 1);
    let event = &stored[0];
    assert_eq!(event.transport, TransportKind::Email);
    // Email is a deployment transport: naming an installation here would fail the schema's own
    // coherence check.
    assert_eq!(event.installation_id, None);
    // The Resend mail id, not the Svix delivery id, so a redelivery collapses onto one row.
    assert_eq!(event.external_event_key.as_str(), "email-1");
    assert_eq!(event.payload.as_bytes(), body.as_bytes());
}

#[tokio::test]
async fn the_stored_facts_name_the_delivery_and_never_the_signature() {
    let harness = Harness::enabled();
    let body = received_event("email-1", "support@acme.localhost");
    harness.post_signed(&body).await;

    let stored = harness.stored();
    let facts: Vec<(&str, &str)> = stored[0].safe_header_facts.iter().collect();
    assert!(
        facts
            .iter()
            .any(|(name, value)| *name == SVIX_ID_FACT && *value == "msg_1")
    );
    assert!(facts.iter().any(|(name, _)| *name == SVIX_TIMESTAMP_FACT));
    assert!(
        !facts.iter().any(|(name, _)| name.contains("signature")),
        "a stored row must not be somewhere a credential is kept: {facts:?}"
    );
}

#[tokio::test]
async fn a_redelivery_of_the_same_mail_is_accepted_and_stored_once() {
    let harness = Harness::enabled();
    let body = received_event("email-1", "support@acme.localhost");

    assert_eq!(harness.post_signed(&body).await, StatusCode::OK);
    // Svix redelivers under a new delivery id; the mail is the same mail.
    assert_eq!(
        harness
            .post(&body, signed_headers("msg_2", &body, SECRET))
            .await,
        StatusCode::OK
    );
    assert_eq!(harness.stored().len(), 1);
}

#[tokio::test]
async fn every_other_event_type_is_acknowledged_and_dropped() {
    let harness = Harness::enabled();
    for event_type in [
        "email.delivered",
        "email.bounced",
        "email.opened",
        "domain.created",
    ] {
        let body = serde_json::json!({
            "type": event_type,
            "created_at": "2026-02-22T23:41:12.126Z",
            "data": { "email_id": "email-1" }
        })
        .to_string();
        // 2xx, not a 4xx: a refusal is what makes Svix retry an event we have nothing to do with.
        assert_eq!(
            harness.post_signed(&body).await,
            StatusCode::OK,
            "{event_type}"
        );
    }
    assert!(harness.stored().is_empty());
}

#[tokio::test]
async fn a_recipient_belonging_to_no_company_is_acknowledged_and_dropped() {
    let harness = Harness::enabled();
    for recipient in [
        // A platform address whose company slug is nobody's.
        "support@stranger.localhost",
        // Not a platform address at all.
        "someone@example.com",
    ] {
        let body = received_event("email-1", recipient);
        // Deliberately not a bounce: nothing has authenticated this sender yet, so answering the
        // envelope address would be backscatter.
        assert_eq!(
            harness.post_signed(&body).await,
            StatusCode::OK,
            "{recipient}"
        );
    }
    assert!(harness.stored().is_empty());
}

#[tokio::test]
async fn the_tenant_comes_from_what_the_mail_was_received_for_not_from_what_it_claims() {
    let harness = Harness::enabled();
    // `to` names an address this deployment does not serve -- a mailing list rewrote it. The
    // routing fact is the address Resend accepted the mail for.
    let body = serde_json::json!({
        "type": "email.received",
        "data": {
            "email_id": "email-1",
            "from": "someone@example.com",
            "to": ["list@example.com"],
            "received_for": ["support@acme.localhost"]
        }
    })
    .to_string();

    assert_eq!(harness.post_signed(&body).await, StatusCode::OK);
    assert_eq!(harness.stored().len(), 1);
}

#[tokio::test]
async fn a_body_past_the_inbox_bound_is_refused_before_it_is_parsed() {
    let harness = Harness::enabled();
    let oversized = format!(
        r#"{{"type":"email.received","data":{{"email_id":"email-1","subject":"{}"}}}}"#,
        "x".repeat(MAX_REQUEST_BODY_BYTES)
    );

    assert_eq!(
        harness.post_signed(&oversized).await,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert!(harness.stored().is_empty());
}

#[test]
fn the_request_bound_is_the_bound_the_inbox_itself_enforces() {
    // Two numbers here is how one of them stops being enforced: a request this route accepted but
    // the row rejects would be a 500 after the work of reading it.
    assert_eq!(MAX_REQUEST_BODY_BYTES, MAX_INBOUND_EVENT_PAYLOAD_BYTES);
}

#[test]
fn a_configured_secret_is_decodable_at_startup() {
    let config = ResendInboundConfig {
        signing_secret: SecretString::from(SECRET),
        webhook_max_age_secs: 300,
        authserv_id: "resend.com".to_string(),
    };
    assert!(decode_signing_secret(config.signing_secret.expose_secret()).is_some());
}
