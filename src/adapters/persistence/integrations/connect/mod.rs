use super::{
    PostgresVault,
    accounts::{self, AccountRead, lock_owner},
    storage_error,
};
use crate::{
    application::integrations::{
        connect::*, management::AccountWrite, private::CredentialRecord, *,
    },
    domain::integrations::*,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use secrecy::SecretString;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;
mod attempts;
mod completion;
pub(crate) mod inventory;
#[derive(sqlx::FromRow)]
struct AttemptRow {
    id: Uuid,
    facts: String,
    status: String,
    account_id: Option<Uuid>,
    envelope: Option<String>,
}
impl AttemptRow {
    fn facts(&self) -> IntegrationResult<AttemptFacts> {
        if self.facts.len() > MAX_METADATA_BYTES {
            return Err(IntegrationError::Storage);
        }
        serde_json::from_str(&self.facts).map_err(|_| IntegrationError::Storage)
    }
    fn projection(&self) -> IntegrationResult<ConnectAttempt> {
        Ok(ConnectAttempt {
            return_target: self.facts()?.return_target,
            id: ConnectAttemptId::new(self.id),
            status: serde_json::from_value(serde_json::Value::String(self.status.clone()))
                .map_err(|_| IntegrationError::Storage)?,
            account: self.account_id.map(AccountId::new),
        })
    }
}
async fn begin(vault: &PostgresVault) -> IntegrationResult<Transaction<'_, Postgres>> {
    let mut tx = vault
        .persistence
        .pool()
        .begin()
        .await
        .map_err(storage_error)?;
    sqlx::query("SET LOCAL lock_timeout='5s'")
        .execute(&mut *tx)
        .await
        .map_err(storage_error)?;
    Ok(tx)
}
async fn row(
    tx: &mut Transaction<'_, Postgres>,
    owner: &AccountOwner,
    id: ConnectAttemptId,
) -> IntegrationResult<AttemptRow> {
    sqlx::query_as("SELECT id,facts,status,account_id,envelope FROM integration_connect_attempts WHERE owner_namespace=$1 AND owner_subject=$2 AND id=$3 FOR UPDATE")
        .bind(owner.namespace.as_str()).bind(owner.subject.as_str()).bind(id.as_uuid()).fetch_optional(&mut **tx).await.map_err(storage_error)?.ok_or(IntegrationError::NotFound)
}
async fn maintain(
    tx: &mut Transaction<'_, Postgres>,
    owner: &AccountOwner,
    reserved_terminals: i64,
) -> IntegrationResult<()> {
    let expired:i64=sqlx::query_scalar("SELECT count(*) FROM integration_connect_attempts WHERE owner_namespace=$1 AND owner_subject=$2 AND status IN ('pending','processing') AND expires_at<=clock_timestamp()")
        .bind(owner.namespace.as_str()).bind(owner.subject.as_str()).fetch_one(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("DELETE FROM integration_connect_attempts WHERE id IN (SELECT id FROM integration_connect_attempts WHERE owner_namespace=$1 AND owner_subject=$2 AND status NOT IN ('pending','processing') ORDER BY created_at DESC,id DESC OFFSET $3 LIMIT 40)")
        .bind(owner.namespace.as_str()).bind(owner.subject.as_str()).bind((32-reserved_terminals-expired).max(0)).execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("UPDATE integration_connect_attempts SET status='expired',envelope=NULL WHERE owner_namespace=$1 AND owner_subject=$2 AND status IN ('pending','processing') AND expires_at<=clock_timestamp()")
        .bind(owner.namespace.as_str()).bind(owner.subject.as_str()).execute(&mut **tx).await.map_err(storage_error)?;
    Ok(())
}
async fn set_terminal(
    tx: &mut Transaction<'_, Postgres>,
    owner: &AccountOwner,
    id: ConnectAttemptId,
    status: ConnectStatus,
    account: Option<AccountId>,
) -> IntegrationResult<ConnectAttempt> {
    maintain(tx, owner, 1).await?;
    sqlx::query("UPDATE integration_connect_attempts SET status=$4,envelope=NULL,account_id=$5 WHERE owner_namespace=$1 AND owner_subject=$2 AND id=$3 AND status IN ('pending','processing')")
        .bind(owner.namespace.as_str()).bind(owner.subject.as_str()).bind(id.as_uuid()).bind(status.as_str()).bind(account.map(|a|a.as_uuid())).execute(&mut **tx).await.map_err(storage_error)?;
    row(tx, owner, id).await?.projection()
}
#[async_trait]
impl ConnectStore for PostgresVault {
    async fn start(
        &self,
        facts: &AttemptFacts,
        verifier: SecretString,
    ) -> IntegrationResult<ConnectAttempt> {
        attempts::start(self, facts, verifier).await
    }
    async fn consume(&self, facts: &AttemptFacts) -> IntegrationResult<ConsumedAttempt> {
        attempts::consume(self, facts).await
    }
    async fn status(
        &self,
        owner: &AccountOwner,
        id: ConnectAttemptId,
    ) -> IntegrationResult<ConnectAttempt> {
        let mut tx = begin(self).await?;
        lock_owner(&mut tx, owner).await?;
        maintain(&mut tx, owner, 0).await?;
        // Metadata projection never selects the verifier or encrypted candidate.
        let (status,account,return_target):(String,Option<Uuid>,String)=sqlx::query_as("SELECT status,account_id,facts::jsonb->>'return_target' AS return_target FROM integration_connect_attempts WHERE owner_namespace=$1 AND owner_subject=$2 AND id=$3")
            .bind(owner.namespace.as_str()).bind(owner.subject.as_str()).bind(id.as_uuid()).fetch_optional(&mut *tx).await.map_err(storage_error)?.ok_or(IntegrationError::NotFound)?;
        let result = ConnectAttempt {
            return_target: SafeReturnTarget::parse(return_target)?,
            id,
            status: serde_json::from_value(serde_json::Value::String(status))
                .map_err(|_| IntegrationError::Storage)?,
            account: account.map(AccountId::new),
        };
        tx.commit().await.map_err(storage_error)?;
        Ok(result)
    }
    async fn terminal(
        &self,
        owner: &AccountOwner,
        id: ConnectAttemptId,
        status: ConnectStatus,
    ) -> IntegrationResult<ConnectAttempt> {
        if matches!(
            status,
            ConnectStatus::Pending | ConnectStatus::Processing | ConnectStatus::Connected
        ) {
            return Err(IntegrationError::Denied);
        }
        let mut tx = begin(self).await?;
        lock_owner(&mut tx, owner).await?;
        let result = set_terminal(&mut tx, owner, id, status, None).await?;
        tx.commit().await.map_err(storage_error)?;
        Ok(result)
    }
    async fn finalize(
        &self,
        facts: &AttemptFacts,
        candidate: &SecretString,
        valid_until: DateTime<Utc>,
    ) -> IntegrationResult<ConnectAttempt> {
        let remaining = (valid_until.min(facts.expires_at) - Utc::now())
            .to_std()
            .map_err(|_| IntegrationError::Denied)?;
        // Session expiry is independent of row locks; cancel the actual lock wait.
        tokio::time::timeout(
            remaining,
            completion::finalize(self, facts, candidate, valid_until),
        )
        .await
        .map_err(|_| IntegrationError::Denied)?
    }
}
