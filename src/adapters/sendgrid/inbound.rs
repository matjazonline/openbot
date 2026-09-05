//! Decode SendGrid's Inbound Parse request into the shared email-ingress adapter.
//!
//! The HTTP route authenticates the untouched bytes first. Only then does this module interpret
//! the provider envelope, re-verify the raw MIME against the sending IP, and hand the result to
//! `protocols::email`. Provider-supplied SPF/DKIM fields are deliberately never trusted.

use std::net::IpAddr;

use axum::{
    Form, Json,
    extract::{FromRequest, Multipart},
    http::StatusCode,
};
use serde::Deserialize;

use crate::{
    adapters::protocols::email::{
        EmailIngressAdapter, EmailIngressTrust, VerifiedEmailAuth,
        ingress::AcceptedEmail,
        parse_raw_mime_to_payload,
        parser::{MAX_INBOUND_MESSAGE_BYTES, RawAttachmentData, RawInboundPayload},
        verify_email_authentication,
    },
    infra::config::AppConfig,
};

/// The largest request body the SendGrid endpoint reads.
///
/// The mail itself is bounded by [`MAX_INBOUND_MESSAGE_BYTES`], the same limit the SMTP listener
/// enforces; the extra megabyte is the multipart framing the provider wraps it in.
pub const MAX_SENDGRID_REQUEST_BODY_BYTES: usize = MAX_INBOUND_MESSAGE_BYTES + 1024 * 1024;

#[derive(Debug, Clone, Deserialize)]
struct SendGridPayload {
    to: Option<String>,
    from: Option<String>,
    cc: Option<String>,
    subject: Option<String>,
    text: Option<String>,
    html: Option<String>,
    headers: Option<String>,
    envelope: Option<String>,
    spam_score: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
struct SendGridEnvelope {
    to: Option<Vec<String>>,
    from: Option<String>,
}

struct DecodedRequest {
    payload: RawInboundPayload,
    raw_mime: Vec<u8>,
    sender_ip: IpAddr,
}

/// Decode an already-authenticated request and produce the shared email boundary value.
pub async fn decode_sendgrid_request(
    request: axum::extract::Request,
    config: &AppConfig,
) -> Result<AcceptedEmail, StatusCode> {
    let content_type = request
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let decoded = decode_provider_request(request, &content_type).await?;
    if decoded.raw_mime.len() > MAX_INBOUND_MESSAGE_BYTES {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }

    let envelope_from = decoded.payload.from.clone();
    let envelope_to = decoded.payload.to.clone();
    let auth =
        verify_email_authentication(&decoded.raw_mime, Some(&envelope_from), decoded.sender_ip)
            .await;
    let payload = parse_raw_mime_to_payload(
        &decoded.raw_mime,
        Some(&envelope_from),
        Some(&envelope_to),
        std::slice::from_ref(&envelope_to),
        auth.spf,
        auth.dkim,
        auth.dmarc,
    );
    // Preserve the existing trust boundary: the form-field spam score is discarded with the
    // other provider assertions when the verified raw MIME becomes the canonical payload.
    let spam_score = payload.spam_score;

    EmailIngressAdapter::for_config(config)
        .accept(
            payload,
            EmailIngressTrust::Verified(VerifiedEmailAuth {
                spf: auth.spf,
                dkim: auth.dkim,
                dmarc: auth.dmarc,
                spam_score,
            }),
        )
        .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)
}

async fn decode_provider_request(
    request: axum::extract::Request,
    content_type: &str,
) -> Result<DecodedRequest, StatusCode> {
    let mut payload = RawInboundPayload::default();
    let mut raw_mime = None;
    let mut sender_ip = None;

    if content_type.contains("multipart/form-data") {
        decode_multipart(request, &mut payload, &mut raw_mime, &mut sender_ip).await;
    } else if content_type.contains("application/json") {
        if let Ok(Json(provider)) = Json::<SendGridPayload>::from_request(request, &()).await {
            apply_payload(provider, &mut payload);
        }
    } else if let Ok(Form(provider)) = Form::<SendGridPayload>::from_request(request, &()).await {
        apply_payload(provider, &mut payload);
    }

    Ok(DecodedRequest {
        payload,
        raw_mime: raw_mime.ok_or(StatusCode::UNPROCESSABLE_ENTITY)?,
        sender_ip: sender_ip.ok_or(StatusCode::UNPROCESSABLE_ENTITY)?,
    })
}

async fn decode_multipart(
    request: axum::extract::Request,
    payload: &mut RawInboundPayload,
    raw_mime: &mut Option<Vec<u8>>,
    sender_ip: &mut Option<IpAddr>,
) {
    let Ok(mut multipart) = Multipart::from_request(request, &()).await else {
        return;
    };
    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or_default().to_string();
        let file_name = field.file_name().map(str::to_string);
        let content_type = field.content_type().map(str::to_string);

        if name == "email" {
            *raw_mime = field.bytes().await.ok().map(|bytes| bytes.to_vec());
            continue;
        }
        if name == "sender_ip" {
            *sender_ip = field.text().await.ok().and_then(|value| value.parse().ok());
            continue;
        }
        if let Some(filename) = file_name {
            if let Ok(bytes) = field.bytes().await {
                payload.attachments_data.push(RawAttachmentData {
                    filename,
                    content_type: content_type
                        .unwrap_or_else(|| "application/octet-stream".to_string()),
                    content: bytes.to_vec(),
                    stored_key: None,
                });
            }
            continue;
        }
        if let Ok(value) = field.text().await {
            apply_field(payload, &name, value);
        }
    }
}

fn apply_field(payload: &mut RawInboundPayload, name: &str, value: String) {
    match name {
        "to" if payload.to.is_empty() => payload.to = value,
        "from" if payload.from.is_empty() => payload.from = value,
        "cc" => payload.cc = Some(value),
        "subject" => payload.subject = Some(value),
        "text" => payload.text = Some(value),
        "html" => payload.html = Some(value),
        "headers" => payload.headers = Some(value),
        "spam_score" => payload.spam_score = value.parse().ok(),
        "envelope" => apply_envelope(payload, &value),
        // These are assertions made by form fields. The verdicts used above come from verifying
        // the raw MIME against `sender_ip`, so accepting these would create a second trust path.
        "spf" | "dkim" | "dmarc" => {}
        _ => {}
    }
}

fn apply_payload(provider: SendGridPayload, payload: &mut RawInboundPayload) {
    payload.subject = provider.subject;
    payload.text = provider.text;
    payload.html = provider.html;
    payload.headers = provider.headers;
    payload.cc = provider.cc;
    payload.spam_score = provider.spam_score;
    if let Some(envelope) = provider.envelope.as_deref() {
        apply_envelope(payload, envelope);
    }
    if payload.to.is_empty()
        && let Some(to) = provider.to
    {
        payload.to = to;
    }
    if payload.from.is_empty()
        && let Some(from) = provider.from
    {
        payload.from = from;
    }
}

fn apply_envelope(payload: &mut RawInboundPayload, envelope: &str) {
    let Ok(envelope) = serde_json::from_str::<SendGridEnvelope>(envelope) else {
        return;
    };
    if let Some(recipient) = envelope
        .to
        .and_then(|recipients| recipients.into_iter().next())
    {
        payload.to = recipient;
    }
    if let Some(sender) = envelope.from {
        payload.from = sender;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_envelope_is_the_routing_source_of_truth() {
        let provider = SendGridPayload {
            to: Some("header@example.com".to_string()),
            from: Some("header-sender@example.com".to_string()),
            cc: None,
            subject: None,
            text: None,
            html: None,
            headers: None,
            envelope: Some(
                serde_json::json!({
                    "to": ["inbound@acme.example.com"],
                    "from": "sender@example.com"
                })
                .to_string(),
            ),
            spam_score: None,
        };
        let mut payload = RawInboundPayload::default();

        apply_payload(provider, &mut payload);

        assert_eq!(payload.to, "inbound@acme.example.com");
        assert_eq!(payload.from, "sender@example.com");
    }

    #[tokio::test]
    async fn multipart_decode_preserves_the_raw_mail_and_reads_transport_facts() {
        const BOUNDARY: &str = "sendgrid-test-boundary";
        const RAW_MIME: &str =
            "From: sender@example.com\r\nTo: inbox@acme.example.com\r\n\r\nhello";
        let body = format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"envelope\"\r\n\r\n{{\"to\":[\"inbox@acme.example.com\"],\"from\":\"sender@example.com\"}}\r\n\
             --{BOUNDARY}\r\nContent-Disposition: form-data; name=\"sender_ip\"\r\n\r\n192.0.2.10\r\n\
             --{BOUNDARY}\r\nContent-Disposition: form-data; name=\"email\"\r\n\r\n{RAW_MIME}\r\n\
             --{BOUNDARY}--\r\n"
        );
        let content_type = format!("multipart/form-data; boundary={BOUNDARY}");
        let request = axum::extract::Request::builder()
            .header("content-type", &content_type)
            .body(axum::body::Body::from(body))
            .unwrap();

        let decoded = decode_provider_request(request, &content_type)
            .await
            .expect("a valid SendGrid multipart request decodes");

        assert_eq!(decoded.raw_mime, RAW_MIME.as_bytes());
        assert_eq!(decoded.sender_ip, "192.0.2.10".parse::<IpAddr>().unwrap());
        assert_eq!(decoded.payload.to, "inbox@acme.example.com");
        assert_eq!(decoded.payload.from, "sender@example.com");
    }

    #[test]
    fn provider_authentication_fields_are_ignored() {
        let mut payload = RawInboundPayload::default();
        let before = (payload.spf, payload.dkim, payload.dmarc);
        apply_field(&mut payload, "spf", "pass".to_string());
        apply_field(&mut payload, "dkim", "pass".to_string());
        apply_field(&mut payload, "dmarc", "pass".to_string());

        assert_eq!((payload.spf, payload.dkim, payload.dmarc), before);
    }
}
