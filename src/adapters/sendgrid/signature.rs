//! SendGrid webhook signature verification.
//!
//! Pure and clock-injected at its core, like the Resend/Svix verifier. The signature covers the
//! timestamp followed by the exact request bytes, so callers must verify before parsing or
//! reconstructing the provider payload.

use axum::http::{HeaderMap, StatusCode};
use base64::Engine;
use p256::ecdsa::{Signature, VerifyingKey, signature::Verifier};

use crate::infra::config::SendGridInboundConfig;

pub const SENDGRID_SIGNATURE_HEADER: &str = "x-twilio-email-event-webhook-signature";
pub const SENDGRID_TIMESTAMP_HEADER: &str = "x-twilio-email-event-webhook-timestamp";

/// Verify one request using the current system time.
pub fn verify_sendgrid_signature(
    headers: &HeaderMap,
    body: &[u8],
    config: &SendGridInboundConfig,
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

/// Verify a request at a stated time, without parsing or normalising its body.
pub fn verify_sendgrid_signature_at(
    headers: &HeaderMap,
    body: &[u8],
    verifying_key: &VerifyingKey,
    max_age_secs: u64,
    now: u64,
) -> Result<(), StatusCode> {
    let timestamp = header_str(headers, SENDGRID_TIMESTAMP_HEADER)?;
    let timestamp_secs: u64 = timestamp.parse().map_err(|_| StatusCode::UNAUTHORIZED)?;
    if timestamp_secs > now || now - timestamp_secs > max_age_secs {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let signature = header_str(headers, SENDGRID_SIGNATURE_HEADER).and_then(|value| {
        base64::engine::general_purpose::STANDARD
            .decode(value)
            .map_err(|_| StatusCode::UNAUTHORIZED)
    })?;
    let signature = Signature::from_der(&signature)
        .or_else(|_| Signature::from_slice(&signature))
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    let mut signed = Vec::with_capacity(timestamp.len() + body.len());
    signed.extend_from_slice(timestamp.as_bytes());
    signed.extend_from_slice(body);
    verifying_key
        .verify(&signed, &signature)
        .map_err(|_| StatusCode::UNAUTHORIZED)
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, StatusCode> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(StatusCode::UNAUTHORIZED)
}

#[cfg(test)]
mod tests {
    use p256::ecdsa::{SigningKey, signature::Signer};

    use super::*;

    fn signed_headers(signing_key: &SigningKey, timestamp: &str, body: &[u8]) -> HeaderMap {
        let mut signed = timestamp.as_bytes().to_vec();
        signed.extend_from_slice(body);
        let signature: Signature = signing_key.sign(&signed);
        let mut headers = HeaderMap::new();
        headers.insert(SENDGRID_TIMESTAMP_HEADER, timestamp.parse().unwrap());
        headers.insert(
            SENDGRID_SIGNATURE_HEADER,
            base64::engine::general_purpose::STANDARD
                .encode(signature.to_der().as_bytes())
                .parse()
                .unwrap(),
        );
        headers
    }

    #[test]
    fn the_signature_covers_the_timestamp_and_exact_body() {
        let signing_key = SigningKey::from_bytes((&[7_u8; 32]).into()).unwrap();
        let body = b"multipart bytes must stay exactly like this\r\n";
        let headers = signed_headers(&signing_key, "1000", body);

        assert!(
            verify_sendgrid_signature_at(&headers, body, signing_key.verifying_key(), 300, 1100,)
                .is_ok()
        );
        assert_eq!(
            verify_sendgrid_signature_at(
                &headers,
                b"changed",
                signing_key.verifying_key(),
                300,
                1100,
            ),
            Err(StatusCode::UNAUTHORIZED)
        );

        let other_timestamp = signed_headers(&signing_key, "1001", body);
        assert_eq!(
            verify_sendgrid_signature_at(
                &other_timestamp,
                body,
                signing_key.verifying_key(),
                300,
                1000,
            ),
            Err(StatusCode::UNAUTHORIZED)
        );
    }

    #[test]
    fn stale_future_missing_and_malformed_signatures_are_refused() {
        let signing_key = SigningKey::from_bytes((&[7_u8; 32]).into()).unwrap();
        let body = b"body";
        let headers = signed_headers(&signing_key, "1000", body);

        assert!(
            verify_sendgrid_signature_at(&headers, body, signing_key.verifying_key(), 300, 1300,)
                .is_ok()
        );
        assert_eq!(
            verify_sendgrid_signature_at(&headers, body, signing_key.verifying_key(), 300, 1301,),
            Err(StatusCode::UNAUTHORIZED)
        );
        assert_eq!(
            verify_sendgrid_signature_at(&headers, body, signing_key.verifying_key(), 300, 999,),
            Err(StatusCode::UNAUTHORIZED)
        );

        let mut missing = HeaderMap::new();
        missing.insert(SENDGRID_TIMESTAMP_HEADER, "1000".parse().unwrap());
        assert_eq!(
            verify_sendgrid_signature_at(&missing, body, signing_key.verifying_key(), 300, 1100,),
            Err(StatusCode::UNAUTHORIZED)
        );
        missing.insert(SENDGRID_SIGNATURE_HEADER, "not-base64".parse().unwrap());
        assert_eq!(
            verify_sendgrid_signature_at(&missing, body, signing_key.verifying_key(), 300, 1100,),
            Err(StatusCode::UNAUTHORIZED)
        );
    }
}
