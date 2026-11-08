use super::*;
use crate::{
    adapters::persistence::{
        PostgresPersistence,
        credentials::{CredentialCipher, envelope::CredentialContext},
        test_support::{OwnDatabase, own_database, test_pool},
    },
    application::integrations::{
        lifecycle::TeardownDrain,
        management::{AccountRepository, AccountWrite},
        private::*,
        registry::*,
    },
    domain::integrations::*,
};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
struct Fixture {
    vault: PostgresVault,
    owner: AccountOwner,
    _database: Option<OwnDatabase>,
}
impl Fixture {
    fn on(pool: sqlx::PgPool) -> Self {
        let persistence = PostgresPersistence::with_credential_cipher(
            pool,
            CredentialCipher::for_test_with_keys(&[(1, [7; 32])], 1),
        );
        let owner = AccountOwner {
            namespace: OwnerNamespace::parse("vault-test").unwrap(),
            subject: OwnerSubject::parse(Uuid::new_v4().to_string()).unwrap(),
        };
        Self {
            vault: PostgresVault::new(persistence),
            owner,
            _database: None,
        }
    }
    async fn shared() -> Self {
        Self::on(test_pool().await.expect("disposable database required"))
    }
    async fn isolated() -> Self {
        let database = own_database().await.expect("disposable database required");
        let mut f = Self::on(database.pool.clone());
        f._database = Some(database);
        f
    }
    fn pool(&self) -> &sqlx::PgPool {
        self.vault.persistence.pool()
    }
    async fn create(&self, key: &str) -> ConnectedAccount {
        self.vault
            .create(&self.owner, static_write(key, "fixture-secret"))
            .await
            .unwrap()
    }
    fn rotating(&self) -> PostgresPersistence {
        PostgresPersistence::with_credential_cipher(
            self.pool().clone(),
            CredentialCipher::for_test_with_keys(&[(1, [7; 32]), (2, [8; 32])], 2),
        )
    }
}
fn static_write(key: &str, value: &str) -> AccountWrite {
    AccountWrite {
        key: AccountKey::parse(key).unwrap(),
        provider: ProviderId::parse("static-bearer").unwrap(),
        config: AuthConfigId::parse("static-bearer").unwrap(),
        method: AuthMethod::StaticSecret,
        schema: CredentialSchema::parse("static-v1").unwrap(),
        identity: None,
        metadata: AccountMetadata::new(key.into(), None).unwrap(),
        payload: SecretString::from(value.to_owned()),
    }
}
struct WorkspaceAdapter {
    client: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspacePayload {
    workspace: ExternalSubject,
    client: ExternalSubject,
    token: SecretString,
    generation: CredentialGeneration,
    permissions: Vec<Scope>,
}
impl WorkspaceAdapter {
    fn decode(&self, raw: &SecretString) -> IntegrationResult<WorkspacePayload> {
        if raw.expose_secret().len() > MAX_CREDENTIAL_BYTES {
            return Err(IntegrationError::Invalid);
        }
        let payload: WorkspacePayload =
            serde_json::from_str(raw.expose_secret()).map_err(|_| IntegrationError::Credential)?;
        if payload.client.as_str() != self.client
            || payload.token.expose_secret().is_empty()
            || payload.permissions.len() > MAX_SCOPES
        {
            return Err(IntegrationError::Credential);
        }
        let _ = payload.generation;
        Ok(payload)
    }
}
impl CredentialAdapter for WorkspaceAdapter {
    fn policy_identity(&self, app: &AppDefinition) -> IntegrationResult<String> {
        Ok(format!("workspace/v1/{:?}", app.capabilities))
    }
    fn validate_use(&self, raw: &SecretString, app: &AppDefinition) -> IntegrationResult<()> {
        let payload = self.decode(raw)?;
        if app.capabilities.iter().any(|cap| {
            !payload
                .permissions
                .iter()
                .any(|p| p.as_str() == cap.as_str())
        }) {
            return Err(IntegrationError::Denied);
        }
        Ok(())
    }
    fn validate(&self, raw: &SecretString) -> IntegrationResult<Option<ExternalIdentity>> {
        let payload = self.decode(raw)?;
        let key = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(payload.workspace.as_str(), payload.client.as_str())).unwrap()
            )
        );
        Ok(Some(ExternalIdentity {
            kind: ExternalIdentityKind::parse("workspace")?,
            subject: payload.workspace,
            authorization_key: AuthorizationKey::parse(key)?,
        }))
    }
    fn validate_cache_update(&self, update: CredentialCacheUpdate<'_>) -> IntegrationResult<()> {
        let previous = self.decode(update.previous)?;
        let replacement = self.decode(update.replacement)?;
        let old: std::collections::BTreeSet<_> = previous.permissions.into_iter().collect();
        let new: std::collections::BTreeSet<_> = replacement.permissions.into_iter().collect();
        if previous.workspace != replacement.workspace
            || previous.client != replacement.client
            || old != new
        {
            return Err(IntegrationError::Denied);
        }
        Ok(())
    }
    fn injection(&self, raw: &SecretString) -> IntegrationResult<TypedInjection> {
        Ok(TypedInjection {
            method: InjectionMethod::Bearer,
            value: self.decode(raw)?.token,
        })
    }
}
fn managed_registry() -> Arc<CodeOwnedRegistry> {
    let mut registry = static_registry();
    for (name, client) in [
        ("workspace-a", "same-client"),
        ("workspace-b", "same-client"),
        ("workspace-other", "other-client"),
    ] {
        let provider = ProviderId::parse("workspace-fixture").unwrap();
        let config = AuthConfigId::parse(name).unwrap();
        registry
            .register(Registration {
                authorization: None,
                renewal: crate::application::integrations::renewal::RenewalStrategy::NoRenewal,
                provider: AuthProvider {
                    id: provider.clone(),
                    method: AuthMethod::OAuthAuthorizationCode,
                    schema: CredentialSchema::parse("workspace-fixture-v1").unwrap(),
                },
                config: AuthConfig {
                    id: config.clone(),
                    provider: provider.clone(),
                    client_identity: ExternalSubject::parse(client).unwrap(),
                    client_secret_reference: None,
                },
                app: AppDefinition {
                    transport: crate::application::integrations::destinations::AppTransportPolicy::OwnerApproved,
                    id: AppId::parse(name).unwrap(),
                    provider,
                    config,
                    destinations: vec![],
                    capabilities: vec![],
                },
                adapter: Arc::new(WorkspaceAdapter {
                    client: client.into(),
                }),
            })
            .unwrap();
    }
    Arc::new(registry)
}
fn managed_payload(client: &str, generation: i64) -> SecretString {
    SecretString::from(json!({"workspace":"workspace-without-email","client":client,"token":"fixture-managed-secret","generation":generation,"permissions":["resource.read"]}).to_string())
}
fn managed_write(registry: &CodeOwnedRegistry, key: &str, app: &str) -> AccountWrite {
    let registration = registry.get(&AppId::parse(app).unwrap()).unwrap();
    let payload = managed_payload(registration.config.client_identity.as_str(), 1);
    registered_write(registration, key, payload)
}
fn registered_write(registration: &Registration, key: &str, payload: SecretString) -> AccountWrite {
    AccountWrite {
        key: AccountKey::parse(key).unwrap(),
        provider: registration.provider.id.clone(),
        config: registration.config.id.clone(),
        method: registration.provider.method,
        schema: registration.provider.schema.clone(),
        identity: registration.adapter.validate(&payload).unwrap(),
        metadata: AccountMetadata::new(key.into(), None).unwrap(),
        payload,
    }
}
#[path = "lifecycle_tests.rs"]
mod lifecycle_tests;
#[path = "rotation_tests.rs"]
mod rotation_tests;
#[path = "storage_tests.rs"]
mod storage_tests;

fn google_registry() -> Arc<CodeOwnedRegistry> {
    let mut registry = static_registry();
    let provider = ProviderId::parse("google").unwrap();
    let config = AuthConfigId::parse("google-fixture").unwrap();
    let client = ExternalSubject::parse("google-client-fixture").unwrap();
    registry
        .register(Registration {
            authorization: None,
            renewal: crate::application::integrations::renewal::RenewalStrategy::NoRenewal,
            provider: AuthProvider {
                id: provider.clone(),
                method: AuthMethod::OAuthAuthorizationCode,
                schema: CredentialSchema::parse("google-v1").unwrap(),
            },
            config: AuthConfig {
                id: config.clone(),
                provider: provider.clone(),
                client_identity: client.clone(),
                client_secret_reference: None,
            },
            app: AppDefinition {
                    transport: crate::application::integrations::destinations::AppTransportPolicy::OwnerApproved,
                id: AppId::parse("google-fixture").unwrap(),
                provider,
                config,
                destinations: vec![],
                capabilities: vec![],
            },
            adapter: Arc::new(
                crate::adapters::integrations::google_credentials::GoogleCredentialAdapter {
                    client,
                },
            ),
        })
        .unwrap();
    Arc::new(registry)
}

#[path = "cache_update_tests.rs"]
mod cache_update_tests;

pub(super) fn workspace_connect_entry() -> Registration {
    let provider = ProviderId::parse("workspace-fixture").unwrap();
    let config = AuthConfigId::parse("workspace-a").unwrap();
    Registration {
        provider: AuthProvider {
            id: provider.clone(),
            method: AuthMethod::OAuthAuthorizationCode,
            schema: CredentialSchema::parse("workspace-fixture-v1").unwrap(),
        },
        config: AuthConfig {
            id: config.clone(),
            provider: provider.clone(),
            client_identity: ExternalSubject::parse("same-client").unwrap(),
            client_secret_reference: None,
        },
        app: AppDefinition {
            transport:
                crate::application::integrations::destinations::AppTransportPolicy::OwnerApproved,
            id: AppId::parse("workspace-a").unwrap(),
            provider,
            config,
            destinations: vec![],
            capabilities: vec![Capability::parse("resource.read").unwrap()],
        },
        adapter: Arc::new(WorkspaceAdapter {
            client: "same-client".into(),
        }),
        authorization: None,
        renewal: crate::application::integrations::renewal::RenewalStrategy::NoRenewal,
    }
}
