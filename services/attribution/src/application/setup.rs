//! Merchant setup wire types and normalized invariants. No transport or database I/O.
use super::identity::{CompanyId, Failure, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ids::entity_id;
entity_id!(StoreId);
entity_id!(DomainId);
entity_id!(InstallationId);
entity_id!(CredentialId);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum KeyScope {
    #[serde(rename = "conversions:write")]
    ConversionsWrite,
    #[serde(rename = "checkouts:write")]
    CheckoutsWrite,
    #[serde(rename = "links:read")]
    LinksRead,
    #[serde(rename = "links:write")]
    LinksWrite,
    #[serde(rename = "analytics:read")]
    AnalyticsRead,
}
impl KeyScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ConversionsWrite => "conversions:write",
            Self::CheckoutsWrite => "checkouts:write",
            Self::LinksRead => "links:read",
            Self::LinksWrite => "links:write",
            Self::AnalyticsRead => "analytics:read",
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Store {
    pub id: StoreId,
    pub company_id: CompanyId,
    pub name: String,
    pub timezone: String,
    pub enabled: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Domain {
    pub id: DomainId,
    pub store_id: StoreId,
    pub host: String,
    pub status: String,
    pub verified_until: Option<DateTime<Utc>>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Installation {
    pub id: InstallationId,
    pub store_id: StoreId,
    pub domain_id: DomainId,
    pub name: String,
    pub enabled: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credential {
    pub id: CredentialId,
    pub store_id: StoreId,
    pub label: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreWrite {
    pub name: String,
    pub timezone: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreUpdate {
    #[serde(default, deserialize_with = "optional_nonnull")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "optional_nonnull")]
    pub timezone: Option<String>,
    #[serde(default, deserialize_with = "optional_nonnull")]
    pub enabled: Option<bool>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainWrite {
    pub store_id: StoreId,
    pub host: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallationWrite {
    pub store_id: StoreId,
    pub domain_id: DomainId,
    pub name: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnabledUpdate {
    pub enabled: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialWrite {
    pub store_id: StoreId,
    pub label: String,
    pub scopes: Vec<KeyScope>,
    #[serde(deserialize_with = "required_nullable")]
    pub expires_at: Option<DateTime<Utc>>,
}
fn optional_nonnull<'de, T: Deserialize<'de>, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}
fn required_nullable<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<DateTime<Utc>>, D::Error> {
    let value = Option::<String>::deserialize(d)?;
    value
        .map(|value| {
            if value.len() > 40 {
                return Err(serde::de::Error::custom("timestamp too long"));
            }
            DateTime::parse_from_rfc3339(&value)
                .map(|time| time.with_timezone(&Utc))
                .map_err(|_| serde::de::Error::custom("invalid timestamp"))
        })
        .transpose()
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RotateCredential {
    pub overlap_seconds: u32,
}

#[derive(Clone, Serialize)]
pub enum Mutation {
    CreateStore(StoreWrite),
    UpdateStore(StoreId, StoreUpdate),
    CreateDomain(DomainWrite),
    VerifyDomain(DomainId),
    RevokeDomain(DomainId),
    CreateInstallation(InstallationWrite),
    UpdateInstallation(InstallationId, EnabledUpdate),
    CreateCredential(CredentialWrite),
    RevokeCredential(CredentialId),
    RotateCredential(CredentialId, RotateCredential),
}
impl Mutation {
    pub fn normalize(mut self) -> Result<Self> {
        match &mut self {
            Self::CreateStore(w) => {
                name(&mut w.name, 120)?;
                timezone(&w.timezone)?;
            }
            Self::UpdateStore(_, w) => {
                if w.name.is_none() && w.timezone.is_none() && w.enabled.is_none() {
                    return Err(Failure::Invalid);
                }
                if let Some(n) = &mut w.name {
                    name(n, 120)?;
                }
                if let Some(t) = &w.timezone {
                    timezone(t)?;
                }
            }
            Self::CreateDomain(w) => {
                w.host = canonical_host(&w.host)?;
            }
            Self::CreateInstallation(w) => name(&mut w.name, 120)?,
            Self::CreateCredential(w) => {
                name(&mut w.label, 80)?;
                if w.scopes.is_empty() || w.scopes.len() > 5 {
                    return Err(Failure::Invalid);
                }
                w.scopes.sort();
                if w.scopes.windows(2).any(|pair| pair[0] == pair[1]) {
                    return Err(Failure::Invalid);
                }
            }
            Self::RotateCredential(_, w) if w.overlap_seconds > 86400 => {
                return Err(Failure::Invalid);
            }
            _ => {}
        }
        Ok(self)
    }
    pub fn operation(&self) -> String {
        match self {
            Self::CreateStore(_) => "POST /v1/stores".into(),
            Self::UpdateStore(id, _) => format!("PATCH /v1/stores/{}", id.0),
            Self::CreateDomain(_) => "POST /v1/domains".into(),
            Self::VerifyDomain(id) => format!("POST /v1/domains/{}/verify", id.0),
            Self::RevokeDomain(id) => format!("POST /v1/domains/{}/revoke", id.0),
            Self::CreateInstallation(_) => "POST /v1/installations".into(),
            Self::UpdateInstallation(id, _) => format!("PATCH /v1/installations/{}", id.0),
            Self::CreateCredential(_) => "POST /v1/credentials".into(),
            Self::RevokeCredential(id) => format!("POST /v1/credentials/{}/revoke", id.0),
            Self::RotateCredential(id, _) => format!("POST /v1/credentials/{}/rotate", id.0),
        }
    }
    pub fn action(&self) -> &'static str {
        match self {
            Self::CreateStore(_) => "store.create",
            Self::UpdateStore(..) => "store.update",
            Self::CreateDomain(_) => "domain.create",
            Self::VerifyDomain(_) => "domain.verify",
            Self::RevokeDomain(_) => "domain.revoke",
            Self::CreateInstallation(_) => "installation.create",
            Self::UpdateInstallation(..) => "installation.update",
            Self::CreateCredential(_) => "credential.create",
            Self::RevokeCredential(_) => "credential.revoke",
            Self::RotateCredential(..) => "credential.rotate",
        }
    }
}
pub struct Retry {
    pub key: Uuid,
    pub created_at: DateTime<Utc>,
}
impl Retry {
    pub fn validate(&self, now: DateTime<Utc>) -> Result<()> {
        if self.key.is_nil() || self.created_at > now + chrono::Duration::seconds(30) {
            return Err(Failure::Invalid);
        }
        if self.created_at < now - chrono::Duration::hours(24) {
            return Err(Failure::Gone);
        }
        Ok(())
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resource {
    Stores,
    Domains,
    Installations,
    Credentials,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListQuery {
    pub limit: Option<u16>,
    pub cursor: Option<String>,
    pub store_id: Option<StoreId>,
}
impl ListQuery {
    pub fn limit(&self) -> Result<u16> {
        let n = self.limit.unwrap_or(25);
        if !(1..=100).contains(&n)
            || self
                .cursor
                .as_ref()
                .is_some_and(|s| s.is_empty() || s.len() > 512)
        {
            return Err(Failure::Invalid);
        }
        Ok(n)
    }
}
pub fn name(value: &mut String, max: usize) -> Result<()> {
    *value = value.trim().to_owned();
    if value.is_empty() || value.chars().count() > max || value.chars().any(char::is_control) {
        return Err(Failure::Invalid);
    }
    Ok(())
}
fn timezone(value: &str) -> Result<()> {
    if value.len() > 64 || value.parse::<chrono_tz::Tz>().is_err() {
        return Err(Failure::Invalid);
    }
    Ok(())
}
pub fn canonical_host(input: &str) -> Result<String> {
    if input.is_empty()
        || input.len() > 1012
        || input.chars().any(|c| c.is_control() || c.is_whitespace())
        || input.contains(['/', ':', '@', '%', '\\', '#', '?'])
        || input.ends_with('.')
    {
        return Err(Failure::Invalid);
    }
    let url::Host::Domain(host) = url::Host::parse(input).map_err(|_| Failure::Invalid)? else {
        return Err(Failure::Invalid);
    };
    if host.len() > 240
        || !host.contains('.')
        || host.split('.').any(|p| {
            p.is_empty()
                || p.len() > 63
                || p.starts_with('-')
                || p.ends_with('-')
                || !p.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        })
    {
        return Err(Failure::Invalid);
    }
    const RESERVED: &[&str] = &[
        "localhost",
        "local",
        "internal",
        "lan",
        "home",
        "home.arpa",
        "test",
        "invalid",
        "example",
        "onion",
        "arpa",
    ];
    if RESERVED
        .iter()
        .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
    {
        return Err(Failure::Invalid);
    }
    Ok(host)
}
/// Pure URL policy. The SQL adapter separately checks this exact host's live approval.
pub fn destination(input: &str) -> Result<url::Url> {
    if input.is_empty()
        || input.len() > 2048
        || input.chars().any(|c| c.is_control() || c.is_whitespace())
        || input.contains('\\')
    {
        return Err(Failure::Invalid);
    }
    let url = url::Url::parse(input).map_err(|_| Failure::Invalid)?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
    {
        return Err(Failure::Invalid);
    }
    let raw_authority = input
        .split_once("://")
        .ok_or(Failure::Invalid)?
        .1
        .split(['/', '?', '#'])
        .next()
        .ok_or(Failure::Invalid)?;
    if raw_authority.is_empty() || raw_authority.contains(['@', '%']) {
        return Err(Failure::Invalid);
    }
    canonical_host(url.host_str().ok_or(Failure::Invalid)?)?;
    if url.as_str().len() > 2048 {
        return Err(Failure::Invalid);
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attribution_destination_exact_idna_and_unsafe_urls() {
        let maximum = format!(
            "{}.{}.{}.{}",
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(48)
        );
        assert_eq!(
            format!("_attribution.{}", canonical_host(&maximum).unwrap()).len(),
            253
        );
        assert!(canonical_host(&(maximum + "d")).is_err());
        assert_eq!(canonical_host("BÜCHER.de").unwrap(), "xn--bcher-kva.de");
        assert_eq!(
            destination("https://BÜCHER.de:443/a?x=1&x=2#part")
                .unwrap()
                .as_str(),
            "https://xn--bcher-kva.de/a?x=1&x=2#part"
        );
        for bad in [
            "https://localhost/",
            "https://127.1/",
            "https://[::1]/",
            "http://shop.com/",
            "https://shop.com:8443/",
            "https://user@shop.com/",
            "https://@shop.com/",
            "https://shop.com./",
            "https://shop%2ecom/",
            "https://shop.com\\@evil.com/",
            "https://a.local/",
            "https://shop.com/\n",
        ] {
            assert!(destination(bad).is_err(), "{bad}");
        }
        assert_eq!(
            canonical_host("shop.com.attacker.com").unwrap(),
            "shop.com.attacker.com"
        );
    }
}
