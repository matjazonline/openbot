//! Svix webhook signature verification, which is how Resend proves a request is its own.
//!
//! Pure and clock-injected on purpose. Everything this endpoint decides before it will look at a
//! payload -- is the timestamp inside the replay window, does the HMAC cover these exact bytes --
//! is decided here, so the rules can be tested against real vectors with no router, no state and
//! no wall clock.

use axum::http::{HeaderMap, StatusCode};
use base64::Engine;
use ring::hmac;
use secrecy::{ExposeSecret, SecretString};

// The decode lives with the credential it belongs to, so the check a settings form makes before
// storing a secret and the check this module makes before trusting one are the same function.
pub use crate::entities::company_resend_api::decode_signing_secret;

/// The header naming one delivery attempt. Stored with the event so a redelivery is recognisable.
pub const SVIX_ID_HEADER: &str = "svix-id";
pub const SVIX_TIMESTAMP_HEADER: &str = "svix-timestamp";
pub const SVIX_SIGNATURE_HEADER: &str = "svix-signature";

/// The longest `svix-id` this endpoint will store. `inbound_events.external_event_key` is bounded
/// at 512 bytes and a delivery id is a short token; anything longer is not one.
const MAX_SVIX_ID_BYTES: usize = 128;

/// The delivery id a verified request carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SvixDelivery {
    pub id: String,
    pub timestamp: String,
}

/// Verify one request against the configured secret at a stated time.
///
/// `body` must be the bytes as they arrived. Any re-serialisation between reading and verifying --
/// a JSON round trip, a normalised header -- changes the signature, and the failure looks like a
/// wrong secret rather than like the bug it is.
pub fn verify_svix_signature_at(
    headers: &HeaderMap,
    body: &[u8],
    secret: &SecretString,
    max_age_secs: u64,
    now: u64,
) -> Result<SvixDelivery, StatusCode> {
    let id = header_str(headers, SVIX_ID_HEADER)?;
    if id.len() > MAX_SVIX_ID_BYTES {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let timestamp = header_str(headers, SVIX_TIMESTAMP_HEADER)?;
    let timestamp_secs: u64 = timestamp.parse().map_err(|_| StatusCode::UNAUTHORIZED)?;
    if timestamp_secs > now || now - timestamp_secs > max_age_secs {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let offered = header_str(headers, SVIX_SIGNATURE_HEADER)?;

    let key = decode_signing_secret(secret.expose_secret()).ok_or(StatusCode::UNAUTHORIZED)?;
    let key = hmac::Key::new(hmac::HMAC_SHA256, &key);
    let mut signed = Vec::with_capacity(id.len() + timestamp.len() + body.len() + 2);
    signed.extend_from_slice(id.as_bytes());
    signed.push(b'.');
    signed.extend_from_slice(timestamp.as_bytes());
    signed.push(b'.');
    signed.extend_from_slice(body);

    // The header carries every signature valid for this delivery, which is what makes a secret
    // rotation a window rather than an outage: during it Svix signs with both keys and one of the
    // entries matches whichever key this process holds.
    if offered
        .split(' ')
        .filter_map(|entry| entry.strip_prefix("v1,"))
        .filter_map(|encoded| {
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .ok()
        })
        // `hmac::verify` recomputes the tag and compares it in constant time, which is what keeps
        // a wrong secret from being narrowed one byte at a time by timing the answer.
        .any(|candidate| hmac::verify(&key, &signed, &candidate).is_ok())
    {
        return Ok(SvixDelivery {
            id: id.to_string(),
            timestamp: timestamp.to_string(),
        });
    }
    Err(StatusCode::UNAUTHORIZED)
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
    use super::*;

    const SECRET: &str = "whsec_MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw";
    const BODY: &[u8] = br#"{"type":"email.received","data":{"email_id":"abc"}}"#;

    fn sign(id: &str, timestamp: &str, body: &[u8], secret: &str) -> String {
        let key = hmac::Key::new(
            hmac::HMAC_SHA256,
            &decode_signing_secret(secret).expect("a test secret decodes"),
        );
        let mut signed = Vec::new();
        signed.extend_from_slice(id.as_bytes());
        signed.push(b'.');
        signed.extend_from_slice(timestamp.as_bytes());
        signed.push(b'.');
        signed.extend_from_slice(body);
        format!(
            "v1,{}",
            base64::engine::general_purpose::STANDARD.encode(hmac::sign(&key, &signed).as_ref())
        )
    }

    fn delivery_headers(id: &str, timestamp: &str, signature: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(SVIX_ID_HEADER, id.parse().unwrap());
        headers.insert(SVIX_TIMESTAMP_HEADER, timestamp.parse().unwrap());
        headers.insert(SVIX_SIGNATURE_HEADER, signature.parse().unwrap());
        headers
    }

    fn verify(headers: &HeaderMap, body: &[u8], now: u64) -> Result<SvixDelivery, StatusCode> {
        verify_svix_signature_at(headers, body, &SecretString::from(SECRET), 300, now)
    }

    #[test]
    fn a_secret_decodes_with_or_without_its_prefix() {
        assert_eq!(
            decode_signing_secret(SECRET),
            decode_signing_secret("MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw")
        );
        assert!(decode_signing_secret("whsec_").is_none());
        assert!(decode_signing_secret("not base64 at all!!").is_none());
    }

    #[test]
    fn a_signature_covers_the_id_the_timestamp_and_the_exact_body() {
        let signature = sign("msg_1", "1000", BODY, SECRET);
        let headers = delivery_headers("msg_1", "1000", &signature);

        assert_eq!(
            verify(&headers, BODY, 1100).expect("the signed request verifies"),
            SvixDelivery {
                id: "msg_1".to_string(),
                timestamp: "1000".to_string(),
            }
        );
        // One byte of the body, and nothing else, changed.
        let mut tampered = BODY.to_vec();
        let last = tampered.len() - 1;
        tampered[last] = b' ';
        assert_eq!(
            verify(&headers, &tampered, 1100),
            Err(StatusCode::UNAUTHORIZED)
        );
        // The same body under a different delivery id or timestamp is a different signature.
        assert_eq!(
            verify(&delivery_headers("msg_2", "1000", &signature), BODY, 1100),
            Err(StatusCode::UNAUTHORIZED)
        );
        assert_eq!(
            verify(&delivery_headers("msg_1", "1001", &signature), BODY, 1100),
            Err(StatusCode::UNAUTHORIZED)
        );
    }

    #[test]
    fn a_stale_or_future_timestamp_is_refused_before_the_hmac() {
        let headers = delivery_headers("msg_1", "1000", &sign("msg_1", "1000", BODY, SECRET));
        assert!(verify(&headers, BODY, 1300).is_ok());
        assert_eq!(verify(&headers, BODY, 1301), Err(StatusCode::UNAUTHORIZED));
        assert_eq!(verify(&headers, BODY, 999), Err(StatusCode::UNAUTHORIZED));
    }

    #[test]
    fn any_offered_signature_may_match_so_a_rotation_is_a_window() {
        let other = "whsec_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        let header = format!(
            "{} {}",
            sign("msg_1", "1000", BODY, other),
            sign("msg_1", "1000", BODY, SECRET)
        );
        assert!(verify(&delivery_headers("msg_1", "1000", &header), BODY, 1100).is_ok());
        // ... but only when one of them was made with the key this process holds.
        let only_other = sign("msg_1", "1000", BODY, other);
        assert_eq!(
            verify(&delivery_headers("msg_1", "1000", &only_other), BODY, 1100),
            Err(StatusCode::UNAUTHORIZED)
        );
    }

    #[test]
    fn a_missing_or_unversioned_signature_is_refused() {
        let mut without = HeaderMap::new();
        without.insert(SVIX_ID_HEADER, "msg_1".parse().unwrap());
        without.insert(SVIX_TIMESTAMP_HEADER, "1000".parse().unwrap());
        assert_eq!(verify(&without, BODY, 1100), Err(StatusCode::UNAUTHORIZED));

        // A future version this build cannot check is not a signature this build may accept.
        let v2 = sign("msg_1", "1000", BODY, SECRET).replace("v1,", "v2,");
        assert_eq!(
            verify(&delivery_headers("msg_1", "1000", &v2), BODY, 1100),
            Err(StatusCode::UNAUTHORIZED)
        );
    }
}
