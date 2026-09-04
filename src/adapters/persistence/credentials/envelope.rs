//! `enc:v2` — per-credential envelope encryption bound to the row it belongs to.
//!
//! # Why a second format exists
//!
//! `enc:v1` (still in [`super`], still what `company_model_connections` stores) encrypts every
//! credential directly under one long-lived environment key with empty associated data. Two
//! consequences follow, and both matter for provider tokens:
//!
//! * a valid ciphertext can be moved from one company's row to another's and still decrypt, so the
//!   database row is not part of what the tag authenticates; and
//! * rotating the key means decrypting and re-encrypting every secret, so the rotation job
//!   handles plaintext for every row it touches.
//!
//! `enc:v2` fixes both. A fresh 256-bit *data encryption key* is generated per credential and
//! encrypts the token; the configured *key encryption key* wraps only that data key. Rotating a
//! KEK therefore rewraps 48 bytes and never decrypts a token
//! ([`rewrap`]). Both layers bind the same [`CredentialContext`] as associated data, so a row's
//! company, installation, transport and credential kind are covered by the authentication tag.
//!
//! # Threat model this does and does not cover
//!
//! The KEK is held in process memory, loaded from a deployment secret. That is the "bounded
//! launch alternative" of `plan/db_improve/improve-key-credentials.md`: it gives envelope
//! structure, per-row context binding and cheap rewrapping, but it does not protect a token from
//! a compromised application process, because the process can legitimately ask for the plaintext
//! in order to call the provider. Moving the KEK behind a KMS changes only [`wrap_data_key`] and
//! [`unwrap_data_key`]; the stored format does not change.
//!
//! # Wire format
//!
//! ```text
//! enc:v2:<kek version>:<dek nonce>:<wrapped dek>:<data nonce>:<ciphertext||tag>
//! ```
//!
//! Every binary field is unpadded base64. The format version pins both layers to AES-256-GCM with
//! 96-bit nonces; a future algorithm change takes a new version rather than a negotiated
//! parameter, so there is nothing here an attacker can downgrade.

use base64::{
    Engine,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
};
use ring::{
    aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey},
    rand::{SecureRandom, SystemRandom},
};
use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;

use crate::{
    app_error::AppError,
    entities::{
        company_resend_api::ResendApiCredentialKind,
        transport::{IntegrationCredentialKind, TransportKind},
    },
};

const ENVELOPE_PREFIX: &str = "enc:v2";

const NONCE_BYTES: usize = 12;
const DATA_KEY_BYTES: usize = 32;
/// A wrapped data key is the key itself plus one GCM tag; nothing else may be that length.
const WRAPPED_DATA_KEY_BYTES: usize = DATA_KEY_BYTES + 16;
/// The largest provider credential this format will seal. Provider tokens are short; a caller
/// handing over a megabyte is a bug or an attack, not a token.
const MAX_CREDENTIAL_BYTES: usize = 4096;

/// The domain separator for the whole context encoding. Bumping it invalidates every stored
/// envelope on purpose, which is what makes it a version rather than a comment.
const CONTEXT_LABEL: &[u8] = b"credential-context-v1";

/// The two layers of one envelope. They authenticate the same record identity under different
/// labels, so a wrapped data key can never be opened as if it were the payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContextLayer {
    DataKey,
    Payload,
}

impl ContextLayer {
    const fn as_bytes(self) -> &'static [u8] {
        match self {
            Self::DataKey => b"dek",
            Self::Payload => b"data",
        }
    }
}

/// The record identity an envelope is authenticated against.
///
/// Contains identifiers only — everything in here is already stored in the clear beside the
/// ciphertext — so it is safe to `Debug`, log and compare. What it must never become is a place
/// to put a mutable value: changing a field of a stored credential's context makes that
/// credential permanently unreadable, which is the same failure as losing the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialContext {
    scope: &'static str,
    fields: Vec<Vec<u8>>,
}

impl CredentialContext {
    /// The context for one installation's secret.
    ///
    /// Every field is part of the primary key or of the installation it hangs off, so an operator
    /// who copies a row's `envelope` into another company, another installation, another
    /// transport, or the neighbouring credential kind produces a row that fails to open rather
    /// than one that silently yields the original token.
    pub fn integration_credential(
        company_id: Uuid,
        installation_id: Uuid,
        transport: TransportKind,
        credential_kind: IntegrationCredentialKind,
    ) -> Self {
        Self {
            scope: "integration_credential",
            fields: vec![
                company_id.as_bytes().to_vec(),
                installation_id.as_bytes().to_vec(),
                transport.as_str().as_bytes().to_vec(),
                credential_kind.as_str().as_bytes().to_vec(),
            ],
        }
    }

    /// The context for one company's Resend secret.
    ///
    /// The row is keyed by company alone, so the kind is what carries the weight here: it is the
    /// only thing stopping the API key column and the signing secret column from being swapped
    /// into each other and still opening.
    pub fn company_resend_api_credential(company_id: Uuid, kind: ResendApiCredentialKind) -> Self {
        Self {
            scope: "company_resend_api_credential",
            fields: vec![
                company_id.as_bytes().to_vec(),
                kind.as_str().as_bytes().to_vec(),
            ],
        }
    }

    /// Length-delimited so no two different field combinations can encode to the same bytes:
    /// without the lengths, a company whose id ended where a transport name began would produce
    /// the same associated data as a different row.
    fn associated_data(&self, layer: ContextLayer) -> Vec<u8> {
        let mut encoded = Vec::with_capacity(CONTEXT_LABEL.len() + 64);
        encoded.extend_from_slice(CONTEXT_LABEL);
        for field in std::iter::once(self.scope.as_bytes())
            .chain(self.fields.iter().map(Vec::as_slice))
            .chain(std::iter::once(layer.as_bytes()))
        {
            let length = u32::try_from(field.len()).unwrap_or(u32::MAX);
            encoded.extend_from_slice(&length.to_be_bytes());
            encoded.extend_from_slice(field);
        }
        encoded
    }
}

/// What went wrong, in a form that can be logged.
///
/// No variant carries key material, ciphertext, a nonce or a plaintext: the whole point of naming
/// the failure classes is that a structured log can say *which* check failed without the operator
/// having to reproduce it against real data.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EnvelopeError {
    #[error("stored credential is not an enc:v2 envelope")]
    NotAnEnvelope,
    #[error("stored credential has a malformed {0}")]
    MalformedField(&'static str),
    #[error("credential key version {0} is not configured")]
    UnavailableKeyVersion(u32),
    #[error("the wrapped data key failed authentication for this credential's context")]
    DataKeyAuthentication,
    #[error("the credential payload failed authentication for this credential's context")]
    PayloadAuthentication,
    #[error("the decrypted credential is not UTF-8")]
    NotUtf8,
    #[error("the credential exceeds the {MAX_CREDENTIAL_BYTES}-byte limit")]
    TooLong,
    #[error("credential encryption failed")]
    EncryptionFailed,
}

impl From<EnvelopeError> for AppError {
    /// Callers get an internal error with the failure *class* and no row content; the row
    /// identifiers belong in the caller's own structured log, where they are already in scope.
    fn from(error: EnvelopeError) -> Self {
        match error {
            EnvelopeError::TooLong => AppError::BadRequest(error.to_string()),
            other => AppError::Internal(other.to_string()),
        }
    }
}

/// A structurally valid envelope. Being parsed proves nothing about authenticity; only
/// [`open_payload`] and [`unwrap_data_key`] do that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedEnvelope {
    pub key_version: u32,
    data_key_nonce: [u8; NONCE_BYTES],
    wrapped_data_key: Vec<u8>,
    payload_nonce: [u8; NONCE_BYTES],
    payload: Vec<u8>,
}

/// Strict parse: exact field count, supported prefix, positive version, exact nonce and wrapped-key
/// lengths, and a payload long enough to hold its own tag.
pub fn parse(stored: &str) -> Result<ParsedEnvelope, EnvelopeError> {
    let mut parts = stored.split(':');
    if parts.next() != Some("enc") || parts.next() != Some("v2") {
        return Err(EnvelopeError::NotAnEnvelope);
    }

    let key_version = parts
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|version| *version > 0)
        .ok_or(EnvelopeError::MalformedField("key version"))?;
    let data_key_nonce = fixed_field(parts.next(), "data key nonce")?;
    let wrapped_data_key = binary_field(parts.next(), "wrapped data key")?;
    let payload_nonce = fixed_field(parts.next(), "payload nonce")?;
    let payload = binary_field(parts.next(), "payload")?;

    if parts.next().is_some() {
        return Err(EnvelopeError::NotAnEnvelope);
    }
    if wrapped_data_key.len() != WRAPPED_DATA_KEY_BYTES {
        return Err(EnvelopeError::MalformedField("wrapped data key"));
    }
    if payload.len() < AES_256_GCM.tag_len() {
        return Err(EnvelopeError::MalformedField("payload"));
    }

    Ok(ParsedEnvelope {
        key_version,
        data_key_nonce,
        wrapped_data_key,
        payload_nonce,
        payload,
    })
}

/// Encrypt one credential under a fresh data key, then wrap that data key under `kek`.
pub fn seal(
    kek: &LessSafeKey,
    key_version: u32,
    random: &SystemRandom,
    context: &CredentialContext,
    secret: &SecretString,
) -> Result<String, EnvelopeError> {
    let plaintext = secret.expose_secret();
    if plaintext.len() > MAX_CREDENTIAL_BYTES {
        return Err(EnvelopeError::TooLong);
    }

    let mut data_key = [0_u8; DATA_KEY_BYTES];
    random
        .fill(&mut data_key)
        .map_err(|_| EnvelopeError::EncryptionFailed)?;
    let payload_nonce = random_nonce(random)?;
    let mut payload = plaintext.as_bytes().to_vec();
    data_key_cipher(&data_key)?
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(payload_nonce),
            Aad::from(context.associated_data(ContextLayer::Payload)),
            &mut payload,
        )
        .map_err(|_| EnvelopeError::EncryptionFailed)?;

    let (data_key_nonce, wrapped_data_key) = wrap_data_key(kek, random, context, &data_key)?;
    Ok(render(&ParsedEnvelope {
        key_version,
        data_key_nonce,
        wrapped_data_key,
        payload_nonce,
        payload,
    }))
}

/// Decrypt one credential. Both layers must authenticate against `context`.
pub fn open(
    kek: &LessSafeKey,
    context: &CredentialContext,
    envelope: &ParsedEnvelope,
) -> Result<SecretString, EnvelopeError> {
    let data_key = unwrap_data_key(kek, context, envelope)?;
    open_payload(&data_key, context, envelope)
}

/// Move an envelope onto a new key-encryption key **without decrypting the credential**.
///
/// This is what makes KEK rotation cheap and safe: the data key is unwrapped and re-wrapped, the
/// payload bytes are copied through untouched, and no plaintext token exists at any point. It is
/// deliberately not a re-encryption — a compromised *provider token* is rotated at the provider,
/// and a compromised *data key* means the ciphertext was already readable.
pub fn rewrap(
    current_kek: &LessSafeKey,
    target_kek: &LessSafeKey,
    target_version: u32,
    random: &SystemRandom,
    context: &CredentialContext,
    envelope: &ParsedEnvelope,
) -> Result<String, EnvelopeError> {
    let data_key = unwrap_data_key(current_kek, context, envelope)?;
    let (data_key_nonce, wrapped_data_key) = wrap_data_key(target_kek, random, context, &data_key)?;
    Ok(render(&ParsedEnvelope {
        key_version: target_version,
        data_key_nonce,
        wrapped_data_key,
        payload_nonce: envelope.payload_nonce,
        payload: envelope.payload.clone(),
    }))
}

fn wrap_data_key(
    kek: &LessSafeKey,
    random: &SystemRandom,
    context: &CredentialContext,
    data_key: &[u8; DATA_KEY_BYTES],
) -> Result<([u8; NONCE_BYTES], Vec<u8>), EnvelopeError> {
    let nonce = random_nonce(random)?;
    let mut wrapped = data_key.to_vec();
    kek.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::from(context.associated_data(ContextLayer::DataKey)),
        &mut wrapped,
    )
    .map_err(|_| EnvelopeError::EncryptionFailed)?;
    Ok((nonce, wrapped))
}

fn unwrap_data_key(
    kek: &LessSafeKey,
    context: &CredentialContext,
    envelope: &ParsedEnvelope,
) -> Result<[u8; DATA_KEY_BYTES], EnvelopeError> {
    let mut wrapped = envelope.wrapped_data_key.clone();
    let data_key = kek
        .open_in_place(
            Nonce::assume_unique_for_key(envelope.data_key_nonce),
            Aad::from(context.associated_data(ContextLayer::DataKey)),
            &mut wrapped,
        )
        .map_err(|_| EnvelopeError::DataKeyAuthentication)?;
    data_key
        .try_into()
        .map_err(|_| EnvelopeError::MalformedField("wrapped data key"))
}

fn open_payload(
    data_key: &[u8; DATA_KEY_BYTES],
    context: &CredentialContext,
    envelope: &ParsedEnvelope,
) -> Result<SecretString, EnvelopeError> {
    let mut payload = envelope.payload.clone();
    let plaintext = data_key_cipher(data_key)?
        .open_in_place(
            Nonce::assume_unique_for_key(envelope.payload_nonce),
            Aad::from(context.associated_data(ContextLayer::Payload)),
            &mut payload,
        )
        .map_err(|_| EnvelopeError::PayloadAuthentication)?;
    let plaintext = std::str::from_utf8(plaintext).map_err(|_| EnvelopeError::NotUtf8)?;
    Ok(SecretString::from(plaintext.to_string()))
}

fn data_key_cipher(data_key: &[u8; DATA_KEY_BYTES]) -> Result<LessSafeKey, EnvelopeError> {
    UnboundKey::new(&AES_256_GCM, data_key)
        .map(LessSafeKey::new)
        .map_err(|_| EnvelopeError::EncryptionFailed)
}

fn random_nonce(random: &SystemRandom) -> Result<[u8; NONCE_BYTES], EnvelopeError> {
    let mut nonce = [0_u8; NONCE_BYTES];
    random
        .fill(&mut nonce)
        .map_err(|_| EnvelopeError::EncryptionFailed)?;
    Ok(nonce)
}

fn render(envelope: &ParsedEnvelope) -> String {
    format!(
        "{ENVELOPE_PREFIX}:{}:{}:{}:{}:{}",
        envelope.key_version,
        STANDARD_NO_PAD.encode(envelope.data_key_nonce),
        STANDARD_NO_PAD.encode(&envelope.wrapped_data_key),
        STANDARD_NO_PAD.encode(envelope.payload_nonce),
        STANDARD_NO_PAD.encode(&envelope.payload),
    )
}

fn binary_field(value: Option<&str>, field: &'static str) -> Result<Vec<u8>, EnvelopeError> {
    let value = value.ok_or(EnvelopeError::NotAnEnvelope)?;
    STANDARD
        .decode(value)
        .or_else(|_| STANDARD_NO_PAD.decode(value))
        .map_err(|_| EnvelopeError::MalformedField(field))
}

fn fixed_field(
    value: Option<&str>,
    field: &'static str,
) -> Result<[u8; NONCE_BYTES], EnvelopeError> {
    binary_field(value, field)?
        .try_into()
        .map_err(|_| EnvelopeError::MalformedField(field))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kek(byte: u8) -> LessSafeKey {
        LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &[byte; 32]).unwrap())
    }

    fn context() -> CredentialContext {
        CredentialContext::integration_credential(
            Uuid::from_u128(1),
            Uuid::from_u128(2),
            TransportKind::Slack,
            IntegrationCredentialKind::BotAccessToken,
        )
    }

    fn sealed(secret: &str) -> String {
        seal(
            &kek(7),
            1,
            &SystemRandom::new(),
            &context(),
            &SecretString::from(secret.to_string()),
        )
        .unwrap()
    }

    #[test]
    fn a_credential_round_trips_under_its_own_context() {
        let stored = sealed("xoxb-token");

        assert!(stored.starts_with("enc:v2:1:"));
        let opened = open(&kek(7), &context(), &parse(&stored).unwrap()).unwrap();
        assert_eq!(opened.expose_secret(), "xoxb-token");
    }

    /// The reason the context exists: an operator who copies one row's ciphertext into another
    /// company's row must not get a working credential out of it.
    #[test]
    fn an_envelope_moved_to_another_row_fails_to_open() {
        let stored = sealed("xoxb-token");
        let envelope = parse(&stored).unwrap();

        for foreign in [
            CredentialContext::integration_credential(
                Uuid::from_u128(99),
                Uuid::from_u128(2),
                TransportKind::Slack,
                IntegrationCredentialKind::BotAccessToken,
            ),
            CredentialContext::integration_credential(
                Uuid::from_u128(1),
                Uuid::from_u128(99),
                TransportKind::Slack,
                IntegrationCredentialKind::BotAccessToken,
            ),
            CredentialContext::integration_credential(
                Uuid::from_u128(1),
                Uuid::from_u128(2),
                TransportKind::Email,
                IntegrationCredentialKind::BotAccessToken,
            ),
            CredentialContext::integration_credential(
                Uuid::from_u128(1),
                Uuid::from_u128(2),
                TransportKind::Slack,
                IntegrationCredentialKind::UserAccessToken,
            ),
        ] {
            assert_eq!(
                open(&kek(7), &foreign, &envelope).unwrap_err(),
                EnvelopeError::DataKeyAuthentication
            );
        }
    }

    #[test]
    fn the_two_layers_cannot_be_substituted_for_each_other() {
        let stored = sealed("xoxb-token");
        let envelope = parse(&stored).unwrap();
        let context = context();

        assert_ne!(
            context.associated_data(ContextLayer::DataKey),
            context.associated_data(ContextLayer::Payload)
        );
        // The wrapped data key presented as the payload authenticates under neither key.
        let substituted = ParsedEnvelope {
            payload: envelope.wrapped_data_key.clone(),
            payload_nonce: envelope.data_key_nonce,
            ..envelope
        };
        assert!(open(&kek(7), &context, &substituted).is_err());
    }

    #[test]
    fn tampering_with_any_field_fails_closed() {
        let stored = sealed("xoxb-token");
        let fields: Vec<&str> = stored.split(':').collect();

        for index in 3..fields.len() {
            let mut tampered = fields.clone();
            let flipped = flip_last_base64_char(tampered[index]);
            tampered[index] = &flipped;
            let candidate = tampered.join(":");
            let outcome = parse(&candidate).and_then(|envelope| {
                open(&kek(7), &context(), &envelope)
                    .map(|secret| secret.expose_secret().to_string())
            });
            assert!(
                outcome.is_err(),
                "field {index} was accepted after tampering"
            );
        }

        assert!(parse("plaintext-token").is_err());
        assert!(parse("enc:v1:1:AAAA:BBBB").is_err());
        assert!(parse(&format!("{stored}:extra")).is_err());
        assert!(parse(&stored.replacen("enc:v2:1:", "enc:v2:0:", 1)).is_err());
    }

    #[test]
    fn rewrapping_moves_the_key_version_without_touching_the_payload() {
        let stored = sealed("xoxb-token");
        let before = parse(&stored).unwrap();

        let rotated = rewrap(
            &kek(7),
            &kek(8),
            2,
            &SystemRandom::new(),
            &context(),
            &before,
        )
        .unwrap();
        let after = parse(&rotated).unwrap();

        assert_eq!(after.key_version, 2);
        assert_eq!(
            after.payload, before.payload,
            "the credential is not re-encrypted"
        );
        assert_ne!(after.wrapped_data_key, before.wrapped_data_key);
        assert_eq!(
            open(&kek(8), &context(), &after).unwrap().expose_secret(),
            "xoxb-token"
        );
        // The retired key can no longer open the rewrapped row, which is what makes a completed
        // rotation meaningful.
        assert!(open(&kek(7), &context(), &after).is_err());
    }

    #[test]
    fn every_sealing_of_the_same_secret_differs() {
        let first = parse(&sealed("same")).unwrap();
        let second = parse(&sealed("same")).unwrap();

        assert_ne!(first.payload, second.payload);
        assert_ne!(first.wrapped_data_key, second.wrapped_data_key);
        assert_ne!(first.payload_nonce, second.payload_nonce);
    }

    #[test]
    fn oversized_credentials_are_refused_at_the_boundary() {
        let error = seal(
            &kek(7),
            1,
            &SystemRandom::new(),
            &context(),
            &SecretString::from("x".repeat(MAX_CREDENTIAL_BYTES + 1)),
        )
        .unwrap_err();

        assert_eq!(error, EnvelopeError::TooLong);
    }

    #[test]
    fn no_error_or_debug_output_carries_secret_material() {
        let stored = sealed("xoxb-super-secret");
        let envelope = parse(&stored).unwrap();
        let rendered = format!(
            "{:?} {:?} {}",
            context(),
            EnvelopeError::PayloadAuthentication,
            open(&kek(8), &context(), &envelope).unwrap_err()
        );

        assert!(!rendered.contains("xoxb"));
        assert!(!rendered.contains(&STANDARD_NO_PAD.encode(&envelope.payload)));
    }

    fn flip_last_base64_char(field: &str) -> String {
        let mut characters: Vec<char> = field.chars().collect();
        let last = characters.len() - 1;
        characters[last] = if characters[last] == 'A' { 'B' } else { 'A' };
        characters.into_iter().collect()
    }
}
