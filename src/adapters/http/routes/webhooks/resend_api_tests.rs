use std::sync::Mutex;

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use base64::Engine;
use ring::hmac;
use secrecy::SecretString;
use tower::ServiceExt;
use uuid::Uuid;

use super::*;
use crate::{
    adapters::{monitoring::InMemoryMonitor, resend_api::signature::decode_signing_secret},
    app_error::AppResult,
    entities::{
        company_resend_api::{ResendApiAccountCredentials, ResendApiInboundCredentials},
        transport::InboundEventId,
        value_objects::AuthservId,
    },
    transport::InboundEventStoreOutcome,
};

const SECRET: &str = "whsec_MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw";
/// Another tenant's secret, structurally valid and wrong.
const OTHER_SECRET: &str = "whsec_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const TOKEN: &str = "0123456789abcdef0123456789abcdef";
const UNKNOWN_TOKEN: &str = "ffffffffffffffffffffffffffffffff";

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

/// One company's integration, found by its token -- and nothing found by any other token.
struct OneIntegration {
    company_id: Uuid,
    token: String,
    signing_secret: String,
    enabled: bool,
}

#[async_trait]
impl CompanyResendApiAccounts for OneIntegration {
    async fn inbound_credentials(
        &self,
        token: &ResendApiWebhookToken,
    ) -> AppResult<Option<ResendApiInboundCredentials>> {
        // Disabled reads as absent, the way the store's own `AND enabled` does.
        if !self.enabled || token.as_str() != self.token {
            return Ok(None);
        }
        Ok(Some(ResendApiInboundCredentials {
            company_id: self.company_id,
            signing_secret: SecretString::from(self.signing_secret.clone()),
        }))
    }

    async fn account_credentials(
        &self,
        _company_id: Uuid,
    ) -> AppResult<Option<ResendApiAccountCredentials>> {
        unimplemented!("the webhook makes no provider call")
    }
}

struct Harness {
    state: ResendApiWebhookState,
    inbox: Arc<RecordingInbox>,
    company_id: Uuid,
}

impl Harness {
    /// A company that has connected Resend, reachable at [`TOKEN`].
    fn connected() -> Self {
        Self::new(true)
    }

    fn new(enabled: bool) -> Self {
        let inbox = Arc::new(RecordingInbox::default());
        let company_id = Uuid::new_v4();
        let config = Arc::new(AppConfig {
            app_domain_name: "localhost".to_string(),
            ..AppConfig::for_test()
        });
        Self {
            state: ResendApiWebhookState {
                config,
                accounts: Arc::new(OneIntegration {
                    company_id,
                    token: TOKEN.to_string(),
                    signing_secret: SECRET.to_string(),
                    enabled,
                }),
                inbox: inbox.clone(),
                wakeups: InboundEventWakeups::new(),
                monitoring: Arc::new(InMemoryMonitor::new()),
            },
            inbox,
            company_id,
        }
    }

    async fn post_to(&self, token: &str, body: &str, headers: Vec<(&str, String)>) -> StatusCode {
        let mut request = Request::builder()
            .method("POST")
            .uri(format!("{RESEND_API_WEBHOOK_PATH}/{token}"))
            .header("content-type", "application/json");
        for (name, value) in headers {
            request = request.header(name, value);
        }
        test_router()
            .with_state(self.state.clone())
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap()
            .status()
    }

    async fn post(&self, body: &str, headers: Vec<(&str, String)>) -> StatusCode {
        self.post_to(TOKEN, body, headers).await
    }

    /// A correctly signed delivery of `body` to this company's own endpoint.
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

/// The handler under its real path, over the sub-state alone rather than a whole `AppState`.
///
/// Mounted from `RESEND_API_WEBHOOK_PATH` -- the same constant the router and the settings page use --
/// so a change to the path shape cannot pass these tests while breaking the URL operators paste
/// into Resend.
fn test_router() -> Router<ResendApiWebhookState> {
    Router::new().route(
        &format!("{RESEND_API_WEBHOOK_PATH}/{{token}}"),
        axum::routing::post(resend_api_inbound_webhook),
    )
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
async fn a_token_no_company_holds_has_no_endpoint() {
    let harness = Harness::connected();
    let body = received_event("email-1", "support@acme.localhost");

    for token in [
        // Nobody's token.
        UNKNOWN_TOKEN,
        // Not a token this application would ever have issued: refused without a lookup.
        "not-a-token",
        "0123456789ABCDEF0123456789ABCDEF",
    ] {
        assert_eq!(
            harness
                .post_to(token, &body, signed_headers("msg_1", &body, SECRET))
                .await,
            StatusCode::NOT_FOUND,
            "{token}"
        );
    }
    assert!(harness.stored().is_empty());
}

#[tokio::test]
async fn a_switched_off_integration_has_no_endpoint_either() {
    let harness = Harness::new(false);
    let body = received_event("email-1", "support@acme.localhost");

    // The same 404 an unknown token gets, deliberately: whether a token exists is not something
    // this endpoint tells an unauthenticated caller.
    assert_eq!(harness.post_signed(&body).await, StatusCode::NOT_FOUND);
    assert!(harness.stored().is_empty());
}

#[tokio::test]
async fn an_unsigned_or_wrongly_signed_request_stores_nothing() {
    let harness = Harness::connected();
    let body = received_event("email-1", "support@acme.localhost");

    assert_eq!(
        harness.post(&body, Vec::new()).await,
        StatusCode::UNAUTHORIZED
    );
    // A valid token proves nothing on its own: signed with another company's secret, this is the
    // same refusal as no signature at all.
    assert_eq!(
        harness
            .post(&body, signed_headers("msg_1", &body, OTHER_SECRET))
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
async fn a_signed_received_event_is_stored_under_the_tenant_the_token_names() {
    let harness = Harness::connected();
    let body = received_event("email-1", "support@acme.localhost");

    assert_eq!(harness.post_signed(&body).await, StatusCode::OK);

    let stored = harness.stored();
    assert_eq!(stored.len(), 1);
    let event = &stored[0];
    assert_eq!(event.transport, TransportKind::Email);
    // The tenant is the one whose endpoint this was, not one derived from an address in the body.
    assert_eq!(event.company_id, harness.company_id);
    // Email is a deployment transport: naming an installation here would fail the schema's own
    // coherence check.
    assert_eq!(event.installation_id, None);
    // The Resend mail id, not the Svix delivery id, so a redelivery collapses onto one row.
    assert_eq!(event.external_event_key.as_str(), "email-1");
    assert_eq!(event.payload.as_bytes(), body.as_bytes());
}

#[tokio::test]
async fn the_recipient_in_the_body_does_not_decide_the_tenant() {
    let harness = Harness::connected();
    // Every address here belongs to somebody else, or to nobody. The endpoint is what says whose
    // mail this is, so the event still lands under this company.
    let body = serde_json::json!({
        "type": "email.received",
        "data": {
            "email_id": "email-1",
            "from": "someone@example.com",
            "to": ["list@example.com"],
            "received_for": ["support@stranger.localhost"]
        }
    })
    .to_string();

    assert_eq!(harness.post_signed(&body).await, StatusCode::OK);
    let stored = harness.stored();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].company_id, harness.company_id);
}

#[tokio::test]
async fn the_stored_facts_name_the_delivery_and_never_the_signature() {
    let harness = Harness::connected();
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
    let harness = Harness::connected();
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
    let harness = Harness::connected();
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
async fn a_body_past_the_inbox_bound_is_refused_before_it_is_parsed() {
    let harness = Harness::connected();
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
fn a_stored_authserv_id_is_one_token() {
    // The decoder reads the first field of `Authentication-Results` and compares it to this, so a
    // value with a space in it could never match. Refused where it is entered, not where it fails.
    assert!(AuthservId::parse("resend.com").is_ok());
    assert!(AuthservId::parse("resend.com is").is_err());
}
