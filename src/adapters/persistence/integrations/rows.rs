use crate::domain::integrations::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;
pub(super) const COLUMNS: &str = "id,owner_namespace,owner_subject,key,provider_id,auth_config_id,auth_method,credential_schema,external_identity,revision,authorization_revision,credential_generation,status,metadata,created_at,updated_at";
#[derive(sqlx::FromRow)]
pub(super) struct AccountRow {
    pub id: Uuid,
    pub owner_namespace: String,
    pub owner_subject: String,
    pub key: String,
    pub provider_id: String,
    pub auth_config_id: String,
    pub auth_method: String,
    pub credential_schema: String,
    pub external_identity: Option<String>,
    pub revision: i64,
    pub authorization_revision: i64,
    pub credential_generation: i64,
    pub status: String,
    pub metadata: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
impl AccountRow {
    pub fn decode(self) -> IntegrationResult<ConnectedAccount> {
        if self.metadata.len() > MAX_METADATA_BYTES
            || self
                .external_identity
                .as_ref()
                .is_some_and(|s| s.len() > 2048)
        {
            return Err(IntegrationError::Storage);
        }
        CredentialGeneration::new(self.credential_generation)?;
        Ok(ConnectedAccount {
            id: AccountId::new(self.id),
            owner: AccountOwner {
                namespace: OwnerNamespace::parse(self.owner_namespace)?,
                subject: OwnerSubject::parse(self.owner_subject)?,
            },
            key: AccountKey::parse(self.key)?,
            provider_id: ProviderId::parse(self.provider_id)?,
            auth_config_id: AuthConfigId::parse(self.auth_config_id)?,
            auth_method: match self.auth_method.as_str() {
                "static_secret" => AuthMethod::StaticSecret,
                "oauth_authorization_code" => AuthMethod::OAuthAuthorizationCode,
                _ => return Err(IntegrationError::Storage),
            },
            credential_schema: CredentialSchema::parse(self.credential_schema)?,
            external_identity: self
                .external_identity
                .map(|s| serde_json::from_str(&s))
                .transpose()
                .map_err(|_| IntegrationError::Storage)?,
            revision: AccountRevision::new(self.revision)?,
            authorization_revision: AuthorizationRevision::new(self.authorization_revision)?,
            status: match self.status.as_str() {
                "ready" => AccountStatus::Ready,
                "reconnect_required" => AccountStatus::ReconnectRequired,
                "disconnected" => AccountStatus::Disconnected,
                _ => return Err(IntegrationError::Storage),
            },
            metadata: serde_json::from_str(&self.metadata)
                .map_err(|_| IntegrationError::Storage)?,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}
