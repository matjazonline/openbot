use super::*;
use crate::adapters::persistence::credentials::envelope::CredentialContext;
struct PreviousAuthorization {
    account: ConnectedAccount,
    versions: CredentialVersions,
    record: Option<CredentialRecord>,
}
async fn previous(
    vault: &PostgresVault,
    tx: &mut Transaction<'_, Postgres>,
    f: &AttemptFacts,
) -> IntegrationResult<Option<PreviousAuthorization>> {
    let Some(target) = f.reconnect else {
        return Ok(None);
    };
    let row = accounts::load(tx, &f.owner, target.account, AccountRead::Locked).await?;
    let generation = CredentialGeneration::new(row.credential_generation)?;
    let account = row.decode()?;
    let entry = vault.registry.get(&f.app)?;
    if account.authorization_revision != target.authorization
        || account.provider_id != f.provider
        || account.auth_config_id != f.config
        || account.auth_method != entry.provider.method
        || account.credential_schema != entry.provider.schema
    {
        return Err(IntegrationError::Conflict);
    }
    let versions = CredentialVersions {
        revision: account.revision,
        authorization: account.authorization_revision,
        generation,
    };
    let envelope: Option<String> =
        sqlx::query_scalar("SELECT envelope FROM integration_accounts WHERE id=$1")
            .bind(target.account.as_uuid())
            .fetch_one(&mut **tx)
            .await
            .map_err(storage_error)?;
    let record = if let Some(envelope) = envelope {
        if envelope.len() > 23000 {
            return Err(IntegrationError::Credential);
        }
        Some(CredentialRecord {
            account: account.clone(),
            versions,
            payload: vault
                .persistence
                .credential_cipher()
                .map_err(|_| IntegrationError::Credential)?
                .open_envelope(&CredentialContext::integration_account(&account), &envelope)
                .map_err(|_| IntegrationError::Credential)?,
        })
    } else {
        None
    };
    Ok(Some(PreviousAuthorization {
        account,
        versions,
        record,
    }))
}
async fn temporal(
    tx: &mut Transaction<'_, Postgres>,
    f: &AttemptFacts,
    valid_until: DateTime<Utc>,
) -> IntegrationResult<()> {
    let live: bool = sqlx::query_scalar("SELECT clock_timestamp()<$1 AND clock_timestamp()<$2")
        .bind(valid_until)
        .bind(f.expires_at)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    if !live {
        return Err(IntegrationError::Denied);
    }
    Ok(())
}
pub(super) async fn finalize(
    vault: &PostgresVault,
    f: &AttemptFacts,
    candidate: &SecretString,
    valid_until: DateTime<Utc>,
) -> IntegrationResult<ConnectAttempt> {
    let mut tx = begin(vault).await?;
    lock_owner(&mut tx, &f.owner).await?;
    let previous = previous(vault, &mut tx, f).await?;
    let r = row(&mut tx, &f.owner, f.id).await?;
    if r.status != "processing" || !r.facts()?.same(f)? {
        return Err(IntegrationError::Denied);
    }
    let next = previous
        .as_ref()
        .map(|p| p.versions.changed(CredentialChange::Replace))
        .transpose()?
        .unwrap_or(CredentialVersions::INITIAL);
    let prepared = prepare(vault, f, candidate, previous.as_ref(), next)?;
    temporal(&mut tx, f, valid_until).await?;
    let w = match prepared {
        Preparation::Ready(write) => *write,
        Preparation::Refused(status) => {
            let result = set_terminal(&mut tx, &f.owner, f.id, status, None).await?;
            tx.commit().await.map_err(storage_error)?;
            return Ok(result);
        }
    };
    sqlx::query("SAVEPOINT connect_account_write")
        .execute(&mut *tx)
        .await
        .map_err(storage_error)?;
    let account = write(
        vault,
        &mut tx,
        &f.owner,
        w,
        previous.as_ref().map(|p| &p.account),
        next,
    )
    .await;
    let result = match account {
        Ok(account) => {
            let result = set_terminal(
                &mut tx,
                &f.owner,
                f.id,
                ConnectStatus::Connected,
                Some(account),
            )
            .await?;
            // Expiry maintenance must never commit an account with a non-connected outcome.
            if result.status != ConnectStatus::Connected || result.account != Some(account) {
                return Err(IntegrationError::Denied);
            }
            result
        }
        Err(IntegrationError::Conflict) => {
            sqlx::query("ROLLBACK TO SAVEPOINT connect_account_write")
                .execute(&mut *tx)
                .await
                .map_err(storage_error)?;
            set_terminal(&mut tx, &f.owner, f.id, ConnectStatus::Conflict, None).await?
        }
        Err(error) => return Err(error),
    };
    tx.commit().await.map_err(storage_error)?;
    Ok(result)
}
async fn write(
    vault: &PostgresVault,
    tx: &mut Transaction<'_, Postgres>,
    owner: &AccountOwner,
    w: AccountWrite,
    previous: Option<&ConnectedAccount>,
    next: CredentialVersions,
) -> IntegrationResult<AccountId> {
    if let Some(account) = previous {
        let envelope = vault
            .persistence
            .credential_cipher()
            .map_err(|_| IntegrationError::Credential)?
            .seal_envelope(&CredentialContext::integration_account(account), &w.payload)
            .map_err(|_| IntegrationError::Credential)?;
        accounts::save_payload(
            tx,
            account,
            next,
            Some(&envelope),
            AccountStatus::Ready,
            &w.metadata,
        )
        .await?;
        super::super::grants::invalidate_account(tx, account.id).await?;
        Ok(account.id)
    } else {
        Ok(accounts::insert_on(vault, tx, owner, w).await?.id)
    }
}

enum Preparation {
    Ready(Box<AccountWrite>),
    Refused(ConnectStatus),
}
fn prepare(
    vault: &PostgresVault,
    f: &AttemptFacts,
    candidate: &SecretString,
    previous: Option<&PreviousAuthorization>,
    next: CredentialVersions,
) -> IntegrationResult<Preparation> {
    let entry = vault.registry.get(&f.app)?;
    if entry.provider.id != f.provider
        || entry.config.id != f.config
        || vault.registry.policy(&f.app)? != f.policy
    {
        return Err(IntegrationError::Denied);
    }
    let provider = entry
        .authorization
        .as_ref()
        .ok_or(IntegrationError::Unavailable)?;
    let prepared = match provider.finalize(FinalizeAuthorization {
        app: &entry.app,
        candidate,
        previous: previous.and_then(|p| p.record.as_ref()),
        generation: next.generation,
    }) {
        Ok(p) => p,
        Err(status) => return Ok(Preparation::Refused(status)),
    };
    if previous.is_some_and(|p| p.account.external_identity.as_ref() != Some(&prepared.identity)) {
        return Ok(Preparation::Refused(ConnectStatus::IdentityMismatch));
    }
    let w = AccountWrite {
        key: previous
            .map(|p| p.account.key.clone())
            .unwrap_or(AccountKey::parse(format!("oauth-{}", f.id.as_uuid()))?),
        provider: f.provider.clone(),
        config: f.config.clone(),
        method: entry.provider.method,
        schema: entry.provider.schema.clone(),
        identity: Some(prepared.identity),
        metadata: prepared.metadata,
        payload: prepared.payload,
    };
    vault.validate_write(&w)?;
    vault
        .registry
        .validate_generation(&entry.app.id, &w.payload, next.generation)?;
    entry.adapter.validate_use(&w.payload, &entry.app)?;
    Ok(Preparation::Ready(Box::new(w)))
}
