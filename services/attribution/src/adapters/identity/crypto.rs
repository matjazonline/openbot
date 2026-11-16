use crate::application::identity::{Failure, Result};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, Generate, KeyInit, Payload},
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use subtle::ConstantTimeEq;
use uuid::Uuid;

pub fn random() -> String {
    oauth2::CsrfToken::new_random_len(32).secret().to_owned()
}
pub fn verifier(value: &str) -> Vec<u8> {
    Sha256::digest(value.as_bytes()).to_vec()
}
pub fn secret(value: &str) -> anyhow::Result<[u8; 32]> {
    URL_SAFE_NO_PAD
        .decode(value)
        .ok()
        .and_then(|v| v.try_into().ok())
        .ok_or_else(|| anyhow::anyhow!("secret must encode exactly 32 bytes as unpadded base64url"))
}
pub struct RotatingSecret {
    current: Vec<u8>,
    previous: Option<(Vec<u8>, DateTime<Utc>)>,
}
impl RotatingSecret {
    pub fn new(current: &str, previous: Option<(&str, &str)>) -> anyhow::Result<Self> {
        secret(current)?;
        let previous = match previous {
            None => None,
            Some((value, expiry)) => {
                secret(value)?;
                let expiry = DateTime::parse_from_rfc3339(expiry)
                    .map_err(|_| anyhow::anyhow!("previous secret expiry must be RFC3339"))?
                    .with_timezone(&Utc);
                if expiry <= Utc::now() || expiry > Utc::now() + chrono::Duration::days(1) {
                    anyhow::bail!("previous secret expiry must be within 24 hours");
                }
                Some((verifier(value), expiry))
            }
        };
        Ok(Self {
            current: verifier(current),
            previous,
        })
    }
    pub fn accepts(&self, value: &str) -> bool {
        if value.len() > 128 {
            return false;
        }
        let supplied = verifier(value);
        let current = bool::from(self.current.ct_eq(&supplied));
        let previous = self.previous.as_ref().is_some_and(|(hash, expiry)| {
            bool::from(hash.ct_eq(&supplied)) && Utc::now() < *expiry
        });
        current | previous
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyConfig {
    active: u32,
    keys: Vec<KeyEntry>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyEntry {
    version: u32,
    key: String,
}
pub struct Cipher {
    active: u32,
    keys: BTreeMap<u32, XChaCha20Poly1305>,
}
impl Cipher {
    pub fn from_json(value: &str) -> anyhow::Result<Self> {
        if value.len() > 4096 {
            anyhow::bail!("session key configuration too large");
        }
        let config: KeyConfig = serde_json::from_str(value)
            .map_err(|_| anyhow::anyhow!("invalid session key configuration"))?;
        if config.keys.is_empty() || config.keys.len() > 4 {
            anyhow::bail!("session key ring requires 1..=4 keys");
        }
        let mut keys = BTreeMap::new();
        for entry in config.keys {
            let key = secret(&entry.key)?;
            if entry.version == 0
                || keys
                    .insert(
                        entry.version,
                        XChaCha20Poly1305::new_from_slice(&key).unwrap(),
                    )
                    .is_some()
            {
                anyhow::bail!("session key versions must be positive and unique");
            }
        }
        if !keys.contains_key(&config.active) {
            anyhow::bail!("active session key is missing");
        }
        Ok(Self {
            active: config.active,
            keys,
        })
    }
    pub fn seal(&self, purpose: &'static str, id: Uuid, plaintext: &[u8]) -> Result<Vec<u8>> {
        let nonce = XNonce::generate();
        let aad = format!("attribution:{purpose}:{id}:{}", self.active);
        let ciphertext = self.keys[&self.active]
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| Failure::Unavailable)?;
        let mut result = self.active.to_be_bytes().to_vec();
        result.extend_from_slice(&nonce);
        result.extend(ciphertext);
        Ok(result)
    }
    pub fn open(&self, purpose: &'static str, id: Uuid, ciphertext: &[u8]) -> Result<Vec<u8>> {
        if ciphertext.len() < 44 || ciphertext.len() > 4096 {
            return Err(Failure::Unavailable);
        }
        let version = u32::from_be_bytes(ciphertext[..4].try_into().unwrap());
        let key = self.keys.get(&version).ok_or(Failure::Unavailable)?;
        let nonce = XNonce::try_from(&ciphertext[4..28]).map_err(|_| Failure::Unavailable)?;
        let aad = format!("attribution:{purpose}:{id}:{version}");
        key.decrypt(
            &nonce,
            Payload {
                msg: &ciphertext[28..],
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| Failure::Unavailable)
    }
}
pub fn local_return(value: &str) -> Result<String> {
    // Percent escapes are unnecessary for the owned navigation paths and can disguise separators.
    if value.len() > 256
        || !value.starts_with('/')
        || value.starts_with("//")
        || value
            .chars()
            .any(|c| c.is_control() || c == '\\' || c == '%' || c == '#')
    {
        return Err(Failure::Invalid);
    }
    Ok(value.to_owned())
}
