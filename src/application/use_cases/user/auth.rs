//! Authentication digests and admitted-work ownership.
use hmac::{Hmac, Mac};
use secrecy::{ExposeSecret, SecretString};
use sha2::Sha256;
use std::{any::Any, sync::Arc};

/// Independently configured authentication signing key. Debug is redacted by SecretString.
#[derive(Clone, Debug)]
pub struct AuthAttemptSecret(SecretString);
impl AuthAttemptSecret {
    pub(crate) fn expose_secret(&self) -> &str {
        self.0.expose_secret()
    }
}
impl From<String> for AuthAttemptSecret {
    fn from(value: String) -> Self {
        Self(value.into())
    }
}
impl From<&str> for AuthAttemptSecret {
    fn from(value: &str) -> Self {
        value.to_owned().into()
    }
}
impl From<Arc<str>> for AuthAttemptSecret {
    fn from(value: Arc<str>) -> Self {
        value.to_string().into()
    }
}
/// A domain-separated keyed digest. Neither identity nor source is persisted in plaintext.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AuthAttemptKey([u8; 32]);

/// Opaque ownership of an admitted authentication request. The HTTP adapter supplies a source
/// guard; a blocking password job retains a clone until the worker actually finishes.
#[derive(Clone)]
pub struct AuthWorkLifetime {
    _owner: Arc<dyn Any + Send + Sync>,
}

impl AuthWorkLifetime {
    pub(crate) fn new<T: Any + Send + Sync>(owner: T) -> Self {
        Self {
            _owner: Arc::new(owner),
        }
    }
}

impl AuthAttemptKey {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

pub(crate) fn auth_attempt_key(secret: &str, domain: &str, components: &[&[u8]]) -> AuthAttemptKey {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC-SHA256 accepts any key length");
    mac.update(domain.as_bytes());
    for component in components {
        mac.update(&(component.len() as u64).to_be_bytes());
        mac.update(component);
    }
    AuthAttemptKey(mac.finalize().into_bytes().into())
}
