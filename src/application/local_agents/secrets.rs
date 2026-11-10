//! Canonical high-entropy enrollment material; only verifiers cross the persistence port.
use crate::app_error::{AppError, AppResult};
use ring::rand::{SecureRandom, SystemRandom};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fmt;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn canonical(value: &str, bytes: usize) -> bool {
    value.len() == bytes * 2
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

macro_rules! verifier {
    ($($name:ident),+ $(,)?) => {$(
        #[derive(Clone, Copy, PartialEq, Eq, Deserialize)]
        #[serde(try_from = "String")]
        pub struct $name([u8; 32]);
        impl $name {
            pub fn as_bytes(&self) -> &[u8; 32] { &self.0 }
            pub fn from_secret(value: &str) -> Self { Self(Sha256::digest(value.as_bytes()).into()) }
        }
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(concat!(stringify!($name), "([REDACTED])")) }
        }
        impl TryFrom<String> for $name {
            type Error = &'static str;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                if !canonical(&value, 32) { return Err("invalid verifier encoding"); }
                let mut bytes = [0; 32];
                for (index, byte) in bytes.iter_mut().enumerate() {
                    *byte = u8::from_str_radix(&value[index*2..index*2+2], 16).map_err(|_| "invalid verifier encoding")?;
                }
                Ok(Self(bytes))
            }
        }
    )+};
}
verifier!(PairingCodeHash, ClaimVerifier, CredentialVerifier);

macro_rules! secret {
    ($name:ident, $bytes:literal, $verifier:ident) => {
        #[derive(Clone, Debug, Deserialize)]
        #[serde(try_from = "String")]
        pub struct $name(SecretString);
        impl $name {
            pub fn generate() -> AppResult<Self> {
                let mut bytes = [0; $bytes];
                SystemRandom::new()
                    .fill(&mut bytes)
                    .map_err(|_| AppError::Internal("secure random source unavailable".into()))?;
                Ok(Self(hex(&bytes).into()))
            }
            pub fn expose_secret(&self) -> &str {
                self.0.expose_secret()
            }
            pub fn verifier(&self) -> $verifier {
                $verifier::from_secret(self.expose_secret())
            }
        }
        impl TryFrom<String> for $name {
            type Error = &'static str;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                if !canonical(&value, $bytes) {
                    return Err("invalid enrollment secret encoding");
                }
                Ok(Self(value.into()))
            }
        }
    };
}
secret!(PairingCode, 16, PairingCodeHash);
secret!(PairingClaimSecret, 32, ClaimVerifier);
secret!(MachineCredential, 32, CredentialVerifier);
