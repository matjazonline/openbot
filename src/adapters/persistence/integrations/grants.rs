//! The module owns consent, credential compatibility and revocation. Host callbacks
//! enter this adapter only after acquiring their short host authority locks.
mod dispatch;
use super::{
    PostgresVault,
    accounts::lock_owner,
    rows::{AccountRow, COLUMNS},
    storage_error,
};
use crate::{
    adapters::persistence::credentials::envelope::CredentialContext,
    application::integrations::{
        grants::GrantWriter, management::AccountRepository, private::InjectionMethod, *,
    },
    domain::integrations::{authority::*, *},
};
use async_trait::async_trait;
use secrecy::ExposeSecret;
use sqlx::{PgConnection, Postgres, Transaction};
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;

const GRANT_COLUMNS: &str = "id,account_id,owner_namespace,owner_subject,host_scope,host_binding,app_id,app_policy,destination,purpose,account_authorization,revision,enabled";
#[derive(sqlx::FromRow)]
struct GrantRow {
    id: Uuid,
    account_id: Uuid,
    owner_namespace: String,
    owner_subject: String,
    host_scope: String,
    host_binding: String,
    app_id: String,
    app_policy: String,
    destination: String,
    purpose: String,
    account_authorization: i64,
    revision: i64,
    enabled: bool,
}
impl GrantRow {
    fn decode(self) -> IntegrationResult<UseGrant> {
        Ok(UseGrant {
            id: UseGrantId::new(self.id),
            account: AccountId::new(self.account_id),
            owner: AccountOwner {
                namespace: OwnerNamespace::parse(self.owner_namespace.clone())?,
                subject: OwnerSubject::parse(self.owner_subject)?,
            },
            client: OwnerNamespace::parse(self.owner_namespace)?,
            host_scope: HostBinding::parse(self.host_scope)?,
            binding: HostBinding::parse(self.host_binding)?,
            app: AppId::parse(self.app_id)?,
            app_policy: AppPolicyDigest::parse(self.app_policy)?,
            destination: ApprovedDestination::parse(self.destination)?,
            purpose: purpose(&self.purpose)?,
            account_authorization: AuthorizationRevision::new(self.account_authorization)?,
            revision: GrantRevision::new(self.revision)?,
            enabled: self.enabled,
        })
    }
}
fn purpose(value: &str) -> IntegrationResult<InvocationKind> {
    match value {
        "http" => Ok(InvocationKind::DeterministicHttp),
        "mcp" => Ok(InvocationKind::DeterministicMcp),
        _ => Err(IntegrationError::Invalid),
    }
}
pub(crate) fn purpose_name(value: InvocationKind) -> IntegrationResult<&'static str> {
    match value {
        InvocationKind::DeterministicHttp => Ok("http"),
        InvocationKind::DeterministicMcp => Ok("mcp"),
        _ => Err(IntegrationError::Denied),
    }
}
async fn account_on(
    tx: &mut Transaction<'_, Postgres>,
    owner: &AccountOwner,
    id: AccountId,
) -> IntegrationResult<ConnectedAccount> {
    lock_owner(tx, owner).await?;
    sqlx::query_as::<_,AccountRow>(&format!("SELECT {COLUMNS} FROM integration_accounts WHERE owner_namespace=$1 AND owner_subject=$2 AND id=$3 FOR UPDATE"))
        .bind(owner.namespace.as_str()).bind(owner.subject.as_str()).bind(id.as_uuid())
        .fetch_optional(&mut **tx).await.map_err(storage_error)?.ok_or(IntegrationError::NotFound)?.decode()
}
pub(super) async fn read_account_on(
    tx: &mut Transaction<'_, Postgres>,
    owner: &AccountOwner,
    id: AccountId,
) -> IntegrationResult<ConnectedAccount> {
    super::accounts::read_lock_owner(tx, owner).await?;
    sqlx::query_as::<_,AccountRow>(&format!("SELECT {COLUMNS} FROM integration_accounts WHERE owner_namespace=$1 AND owner_subject=$2 AND id=$3 FOR SHARE"))
        .bind(owner.namespace.as_str()).bind(owner.subject.as_str()).bind(id.as_uuid())
        .fetch_optional(&mut **tx).await.map_err(storage_error)?.ok_or(IntegrationError::NotFound)?.decode()
}
pub(super) async fn grant_on(db: &mut PgConnection, id: UseGrantId) -> IntegrationResult<UseGrant> {
    sqlx::query_as::<_, GrantRow>(&format!(
        "SELECT {GRANT_COLUMNS} FROM integration_use_grants WHERE id=$1"
    ))
    .bind(id.as_uuid())
    .fetch_optional(db)
    .await
    .map_err(storage_error)?
    .ok_or(IntegrationError::NotFound)?
    .decode()
}
pub(super) async fn invalidate_account(
    db: &mut PgConnection,
    id: AccountId,
) -> IntegrationResult<()> {
    sqlx::query("UPDATE integration_use_grants SET enabled=false,revision=revision+1 WHERE account_id=$1 AND enabled")
        .bind(id.as_uuid()).execute(db).await.map_err(storage_error)?;
    Ok(())
}
pub(super) async fn pending(
    db: &mut PgConnection,
    owner: &AccountOwner,
    scope: &HostBinding,
) -> IntegrationResult<()> {
    let denied: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM integration_teardown_intents WHERE owner_namespace=$1 AND state IN ('pending','failed') AND (kind='owner' AND owner_subject=$2 OR host_binding=$3 AND (kind='company' OR kind='membership' AND owner_subject=$2)))")
        .bind(owner.namespace.as_str()).bind(owner.subject.as_str()).bind(scope.as_str()).fetch_one(db).await.map_err(storage_error)?;
    if denied {
        return Err(IntegrationError::Denied);
    }
    Ok(())
}
impl PostgresVault {
    pub(crate) async fn grant_metadata_on(
        db: &mut PgConnection,
        id: UseGrantId,
    ) -> IntegrationResult<UseGrant> {
        grant_on(db, id).await
    }
    pub(crate) async fn grant_metadata(&self, id: UseGrantId) -> IntegrationResult<UseGrant> {
        let mut db = self
            .persistence
            .pool()
            .acquire()
            .await
            .map_err(storage_error)?;
        grant_on(&mut db, id).await
    }
    pub(crate) async fn grant_page(
        &self,
        owner: &AccountOwner,
        account: AccountId,
        after: Option<UseGrantId>,
    ) -> IntegrationResult<GrantInspection> {
        self.get(owner, account).await?;
        let rows=sqlx::query_as::<_,GrantRow>(&format!("SELECT {GRANT_COLUMNS} FROM integration_use_grants WHERE owner_namespace=$1 AND owner_subject=$2 AND account_id=$3 AND ($4::uuid IS NULL OR id>$4) ORDER BY id LIMIT 51"))
            .bind(owner.namespace.as_str()).bind(owner.subject.as_str()).bind(account.as_uuid()).bind(after.map(UseGrantId::as_uuid))
            .fetch_all(self.persistence.pool()).await.map_err(storage_error)?;
        let more = rows.len() > 50;
        let grants = rows
            .into_iter()
            .take(50)
            .map(GrantRow::decode)
            .collect::<IntegrationResult<Vec<_>>>()?;
        let next = more.then(|| grants.last().map(|g| g.id)).flatten();
        Ok(GrantInspection {
            grants,
            next,
            attachment_available: true,
        })
    }
    pub(super) async fn validate_binding_payload(
        &self,
        db: &mut PgConnection,
        account: &ConnectedAccount,
        app: &AppId,
        destination: &ApprovedDestination,
        kind: InvocationKind,
    ) -> IntegrationResult<()> {
        let registration = self.registry.get(app)?;
        registration.app.transport.kind(kind)?;
        if account.provider_id != registration.provider.id
            || account.auth_config_id != registration.config.id
            || account.auth_method != registration.provider.method
            || account.credential_schema != registration.provider.schema
            || !registration.app.destinations.is_empty()
                && !registration.app.destinations.contains(destination)
        {
            return Err(IntegrationError::Denied);
        }
        let envelope: Option<String> =
            sqlx::query_scalar("SELECT envelope FROM integration_accounts WHERE id=$1")
                .bind(account.id.as_uuid())
                .fetch_one(db)
                .await
                .map_err(storage_error)?;
        let envelope = envelope.ok_or(IntegrationError::Credential)?;
        if envelope.len() > 23000 {
            return Err(IntegrationError::Credential);
        }
        let payload = self
            .persistence
            .credential_cipher()
            .map_err(|_| IntegrationError::Credential)?
            .open_envelope(&CredentialContext::integration_account(account), &envelope)
            .map_err(|_| IntegrationError::Credential)?;
        registration
            .adapter
            .validate_use(&payload, &registration.app)?;
        // Managed entries may have no cached token at binding time. Their typed
        // adapter validates consent; only static injection needs a size check here.
        if account.auth_method == AuthMethod::StaticSecret {
            let injection = registration.adapter.injection(&payload)?;
            let max = match kind {
                InvocationKind::DeterministicHttp => {
                    crate::application::workflow::http::MAX_HTTP_SECRET
                }
                InvocationKind::DeterministicMcp => {
                    crate::domain::entities::mcp::MAX_MCP_SECRET_BYTES
                }
                _ => return Err(IntegrationError::Denied),
            };
            if injection.value.expose_secret().len() > max
                || kind == InvocationKind::DeterministicMcp
                    && !matches!(injection.method, InjectionMethod::Bearer)
            {
                return Err(IntegrationError::Invalid);
            }
        }
        Ok(())
    }
    /// The required host port calls this module gate while holding host locks.
    /// Track successful callback execution, so a host adapter cannot accidentally
    /// return success without consulting current module authority.
    pub(crate) async fn final_dispatch_guard(
        &self,
        invocation: &AuthorizedInvocation,
        context: &ExecutionContext,
    ) -> IntegrationResult<()> {
        self.dispatch_guard(invocation, context, GenerationCheck::Current)
            .await
    }
    async fn dispatch_guard(
        &self,
        invocation: &AuthorizedInvocation,
        context: &ExecutionContext,
        generation: GenerationCheck,
    ) -> IntegrationResult<()> {
        let gate = CurrentGate {
            vault: self,
            expected: invocation,
            generation,
            passed: AtomicBool::new(false),
        };
        let remaining = (invocation.deadline - chrono::Utc::now())
            .to_std()
            .map_err(|_| IntegrationError::Denied)?;
        tokio::select! { biased;
            _=context.cancellation.cancelled()=>return Err(IntegrationError::Denied),
            _=tokio::time::sleep(remaining)=>return Err(IntegrationError::Denied),
            result=context.authority.check(invocation,&gate)=>result?,
        }
        if !gate.passed.load(Ordering::Acquire) {
            return Err(IntegrationError::Denied);
        }
        Ok(())
    }
}
#[async_trait]
impl GrantWriter for PostgresVault {
    async fn create_confirmed(
        &self,
        c: &OwnerConfirmation,
        facts: GrantAuthorityFacts,
    ) -> IntegrationResult<UseGrant> {
        let mut tx = self
            .persistence
            .pool()
            .begin()
            .await
            .map_err(storage_error)?;
        sqlx::query("SET LOCAL lock_timeout='5s'")
            .execute(&mut *tx)
            .await
            .map_err(storage_error)?;
        let account = account_on(&mut tx, c.owner.owner(), c.account).await?;
        authorize_grant(c.owner.owner(), &account, &c.owner.owner().namespace, facts)?;
        if account.authorization_revision != c.expected_authorization {
            return Err(IntegrationError::Conflict);
        }
        pending(&mut tx, &account.owner, &c.host_scope).await?;
        self.validate_binding_payload(&mut tx, &account, &c.app, &c.destination, c.purpose)
            .await?;
        let id:Uuid=sqlx::query_scalar("INSERT INTO integration_use_grants(id,account_id,owner_namespace,owner_subject,host_scope,host_binding,app_id,destination,purpose,account_authorization,app_policy,revision,enabled) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,1,true) ON CONFLICT(account_id,host_binding) DO UPDATE SET app_id=EXCLUDED.app_id,app_policy=EXCLUDED.app_policy,destination=EXCLUDED.destination,purpose=EXCLUDED.purpose,account_authorization=EXCLUDED.account_authorization,revision=integration_use_grants.revision+1,enabled=true,confirmed_at=clock_timestamp() RETURNING id")
            .bind(Uuid::new_v4()).bind(account.id.as_uuid()).bind(account.owner.namespace.as_str()).bind(account.owner.subject.as_str())
            .bind(c.host_scope.as_str()).bind(c.binding.as_str()).bind(c.app.as_str()).bind(c.destination.as_str()).bind(purpose_name(c.purpose)?).bind(account.authorization_revision.get()).bind(self.registry.policy(&c.app)?.as_str())
            .fetch_one(&mut *tx).await.map_err(storage_error)?;
        let grant = grant_on(&mut tx, UseGrantId::new(id)).await?;
        tx.commit().await.map_err(storage_error)?;
        Ok(grant)
    }
    async fn revoke(
        &self,
        owner: &AccountOwner,
        id: UseGrantId,
        expected: GrantRevision,
    ) -> IntegrationResult<()> {
        let mut tx = self
            .persistence
            .pool()
            .begin()
            .await
            .map_err(storage_error)?;
        let grant = grant_on(&mut tx, id).await?;
        if &grant.owner != owner {
            return Err(IntegrationError::NotFound);
        }
        account_on(&mut tx, owner, grant.account).await?;
        let updated=sqlx::query("UPDATE integration_use_grants SET enabled=false,revision=revision+1 WHERE id=$1 AND revision=$2")
            .bind(id.as_uuid()).bind(expected.get()).execute(&mut *tx).await.map_err(storage_error)?;
        if updated.rows_affected() != 1 {
            return Err(IntegrationError::Conflict);
        }
        tx.commit().await.map_err(storage_error)?;
        super::observations::Event::GrantRevoked.emit();
        Ok(())
    }
}
struct CurrentGate<'a> {
    vault: &'a PostgresVault,
    expected: &'a AuthorizedInvocation,
    passed: AtomicBool,
    generation: GenerationCheck,
}
#[derive(Clone, Copy)]
enum GenerationCheck {
    Current,
    Exact(CredentialGeneration),
}

#[async_trait]
impl crate::application::integrations::renewal::ExecutionCredentials for PostgresVault {
    async fn resolve_configuration(
        &self,
        context: &OwnerDiscoveryContext,
    ) -> crate::application::integrations::renewal::ResolutionResult<
        crate::application::integrations::renewal::ResolvedCredential,
    > {
        self.resolve_configuration_credential(context).await
    }
    async fn check_configuration(
        &self,
        context: &OwnerDiscoveryContext,
        generation: CredentialGeneration,
    ) -> IntegrationResult<()> {
        self.configuration_generation_guard(context, generation)
            .await
    }

    async fn check_generation(
        &self,
        invocation: &AuthorizedInvocation,
        context: &ExecutionContext,
        generation: CredentialGeneration,
    ) -> IntegrationResult<()> {
        self.dispatch_guard(invocation, context, GenerationCheck::Exact(generation))
            .await
    }
}
#[async_trait]
impl CredentialDispatchGate for CurrentGate<'_> {
    async fn check(
        &self,
        invocation: &AuthorizedInvocation,
        host: HostAuthorityFacts,
    ) -> IntegrationResult<()> {
        if serde_json::to_value(invocation).map_err(|_| IntegrationError::Invalid)?
            != serde_json::to_value(self.expected).map_err(|_| IntegrationError::Invalid)?
        {
            return Err(IntegrationError::Denied);
        }
        if invocation.deadline <= chrono::Utc::now() {
            return Err(IntegrationError::Denied);
        }
        let mut tx = self
            .vault
            .persistence
            .pool()
            .begin()
            .await
            .map_err(storage_error)?;
        sqlx::query("SET LOCAL lock_timeout='5s'")
            .execute(&mut *tx)
            .await
            .map_err(storage_error)?;
        self.vault
            .module_gate(
                &mut tx,
                &invocation.authorization,
                self.generation,
                dispatch::ModuleAuthority::Business(host),
            )
            .await?;
        tx.commit().await.map_err(storage_error)?;
        self.passed.store(true, Ordering::Release);
        Ok(())
    }
}
