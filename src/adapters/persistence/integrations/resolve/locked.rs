use super::*;
use crate::domain::integrations::authority::{UseGrant, authorize_credential};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PendingRenewal {
    pub id: Uuid,
    pub generation: CredentialGeneration,
    pub started: DateTime<Utc>,
    pub deadline: DateTime<Utc>,
}
#[derive(sqlx::FromRow)]
struct PrivateRow {
    envelope: Option<String>,
    renewal_attempt_id: Option<Uuid>,
    renewal_generation: Option<i64>,
    renewal_started_at: Option<DateTime<Utc>>,
    renewal_deadline: Option<DateTime<Utc>>,
}
impl PrivateRow {
    fn pending(&self) -> ResolutionResult<Option<PendingRenewal>> {
        match (
            self.renewal_attempt_id,
            self.renewal_generation,
            self.renewal_started_at,
            self.renewal_deadline,
        ) {
            (None, None, None, None) => Ok(None),
            (Some(id), Some(generation), Some(started), Some(deadline)) => {
                Ok(Some(PendingRenewal {
                    id,
                    generation: CredentialGeneration::new(generation)?,
                    started,
                    deadline,
                }))
            }
            _ => Err(IntegrationError::Credential.into()),
        }
    }
}
pub(super) struct Locked {
    pub tx: Tx,
    pub account: ConnectedAccount,
    pub versions: CredentialVersions,
    pub payload: SecretString,
    pub envelope: String,
    pub pending: Option<PendingRenewal>,
    pub grant: UseGrant,
}
pub(super) async fn begin(vault: &PostgresVault, work: &RenewalWork) -> ResolutionResult<Tx> {
    let mut tx = vault.persistence.pool().begin().await.map_err(db_error)?;
    let ms = (work.deadline - Utc::now())
        .num_milliseconds()
        .clamp(1, 5000);
    sqlx::query("SELECT set_config('lock_timeout',$1,true),set_config('statement_timeout',$1,true),set_config('idle_in_transaction_session_timeout','12s',true)")
        .bind(format!("{ms}ms")).execute(&mut *tx).await.map_err(db_error)?;
    Ok(tx)
}
async fn private_row(tx: &mut Tx, id: AccountId) -> ResolutionResult<PrivateRow> {
    sqlx::query_as("SELECT envelope,renewal_attempt_id,renewal_generation,renewal_started_at,renewal_deadline FROM integration_accounts WHERE id=$1")
        .bind(id.as_uuid()).fetch_one(&mut **tx).await.map_err(db_error)
}
pub(super) async fn lock(
    vault: &PostgresVault,
    i: &AuthorizedInvocation,
    work: &RenewalWork,
) -> ResolutionResult<Locked> {
    let mut tx = begin(vault, work).await?;
    let grant = super::super::grants::grant_on(&mut tx, i.authorization.grant).await?;
    lock_owner(&mut tx, &grant.owner).await?;
    let row = load_account(&mut tx, &grant.owner, i.authorization.account).await?;
    let generation = CredentialGeneration::new(row.credential_generation)?;
    let account = row.decode()?;
    let grant = super::super::grants::grant_on(&mut tx, grant.id).await?;
    authorize_credential(&account, &grant, &i.authorization, grant.purpose)?;
    super::super::grants::pending(&mut tx, &account.owner, &grant.host_scope).await?;
    if vault.registry.policy(&grant.app)? != grant.app_policy {
        return Err(IntegrationError::Denied.into());
    }
    let entry = vault.registry.get(&grant.app)?;
    if account.provider_id != entry.provider.id
        || account.auth_config_id != entry.config.id
        || account.auth_method != entry.provider.method
        || account.credential_schema != entry.provider.schema
        || !entry.app.destinations.is_empty()
            && !entry.app.destinations.contains(&grant.destination)
    {
        return Err(IntegrationError::Denied.into());
    }
    let private = private_row(&mut tx, account.id).await?;
    let pending = private.pending()?;
    let envelope = private.envelope.ok_or(IntegrationError::Credential)?;
    if envelope.len() > 23000 {
        return Err(IntegrationError::Credential.into());
    }
    let payload = vault
        .persistence
        .credential_cipher()
        .map_err(|_| IntegrationError::Credential)?
        .open_envelope(&CredentialContext::integration_account(&account), &envelope)
        .map_err(|_| IntegrationError::Credential)?;
    if entry.adapter.validate(&payload)? != account.external_identity {
        return Err(IntegrationError::Denied.into());
    }
    entry.adapter.validate_use(&payload, &entry.app)?;
    live(&mut tx, i, work).await?;
    let versions = CredentialVersions {
        revision: account.revision,
        authorization: account.authorization_revision,
        generation,
    };
    Ok(Locked {
        tx,
        account,
        versions,
        payload,
        envelope,
        pending,
        grant,
    })
}
pub(super) async fn live(
    tx: &mut Tx,
    i: &AuthorizedInvocation,
    work: &RenewalWork,
) -> ResolutionResult<()> {
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut **tx)
        .await
        .map_err(db_error)?;
    if work.cancellation.is_cancelled() {
        return Err(ResolutionError::Cancelled);
    }
    if now >= i.deadline || now >= work.deadline {
        return Err(IntegrationError::Denied.into());
    }
    Ok(())
}
pub(super) async fn observe(
    vault: &PostgresVault,
    expected: &Observation,
    work: &RenewalWork,
) -> ResolutionResult<Observation> {
    let mut tx = begin(vault, work).await?;
    lock_owner(&mut tx, &expected.owner).await?;
    let row = load_account(&mut tx, &expected.owner, expected.id).await?;
    let generation = CredentialGeneration::new(row.credential_generation)?;
    let account = row.decode()?;
    let private = private_row(&mut tx, account.id).await?;
    let pending = private.pending()?;
    let result = Observation {
        id: account.id,
        owner: account.owner,
        authorization: account.authorization_revision,
        revision: account.revision,
        generation,
        status: account.status,
        envelope: private.envelope,
        pending,
    };
    tx.rollback().await.map_err(db_error)?;
    Ok(result)
}

async fn lock_owner(tx: &mut Tx, owner: &AccountOwner) -> ResolutionResult<()> {
    let retired:Option<bool>=sqlx::query_scalar("SELECT retired FROM integration_owner_states WHERE owner_namespace=$1 AND owner_subject=$2 FOR SHARE")
        .bind(owner.namespace.as_str()).bind(owner.subject.as_str()).fetch_optional(&mut **tx).await.map_err(db_error)?;
    if retired != Some(false) {
        return Err(IntegrationError::Denied.into());
    }
    Ok(())
}
async fn load_account(
    tx: &mut Tx,
    owner: &AccountOwner,
    id: AccountId,
) -> ResolutionResult<super::super::rows::AccountRow> {
    sqlx::query_as(&format!("SELECT {} FROM integration_accounts WHERE owner_namespace=$1 AND owner_subject=$2 AND id=$3 FOR UPDATE",super::super::rows::COLUMNS))
        .bind(owner.namespace.as_str()).bind(owner.subject.as_str()).bind(id.as_uuid()).fetch_optional(&mut **tx).await.map_err(db_error)?.ok_or(IntegrationError::NotFound.into())
}
