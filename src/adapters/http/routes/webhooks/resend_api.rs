//! The Resend inbound webhook: prove it, place it, and answer.
//!
//! This route does no provider call and makes no routing decision. That is not an optimisation --
//! Resend's `email.received` payload carries no body, headers or attachments, so reading the mail
//! means two further HTTPS round trips, and doing them inside the request would hold the webhook
//! open long enough for Svix to time out and redeliver work that was already half-done. So the
//! exact authenticated bytes go into the durable inbound inbox and the mail is fetched later,
//! under the inbound worker's fenced lease.
//!
//! What it does decide is the tenant, because `inbound_events.company_id` is not nullable and a
//! stored event has to belong to somebody. The URL says which: every company registers an endpoint
//! ending in its own opaque token, and that token is what the row's signing secret is then found
//! by. Nothing else -- which channel, whether the sender may write to it, whether the mail
//! authenticates -- is decided here; that happens in the decoder with the mail in hand.
//!
//! Finding a row is not authenticating a request. The token only selects *whose* secret this
//! request must be proved against; an unsigned or wrongly signed request with a valid token is
//! refused exactly as one with no token at all.

use std::{collections::BTreeMap, sync::Arc};

use axum::{
    Router,
    body::to_bytes,
    extract::{FromRef, Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use chrono::Utc;
use serde::Deserialize;
use tracing::{instrument, warn};

use crate::{
    adapters::{http::app_state::AppState, resend_api::signature::verify_svix_signature_at},
    domain::monitoring::MonitoringService,
    entities::{
        company_resend_api::RESEND_API_WEBHOOK_PATH, correlation::CorrelationId,
        transport::TransportKind, value_objects::ResendApiWebhookToken,
    },
    infra::config::AppConfig,
    services::inbound_event_worker::InboundEventWakeups,
    transport::{
        AuthenticatedInboundEvent, InboundContentType, InboundEventInbox, InboundEventPayload,
        MAX_INBOUND_EVENT_PAYLOAD_BYTES, SafeHeaderFacts,
    },
    use_cases::company_resend_api::CompanyResendApiAccounts,
};

/// The largest request body this endpoint reads.
///
/// The same bound the inbox itself enforces, deliberately: reading more than can be stored only
/// moves the refusal later, and bounding the request and the row with two numbers is how one of
/// them stops being enforced. An `email.received` payload is a few hundred bytes.
const MAX_REQUEST_BODY_BYTES: usize = MAX_INBOUND_EVENT_PAYLOAD_BYTES;

pub fn router() -> Router<AppState> {
    // Built from the same constant the settings page renders its copyable URL from, so the
    // endpoint an operator is told to register is the endpoint this deployment serves.
    Router::new().route(
        &format!("{RESEND_API_WEBHOOK_PATH}/{{token}}"),
        post(resend_api_inbound_webhook),
    )
}

/// The parts of the envelope this boundary reads. The rest waits for the decoder.
#[derive(Debug, Clone, Deserialize)]
struct ResendApiWebhookEnvelope {
    #[serde(rename = "type")]
    event_type: String,
    data: ResendApiWebhookData,
}

#[derive(Debug, Clone, Deserialize)]
struct ResendApiWebhookData {
    email_id: String,
}

/// Everything this route needs, and nothing else.
///
/// A named sub-state rather than five `State` extractors, so the tests can drive the handler with
/// five in-memory doubles instead of assembling the whole application.
#[derive(Clone)]
pub struct ResendApiWebhookState {
    pub config: Arc<AppConfig>,
    pub accounts: Arc<dyn CompanyResendApiAccounts>,
    pub inbox: Arc<dyn InboundEventInbox>,
    pub wakeups: InboundEventWakeups,
    pub monitoring: Arc<dyn MonitoringService>,
}

impl FromRef<AppState> for ResendApiWebhookState {
    fn from_ref(state: &AppState) -> Self {
        Self {
            config: state.config.clone(),
            accounts: state.company_resend_api_accounts.clone(),
            inbox: state.inbound_event_inbox.clone(),
            wakeups: state.inbound_event_wakeups.clone(),
            monitoring: state.monitoring.clone(),
        }
    }
}

#[instrument(skip_all, fields(provider = "resend_api"))]
async fn resend_api_inbound_webhook(
    State(ResendApiWebhookState {
        config,
        accounts,
        inbox,
        wakeups,
        monitoring,
    }): State<ResendApiWebhookState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Result<impl IntoResponse, StatusCode> {
    // A malformed segment is not a token this application ever issued, so it is refused without a
    // query: the shape check is free, and a lookup on arbitrary path text is not.
    let token = ResendApiWebhookToken::parse(&token).ok_or(StatusCode::NOT_FOUND)?;
    // Whose secret this request must be proved against. A company that has switched its
    // integration off has no endpoint, which is the same 404 an unknown token gets -- the two are
    // deliberately indistinguishable from outside.
    let credentials = accounts
        .inbound_credentials(&token)
        .await
        .map_err(|error| {
            warn!(%error, "Could not resolve the company a Resend webhook token names");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;
    let company_id = credentials.company_id;

    let body = to_bytes(body, MAX_REQUEST_BODY_BYTES)
        .await
        .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;

    // Before anything parses the body, and over the bytes exactly as they arrived.
    let now = Utc::now();
    let delivery = verify_svix_signature_at(
        &headers,
        &body,
        &credentials.signing_secret,
        config.resend_api.webhook_max_age_secs,
        now.timestamp().max(0).unsigned_abs(),
    )?;

    let envelope: ResendApiWebhookEnvelope =
        serde_json::from_slice(&body).map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
    if envelope.event_type != "email.received" {
        // Deliveries, bounces, opens and complaints all arrive here. Answering 2xx is what stops
        // Svix retrying an event this deployment has nothing to do with.
        monitoring.increment_counter(
            "resend_api_webhook_ignored_total",
            1,
            &[("reason", "unsupported_event")],
        );
        return Ok(StatusCode::OK);
    }

    // The Resend email id, not the `svix-id`: `inbound_events` is unique on
    // `(transport, external_event_key)`, so keying on the mail collapses a Svix redelivery *and* a
    // second webhook for the same mail into one row. The delivery id is kept as a header fact so
    // an operator can still join a row to one provider attempt.
    let external_event_key =
        crate::entities::transport::ExternalEventKey::parse(envelope.data.email_id.clone())
            .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;

    let payload =
        InboundEventPayload::parse(body.to_vec()).map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
    let outcome = inbox
        .store_authenticated(AuthenticatedInboundEvent {
            transport: TransportKind::Email,
            company_id,
            // Email is a deployment transport: there is no per-company installation to name, and
            // the schema's coherence check requires this to be absent for exactly that reason.
            installation_id: None,
            external_event_key,
            correlation_id: CorrelationId::new(),
            payload,
            content_type: InboundContentType::parse("application/json").ok(),
            safe_header_facts: safe_header_facts(&delivery.id, &delivery.timestamp),
            received_at: now,
        })
        .await
        .map_err(|error| {
            warn!(%error, "Could not store an authenticated Resend event");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if outcome.was_stored() {
        wakeups.notify();
    }
    Ok(StatusCode::OK)
}

/// The names the delivery identity is stored under.
///
/// Underscored rather than the wire spelling because `SafeHeaderFacts` requires `[a-z0-9_]` names,
/// and it is a fact about the delivery rather than a verbatim header.
const SVIX_ID_FACT: &str = "svix_id";
const SVIX_TIMESTAMP_FACT: &str = "svix_timestamp";

/// The delivery identity, and nothing else.
///
/// `SafeHeaderFacts` rejects a signature header by name, and it should: the point of these is that
/// an operator reading a stored row learns which provider attempt produced it without the row
/// becoming somewhere a credential is kept.
///
/// A rejection here is a fact about the delivery id, never about the mail, so it costs the trace
/// rather than the message -- but it is logged, because silently storing an empty fact set is how
/// a bound stops being enforced without anyone noticing.
fn safe_header_facts(svix_id: &str, svix_timestamp: &str) -> SafeHeaderFacts {
    let facts = BTreeMap::from([
        (SVIX_ID_FACT.to_string(), svix_id.to_string()),
        (SVIX_TIMESTAMP_FACT.to_string(), svix_timestamp.to_string()),
    ]);
    SafeHeaderFacts::parse(facts).unwrap_or_else(|error| {
        warn!(%error, "Storing a Resend event without its delivery identity");
        SafeHeaderFacts::default()
    })
}

#[cfg(test)]
#[path = "resend_api_tests.rs"]
mod tests;
