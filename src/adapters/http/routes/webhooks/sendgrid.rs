//! The SendGrid Inbound Parse webhook.
//!
//! This is deliberately only HTTP orchestration. SendGrid's signature and multipart envelope are
//! provider concerns in `adapters::sendgrid`; MIME interpretation is shared email-adapter work;
//! routing and committing remain application work.

use std::sync::Arc;

use axum::{
    Json, Router,
    body::to_bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use tracing::{instrument, warn};

use crate::{
    adapters::{
        http::app_state::AppState,
        sendgrid::{
            MAX_SENDGRID_REQUEST_BODY_BYTES, decode_sendgrid_request, verify_sendgrid_signature,
        },
        storage::FileStorage,
    },
    infra::config::{AppConfig, SendGridInboundConfig},
    use_cases::thread::{InboundPreflight, IngressOrigin, ReplyDelivery, ThreadUseCases},
};

pub fn router() -> Router<AppState> {
    Router::new().route("/webhooks/email/sendgrid", post(sendgrid_inbound_webhook))
}

#[instrument(skip_all, fields(provider = "sendgrid"))]
async fn sendgrid_inbound_webhook(
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(file_storage): State<Option<Arc<dyn FileStorage>>>,
    headers: HeaderMap,
    request: axum::extract::Request,
) -> Result<impl IntoResponse, StatusCode> {
    let config = thread_use_cases.config();
    let sendgrid_config = configured_sendgrid(config)?;

    // Authenticate the exact bounded bytes before any provider field or MIME header is parsed.
    let (parts, body) = request.into_parts();
    let body = to_bytes(body, MAX_SENDGRID_REQUEST_BODY_BYTES)
        .await
        .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
    verify_sendgrid_signature(&headers, &body, sendgrid_config)?;
    let request = axum::extract::Request::from_parts(parts, axum::body::Body::from(body));

    let accepted = decode_sendgrid_request(request, config)
        .await
        .inspect_err(|status| {
            if *status == StatusCode::UNPROCESSABLE_ENTITY {
                warn!("Refusing a SendGrid message this adapter could not read");
            }
        })?;
    let (inbound, attachments) =
        accepted.into_preflight_parts(IngressOrigin::ExternalTransport, ReplyDelivery::Send);
    let ingest = match thread_use_cases
        .preflight_inbound(inbound)
        .await
        .map_err(|error| {
            warn!(%error, "Could not preflight an inbound SendGrid message");
            StatusCode::INTERNAL_SERVER_ERROR
        })? {
        InboundPreflight::Rejected(result) => *result,
        InboundPreflight::Accepted(mut prepared) => {
            let persisted = attachments
                .persist(config, file_storage.as_deref())
                .await
                .map_err(|error| {
                    warn!(%error, "Could not prepare inbound SendGrid attachment metadata");
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
                    warn!(%error, "Could not ingest an inbound SendGrid message");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?
        }
    };

    if !ingest.accepted {
        // The request task owns this work. A queue write must finish before SendGrid is told the
        // delivery was handled, or a shutdown could lose the rejection bounce.
        thread_use_cases
            .handle_bounce_dispatch(&ingest)
            .await
            .map_err(|error| {
                warn!(%error, "Could not queue an inbound SendGrid rejection bounce");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
    }

    Ok((
        StatusCode::OK,
        Json(serde_json::json!({
            "processed": ingest.accepted,
            "reason": ingest.reason(),
            "thread_id": ingest.thread.as_ref().map(|thread| thread.id),
            "inbound_message_id": ingest
                .inbound_message
                .as_ref()
                .map(|message| message.canonical_id),
        })),
    ))
}

fn configured_sendgrid(config: &AppConfig) -> Result<&SendGridInboundConfig, StatusCode> {
    config
        .sendgrid_inbound
        .as_ref()
        .ok_or(StatusCode::NOT_FOUND)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_webhook_is_absent_when_sendgrid_is_disabled() {
        assert_eq!(
            configured_sendgrid(&AppConfig::for_test()).map(|_| ()),
            Err(StatusCode::NOT_FOUND)
        );
    }
}
