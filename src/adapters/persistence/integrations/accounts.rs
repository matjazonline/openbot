use super::super::credentials::envelope::CredentialContext;
use super::{
    PostgresVault,
    rows::{AccountRow, COLUMNS},
    storage_error,
};
use crate::{
    application::integrations::{
        management::{AccountRepository, AccountWrite},
        private::{CredentialCacheUpdate, CredentialRecord, CredentialStore},
    },
    domain::integrations::*,
};
use async_trait::async_trait;
use chrono::Utc;
use secrecy::SecretString;
use sqlx::{PgConnection, Postgres, Transaction};
use uuid::Uuid;

pub(super) async fn lock_owner(
    tx: &mut Transaction<'_, Postgres>,
    owner: &AccountOwner,
) -> IntegrationResult<()> {
    let retired=sqlx::query_scalar::<_,bool>("SELECT retired FROM integration_owner_states WHERE owner_namespace=$1 AND owner_subject=$2 FOR UPDATE")
        .bind(owner.namespace.as_str()).bind(owner.subject.as_str()).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    if retired != Some(false) {
        return Err(IntegrationError::Denied);
    }
    Ok(())
}
pub(super) async fn read_lock_owner(
    db: &mut sqlx::PgConnection,
    owner: &AccountOwner,
) -> IntegrationResult<()> {
    let retired=sqlx::query_scalar::<_,bool>("SELECT retired FROM integration_owner_states WHERE owner_namespace=$1 AND owner_subject=$2 FOR SHARE")
        .bind(owner.namespace.as_str()).bind(owner.subject.as_str()).fetch_optional(db).await.map_err(storage_error)?;
    if retired != Some(false) {
        return Err(IntegrationError::Denied);
    }
    Ok(())
}
pub(super) enum AccountRead {
    Metadata,
    Locked,
}
pub(super) async fn load(
    connection: &mut PgConnection,
    owner: &AccountOwner,
    id: AccountId,
    mode: AccountRead,
) -> IntegrationResult<AccountRow> {
    let suffix = match mode {
        AccountRead::Locked => " FOR UPDATE",
        AccountRead::Metadata => "",
    };
    sqlx::query_as::<_,AccountRow>(&format!("SELECT {COLUMNS} FROM integration_accounts WHERE owner_namespace=$1 AND owner_subject=$2 AND id=$3{suffix}"))
        .bind(owner.namespace.as_str()).bind(owner.subject.as_str()).bind(id.as_uuid()).fetch_optional(connection).await.map_err(storage_error)?.ok_or(IntegrationError::NotFound)
}
pub(super) fn new_account(owner: &AccountOwner, w: &AccountWrite) -> ConnectedAccount {
    ConnectedAccount {
        id: AccountId::new(Uuid::new_v4()),
        owner: owner.clone(),
        key: w.key.clone(),
        provider_id: w.provider.clone(),
        auth_config_id: w.config.clone(),
        auth_method: w.method,
        credential_schema: w.schema.clone(),
        external_identity: w.identity.clone(),
        revision: AccountRevision::INITIAL,
        authorization_revision: AuthorizationRevision::INITIAL,
        status: AccountStatus::Ready,
        metadata: w.metadata.clone(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}
impl PostgresVault {
    pub(super) fn validate_write(&self, w: &AccountWrite) -> IntegrationResult<()> {
        let entry = self.registration(&w.provider, &w.config, w.method, &w.schema)?;
        let identity = self
            .registry
            .validate(&entry.app.id, w.method, &w.schema, &w.payload)?;
        if identity != w.identity {
            return Err(IntegrationError::Invalid);
        }
        Ok(())
    }
    async fn locked_account(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        owner: &AccountOwner,
        id: AccountId,
        expected: AccountRevision,
    ) -> IntegrationResult<(ConnectedAccount, CredentialVersions)> {
        lock_owner(tx, owner).await?;
        let row = load(tx, owner, id, AccountRead::Locked).await?;
        let generation = CredentialGeneration::new(row.credential_generation)?;
        let account = row.decode()?;
        if account.revision != expected {
            return Err(IntegrationError::Conflict);
        }
        let versions = CredentialVersions {
            revision: account.revision,
            authorization: account.authorization_revision,
            generation,
        };
        Ok((account, versions))
    }
    async fn locked_authorization(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        owner: &AccountOwner,
        id: AccountId,
        expected: AuthorizationRevision,
    ) -> IntegrationResult<(ConnectedAccount, CredentialVersions)> {
        lock_owner(tx, owner).await?;
        let row = load(tx, owner, id, AccountRead::Locked).await?;
        let generation = CredentialGeneration::new(row.credential_generation)?;
        let account = row.decode()?;
        if account.authorization_revision != expected {
            return Err(IntegrationError::Conflict);
        }
        let versions = CredentialVersions {
            revision: account.revision,
            authorization: account.authorization_revision,
            generation,
        };
        Ok((account, versions))
    }
    fn seal_managed_cache_update(
        &self,
        account: &ConnectedAccount,
        stored: &str,
        payload: &SecretString,
        current: CredentialGeneration,
        next: CredentialGeneration,
    ) -> IntegrationResult<String> {
        let entry = self.registration(
            &account.provider_id,
            &account.auth_config_id,
            account.auth_method,
            &account.credential_schema,
        )?;
        let identity = self.registry.validate(
            &entry.app.id,
            account.auth_method,
            &account.credential_schema,
            payload,
        )?;
        if identity != account.external_identity {
            return Err(IntegrationError::Denied);
        }
        if stored.len() > 23000 {
            return Err(IntegrationError::Credential);
        }
        let cipher = self
            .persistence
            .credential_cipher()
            .map_err(|_| IntegrationError::Credential)?;
        let context = CredentialContext::integration_account(account);
        let previous = cipher
            .open_envelope(&context, stored)
            .map_err(|_| IntegrationError::Credential)?;
        self.registry
            .validate_generation(&entry.app.id, &previous, current)?;
        self.registry
            .validate_generation(&entry.app.id, payload, next)?;
        entry.adapter.validate_cache_update(CredentialCacheUpdate {
            previous: &previous,
            replacement: payload,
        })?;
        cipher
            .seal_envelope(&context, payload)
            .map_err(|_| IntegrationError::Credential)
    }
    /// Private adapter-only cache write; not a gateway operation and no provider call.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "The private provider renewal writer is used from phase05"
        )
    )]
    pub(crate) async fn write_managed(
        &self,
        owner: &AccountOwner,
        id: AccountId,
        expected: AccountRevision,
        payload: SecretString,
    ) -> IntegrationResult<ConnectedAccount> {
        let mut tx = self
            .persistence
            .pool()
            .begin()
            .await
            .map_err(storage_error)?;
        let (account, versions) = self.locked_account(&mut tx, owner, id, expected).await?;
        let pending: bool = sqlx::query_scalar(
            "SELECT renewal_attempt_id IS NOT NULL FROM integration_accounts WHERE id=$1",
        )
        .bind(id.as_uuid())
        .fetch_one(&mut *tx)
        .await
        .map_err(storage_error)?;
        if pending {
            return Err(IntegrationError::Conflict);
        }
        if account.auth_method != AuthMethod::OAuthAuthorizationCode
            || account.status != AccountStatus::Ready
        {
            return Err(IntegrationError::Denied);
        }
        let stored = sqlx::query_scalar::<_,Option<String>>("SELECT envelope FROM integration_accounts WHERE owner_namespace=$1 AND owner_subject=$2 AND id=$3")
            .bind(owner.namespace.as_str()).bind(owner.subject.as_str()).bind(id.as_uuid())
            .fetch_one(&mut *tx).await.map_err(storage_error)?.ok_or(IntegrationError::Credential)?;
        let next = versions.changed(CredentialChange::Refresh)?;
        let envelope = self.seal_managed_cache_update(
            &account,
            &stored,
            &payload,
            versions.generation,
            next.generation,
        )?;
        save_payload(
            &mut tx,
            &account,
            next,
            Some(&envelope),
            AccountStatus::Ready,
            &account.metadata,
        )
        .await?;
        let result = load(&mut tx, owner, id, AccountRead::Metadata)
            .await?
            .decode()?;
        tx.commit().await.map_err(storage_error)?;
        Ok(result)
    }
}
pub(super) async fn save_payload(
    tx: &mut Transaction<'_, Postgres>,
    account: &ConnectedAccount,
    versions: CredentialVersions,
    envelope: Option<&str>,
    status: AccountStatus,
    metadata: &AccountMetadata,
) -> IntegrationResult<()> {
    let metadata = serde_json::to_string(metadata).map_err(|_| IntegrationError::Invalid)?;
    sqlx::query("UPDATE integration_accounts SET renewal_attempt_id=NULL,renewal_generation=NULL,renewal_started_at=NULL,renewal_deadline=NULL,envelope=$4,status=$5,revision=$6,authorization_revision=$7,credential_generation=$8,metadata=$9,updated_at=clock_timestamp() WHERE owner_namespace=$1 AND owner_subject=$2 AND id=$3")
        .bind(account.owner.namespace.as_str()).bind(account.owner.subject.as_str()).bind(account.id.as_uuid()).bind(envelope).bind(status.as_str())
        .bind(versions.revision.get()).bind(versions.authorization.get()).bind(versions.generation.get()).bind(metadata).execute(&mut **tx).await.map_err(storage_error)?;
    Ok(())
}
#[async_trait]
impl AccountRepository for PostgresVault {
    async fn grants(
        &self,
        owner: &AccountOwner,
        account: AccountId,
        after: Option<UseGrantId>,
    ) -> IntegrationResult<crate::application::integrations::GrantInspection> {
        self.grant_page(owner, account, after).await
    }
    async fn list(
        &self,
        owner: &AccountOwner,
        page: PageRequest,
    ) -> IntegrationResult<AccountPage> {
        let rows=sqlx::query_as::<_,AccountRow>(&format!("SELECT {COLUMNS} FROM integration_accounts WHERE owner_namespace=$1 AND owner_subject=$2 AND ($3::uuid IS NULL OR id>$3) ORDER BY id LIMIT $4"))
            .bind(owner.namespace.as_str()).bind(owner.subject.as_str()).bind(page.after().map(AccountId::as_uuid)).bind((page.size()+1) as i64).fetch_all(self.persistence.pool()).await.map_err(storage_error)?;
        let more = rows.len() > page.size();
        let accounts = rows
            .into_iter()
            .take(page.size())
            .map(AccountRow::decode)
            .collect::<IntegrationResult<Vec<_>>>()?;
        let next = if more {
            accounts.last().map(|a| a.id)
        } else {
            None
        };
        Ok(AccountPage { accounts, next })
    }
    async fn get(
        &self,
        owner: &AccountOwner,
        id: AccountId,
    ) -> IntegrationResult<ConnectedAccount> {
        let mut connection = self
            .persistence
            .pool()
            .acquire()
            .await
            .map_err(storage_error)?;
        load(&mut connection, owner, id, AccountRead::Metadata)
            .await?
            .decode()
    }
    async fn create(
        &self,
        owner: &AccountOwner,
        w: AccountWrite,
    ) -> IntegrationResult<ConnectedAccount> {
        let mut tx = self
            .persistence
            .pool()
            .begin()
            .await
            .map_err(storage_error)?;
        let result = insert_on(self, &mut tx, owner, w).await?;
        tx.commit().await.map_err(storage_error)?;
        Ok(result)
    }
    async fn replace(
        &self,
        owner: &AccountOwner,
        id: AccountId,
        expected: AccountRevision,
        w: AccountWrite,
    ) -> IntegrationResult<ConnectedAccount> {
        self.validate_write(&w)?;
        if w.method != AuthMethod::StaticSecret {
            return Err(IntegrationError::Denied);
        }
        let mut tx = self
            .persistence
            .pool()
            .begin()
            .await
            .map_err(storage_error)?;
        let (account, versions) = self.locked_account(&mut tx, owner, id, expected).await?;
        if account.auth_method != w.method
            || account.key != w.key
            || account.provider_id != w.provider
            || account.auth_config_id != w.config
            || account.credential_schema != w.schema
            || account.external_identity != w.identity
        {
            return Err(IntegrationError::Denied);
        }
        let envelope = self
            .persistence
            .credential_cipher()
            .map_err(|_| IntegrationError::Credential)?
            .seal_envelope(
                &CredentialContext::integration_account(&account),
                &w.payload,
            )
            .map_err(|_| IntegrationError::Credential)?;
        save_payload(
            &mut tx,
            &account,
            versions.changed(CredentialChange::Replace)?,
            Some(&envelope),
            AccountStatus::Ready,
            &w.metadata,
        )
        .await?;
        let result = load(&mut tx, owner, id, AccountRead::Metadata)
            .await?
            .decode()?;
        tx.commit().await.map_err(storage_error)?;
        Ok(result)
    }
    async fn disconnect(
        &self,
        owner: &AccountOwner,
        id: AccountId,
        expected: AuthorizationRevision,
    ) -> IntegrationResult<ConnectedAccount> {
        let mut tx = self
            .persistence
            .pool()
            .begin()
            .await
            .map_err(storage_error)?;
        let (account, versions) = self
            .locked_authorization(&mut tx, owner, id, expected)
            .await?;
        save_payload(
            &mut tx,
            &account,
            versions.changed(CredentialChange::Disconnect)?,
            None,
            AccountStatus::Disconnected,
            &account.metadata,
        )
        .await?;
        super::grants::invalidate_account(&mut tx, id).await?;
        let result = load(&mut tx, owner, id, AccountRead::Metadata)
            .await?
            .decode()?;
        tx.commit().await.map_err(storage_error)?;
        Ok(result)
    }
    async fn delete(
        &self,
        owner: &AccountOwner,
        id: AccountId,
        expected: AuthorizationRevision,
    ) -> IntegrationResult<()> {
        let mut tx = self
            .persistence
            .pool()
            .begin()
            .await
            .map_err(storage_error)?;
        let (account, versions) = self
            .locked_authorization(&mut tx, owner, id, expected)
            .await?;
        // Disconnect and grant invalidation agree in the same module transaction.
        save_payload(
            &mut tx,
            &account,
            versions.changed(CredentialChange::Disconnect)?,
            None,
            AccountStatus::Disconnected,
            &account.metadata,
        )
        .await?;
        super::grants::invalidate_account(&mut tx, id).await?;
        sqlx::query("DELETE FROM integration_accounts WHERE owner_namespace=$1 AND owner_subject=$2 AND id=$3").bind(owner.namespace.as_str()).bind(owner.subject.as_str()).bind(id.as_uuid()).execute(&mut *tx).await.map_err(storage_error)?;
        tx.commit().await.map_err(storage_error)?;
        Ok(())
    }
}
#[async_trait]
impl CredentialStore for PostgresVault {
    async fn read_usable(
        &self,
        owner: &AccountOwner,
        id: AccountId,
    ) -> IntegrationResult<CredentialRecord> {
        let mut tx = self
            .persistence
            .pool()
            .begin()
            .await
            .map_err(storage_error)?;
        let row = load(&mut tx, owner, id, AccountRead::Metadata).await?;
        let generation = CredentialGeneration::new(row.credential_generation)?;
        let account = row.decode()?;
        if account.status != AccountStatus::Ready {
            return Err(IntegrationError::Denied);
        }
        let envelope=sqlx::query_scalar::<_,Option<String>>("SELECT envelope FROM integration_accounts WHERE owner_namespace=$1 AND owner_subject=$2 AND id=$3 AND revision=$4 AND status='ready'")
            .bind(owner.namespace.as_str()).bind(owner.subject.as_str()).bind(id.as_uuid()).bind(account.revision.get()).fetch_optional(&mut *tx).await.map_err(storage_error)?.flatten().ok_or(IntegrationError::Conflict)?;
        if envelope.len() > 23000 {
            return Err(IntegrationError::Credential);
        }
        let payload = self
            .persistence
            .credential_cipher()
            .map_err(|_| IntegrationError::Credential)?
            .open_envelope(&CredentialContext::integration_account(&account), &envelope)
            .map_err(|_| IntegrationError::Credential)?;
        let entry = self.registration(
            &account.provider_id,
            &account.auth_config_id,
            account.auth_method,
            &account.credential_schema,
        )?;
        let identity = self.registry.validate(
            &entry.app.id,
            account.auth_method,
            &account.credential_schema,
            &payload,
        )?;
        if identity != account.external_identity {
            return Err(IntegrationError::Credential);
        }
        tx.commit().await.map_err(storage_error)?;
        let versions = CredentialVersions {
            revision: account.revision,
            authorization: account.authorization_revision,
            generation,
        };
        Ok(CredentialRecord {
            account,
            versions,
            payload,
        })
    }
}

pub(super) async fn insert_on(
    vault: &PostgresVault,
    tx: &mut Transaction<'_, Postgres>,
    owner: &AccountOwner,
    w: AccountWrite,
) -> IntegrationResult<ConnectedAccount> {
    vault.validate_write(&w)?;
    let entry = vault.registration(&w.provider, &w.config, w.method, &w.schema)?;
    vault
        .registry
        .validate_generation(&entry.app.id, &w.payload, CredentialGeneration::INITIAL)?;
    let account = new_account(owner, &w);
    let envelope = vault
        .persistence
        .credential_cipher()
        .map_err(|_| IntegrationError::Credential)?
        .seal_envelope(
            &CredentialContext::integration_account(&account),
            &w.payload,
        )
        .map_err(|_| IntegrationError::Credential)?;
    let metadata = serde_json::to_string(&w.metadata).map_err(|_| IntegrationError::Invalid)?;
    let identity = w
        .identity
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|_| IntegrationError::Invalid)?;
    let row=sqlx::query_as::<_,AccountRow>(&format!("INSERT INTO integration_accounts(id,owner_namespace,owner_subject,key,provider_id,auth_config_id,auth_method,credential_schema,external_identity,authorization_key,envelope,status,metadata) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'ready',$12) RETURNING {COLUMNS}"))
            .bind(account.id.as_uuid()).bind(owner.namespace.as_str()).bind(owner.subject.as_str()).bind(w.key.as_str()).bind(w.provider.as_str()).bind(w.config.as_str())
            .bind(w.method.as_str()).bind(w.schema.as_str()).bind(identity).bind(w.identity.as_ref().map(|i|i.authorization_key.as_str())).bind(envelope).bind(metadata)
            .fetch_one(&mut **tx).await.map_err(storage_error)?;
    let result = row.decode()?;
    Ok(result)
}
