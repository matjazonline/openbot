use super::*;
use crate::adapters::persistence::credentials::envelope::CredentialContext;
use secrecy::ExposeSecret;
async fn ensure_owner(
    tx: &mut Transaction<'_, Postgres>,
    owner: &AccountOwner,
) -> IntegrationResult<()> {
    sqlx::query("INSERT INTO integration_owner_states(owner_namespace,owner_subject,retired) VALUES($1,$2,FALSE) ON CONFLICT DO NOTHING")
        .bind(owner.namespace.as_str()).bind(owner.subject.as_str()).execute(&mut **tx).await.map_err(storage_error)?;
    lock_owner(tx, owner).await
}
pub(super) async fn start(
    vault: &PostgresVault,
    f: &AttemptFacts,
    verifier: SecretString,
) -> IntegrationResult<ConnectAttempt> {
    if !(43..=128).contains(&verifier.expose_secret().len()) {
        return Err(IntegrationError::Invalid);
    }
    let entry = vault.registry.get(&f.app)?;
    if entry.config.id != f.config
        || entry.provider.id != f.provider
        || vault.registry.policy(&f.app)? != f.policy
        || entry.authorization.is_none()
    {
        return Err(IntegrationError::Denied);
    }
    let raw = serde_json::to_string(f).map_err(|_| IntegrationError::Invalid)?;
    if raw.len() > MAX_METADATA_BYTES {
        return Err(IntegrationError::Limit);
    }
    let envelope = vault
        .persistence
        .credential_cipher()
        .map_err(|_| IntegrationError::Credential)?
        .seal_envelope(&CredentialContext::integration_connect(f), &verifier)
        .map_err(|_| IntegrationError::Credential)?;
    let mut tx = begin(vault).await?;
    ensure_owner(&mut tx, &f.owner).await?;
    maintain(&mut tx, &f.owner, 0).await?;
    if let Some(target) = f.reconnect {
        let account = accounts::load(&mut tx, &f.owner, target.account, AccountRead::Locked)
            .await?
            .decode()?;
        if account.authorization_revision != target.authorization
            || account.provider_id != f.provider
            || account.auth_config_id != f.config
            || account.auth_method != entry.provider.method
            || account.credential_schema != entry.provider.schema
        {
            return Err(IntegrationError::Conflict);
        }
    }
    sqlx::query("INSERT INTO integration_connect_attempts(id,owner_namespace,owner_subject,facts,status,envelope,expires_at) VALUES($1,$2,$3,$4,'pending',$5,$6)")
        .bind(f.id.as_uuid()).bind(f.owner.namespace.as_str()).bind(f.owner.subject.as_str()).bind(raw).bind(envelope).bind(f.expires_at).execute(&mut *tx).await.map_err(storage_error)?;
    tx.commit().await.map_err(storage_error)?;
    Ok(ConnectAttempt {
        return_target: f.return_target.clone(),
        id: f.id,
        status: ConnectStatus::Pending,
        account: None,
    })
}
pub(super) async fn consume(
    vault: &PostgresVault,
    f: &AttemptFacts,
) -> IntegrationResult<ConsumedAttempt> {
    let mut tx = begin(vault).await?;
    lock_owner(&mut tx, &f.owner).await?;
    maintain(&mut tx, &f.owner, 0).await?;
    let r = row(&mut tx, &f.owner, f.id).await?;
    let stored = r.facts()?;
    if r.status != "pending" || !stored.same(f)? || f.expires_at <= Utc::now() {
        return Err(IntegrationError::Denied);
    }
    let envelope = r.envelope.ok_or(IntegrationError::Credential)?;
    if envelope.len() > 23000 {
        return Err(IntegrationError::Credential);
    }
    sqlx::query(
        "UPDATE integration_connect_attempts SET status='processing',envelope=NULL WHERE id=$1",
    )
    .bind(f.id.as_uuid())
    .execute(&mut *tx)
    .await
    .map_err(storage_error)?;
    tx.commit().await.map_err(storage_error)?;
    let verifier = vault
        .persistence
        .credential_cipher()
        .map_err(|_| IntegrationError::Credential)?
        .open_envelope(&CredentialContext::integration_connect(f), &envelope)
        .map_err(|_| IntegrationError::Credential)?;
    if !(43..=128).contains(&verifier.expose_secret().len()) {
        return Err(IntegrationError::Credential);
    }
    Ok(ConsumedAttempt {
        facts: stored,
        verifier,
    })
}
