//! Authentication and the single `execute` entry point shared by HTTP and MCP.
use super::{
    contract::*,
    embedding::EmbeddingPool,
    operations::{OperationSpec, Surface},
    ports::{Actor, CredentialPage, GenerationProvider, NewCredential, Store, TenantAction},
};
use crate::domain::{
    admin_policy::operator_only,
    correlation::CorrelationId,
    credential::{self, TokenKind},
    error::{ErrorCode, ServiceError, valid_external_id},
    grant::NamespaceGrant,
};
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

pub use crate::domain::bounds::{MAX_GRANTS_PER_CREDENTIAL, MAX_LABEL_CHARS};

#[derive(Debug, Clone)]
pub struct ClientPrincipal {
    pub credential_id: Uuid,
    pub tenant_id: Uuid,
    pub tenant: String,
    pub label: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub grants: Vec<NamespaceGrant>,
}

#[derive(Debug, Clone)]
pub enum Principal {
    Admin(Actor),
    Client(ClientPrincipal),
}

#[derive(Clone)]
pub struct App {
    store: Arc<dyn Store>,
    embeddings: Arc<EmbeddingPool>,
    /// The configured inference provider; without one, generation requests are refused with
    /// `generation_disabled` (D9) and no generation worker runs.
    provider: Option<Arc<dyn GenerationProvider>>,
    /// This process's provider authentication breaker, shared by its workers; the metrics
    /// export reads it (p06 A4-1, review B2 F1b).
    breaker: Option<Arc<super::provider_breaker::ProviderBreaker>>,
}

fn decode<T: DeserializeOwned>(input: Value) -> Result<T, ServiceError> {
    serde_json::from_value(input)
        .map_err(|error| ServiceError::invalid(format!("Invalid request: {error}")))
}

fn encode<T: serde::Serialize>(value: T) -> Result<Value, ServiceError> {
    serde_json::to_value(value).map_err(|_| ServiceError::unavailable("Cannot encode the result"))
}

impl App {
    /// There is no reduced mode: the service always has its embedding model (D7).
    pub fn new(store: Arc<dyn Store>, embeddings: Arc<EmbeddingPool>) -> Self {
        Self {
            store,
            embeddings,
            provider: None,
            breaker: None,
        }
    }

    /// Accept generation requests for `provider` (the workers send them).
    pub fn with_generation(mut self, provider: Arc<dyn GenerationProvider>) -> Self {
        self.provider = Some(provider);
        self
    }

    pub fn provider(&self) -> Option<&Arc<dyn GenerationProvider>> {
        self.provider.as_ref()
    }

    pub fn with_breaker(mut self, breaker: Arc<super::provider_breaker::ProviderBreaker>) -> Self {
        self.breaker = Some(breaker);
        self
    }

    /// Whether this process has paused generation (no breaker: never).
    pub fn generation_paused(&self) -> bool {
        self.breaker
            .as_ref()
            .is_some_and(|breaker| breaker.is_open())
    }

    pub fn store(&self) -> &Arc<dyn Store> {
        &self.store
    }

    pub fn embeddings(&self) -> &Arc<EmbeddingPool> {
        &self.embeddings
    }

    /// Verify a bearer token for `surface`. Unknown ids and wrong secrets are the same 401;
    /// expiry and revocation are reported only after the secret matched.
    pub async fn authenticate(
        &self,
        bearer: Option<&str>,
        surface: Surface,
        now: DateTime<Utc>,
    ) -> Result<Principal, ServiceError> {
        let token = bearer
            .and_then(credential::parse)
            .ok_or_else(ServiceError::unauthenticated)?;
        let expected = match surface {
            Surface::Admin => TokenKind::Admin,
            Surface::Client => TokenKind::Client,
        };
        if token.kind != expected {
            return Err(ServiceError::unauthenticated());
        }
        let lifecycle = |expires_at: Option<DateTime<Utc>>, revoked_at: Option<DateTime<Utc>>| {
            if revoked_at.is_some() {
                Err(ServiceError::new(
                    ErrorCode::CredentialRevoked,
                    "The credential was revoked",
                ))
            } else if expires_at.is_some_and(|at| at <= now) {
                Err(ServiceError::new(
                    ErrorCode::CredentialExpired,
                    "The credential has expired",
                ))
            } else {
                Ok(())
            }
        };
        match surface {
            Surface::Admin => {
                let record = self
                    .store
                    .admin_credential(token.id)
                    .await?
                    .filter(|record| token.matches(&record.verifier))
                    .ok_or_else(ServiceError::unauthenticated)?;
                lifecycle(record.expires_at, record.revoked_at)?;
                Ok(Principal::Admin(Actor {
                    id: record.id,
                    scope: record.scope,
                    request_id: None,
                    correlation: None,
                }))
            }
            Surface::Client => {
                let record = self
                    .store
                    .client_credential(token.id)
                    .await?
                    .filter(|record| token.matches(&record.verifier))
                    .ok_or_else(ServiceError::unauthenticated)?;
                lifecycle(record.expires_at, record.revoked_at)?;
                match record.tenant_state {
                    TenantState::Active => {}
                    TenantState::Disabled => {
                        return Err(ServiceError::new(
                            ErrorCode::TenantDisabled,
                            "The tenant is disabled",
                        ));
                    }
                    TenantState::Deleted => {
                        return Err(ServiceError::new(
                            ErrorCode::CredentialRevoked,
                            "The credential was revoked",
                        ));
                    }
                }
                Ok(Principal::Client(ClientPrincipal {
                    credential_id: record.id,
                    tenant_id: record.tenant_id,
                    tenant: record.tenant,
                    label: record.label,
                    expires_at: record.expires_at,
                    grants: record.grants,
                }))
            }
        }
    }

    /// Run one registry operation for an authenticated principal. Both transports call this;
    /// the request id is the one the transport reports in its envelope and `x-request-id`, and
    /// writes record it, with the caller's correlation id, in the audit trail.
    pub async fn execute(
        &self,
        spec: &OperationSpec,
        principal: &Principal,
        input: Value,
        now: DateTime<Utc>,
        ids: impl Into<RequestIds>,
    ) -> Result<Value, ServiceError> {
        let ids = ids.into();
        match (spec.surface, principal) {
            (Surface::Client, Principal::Client(client)) => {
                self.client(spec, client, input, now, ids).await
            }
            (Surface::Admin, Principal::Admin(actor)) => {
                let actor = Actor {
                    request_id: Some(ids.request),
                    correlation: ids.correlation,
                    ..*actor
                };
                self.admin(spec, actor, input, now).await
            }
            _ => Err(ServiceError::unauthenticated()),
        }
    }

    async fn client(
        &self,
        spec: &OperationSpec,
        client: &ClientPrincipal,
        input: Value,
        now: DateTime<Utc>,
        ids: RequestIds,
    ) -> Result<Value, ServiceError> {
        // Validate the body against its declared contract first, so declared-but-unimplemented
        // operations still reject malformed requests with `invalid_request`.
        (spec.parse)(&input)
            .map_err(|error| ServiceError::invalid(format!("Invalid request: {error}")))?;
        match spec.id {
            "credential.get" => encode(credential_view(client)),
            // Declared with schemas; the remaining use cases arrive in phases 03-06.
            _ if !spec.implemented => Err(ServiceError::not_implemented(spec.id)),
            "observations.search" => {
                super::search::search(self.store.as_ref(), &self.embeddings, client, input).await
            }
            "observations.timeline" => {
                super::recall::timeline(self.store.as_ref(), client, input).await
            }
            "context.pack" => {
                super::recall::pack(self.store.as_ref(), &self.embeddings, client, input, now).await
            }
            "context.validate" => super::recall::validate(self.store.as_ref(), client, input).await,
            "generations.request" => {
                super::generation::request(
                    self.store.as_ref(),
                    self.provider.as_deref(),
                    client,
                    input,
                    now,
                    ids,
                )
                .await
            }
            "generations.status" => {
                super::generation::status(self.store.as_ref(), client, input).await
            }
            "generations.cancel" => {
                super::generation::cancel(self.store.as_ref(), client, input, now, ids).await
            }
            _ => {
                super::memory::execute(self.store.as_ref(), spec.id, client, input, now, ids).await
            }
        }
    }

    async fn admin(
        &self,
        spec: &OperationSpec,
        actor: Actor,
        input: Value,
        now: DateTime<Utc>,
    ) -> Result<Value, ServiceError> {
        let store = &self.store;
        match spec.id {
            "admin.tenants.create" => {
                let request: TenantCreateRequest = decode(input)?;
                external_id(&request.external_id)?;
                encode(store.create_tenant(actor, &request.external_id).await?)
            }
            "admin.tenants.disable" | "admin.tenants.enable" | "admin.tenants.delete" => {
                let request: TenantRef = decode(input)?;
                external_id(&request.tenant)?;
                let action = match spec.id {
                    "admin.tenants.disable" => TenantAction::Disable,
                    "admin.tenants.enable" => TenantAction::Enable,
                    _ => TenantAction::Delete,
                };
                encode(store.change_tenant(actor, &request.tenant, action).await?)
            }
            "admin.tenants.settings" => {
                let request: TenantSettingsRequest = decode(input)?;
                require_operator(actor)?;
                external_id(&request.tenant)?;
                if request.generation_daily_token_cap > i64::MAX as u64 {
                    return Err(ServiceError::invalid("The token cap is too large"));
                }
                encode(
                    store
                        .set_tenant_cap(actor, &request.tenant, request.generation_daily_token_cap)
                        .await?,
                )
            }
            "admin.namespaces.create" | "admin.namespaces.delete" => {
                let request: NamespaceRequest = decode(input)?;
                external_id(&request.tenant)?;
                external_id(&request.namespace)?;
                if spec.id == "admin.namespaces.create" {
                    encode(
                        store
                            .create_namespace(actor, &request.tenant, &request.namespace)
                            .await?,
                    )
                } else {
                    encode(
                        store
                            .delete_namespace(actor, &request.tenant, &request.namespace)
                            .await?,
                    )
                }
            }
            "admin.credentials.issue" => {
                let request: CredentialIssueRequest = decode(input)?;
                external_id(&request.tenant)?;
                label(&request.label)?;
                future_expiry(request.expires_at, now)?;
                validate_grants(&request.grants)?;
                let issued = credential::issue(TokenKind::Client)
                    .map_err(|_| ServiceError::unavailable("No randomness available"))?;
                store
                    .issue_credential(
                        actor,
                        &request.tenant,
                        NewCredential {
                            id: issued.id,
                            label: request.label,
                            expires_at: request.expires_at,
                            verifier: issued.verifier,
                            grants: request.grants,
                        },
                    )
                    .await?;
                encode(IssuedCredential {
                    credential_id: issued.id,
                    token: issued.token,
                    expires_at: request.expires_at,
                })
            }
            "admin.credentials.list" => {
                let request: CredentialListRequest = decode(input)?;
                external_id(&request.tenant)?;
                if let Some(value) = &request.label {
                    label(value)?;
                }
                let limit = request.limit.unwrap_or(MAX_CREDENTIAL_PAGE);
                if !(1..=MAX_CREDENTIAL_PAGE).contains(&limit) {
                    return Err(ServiceError::invalid("limit is 1-1000"));
                }
                encode(
                    store
                        .list_credentials(
                            actor,
                            &request.tenant,
                            CredentialPage {
                                label: request.label.as_deref(),
                                active_only: request.active_only,
                                after: request.after,
                                limit,
                            },
                        )
                        .await?,
                )
            }
            "admin.tenants.reassign" => {
                let request: TenantReassignRequest = decode(input)?;
                require_operator(actor)?;
                let limit = request.limit.unwrap_or(MAX_REASSIGN_BATCH);
                if !(1..=MAX_REASSIGN_BATCH).contains(&limit) {
                    return Err(ServiceError::invalid("limit is 1-500"));
                }
                if request.from == request.to {
                    return Err(invalid_target());
                }
                encode(
                    store
                        .reassign_tenants(actor, request.from, request.to, limit)
                        .await?,
                )
            }
            "admin.credentials.revoke" => {
                let request: CredentialRevokeRequest = decode(input)?;
                external_id(&request.tenant)?;
                encode(
                    store
                        .revoke_credential(actor, &request.tenant, request.credential_id)
                        .await?,
                )
            }
            "admin.admin_credentials.issue" => {
                let request: AdminCredentialIssueRequest = decode(input)?;
                require_operator(actor)?;
                label(&request.label)?;
                future_expiry(request.expires_at, now)?;
                let issued = credential::issue(TokenKind::Admin)
                    .map_err(|_| ServiceError::unavailable("No randomness available"))?;
                store
                    .issue_admin_credential(
                        actor,
                        issued.id,
                        request.scope,
                        &request.label,
                        request.expires_at,
                        issued.verifier,
                    )
                    .await?;
                encode(IssuedCredential {
                    credential_id: issued.id,
                    token: issued.token,
                    expires_at: request.expires_at,
                })
            }
            "admin.admin_credentials.revoke" => {
                let request: AdminCredentialRevokeRequest = decode(input)?;
                require_operator(actor)?;
                encode(
                    store
                        .revoke_admin_credential(actor, request.credential_id)
                        .await?,
                )
            }
            _ => Err(ServiceError::not_implemented(spec.id)),
        }
    }
}

pub fn credential_view(client: &ClientPrincipal) -> CredentialView {
    CredentialView {
        tenant: client.tenant.clone(),
        credential_id: client.credential_id,
        label: client.label.clone(),
        expires_at: client.expires_at,
        revoked_at: None,
        grants: client.grants.iter().map(grant_spec).collect(),
    }
}

pub fn grant_spec(grant: &NamespaceGrant) -> GrantSpec {
    GrantSpec {
        namespace: grant.namespace.clone(),
        read_max: grant.read_max,
        write_max: grant.write_max,
        operations: grant.operations.iter().copied().collect(),
    }
}

/// The ids one request carries (06A A2-2): the service-minted request id and the caller's
/// optional correlation id.
#[derive(Debug, Clone, Copy)]
pub struct RequestIds {
    pub request: Uuid,
    pub correlation: Option<CorrelationId>,
}

impl From<Uuid> for RequestIds {
    fn from(request: Uuid) -> Self {
        Self {
            request,
            correlation: None,
        }
    }
}

/// Page bounds of the admin listings (06A).
pub const MAX_CREDENTIAL_PAGE: u32 = 1_000;
pub const MAX_REASSIGN_BATCH: u32 = 500;

/// A reassignment target that is not an active, unexpired provisioner credential other than the
/// current manager: a 409 `conflict` whose details name the reason.
pub fn invalid_target() -> ServiceError {
    ServiceError::new(
        ErrorCode::Conflict,
        "The target must be an active provisioner credential other than the current manager",
    )
    .with_details(serde_json::json!({ "reason": "invalid_target" }))
}

fn require_operator(actor: Actor) -> Result<(), ServiceError> {
    if operator_only(actor.scope) {
        Ok(())
    } else {
        Err(ServiceError::forbidden())
    }
}

fn external_id(value: &str) -> Result<(), ServiceError> {
    if valid_external_id(value) {
        Ok(())
    } else {
        Err(ServiceError::invalid(
            "Identifiers are 1-128 bytes of [A-Za-z0-9._:-]",
        ))
    }
}

/// Admin and client credential labels: 1-128 characters, not blank.
pub fn label(value: &str) -> Result<(), ServiceError> {
    if value.trim().is_empty() || value.chars().count() > MAX_LABEL_CHARS {
        Err(ServiceError::invalid("Labels are 1-128 characters"))
    } else {
        Ok(())
    }
}

fn future_expiry(
    expires_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Result<(), ServiceError> {
    match expires_at {
        Some(at) if at <= now => Err(ServiceError::invalid("The expiry must be in the future")),
        _ => Ok(()),
    }
}

fn validate_grants(grants: &[GrantSpec]) -> Result<(), ServiceError> {
    if grants.is_empty() || grants.len() > MAX_GRANTS_PER_CREDENTIAL {
        return Err(ServiceError::invalid(
            "A credential needs 1-64 namespace grants",
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for grant in grants {
        external_id(&grant.namespace)?;
        if !seen.insert(&grant.namespace) {
            return Err(ServiceError::invalid("A namespace is granted twice"));
        }
        if grant.operations.is_empty() {
            return Err(ServiceError::invalid(
                "A grant needs at least one operation",
            ));
        }
        // A grant must be usable as issued: reads name a ceiling within read_max, and writes
        // (including delete and purge) need write authority (D3, Operation::reads/writes).
        if grant.operations.iter().any(|op| op.reads()) && grant.read_max.is_none() {
            return Err(ServiceError::invalid(
                "A grant with read operations needs read_max",
            ));
        }
        if grant.operations.iter().any(|op| op.writes()) && grant.write_max.is_none() {
            return Err(ServiceError::invalid(
                "A grant with write operations needs write_max",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{access_level::AccessLevel, grant::Operation};

    fn spec(read: Option<i64>, write: Option<i64>, operations: &[Operation]) -> GrantSpec {
        GrantSpec {
            namespace: "ns".into(),
            read_max: read.map(|l| AccessLevel::new(l).unwrap()),
            write_max: write.map(|l| AccessLevel::new(l).unwrap()),
            operations: operations.to_vec(),
        }
    }

    #[test]
    fn memory_authority_grants_must_be_usable_as_issued() {
        use Operation as O;
        // Every write operation needs write_max; every read operation needs read_max.
        for op in Operation::ALL.into_iter().filter(|op| op.writes()) {
            assert!(
                validate_grants(&[spec(Some(9), None, &[op])]).is_err(),
                "{op:?}"
            );
            assert!(
                validate_grants(&[spec(None, Some(0), &[op])]).is_ok() || op.reads(),
                "{op:?}"
            );
        }
        for op in Operation::ALL.into_iter().filter(|op| op.reads()) {
            assert!(
                validate_grants(&[spec(None, Some(9), &[op])]).is_err(),
                "{op:?}"
            );
        }
        // Delete reads (above-ceiling ids are not_found) and writes; purge only writes.
        assert!(validate_grants(&[spec(Some(3), None, &[O::Delete])]).is_err());
        assert!(validate_grants(&[spec(None, Some(3), &[O::Delete])]).is_err());
        assert!(validate_grants(&[spec(Some(3), Some(3), &[O::Delete])]).is_ok());
        assert!(validate_grants(&[spec(None, Some(0), &[O::Purge])]).is_ok());
        assert!(validate_grants(&[spec(Some(6), None, &[O::Search, O::Fetch])]).is_ok());
        assert!(validate_grants(&[spec(Some(6), None, &[O::Search, O::Save])]).is_err());
        // Shape bounds.
        assert!(validate_grants(&[]).is_err());
        assert!(validate_grants(&[spec(Some(1), None, &[])]).is_err());
        let twice = spec(Some(1), None, &[O::Search]);
        assert!(validate_grants(&[twice.clone(), twice]).is_err());
        let many: Vec<_> = (0..=MAX_GRANTS_PER_CREDENTIAL)
            .map(|i| GrantSpec {
                namespace: format!("ns-{i}"),
                ..spec(Some(1), None, &[O::Search])
            })
            .collect();
        assert!(validate_grants(&many).is_err());
    }
}
