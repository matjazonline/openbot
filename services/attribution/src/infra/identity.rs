use crate::adapters::identity::crypto::{Cipher, RotatingSecret, secret};
use anyhow::{Result, bail};
use std::sync::Arc;
use url::Url;

pub struct IdentityConfig {
    pub openbot_origin: Url,
    pub public_origin: Url,
    pub client_secret: String,
    pub lifecycle_secret: RotatingSecret,
    pub cipher: Arc<Cipher>,
}
impl IdentityConfig {
    pub fn from_env(public_origin: &Url) -> Result<Self> {
        Self::from_lookup(public_origin, |name| std::env::var(name).ok())
    }
    pub fn from_lookup(
        public_origin: &Url,
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<Self> {
        let required =
            |name: &str| lookup(name).ok_or_else(|| anyhow::anyhow!("{name} is required"));
        let openbot_origin = super::config::origin(&required("ATTRIBUTION_OPENBOT_ORIGIN")?).map_err(|_| anyhow::anyhow!("ATTRIBUTION_OPENBOT_ORIGIN must be a valid HTTPS origin (loopback HTTP for tests)"))?;
        if openbot_origin.origin() == public_origin.origin() {
            bail!("Openbot and attribution require separate origins");
        }
        let client_secret = required("ATTRIBUTION_CLIENT_SECRET")?;
        secret(&client_secret).map_err(|_| {
            anyhow::anyhow!("ATTRIBUTION_CLIENT_SECRET must encode 32 random bytes")
        })?;
        let lifecycle = required("ATTRIBUTION_LIFECYCLE_SECRET")?;
        let previous = lookup("ATTRIBUTION_PREVIOUS_LIFECYCLE_SECRET");
        if lifecycle == client_secret || previous.as_ref() == Some(&client_secret) {
            bail!("client and lifecycle credentials must be distinct");
        }
        let expiry = lookup("ATTRIBUTION_PREVIOUS_LIFECYCLE_SECRET_EXPIRES_AT");
        let previous = match (&previous, &expiry) {
            (None, None) => None,
            (Some(value), Some(expiry)) => Some((value.as_str(), expiry.as_str())),
            _ => bail!("previous lifecycle secret requires its expiry"),
        };
        let lifecycle_secret = RotatingSecret::new(&lifecycle, previous)?;
        let cipher = Arc::new(Cipher::from_json(&required("ATTRIBUTION_SESSION_KEYS")?)?);
        Ok(Self {
            openbot_origin,
            public_origin: public_origin.clone(),
            client_secret,
            lifecycle_secret,
            cipher,
        })
    }
}

pub fn compose(
    pool: sqlx::PgPool,
    config: Arc<IdentityConfig>,
) -> Result<crate::adapters::identity::http::IdentityHttp> {
    use crate::{
        adapters::identity::{
            authority::HttpAuthority, http::IdentityHttp, postgres::IdentityPostgres,
        },
        application::identity::IdentityService,
    };
    let authority = Arc::new(HttpAuthority::new(config.clone())?);
    let store = Arc::new(IdentityPostgres {
        pool,
        cipher: config.cipher.clone(),
    });
    let service = Arc::new(IdentityService::new(authority, store.clone(), store));
    Ok(IdentityHttp::new(service, config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

    #[test]
    fn attribution_session_configuration_rejects_incomplete_and_cross_class_credentials() {
        let origin = Url::parse("https://attribution.example.test").unwrap();
        let client = URL_SAFE_NO_PAD.encode([11u8; 32]);
        let lifecycle = URL_SAFE_NO_PAD.encode([12u8; 32]);
        let keys = serde_json::json!({"active": 1, "keys": [{"version": 1, "key": URL_SAFE_NO_PAD.encode([13u8; 32])}]}).to_string();
        for mode in 0..6 {
            let result = IdentityConfig::from_lookup(&origin, |name| match name {
                "ATTRIBUTION_OPENBOT_ORIGIN" => Some(
                    if mode == 1 {
                        "http://remote.test"
                    } else {
                        "https://openbot.example.test"
                    }
                    .into(),
                ),
                "ATTRIBUTION_CLIENT_SECRET" if mode != 2 => Some(client.clone()),
                "ATTRIBUTION_LIFECYCLE_SECRET" => Some(if mode == 3 {
                    client.clone()
                } else {
                    lifecycle.clone()
                }),
                "ATTRIBUTION_SESSION_KEYS" => {
                    Some(if mode == 4 { "{}".into() } else { keys.clone() })
                }
                "ATTRIBUTION_PREVIOUS_LIFECYCLE_SECRET" if mode == 5 => Some(client.clone()),
                _ => None,
            });
            assert_eq!(result.is_ok(), mode == 0, "configuration case {mode}");
        }
    }
}
