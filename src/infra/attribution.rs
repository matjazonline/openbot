//! Optional deployment registration for the dedicated attribution application.
use crate::{
    adapters::{
        http::{attribution::Bridge, session::SessionAuthority},
        persistence::PostgresPersistence,
    },
    application::attribution::SecretHash,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use subtle::ConstantTimeEq;
use url::Url;

pub struct AttributionConfig {
    pub origin: Url,
    pub callback: String,
    pub delivery: String,
    client_hash: Vec<u8>,
    previous: Option<(Vec<u8>, DateTime<Utc>)>,
    pub(crate) cursor_key: [u8; 32],
    pub lifecycle_secret: String,
}
pub fn hash(value: &str) -> SecretHash {
    SecretHash(Sha256::digest(value.as_bytes()).to_vec())
}
fn secret(value: &str) -> anyhow::Result<[u8; 32]> {
    URL_SAFE_NO_PAD
        .decode(value)
        .ok()
        .and_then(|v| v.try_into().ok())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "attribution secret must encode exactly 32 random bytes as unpadded base64url"
            )
        })
}
impl AttributionConfig {
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> anyhow::Result<Option<Self>> {
        let names = [
            "OPENBOT_ATTRIBUTION_ORIGIN",
            "OPENBOT_ATTRIBUTION_CLIENT_SECRET",
            "OPENBOT_ATTRIBUTION_LIFECYCLE_SECRET",
            "OPENBOT_ATTRIBUTION_PREVIOUS_CLIENT_SECRET",
            "OPENBOT_ATTRIBUTION_PREVIOUS_CLIENT_SECRET_EXPIRES_AT",
        ];
        if names.iter().all(|name| lookup(name).is_none()) {
            return Ok(None);
        }
        let required =
            |name: &str| lookup(name).ok_or_else(|| anyhow::anyhow!("{name} is required"));
        let raw = required(names[0])?;
        if raw.len() > 2048 {
            anyhow::bail!("OPENBOT_ATTRIBUTION_ORIGIN is too long");
        }
        let origin = Url::parse(&raw)
            .map_err(|_| anyhow::anyhow!("OPENBOT_ATTRIBUTION_ORIGIN is invalid"))?;
        let loopback = match origin.host() {
            Some(url::Host::Domain("localhost")) => true,
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            _ => false,
        };
        if origin.host().is_none()
            || !(origin.scheme() == "https" || (origin.scheme() == "http" && loopback))
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.path() != "/"
            || origin.query().is_some()
            || origin.fragment().is_some()
        {
            anyhow::bail!(
                "OPENBOT_ATTRIBUTION_ORIGIN must be an HTTPS origin (loopback HTTP for tests), without credentials/path/query/fragment"
            );
        }
        let client = required(names[1])?;
        let cursor_key = secret(&client)?;
        let lifecycle_secret = required(names[2])?;
        secret(&lifecycle_secret)?;
        if lifecycle_secret == client || lookup(names[3]).as_ref() == Some(&lifecycle_secret) {
            anyhow::bail!("attribution client and lifecycle credentials must be distinct");
        }
        let previous = match (lookup(names[3]), lookup(names[4])) {
            (None, None) => None,
            (Some(value), Some(expiry)) => {
                secret(&value)?;
                let expiry = DateTime::parse_from_rfc3339(&expiry)
                    .map_err(|_| {
                        anyhow::anyhow!("previous attribution secret expiry must be RFC3339")
                    })?
                    .with_timezone(&Utc);
                if expiry <= Utc::now() || expiry > Utc::now() + chrono::Duration::days(1) {
                    anyhow::bail!("previous attribution secret expiry must be within 24 hours");
                }
                Some((hash(&value).0, expiry))
            }
            _ => anyhow::bail!("previous attribution client secret requires its expiry"),
        };
        Ok(Some(Self {
            callback: format!("{origin}auth/callback"),
            delivery: format!("{origin}internal/v1/company-lifecycle"),
            origin,
            client_hash: hash(&client).0,
            previous,
            cursor_key,
            lifecycle_secret,
        }))
    }
    pub fn accepts(&self, secret: &str) -> bool {
        if secret.len() > 128 {
            return false;
        }
        let supplied = hash(secret).0;
        bool::from(self.client_hash.ct_eq(&supplied))
            | self.previous.as_ref().is_some_and(|(hash, expiry)| {
                bool::from(hash.ct_eq(&supplied)) && Utc::now() < *expiry
            })
    }
}
pub fn build(
    store: Arc<PostgresPersistence>,
    sessions: Arc<SessionAuthority>,
) -> anyhow::Result<Option<Arc<Bridge>>> {
    AttributionConfig::from_lookup(|name| std::env::var(name).ok())?
        .map(|config| Bridge::new(Arc::new(config), store.clone(), store, sessions).map(Arc::new))
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribution_session_registration_is_complete_and_credentials_are_separate() {
        assert!(AttributionConfig::from_lookup(|_| None).unwrap().is_none());
        let client = URL_SAFE_NO_PAD.encode([11u8; 32]);
        let lifecycle = URL_SAFE_NO_PAD.encode([12u8; 32]);
        for mode in 0..5 {
            let result = AttributionConfig::from_lookup(|name| match name {
                "OPENBOT_ATTRIBUTION_ORIGIN" => Some(
                    if mode == 1 {
                        "http://remote.test"
                    } else {
                        "https://attribution.example.test"
                    }
                    .into(),
                ),
                "OPENBOT_ATTRIBUTION_CLIENT_SECRET" if mode != 2 => Some(client.clone()),
                "OPENBOT_ATTRIBUTION_LIFECYCLE_SECRET" => Some(if mode == 3 {
                    client.clone()
                } else {
                    lifecycle.clone()
                }),
                "OPENBOT_ATTRIBUTION_PREVIOUS_CLIENT_SECRET" if mode == 4 => {
                    Some(lifecycle.clone())
                }
                _ => None,
            });
            assert_eq!(result.is_ok(), mode == 0, "registration case {mode}");
        }
    }
}
