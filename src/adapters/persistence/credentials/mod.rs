//! Credential encryption for the two shapes of secret this database stores.
//!
//! `enc:v1` — this module — is the original format: one long-lived environment key encrypts each
//! value directly, with empty associated data. It is what `company_model_connections` stores and
//! `plan/db_improve/improve-key-credentials.md` is the plan to retire it.
//!
//! [`envelope`] is `enc:v2`, the per-credential data-key format that integration credentials use.
//! New secret storage goes there; nothing new is added to `enc:v1`.

pub mod envelope;

use std::collections::HashMap;

use base64::{
    Engine,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
};
use ring::{
    aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey},
    rand::{SecureRandom, SystemRandom},
};

use secrecy::SecretString;

use crate::app_error::{AppError, AppResult};

pub use envelope::CredentialContext;

const PREFIX: &str = "enc:v1";
const KEYS_ENV: &str = "CREDENTIAL_ENCRYPTION_KEYS";
const ACTIVE_VERSION_ENV: &str = "CREDENTIAL_ENCRYPTION_ACTIVE_VERSION";

pub struct CredentialCipher {
    active_version: u32,
    keys: HashMap<u32, LessSafeKey>,
    random: SystemRandom,
}

/// How one stored credential column is protected.
///
/// The inventory in `credential_rotation.rs` walks two tables that do not share a format, and a
/// credential's format is a property of the *column*, not of the string: reading it off the
/// string would let a plaintext value pick its own reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CredentialFormat {
    /// `company_model_connections.api_key`: `enc:v1`, direct master key, no row context.
    LegacyDirectKey,
    /// `integration_credentials.envelope`: `enc:v2`, bound to the row it belongs to.
    Envelope(CredentialContext),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CredentialState {
    Active { version: u32 },
    Old { version: u32 },
    Unavailable { version: u32 },
    Malformed,
}

struct CredentialEnvelope {
    version: u32,
    nonce: [u8; 12],
    ciphertext: Vec<u8>,
}

impl CredentialCipher {
    pub fn from_env() -> AppResult<Self> {
        let encoded = std::env::var(KEYS_ENV).map_err(|_| {
            AppError::Internal(format!(
                "{KEYS_ENV} must contain version:base64-key entries"
            ))
        })?;
        let active_version = std::env::var(ACTIVE_VERSION_ENV).map_err(|_| {
            AppError::Internal(format!(
                "{ACTIVE_VERSION_ENV} must name a configured positive key version"
            ))
        })?;
        Self::parse(&encoded, &active_version)
    }

    fn parse(encoded: &str, active_version: &str) -> AppResult<Self> {
        let active_version = parse_positive_version(active_version, ACTIVE_VERSION_ENV)?;
        let mut keys = HashMap::new();
        if encoded.trim().is_empty() {
            return Err(AppError::Internal(format!("{KEYS_ENV} contains no keys")));
        }

        for entry in encoded.split(',') {
            let entry = entry.trim();
            if entry.is_empty() {
                return Err(AppError::Internal(format!(
                    "{KEYS_ENV} contains an empty key entry"
                )));
            }
            let (version, encoded_key) = entry.split_once(':').ok_or_else(|| {
                AppError::Internal("Credential key entries must be version:base64-key".into())
            })?;
            let version = parse_positive_version(version, "Credential key version")?;
            let bytes = decode_base64(encoded_key)
                .map_err(|_| AppError::Internal("Credential key must be base64".into()))?;
            let key = UnboundKey::new(&AES_256_GCM, &bytes).map_err(|_| {
                AppError::Internal("Credential encryption keys must be 32 bytes".into())
            })?;
            if keys.insert(version, LessSafeKey::new(key)).is_some() {
                return Err(AppError::Internal(
                    "Credential key versions must be unique".into(),
                ));
            }
        }
        if !keys.contains_key(&active_version) {
            return Err(AppError::Internal(format!(
                "{ACTIVE_VERSION_ENV} must refer to a version present in {KEYS_ENV}"
            )));
        }
        Ok(Self {
            active_version,
            keys,
            random: SystemRandom::new(),
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self::for_test_with_keys(&[(1, [42_u8; 32])], 1)
    }

    #[cfg(test)]
    pub(crate) fn for_test_with_keys(keys: &[(u32, [u8; 32])], active_version: u32) -> Self {
        let encoded = keys
            .iter()
            .map(|(version, key)| format!("{version}:{}", STANDARD_NO_PAD.encode(key)))
            .collect::<Vec<_>>()
            .join(",");
        Self::parse(&encoded, &active_version.to_string())
            .expect("the fixed test credential key ring is valid")
    }

    pub fn active_version(&self) -> u32 {
        self.active_version
    }

    pub fn available_versions(&self) -> Vec<u32> {
        let mut versions = self.keys.keys().copied().collect::<Vec<_>>();
        versions.sort_unstable();
        versions
    }

    pub fn encrypt(&self, plaintext: &str) -> AppResult<String> {
        let mut nonce_bytes = [0_u8; 12];
        self.random
            .fill(&mut nonce_bytes)
            .map_err(|_| AppError::Internal("Credential nonce generation failed".into()))?;
        let mut ciphertext = plaintext.as_bytes().to_vec();
        self.keys[&self.active_version]
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce_bytes),
                Aad::empty(),
                &mut ciphertext,
            )
            .map_err(|_| AppError::Internal("Credential encryption failed".into()))?;
        Ok(format!(
            "{PREFIX}:{}:{}:{}",
            self.active_version,
            STANDARD_NO_PAD.encode(nonce_bytes),
            STANDARD_NO_PAD.encode(ciphertext)
        ))
    }

    pub fn decrypt(&self, stored: &str) -> AppResult<String> {
        if !stored.starts_with("enc:") {
            return Ok(stored.to_string());
        }
        self.decrypt_parsed(parse_envelope(stored)?)
    }

    pub(crate) fn inspect(&self, stored: &str) -> CredentialState {
        let Ok(envelope) = parse_envelope(stored) else {
            return CredentialState::Malformed;
        };
        let version = envelope.version;
        if !self.keys.contains_key(&version) {
            return CredentialState::Unavailable { version };
        }
        if self.decrypt_parsed(envelope).is_err() {
            return CredentialState::Malformed;
        }
        if version == self.active_version {
            CredentialState::Active { version }
        } else {
            CredentialState::Old { version }
        }
    }

    pub(crate) fn decrypt_envelope(&self, stored: &str) -> AppResult<String> {
        self.decrypt_parsed(parse_envelope(stored)?)
    }

    /// Encrypt one integration credential under a fresh data key, wrapped by the active KEK.
    pub(crate) fn seal_envelope(
        &self,
        context: &CredentialContext,
        secret: &SecretString,
    ) -> AppResult<String> {
        let kek = self.key_encryption_key(self.active_version)?;
        envelope::seal(kek, self.active_version, &self.random, context, secret).map_err(Into::into)
    }

    /// Decrypt one integration credential. The caller must present the row's exact context.
    pub(crate) fn open_envelope(
        &self,
        context: &CredentialContext,
        stored: &str,
    ) -> AppResult<SecretString> {
        let parsed = envelope::parse(stored)?;
        let kek = self.key_encryption_key(parsed.key_version)?;
        envelope::open(kek, context, &parsed).map_err(Into::into)
    }

    /// Classify one stored credential without returning any of it.
    pub(crate) fn classify(&self, format: &CredentialFormat, stored: &str) -> CredentialState {
        match format {
            CredentialFormat::LegacyDirectKey => self.inspect(stored),
            CredentialFormat::Envelope(context) => self.classify_envelope(context, stored),
        }
    }

    fn classify_envelope(&self, context: &CredentialContext, stored: &str) -> CredentialState {
        let Ok(parsed) = envelope::parse(stored) else {
            return CredentialState::Malformed;
        };
        let Some(kek) = self.keys.get(&parsed.key_version) else {
            return CredentialState::Unavailable {
                version: parsed.key_version,
            };
        };
        // Opening, not merely unwrapping: a row whose payload was tampered with must classify as
        // malformed rather than rotate cleanly into the new key version.
        if envelope::open(kek, context, &parsed).is_err() {
            return CredentialState::Malformed;
        }
        if parsed.key_version == self.active_version {
            CredentialState::Active {
                version: parsed.key_version,
            }
        } else {
            CredentialState::Old {
                version: parsed.key_version,
            }
        }
    }

    /// Move one stored credential onto the active key version.
    ///
    /// The envelope path rewraps the data key and never materializes the credential; the legacy
    /// path has no such option and decrypts, which is one more reason it is being retired.
    pub(crate) fn rotate_to_active(
        &self,
        format: &CredentialFormat,
        stored: &str,
    ) -> AppResult<String> {
        match format {
            CredentialFormat::LegacyDirectKey => self.encrypt(&self.decrypt_envelope(stored)?),
            CredentialFormat::Envelope(context) => {
                let parsed = envelope::parse(stored)?;
                let current = self.key_encryption_key(parsed.key_version)?;
                let target = self.key_encryption_key(self.active_version)?;
                envelope::rewrap(
                    current,
                    target,
                    self.active_version,
                    &self.random,
                    context,
                    &parsed,
                )
                .map_err(Into::into)
            }
        }
    }

    /// The key-encryption key for one envelope version, as an envelope failure class rather than a
    /// generic error, so an unavailable version is distinguishable from a tampered row in a log.
    fn key_encryption_key(&self, version: u32) -> Result<&LessSafeKey, envelope::EnvelopeError> {
        self.keys
            .get(&version)
            .ok_or(envelope::EnvelopeError::UnavailableKeyVersion(version))
    }

    fn decrypt_parsed(&self, envelope: CredentialEnvelope) -> AppResult<String> {
        let key = self.keys.get(&envelope.version).ok_or_else(|| {
            AppError::Internal(format!(
                "Credential key version {} is unavailable",
                envelope.version
            ))
        })?;
        let mut ciphertext = envelope.ciphertext;
        let plaintext = key
            .open_in_place(
                Nonce::assume_unique_for_key(envelope.nonce),
                Aad::empty(),
                &mut ciphertext,
            )
            .map_err(|_| AppError::Internal("Credential decryption failed".into()))?;
        String::from_utf8(plaintext.to_vec())
            .map_err(|_| AppError::Internal("Decrypted credential is not UTF-8".into()))
    }
}

fn parse_positive_version(value: &str, name: &str) -> AppResult<u32> {
    let version = value
        .trim()
        .parse::<u32>()
        .map_err(|_| AppError::Internal(format!("{name} must be a positive integer")))?;
    if version == 0 {
        return Err(AppError::Internal(format!(
            "{name} must be a positive integer"
        )));
    }
    Ok(version)
}

fn decode_base64(value: &str) -> Result<Vec<u8>, base64::DecodeError> {
    STANDARD
        .decode(value)
        .or_else(|_| STANDARD_NO_PAD.decode(value))
}

fn parse_envelope(stored: &str) -> AppResult<CredentialEnvelope> {
    let mut parts = stored.split(':');
    let valid_prefix = parts.next() == Some("enc") && parts.next() == Some("v1");
    let version = parts.next().and_then(|value| value.parse::<u32>().ok());
    let nonce = parts.next().and_then(|value| decode_base64(value).ok());
    let ciphertext = parts.next().and_then(|value| decode_base64(value).ok());
    if !valid_prefix || parts.next().is_some() {
        return Err(AppError::Internal("Malformed encrypted credential".into()));
    }
    let version = version
        .filter(|version| *version > 0)
        .ok_or_else(|| AppError::Internal("Malformed credential version".into()))?;
    let nonce = nonce
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| AppError::Internal("Malformed credential nonce".into()))?;
    let ciphertext = ciphertext
        .filter(|bytes| bytes.len() >= AES_256_GCM.tag_len())
        .ok_or_else(|| AppError::Internal("Malformed credential ciphertext".into()))?;
    Ok(CredentialEnvelope {
        version,
        nonce,
        ciphertext,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoded_key(byte: u8) -> String {
        STANDARD_NO_PAD.encode([byte; 32])
    }

    #[test]
    fn explicit_active_version_is_required_and_must_be_available() {
        let key = encoded_key(7);
        assert!(CredentialCipher::parse(&format!("1:{key}"), "").is_err());
        assert!(CredentialCipher::parse(&format!("1:{key}"), "0").is_err());
        assert!(CredentialCipher::parse(&format!("1:{key}"), "2").is_err());
        assert!(CredentialCipher::parse(&format!("0:{key}"), "0").is_err());
    }

    #[test]
    fn adding_a_higher_key_does_not_activate_it() {
        let cipher =
            CredentialCipher::parse(&format!("1:{},2:{}", encoded_key(7), encoded_key(8)), "1")
                .unwrap();

        assert_eq!(cipher.active_version(), 1);
        assert!(cipher.encrypt("secret").unwrap().starts_with("enc:v1:1:"));
    }

    #[test]
    fn machines_on_either_active_version_decrypt_both_versions() {
        let keys = [(1, [7_u8; 32]), (2, [8_u8; 32])];
        let old_writer = CredentialCipher::for_test_with_keys(&keys, 1);
        let new_writer = CredentialCipher::for_test_with_keys(&keys, 2);
        let old_ciphertext = old_writer.encrypt("old-write").unwrap();
        let new_ciphertext = new_writer.encrypt("new-write").unwrap();

        assert_eq!(old_writer.decrypt(&new_ciphertext).unwrap(), "new-write");
        assert_eq!(new_writer.decrypt(&old_ciphertext).unwrap(), "old-write");
    }

    #[test]
    fn reusing_a_version_with_different_key_bytes_cannot_decrypt_existing_data() {
        let original = CredentialCipher::for_test_with_keys(&[(1, [7_u8; 32])], 1);
        let ciphertext = original.encrypt("secret").unwrap();
        let replacement = CredentialCipher::for_test_with_keys(&[(1, [8_u8; 32])], 1);

        assert!(replacement.decrypt(&ciphertext).is_err());
        assert_eq!(replacement.inspect(&ciphertext), CredentialState::Malformed);
    }

    #[test]
    fn inspection_authenticates_and_categorizes_without_returning_values() {
        let old = CredentialCipher::for_test_with_keys(&[(1, [7_u8; 32])], 1);
        let old_ciphertext = old.encrypt("secret").unwrap();
        let rotating = CredentialCipher::for_test_with_keys(&[(1, [7_u8; 32]), (2, [8_u8; 32])], 2);
        let unavailable = old_ciphertext.replacen("enc:v1:1:", "enc:v1:3:", 1);

        assert_eq!(
            rotating.inspect(&old_ciphertext),
            CredentialState::Old { version: 1 }
        );
        assert_eq!(
            rotating.inspect(&rotating.encrypt("secret").unwrap()),
            CredentialState::Active { version: 2 }
        );
        assert_eq!(
            rotating.inspect(&unavailable),
            CredentialState::Unavailable { version: 3 }
        );
        assert_eq!(rotating.inspect("plaintext"), CredentialState::Malformed);
        assert_eq!(
            rotating.inspect("enc:v1:2:not-a-nonce:not-ciphertext"),
            CredentialState::Malformed
        );
    }

    #[test]
    fn duplicate_and_empty_key_entries_are_rejected() {
        let key = encoded_key(7);
        assert!(CredentialCipher::parse(&format!("1:{key},1:{key}"), "1").is_err());
        assert!(CredentialCipher::parse(&format!("1:{key},"), "1").is_err());
        assert!(CredentialCipher::parse("", "1").is_err());
    }
}
