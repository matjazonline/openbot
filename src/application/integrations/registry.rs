use super::private::{CredentialAdapter, CredentialCacheUpdate, InjectionMethod, TypedInjection};
use crate::domain::integrations::{authority::ApprovedDestination, *};
use secrecy::{ExposeSecret, SecretString};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone)]
pub struct AuthProvider {
    pub id: ProviderId,
    pub method: AuthMethod,
    pub schema: CredentialSchema,
}
#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub id: AuthConfigId,
    pub provider: ProviderId,
    pub client_identity: ExternalSubject,
    pub(crate) client_secret_reference: Option<ClientSecretReference>,
}
#[derive(Debug, Clone)]
pub struct AppDefinition {
    pub id: AppId,
    pub provider: ProviderId,
    pub config: AuthConfigId,
    pub destinations: Vec<ApprovedDestination>,
    pub capabilities: Vec<Capability>,
}
pub(crate) struct Registration {
    pub provider: AuthProvider,
    pub config: AuthConfig,
    pub app: AppDefinition,
    pub adapter: Arc<dyn CredentialAdapter>,
    pub authorization: Option<Arc<dyn super::connect::AuthorizationProvider>>,
    pub renewal: super::renewal::RenewalStrategy,
}
#[derive(Default)]
pub(crate) struct CodeOwnedRegistry {
    entries: BTreeMap<AppId, Registration>,
}
impl CodeOwnedRegistry {
    pub fn register(&mut self, entry: Registration) -> IntegrationResult<()> {
        if (entry.provider.method == AuthMethod::StaticSecret
            && entry.config.client_secret_reference.is_some())
            || entry.provider.id != entry.config.provider
            || entry.provider.id != entry.app.provider
            || entry.config.id != entry.app.config
            || entry.app.destinations.len() > 16
            || entry.app.capabilities.len() > 64
            || self.entries.contains_key(&entry.app.id)
        {
            return Err(IntegrationError::Invalid);
        }
        if self.entries.values().any(|other| {
            other.provider.id == entry.provider.id
                && other.config.id == entry.config.id
                && other.provider.method == entry.provider.method
                && other.provider.schema == entry.provider.schema
                && other.renewal.protocol() != entry.renewal.protocol()
        }) {
            return Err(IntegrationError::Invalid);
        }
        self.entries.insert(entry.app.id.clone(), entry);
        Ok(())
    }
    pub(crate) fn validate_generation(
        &self,
        app: &AppId,
        payload: &SecretString,
        generation: crate::domain::integrations::CredentialGeneration,
    ) -> IntegrationResult<()> {
        use super::renewal::{RenewalStrategy, ResolutionError};
        match &self.get(app)?.renewal {
            RenewalStrategy::NoRenewal => Ok(()),
            RenewalStrategy::ReusableRefresh(p) | RenewalStrategy::StrictRotating(p) => p
                .assess(payload, generation, chrono::Utc::now())
                .map(|_| ())
                .map_err(|e| match e {
                    ResolutionError::Integration(error) => error,
                    _ => IntegrationError::Credential,
                }),
        }
    }
    /// Only code-owned, nonsecret policy facts enter this digest. Payload/cache
    /// revisions and encryption keys do not change business authorization.
    pub fn policy(&self, app: &AppId) -> IntegrationResult<AppPolicyDigest> {
        use sha2::{Digest, Sha256};
        let entry = self.get(app)?;
        let injection = entry.adapter.policy_identity(&entry.app)?;
        if injection.len() > 16384 {
            return Err(IntegrationError::Limit);
        }
        let bytes = serde_json::to_vec(&(
            &entry.provider.id,
            entry.provider.method,
            &entry.provider.schema,
            &entry.config.id,
            &entry.config.client_identity,
            &entry.app.id,
            &entry.app.destinations,
            &entry.app.capabilities,
            injection,
        ))
        .map_err(|_| IntegrationError::Invalid)?;
        if bytes.len() > 65536 {
            return Err(IntegrationError::Limit);
        }
        AppPolicyDigest::parse(format!("{:x}", Sha256::digest(bytes)))
    }
    pub(crate) fn connectable(&self) -> Vec<super::ConnectApp> {
        self.entries
            .values()
            .filter(|e| e.authorization.is_some())
            .map(|e| super::ConnectApp {
                provider: e.provider.id.clone(),
                id: e.app.id.clone(),
                config: e.config.id.clone(),
                capabilities: e.app.capabilities.clone(),
            })
            .collect()
    }
    pub fn get(&self, app: &AppId) -> IntegrationResult<&Registration> {
        self.entries.get(app).ok_or(IntegrationError::Unavailable)
    }
    pub(crate) fn for_account(
        &self,
        provider: &ProviderId,
        config: &AuthConfigId,
        method: AuthMethod,
        schema: &CredentialSchema,
    ) -> IntegrationResult<&Registration> {
        self.entries
            .values()
            .find(|entry| {
                &entry.provider.id == provider
                    && &entry.config.id == config
                    && entry.provider.method == method
                    && &entry.provider.schema == schema
            })
            .ok_or(IntegrationError::Unavailable)
    }
    pub fn validate(
        &self,
        app: &AppId,
        method: AuthMethod,
        schema: &CredentialSchema,
        payload: &SecretString,
    ) -> IntegrationResult<Option<ExternalIdentity>> {
        let entry = self.get(app)?;
        if method != entry.provider.method
            || schema != &entry.provider.schema
            || payload.expose_secret().is_empty()
            || payload.expose_secret().len() > MAX_CREDENTIAL_BYTES
        {
            return Err(IntegrationError::Invalid);
        }
        entry.adapter.validate(payload)
    }
}

pub(crate) struct StaticCredentialAdapter {
    header: Option<String>,
}
impl StaticCredentialAdapter {
    pub fn bearer() -> Self {
        Self { header: None }
    }
    pub fn api_key_header(header: &str) -> IntegrationResult<Self> {
        if header.len() > 128
            || header.is_empty()
            || !header
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || super::execution::is_operation_header(header)
            || matches!(
                header.to_ascii_lowercase().as_str(),
                "authorization" | "cookie" | "host" | "proxy-authorization" | "content-length"
            )
        {
            return Err(IntegrationError::Invalid);
        }
        Ok(Self {
            header: Some(header.into()),
        })
    }
    fn secret(&self, payload: &SecretString) -> IntegrationResult<SecretString> {
        let raw = payload.expose_secret();
        if raw.is_empty() || raw.len() > MAX_CREDENTIAL_BYTES {
            return Err(IntegrationError::Invalid);
        }
        // Structured credentials are accepted only in this adapter's exact supported shape.
        let structured = raw.trim_start();
        let value = if structured.starts_with('{')
            || structured.starts_with('[')
            || structured.starts_with('"')
        {
            let parsed: serde_json::Value =
                serde_json::from_str(raw).map_err(|_| IntegrationError::Invalid)?;
            match parsed {
                serde_json::Value::String(value) => value,
                serde_json::Value::Object(mut object) if object.len() == 1 => object
                    .remove(if self.header.is_some() {
                        "api_key"
                    } else {
                        "token"
                    })
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .ok_or(IntegrationError::Invalid)?,
                _ => return Err(IntegrationError::Invalid),
            }
        } else {
            raw.into()
        };
        if value.is_empty()
            || value.len() > MAX_CREDENTIAL_BYTES
            || value.chars().any(char::is_control)
        {
            return Err(IntegrationError::Invalid);
        }
        Ok(SecretString::from(value))
    }
}
impl CredentialAdapter for StaticCredentialAdapter {
    fn policy_identity(&self, _: &AppDefinition) -> IntegrationResult<String> {
        Ok(format!("static-v1/{:?}", self.header))
    }
    fn validate_use(&self, payload: &SecretString, _: &AppDefinition) -> IntegrationResult<()> {
        self.secret(payload)?;
        Ok(())
    }
    fn validate(&self, payload: &SecretString) -> IntegrationResult<Option<ExternalIdentity>> {
        self.secret(payload)?;
        Ok(None)
    }
    fn validate_cache_update(&self, _update: CredentialCacheUpdate<'_>) -> IntegrationResult<()> {
        Err(IntegrationError::Denied)
    }
    fn injection(&self, payload: &SecretString) -> IntegrationResult<TypedInjection> {
        Ok(TypedInjection {
            method: self.header.as_ref().map_or(InjectionMethod::Bearer, |h| {
                InjectionMethod::ApiKeyHeader(h.clone())
            }),
            value: self.secret(payload)?,
        })
    }
}

pub(crate) fn static_registry() -> CodeOwnedRegistry {
    let mut registry = CodeOwnedRegistry::default();
    for (name, adapter) in [
        ("static-bearer", StaticCredentialAdapter::bearer()),
        (
            "static-api-key",
            StaticCredentialAdapter::api_key_header("X-API-Key").expect("code-owned header"),
        ),
    ] {
        let provider = ProviderId::parse(name).unwrap();
        let config = AuthConfigId::parse(name).unwrap();
        let app = AppId::parse(name).unwrap();
        registry
            .register(Registration {
                authorization: None,
                renewal: super::renewal::RenewalStrategy::NoRenewal,
                provider: AuthProvider {
                    id: provider.clone(),
                    method: AuthMethod::StaticSecret,
                    schema: CredentialSchema::parse("static-v1").unwrap(),
                },
                config: AuthConfig {
                    id: config.clone(),
                    provider: provider.clone(),
                    client_identity: ExternalSubject::parse("openbot-static").unwrap(),
                    client_secret_reference: None,
                },
                app: AppDefinition {
                    id: app,
                    provider,
                    config,
                    destinations: vec![],
                    capabilities: vec![],
                },
                adapter: Arc::new(adapter),
            })
            .unwrap();
    }
    registry
}
