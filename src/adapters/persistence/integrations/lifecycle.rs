use super::{PostgresVault, storage_error};
use crate::{application::integrations::lifecycle::TeardownDrain, domain::integrations::*};
use async_trait::async_trait;
use sqlx::{Acquire, Postgres, Transaction};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Local implementation hooks: the host owns the surrounding deletion transaction.
/// No host-table query or host foreign key exists inside the integrations module.
pub(crate) async fn retire_owner_on(
    tx: &mut Transaction<'_, Postgres>,
    owner: &AccountOwner,
) -> IntegrationResult<()> {
    sqlx::query("INSERT INTO integration_owner_states(owner_namespace,owner_subject,retired) VALUES($1,$2,true) ON CONFLICT(owner_namespace,owner_subject) DO UPDATE SET retired=true")
        .bind(owner.namespace.as_str()).bind(owner.subject.as_str()).execute(&mut **tx).await.map_err(storage_error)?;
    enqueue(tx, "owner", &owner.namespace, owner.subject.as_str(), "").await?;
    sqlx::query(
        "DELETE FROM integration_connect_attempts WHERE owner_namespace=$1 AND owner_subject=$2",
    )
    .bind(owner.namespace.as_str())
    .bind(owner.subject.as_str())
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    sqlx::query("UPDATE integration_accounts SET renewal_attempt_id=NULL,renewal_generation=NULL,renewal_started_at=NULL,renewal_deadline=NULL,status='disconnected',envelope=NULL,revision=revision+1,authorization_revision=authorization_revision+1,credential_generation=credential_generation+1,updated_at=clock_timestamp() WHERE owner_namespace=$1 AND owner_subject=$2 AND status<>'disconnected'")
        .bind(owner.namespace.as_str()).bind(owner.subject.as_str()).execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("UPDATE integration_use_grants SET enabled=false,revision=revision+1 WHERE owner_namespace=$1 AND owner_subject=$2 AND enabled")
        .bind(owner.namespace.as_str()).bind(owner.subject.as_str()).execute(&mut **tx).await.map_err(storage_error)?;
    Ok(())
}
pub(crate) async fn revoke_company_on(
    tx: &mut Transaction<'_, Postgres>,
    client: &OwnerNamespace,
    binding: &HostBinding,
) -> IntegrationResult<()> {
    enqueue(tx, "company", client, "", binding.as_str()).await
}
pub(crate) async fn revoke_membership_on(
    tx: &mut Transaction<'_, Postgres>,
    owner: &AccountOwner,
    binding: &HostBinding,
) -> IntegrationResult<()> {
    enqueue(
        tx,
        "membership",
        &owner.namespace,
        owner.subject.as_str(),
        binding.as_str(),
    )
    .await
}
async fn enqueue(
    tx: &mut Transaction<'_, Postgres>,
    kind: &str,
    client: &OwnerNamespace,
    subject: &str,
    binding: &str,
) -> IntegrationResult<()> {
    sqlx::query("INSERT INTO integration_teardown_intents(id,kind,owner_namespace,owner_subject,host_binding,created_at) VALUES($1,$2,$3,$4,$5,clock_timestamp()) ON CONFLICT(kind,owner_namespace,owner_subject,host_binding) DO UPDATE SET state='pending',attempts=0,available_at=clock_timestamp(),created_at=clock_timestamp(),completed_at=NULL")
        .bind(Uuid::new_v4()).bind(kind).bind(client.as_str()).bind(subject).bind(binding).execute(&mut **tx).await.map_err(storage_error)?;
    Ok(())
}
#[derive(sqlx::FromRow)]
struct Intent {
    id: Uuid,
    kind: String,
    owner_namespace: String,
    owner_subject: String,
    attempts: i32,
    host_binding: String,
    created_at: chrono::DateTime<chrono::Utc>,
}
async fn cleanup(tx: &mut Transaction<'_, Postgres>, intent: &Intent) -> IntegrationResult<bool> {
    match intent.kind.as_str() {
        "owner" => {
            sqlx::query(
                "DELETE FROM integration_accounts WHERE owner_namespace=$1 AND owner_subject=$2",
            )
            .bind(&intent.owner_namespace)
            .bind(&intent.owner_subject)
            .execute(&mut **tx)
            .await
            .map_err(storage_error)?;
        }
        "company" | "membership" => {
            let accounts:Vec<Uuid>=sqlx::query_scalar("SELECT account.id FROM integration_accounts AS account WHERE account.owner_namespace=$1 AND EXISTS(SELECT 1 FROM integration_use_grants AS grant_row WHERE grant_row.account_id=account.id AND grant_row.host_scope=$2 AND grant_row.enabled AND grant_row.confirmed_at<=$3 AND ($4='' OR grant_row.owner_subject=$4)) ORDER BY account.id LIMIT 64 FOR UPDATE SKIP LOCKED")
                .bind(&intent.owner_namespace).bind(&intent.host_binding).bind(intent.created_at).bind(&intent.owner_subject).fetch_all(&mut **tx).await.map_err(storage_error)?;
            let updated=sqlx::query("UPDATE integration_use_grants SET enabled=false,revision=revision+1 WHERE owner_namespace=$1 AND host_scope=$2 AND confirmed_at<=$3 AND ($4='' OR owner_subject=$4) AND account_id=ANY($5) AND enabled")
                .bind(&intent.owner_namespace).bind(&intent.host_binding).bind(intent.created_at).bind(&intent.owner_subject).bind(&accounts).execute(&mut **tx).await.map_err(storage_error)?;
            let more:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM integration_use_grants WHERE owner_namespace=$1 AND host_scope=$2 AND confirmed_at<=$3 AND ($4='' OR owner_subject=$4) AND enabled)")
                .bind(&intent.owner_namespace).bind(&intent.host_binding).bind(intent.created_at).bind(&intent.owner_subject).fetch_one(&mut **tx).await.map_err(storage_error)?;
            if more && updated.rows_affected() == 0 {
                return Err(IntegrationError::Conflict);
            }
            return Ok(more);
        }
        _ => return Err(IntegrationError::Invalid),
    }
    Ok(false)
}
impl PostgresVault {
    async fn drain_one(&self, stop: &CancellationToken) -> IntegrationResult<bool> {
        let mut tx = tokio::select! {
            biased;
            _=stop.cancelled()=>return Ok(false),
            transaction=self.persistence.pool().begin()=>transaction.map_err(storage_error)?,
        };
        sqlx::query("SET LOCAL lock_timeout='100ms'")
            .execute(&mut *tx)
            .await
            .map_err(storage_error)?;
        sqlx::query("SET LOCAL statement_timeout='2s'")
            .execute(&mut *tx)
            .await
            .map_err(storage_error)?;
        let claimed = tokio::select! {
            biased;
            _=stop.cancelled()=>None,
            intent=sqlx::query_as::<_,Intent>("SELECT id,kind,owner_namespace,owner_subject,attempts,host_binding,created_at FROM integration_teardown_intents WHERE state='pending' AND available_at<=clock_timestamp() ORDER BY available_at,id LIMIT 1 FOR UPDATE SKIP LOCKED").fetch_optional(&mut *tx)=>Some(intent.map_err(storage_error)?),
        };
        let Some(intent) = claimed else {
            tx.rollback().await.map_err(storage_error)?;
            return Ok(false);
        };
        let Some(intent) = intent else {
            tx.rollback().await.map_err(storage_error)?;
            return Ok(false);
        };
        let mut work = tx.begin().await.map_err(storage_error)?;
        let result = tokio::select! {biased; _=stop.cancelled()=>None,result=cleanup(&mut work,&intent)=>Some(result)};
        let success = matches!(result, Some(Ok(_)));
        if success {
            work.commit().await.map_err(storage_error)?;
        } else {
            work.rollback().await.map_err(storage_error)?;
        }
        if result.is_none() || stop.is_cancelled() {
            tx.rollback().await.map_err(storage_error)?;
            return Ok(false);
        }
        let outcome = match result {
            Some(Ok(true)) => CleanupOutcome::Progress,
            Some(Ok(false)) => CleanupOutcome::Completed,
            _ => CleanupOutcome::Deferred,
        };
        settle(&mut tx, &intent, outcome).await?;
        tx.commit().await.map_err(storage_error)?;
        Ok(true)
    }
}
enum CleanupOutcome {
    Progress,
    Completed,
    Deferred,
}
async fn settle(
    tx: &mut Transaction<'_, Postgres>,
    intent: &Intent,
    outcome: CleanupOutcome,
) -> IntegrationResult<()> {
    if matches!(outcome, CleanupOutcome::Progress) {
        // At least one bounded batch left the eligible grant set. An immediate
        // retry is justified by durable progress, never by a poisoned record.
        sqlx::query(
            "UPDATE integration_teardown_intents SET available_at=clock_timestamp() WHERE id=$1",
        )
        .bind(intent.id)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
    } else if matches!(outcome, CleanupOutcome::Completed) {
        sqlx::query("UPDATE integration_teardown_intents SET state='completed',completed_at=clock_timestamp() WHERE id=$1")
            .bind(intent.id).execute(&mut **tx).await.map_err(storage_error)?;
    } else {
        let attempts = intent.attempts + 1;
        let seconds = 2i32.pow((attempts + 5).min(8) as u32);
        sqlx::query("UPDATE integration_teardown_intents SET attempts=$2,state=CASE WHEN $2>=8 THEN 'failed' ELSE 'pending' END,available_at=clock_timestamp()+$3*interval '1 second' WHERE id=$1")
            .bind(intent.id).bind(attempts).bind(f64::from(seconds)).execute(&mut **tx).await.map_err(storage_error)?;
        tracing::warn!(intent_id=%intent.id,attempt=attempts,terminal=attempts>=8,"Integration teardown deferred");
    }
    Ok(())
}
#[async_trait]
impl TeardownDrain for PostgresVault {
    async fn drain(&self, limit: u16, stop: &CancellationToken) -> IntegrationResult<usize> {
        if limit == 0 || limit > 16 {
            return Err(IntegrationError::Limit);
        }
        let mut count = 0;
        for _ in 0..limit {
            if stop.is_cancelled() || !self.drain_one(stop).await? {
                break;
            }
            count += 1;
        }
        Ok(count)
    }
}
