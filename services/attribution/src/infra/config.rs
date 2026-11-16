use std::{net::SocketAddr, str::FromStr};

use anyhow::{Result, bail};
use sqlx::{ConnectOptions, postgres::PgConnectOptions};
use url::Url;

pub const RUNTIME_THREAD_STACK_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_CONNECTIONS: usize = 64;
pub const MAX_REQUESTS: usize = 32;
pub const MAX_BODY_BYTES: usize = 256 * 1024;

/// Secrets deliberately have no Debug/Display implementation.
pub struct Config {
    pub bind: SocketAddr,
    pub public_origin: Url,
    pub database: PgConnectOptions,
    pub pool_size: u32,
    pub commerce: crate::application::commerce::CommercePolicy,
    pub retention: crate::application::retention::RetentionPolicy,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let database = lookup("ATTRIBUTION_DATABASE_URL")
            .ok_or_else(|| anyhow::anyhow!("ATTRIBUTION_DATABASE_URL is required"))?;
        if !database.starts_with("postgres://") && !database.starts_with("postgresql://") {
            bail!("ATTRIBUTION_DATABASE_URL must be a PostgreSQL URL");
        }
        let database = PgConnectOptions::from_str(&database)
            .map_err(|_| anyhow::anyhow!("ATTRIBUTION_DATABASE_URL is invalid"))?
            .disable_statement_logging()
            .options([
                ("statement_timeout", "5000"),
                ("lock_timeout", "2000"),
                ("idle_in_transaction_session_timeout", "5000"),
            ]);
        let raw_origin = lookup("ATTRIBUTION_PUBLIC_ORIGIN")
            .ok_or_else(|| anyhow::anyhow!("ATTRIBUTION_PUBLIC_ORIGIN is required"))?;
        let public_origin = origin(&raw_origin)?;
        let bind = lookup("ATTRIBUTION_BIND_ADDR")
            .unwrap_or_else(|| "127.0.0.1:8091".into())
            .parse()
            .map_err(|_| anyhow::anyhow!("ATTRIBUTION_BIND_ADDR is invalid"))?;
        let pool_size = lookup("ATTRIBUTION_DB_MAX_CONNECTIONS")
            .unwrap_or_else(|| "5".into())
            .parse::<u32>()
            .map_err(|_| anyhow::anyhow!("ATTRIBUTION_DB_MAX_CONNECTIONS must be 1..=32"))?;
        if !(1..=32).contains(&pool_size) {
            bail!("ATTRIBUTION_DB_MAX_CONNECTIONS must be 1..=32");
        }
        let days = |key: &str| -> Result<u32> {
            let value = lookup(key).ok_or_else(|| anyhow::anyhow!("{key} is required"))?;
            value
                .parse::<u32>()
                .ok()
                .filter(|value| (1..=3650).contains(value))
                .ok_or_else(|| anyhow::anyhow!("{key} must be 1..=3650 days"))
        };
        let commerce = crate::application::commerce::CommercePolicy::new(
            days("ATTRIBUTION_EVENT_MAX_AGE_DAYS")?,
            days("ATTRIBUTION_REFUND_WINDOW_DAYS")?,
        )
        .map_err(|_| anyhow::anyhow!("invalid commerce horizons"))?;
        let retention_days = |key: &str| -> Result<u32> {
            lookup(key)
                .and_then(|v| v.parse::<u32>().ok())
                .filter(|v| (1..=10000).contains(v))
                .ok_or_else(|| anyhow::anyhow!("{key} is required and must be 1..=10000 days"))
        };
        let retention = crate::application::retention::RetentionPolicy::new(commerce,
            retention_days("ATTRIBUTION_CLICK_RETENTION_DAYS")?,
            retention_days("ATTRIBUTION_OBSERVATION_RETENTION_DAYS")?,
            retention_days("ATTRIBUTION_FINANCIAL_RETENTION_DAYS")?,
            retention_days("ATTRIBUTION_EVENT_RETENTION_DAYS")?,
            retention_days("ATTRIBUTION_AUDIT_RETENTION_DAYS")?,
        ).map_err(|_| anyhow::anyhow!("retention must cover attribution, delivery and refund horizons with a one-day margin; observations require at least two days"))?;
        Ok(Self {
            retention,
            bind,
            public_origin,
            database,
            pool_size,
            commerce,
        })
    }
}

pub(super) fn origin(value: &str) -> Result<Url> {
    let invalid = || {
        anyhow::anyhow!(
            "ATTRIBUTION_PUBLIC_ORIGIN must be an HTTPS origin (HTTP only on loopback), without credentials, path, query or fragment"
        )
    };
    if value.len() > 2048 {
        return Err(invalid());
    }
    let url = Url::parse(value).map_err(|_| invalid())?;
    let loopback = match url.host() {
        Some(url::Host::Domain("localhost")) => true,
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    };
    if url.host().is_none()
        || !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid());
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(overrides: &[(&str, &str)]) -> Result<Config> {
        Config::from_lookup(|key| {
            overrides
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).into())
                .or_else(|| match key {
                    "ATTRIBUTION_DATABASE_URL" => {
                        Some("postgres://role@localhost/attribution".into())
                    }
                    "ATTRIBUTION_PUBLIC_ORIGIN" => Some("https://attribution.example.test".into()),
                    "ATTRIBUTION_EVENT_MAX_AGE_DAYS" => Some("7".into()),
                    "ATTRIBUTION_REFUND_WINDOW_DAYS" => Some("365".into()),
                    "ATTRIBUTION_CLICK_RETENTION_DAYS" => Some("38".into()),
                    "ATTRIBUTION_OBSERVATION_RETENTION_DAYS" => Some("30".into()),
                    "ATTRIBUTION_FINANCIAL_RETENTION_DAYS" => Some("400".into()),
                    "ATTRIBUTION_EVENT_RETENTION_DAYS" => Some("8".into()),
                    "ATTRIBUTION_AUDIT_RETENTION_DAYS" => Some("90".into()),
                    _ => None,
                })
        })
    }

    #[test]
    fn attribution_retention_config_requires_explicit_protocol_safe_horizons() {
        for (key, bad) in [
            ("ATTRIBUTION_CLICK_RETENTION_DAYS", "37"),
            ("ATTRIBUTION_OBSERVATION_RETENTION_DAYS", "1"),
            ("ATTRIBUTION_FINANCIAL_RETENTION_DAYS", "372"),
            ("ATTRIBUTION_EVENT_RETENTION_DAYS", "7"),
            ("ATTRIBUTION_AUDIT_RETENTION_DAYS", "0"),
        ] {
            assert!(config(&[(key, bad)]).is_err(), "{key}");
            assert!(config(&[(key, "10001")]).is_err());
            assert!(config(&[(key, "")]).is_err());
        }
        assert!(config(&[("ATTRIBUTION_FINANCIAL_RETENTION_DAYS", "373")]).is_ok());
        assert!(config(&[("ATTRIBUTION_EVENT_MAX_AGE_DAYS", "8")]).is_err());
        assert!(config(&[("ATTRIBUTION_REFUND_WINDOW_DAYS", "400")]).is_err());
    }

    #[test]
    fn attribution_startup_rejects_bad_config_without_echoing_secrets() {
        for value in [
            "http://example.test",
            "https://u:secret@example.test",
            "https://example.test/path",
            "https://example.test?secret=x",
            "https://example.test#x",
        ] {
            let error = config(&[("ATTRIBUTION_PUBLIC_ORIGIN", value)])
                .err()
                .unwrap()
                .to_string();
            assert!(!error.contains(value));
            assert!(error.contains("ATTRIBUTION_PUBLIC_ORIGIN"));
        }
        for value in ["0", "33", "not-a-number"] {
            assert!(config(&[("ATTRIBUTION_DB_MAX_CONNECTIONS", value)]).is_err());
        }
        assert!(config(&[("ATTRIBUTION_BIND_ADDR", "not-an-address")]).is_err());
        assert!(config(&[("ATTRIBUTION_DATABASE_URL", "https://secret@example.test")]).is_err());
        assert!(Config::from_lookup(|_| None).is_err());
    }

    #[test]
    fn attribution_startup_loopback_development_and_stock_budgets() {
        for origin in [
            "http://localhost:8091",
            "http://127.0.0.1:8091",
            "http://[::1]:8091",
        ] {
            assert!(config(&[("ATTRIBUTION_PUBLIC_ORIGIN", origin)]).is_ok());
        }
        let c = config(&[]).unwrap();
        assert_eq!(c.pool_size, 5);
        assert_eq!(RUNTIME_THREAD_STACK_BYTES, 2 * 1024 * 1024);
        assert_eq!(MAX_CONNECTIONS, 64);
        assert_eq!(MAX_REQUESTS, 32);
    }
}
