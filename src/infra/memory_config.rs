//! Startup configuration for the standalone memory service client (phase 05, decision E1).
//!
//! Every key here is read once at startup. Nothing contacts the service: a wrong URL or a revoked
//! provisioner token shows up on the first request, never as a startup claim that it was checked.
//! The provisioner token is a credential, so it is kept out of the broad `AppConfig`.
use secrecy::{ExposeSecret, SecretString};
use std::time::Duration;
use uuid::Uuid;

pub const URL_ENV: &str = "MEMORY_SERVICE_URL";
pub const TOKEN_ENV: &str = "MEMORY_SERVICE_PROVISIONER_TOKEN";
pub const TIMEOUT_ENV: &str = "MEMORY_SERVICE_TIMEOUT_MS";
pub const RECALL_DEADLINE_ENV: &str = "MEMORY_SERVICE_RECALL_DEADLINE_MS";
pub const INTERACTIVE_ENV: &str = "MEMORY_SERVICE_INTERACTIVE_IN_FLIGHT";
pub const BACKGROUND_ENV: &str = "MEMORY_SERVICE_BACKGROUND_IN_FLIGHT";

/// Per-company operations in flight on each pool (E1). Fixed, not configuration: they are the
/// fairness rule, and a recall with its concurrent scope packs counts as one operation.
pub const PER_COMPANY_INTERACTIVE: usize = 2;
pub const PER_COMPANY_BACKGROUND: usize = 1;

/// A bounded integer setting: default and inclusive range.
struct Bound {
    name: &'static str,
    default: u64,
    min: u64,
    max: u64,
}

const TIMEOUT: Bound = Bound {
    name: TIMEOUT_ENV,
    default: 12_000,
    min: 1_000,
    max: 30_000,
};
const RECALL_DEADLINE: Bound = Bound {
    name: RECALL_DEADLINE_ENV,
    default: 2_000,
    min: 250,
    max: 5_000,
};
const INTERACTIVE: Bound = Bound {
    name: INTERACTIVE_ENV,
    default: 8,
    min: 1,
    max: 32,
};
const BACKGROUND: Bound = Bound {
    name: BACKGROUND_ENV,
    default: 4,
    min: 1,
    max: 16,
};

/// The provisioner admin token. Its credential id is not secret; it names the `managed_by` the
/// service records for every tenant this client creates (decisions D3, E2).
#[derive(Clone)]
pub struct ProvisionerToken {
    token: SecretString,
    credential_id: Uuid,
}

impl ProvisionerToken {
    /// Shape only (`msk_adm_<32 hex>_<43 base64url>`); the service is the authority on validity.
    pub fn parse(token: SecretString) -> Result<Self, String> {
        let invalid = || format!("{TOKEN_ENV} must be a memory-service admin token (msk_adm_…)");
        let raw = token.expose_secret();
        let rest = raw.strip_prefix("msk_adm_").ok_or_else(invalid)?;
        let (id, secret) = rest.split_once('_').ok_or_else(invalid)?;
        let base64url = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
        if id.len() != 32
            || !id.bytes().all(|b| b.is_ascii_hexdigit())
            || secret.len() != 43
            || !secret.chars().all(base64url)
        {
            return Err(invalid());
        }
        let credential_id = Uuid::try_parse(id).map_err(|_| invalid())?;
        Ok(Self {
            token,
            credential_id,
        })
    }

    pub fn credential_id(&self) -> Uuid {
        self.credential_id
    }

    pub fn secret(&self) -> &SecretString {
        &self.token
    }
}

impl std::fmt::Debug for ProvisionerToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProvisionerToken")
            .field("credential_id", &self.credential_id)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
pub struct MemoryServiceConfig {
    /// Absolute base URL without a trailing slash; operation paths are appended to it.
    pub base_url: String,
    pub provisioner: ProvisionerToken,
    pub request_timeout: Duration,
    pub recall_deadline: Duration,
    pub interactive_in_flight: usize,
    pub background_in_flight: usize,
}

impl MemoryServiceConfig {
    /// `Ok(None)` when `MEMORY_SERVICE_URL` is unset: memory is unavailable in this deployment.
    pub fn from_env() -> Result<Option<Self>, String> {
        let var = |name: &str| {
            std::env::var(name)
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        };
        Self::from_values(MemoryServiceValues {
            url: var(URL_ENV),
            // Trimmed like the other keys: a secrets file's trailing newline is not part of the token.
            token: std::env::var(TOKEN_ENV)
                .ok()
                .map(|value| SecretString::from(value.trim().to_owned())),
            timeout: var(TIMEOUT_ENV),
            recall_deadline: var(RECALL_DEADLINE_ENV),
            interactive: var(INTERACTIVE_ENV),
            background: var(BACKGROUND_ENV),
        })
    }

    pub(crate) fn from_values(values: MemoryServiceValues) -> Result<Option<Self>, String> {
        let Some(url) = values.url else {
            let stray = [
                (TOKEN_ENV, values.token.is_some()),
                (TIMEOUT_ENV, values.timeout.is_some()),
                (RECALL_DEADLINE_ENV, values.recall_deadline.is_some()),
                (INTERACTIVE_ENV, values.interactive.is_some()),
                (BACKGROUND_ENV, values.background.is_some()),
            ];
            if let Some((name, _)) = stray.iter().find(|(_, set)| *set) {
                return Err(format!("{name} is set but {URL_ENV} is not"));
            }
            return Ok(None);
        };
        let base_url = parse_base_url(&url)?;
        let token = values
            .token
            .filter(|token| !token.expose_secret().trim().is_empty())
            .ok_or_else(|| format!("{TOKEN_ENV} is required when {URL_ENV} is set"))?;
        let provisioner = ProvisionerToken::parse(token)?;
        let request_timeout = Duration::from_millis(bounded(&TIMEOUT, values.timeout)?);
        let recall_deadline =
            Duration::from_millis(bounded(&RECALL_DEADLINE, values.recall_deadline)?);
        if recall_deadline >= request_timeout {
            return Err(format!("{RECALL_DEADLINE_ENV} must be below {TIMEOUT_ENV}"));
        }
        Ok(Some(Self {
            base_url,
            provisioner,
            request_timeout,
            recall_deadline,
            interactive_in_flight: bounded(&INTERACTIVE, values.interactive)? as usize,
            background_in_flight: bounded(&BACKGROUND, values.background)? as usize,
        }))
    }
}

/// Raw values, so validation is testable without mutating the process environment.
#[derive(Default)]
pub(crate) struct MemoryServiceValues {
    pub url: Option<String>,
    pub token: Option<SecretString>,
    pub timeout: Option<String>,
    pub recall_deadline: Option<String>,
    pub interactive: Option<String>,
    pub background: Option<String>,
}

fn bounded(bound: &Bound, value: Option<String>) -> Result<u64, String> {
    let Some(value) = value else {
        return Ok(bound.default);
    };
    value
        .parse::<u64>()
        .ok()
        .filter(|parsed| (bound.min..=bound.max).contains(parsed))
        .ok_or_else(|| {
            format!(
                "{} must be an integer from {} to {}",
                bound.name, bound.min, bound.max
            )
        })
}

/// https, or http only to a loopback host; no userinfo, query or fragment (E1).
fn parse_base_url(value: &str) -> Result<String, String> {
    let invalid = |why: &str| format!("{URL_ENV} {why}");
    let url = url::Url::parse(value).map_err(|_| invalid("must be an absolute URL"))?;
    let loopback = match url.host() {
        Some(url::Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        None => return Err(invalid("must name a host")),
    };
    match url.scheme() {
        "https" => {}
        "http" if loopback => {}
        "http" => return Err(invalid("may use http only for a loopback host")),
        _ => return Err(invalid("must use https")),
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid("must not carry credentials"));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(invalid("must not carry a query or fragment"));
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

#[cfg(test)]
#[path = "memory_config_tests.rs"]
mod tests;
