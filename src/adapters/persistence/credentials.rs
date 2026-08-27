use std::collections::HashMap;

use base64::{Engine, engine::general_purpose::STANDARD_NO_PAD};
use ring::{
    aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey},
    rand::{SecureRandom, SystemRandom},
};

use crate::app_error::{AppError, AppResult};

const PREFIX: &str = "enc:v1";

pub struct CredentialCipher {
    active_version: u32,
    keys: HashMap<u32, LessSafeKey>,
    random: SystemRandom,
}

impl CredentialCipher {
    pub fn from_env() -> AppResult<Self> {
        let encoded = std::env::var("CREDENTIAL_ENCRYPTION_KEYS").map_err(|_| {
            AppError::Internal(
                "CREDENTIAL_ENCRYPTION_KEYS must contain version:base64-key entries".into(),
            )
        })?;
        Self::parse(&encoded)
    }

    fn parse(encoded: &str) -> AppResult<Self> {
        let mut keys = HashMap::new();
        for entry in encoded
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            let (version, key) = entry.split_once(':').ok_or_else(|| {
                AppError::Internal("Credential key entries must be version:base64-key".into())
            })?;
            let version = version.parse::<u32>().map_err(|_| {
                AppError::Internal("Credential key version must be an integer".into())
            })?;
            let bytes = STANDARD_NO_PAD
                .decode(key)
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
        let active_version = keys.keys().copied().max().ok_or_else(|| {
            AppError::Internal("CREDENTIAL_ENCRYPTION_KEYS contains no keys".into())
        })?;
        Ok(Self {
            active_version,
            keys,
            random: SystemRandom::new(),
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self::parse(&format!("1:{}", STANDARD_NO_PAD.encode([42_u8; 32])))
            .expect("the fixed test credential key is valid")
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
        let mut parts = stored.split(':');
        let valid_prefix = parts.next() == Some("enc") && parts.next() == Some("v1");
        let version = parts.next().and_then(|value| value.parse::<u32>().ok());
        let nonce = parts
            .next()
            .and_then(|value| STANDARD_NO_PAD.decode(value).ok());
        let ciphertext = parts
            .next()
            .and_then(|value| STANDARD_NO_PAD.decode(value).ok());
        if !valid_prefix || parts.next().is_some() {
            return Err(AppError::Internal("Malformed encrypted credential".into()));
        }
        let version =
            version.ok_or_else(|| AppError::Internal("Malformed credential version".into()))?;
        let key = self.keys.get(&version).ok_or_else(|| {
            AppError::Internal(format!("Credential key version {version} is unavailable"))
        })?;
        let nonce: [u8; 12] = nonce
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(|| AppError::Internal("Malformed credential nonce".into()))?;
        let mut ciphertext = ciphertext
            .ok_or_else(|| AppError::Internal("Malformed credential ciphertext".into()))?;
        let plaintext = key
            .open_in_place(
                Nonce::assume_unique_for_key(nonce),
                Aad::empty(),
                &mut ciphertext,
            )
            .map_err(|_| AppError::Internal("Credential decryption failed".into()))?;
        String::from_utf8(plaintext.to_vec())
            .map_err(|_| AppError::Internal("Decrypted credential is not UTF-8".into()))
    }

    pub fn needs_rotation(&self, stored: &str) -> bool {
        !stored.starts_with(&format!("{PREFIX}:{}:", self.active_version))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ciphertext_round_trips_and_old_versions_can_rotate() {
        let first = STANDARD_NO_PAD.encode([7_u8; 32]);
        let second = STANDARD_NO_PAD.encode([8_u8; 32]);
        let old = CredentialCipher::parse(&format!("1:{first}")).unwrap();
        let ciphertext = old.encrypt("sk-secret").unwrap();
        let rotating = CredentialCipher::parse(&format!("1:{first},2:{second}")).unwrap();
        assert_eq!(rotating.decrypt(&ciphertext).unwrap(), "sk-secret");
        assert!(rotating.needs_rotation(&ciphertext));
        assert_ne!(ciphertext, rotating.encrypt("sk-secret").unwrap());
    }
}
