//! Company-owned local machines. No network wait occurs inside these transactions.
use super::PostgresPersistence;
use crate::{
    app_error::{AppError, AppResult},
    application::local_agents::{bounds::*, ports::LocalAgentRead, *},
    application::workflow::WorkflowActor,
    domain::{local_agents::*, workflow::CompanyId},
    use_cases::user::AuthAttemptKey,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool, Postgres, Transaction};
use uuid::Uuid;
pub(in crate::adapters::persistence) mod shared;

fn missing() -> AppError {
    AppError::NotFound("local machine resource".into())
}
fn invalid(kind: &str) -> AppError {
    AppError::Internal(format!("invalid persisted local-agent {kind}"))
}
fn decode<T: DeserializeOwned>(value: Value, kind: &str) -> AppResult<T> {
    serde_json::from_value(value).map_err(|_| invalid(kind))
}
fn encode<T: Serialize>(value: &T) -> AppResult<Value> {
    serde_json::to_value(value).map_err(|_| invalid("value"))
}

async fn begin(pool: &PgPool) -> AppResult<Transaction<'_, Postgres>> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('statement_timeout','5s',true),set_config('lock_timeout','5s',true),set_config('transaction_timeout','5s',true)")
        .execute(&mut *tx).await?;
    Ok(tx)
}

async fn authorize(db: &mut PgConnection, company: Uuid, actor: Uuid) -> AppResult<()> {
    super::workflow::authority::authorize_company(
        db,
        CompanyId::new(company),
        WorkflowActor::authenticated(actor)?,
    )
    .await?;
    Ok(())
}

const MACHINE_COLUMNS: &str = "machine.id,machine.company_id,machine.installation_id,machine.name,machine.state,\
    machine.credential_generation,machine.last_seen_at,machine.capabilities_sequence,machine.connection_generation";

#[derive(sqlx::FromRow)]
struct MachineDb {
    id: Uuid,
    company_id: Uuid,
    installation_id: Uuid,
    name: String,
    state: String,
    credential_generation: i64,
    last_seen_at: Option<DateTime<Utc>>,
    capabilities_sequence: Option<i64>,
    connection_generation: i64,
}

impl MachineDb {
    fn entity(self) -> AppResult<LocalMachine> {
        decode(
            json!({"id":self.id,"company_id":self.company_id,"installation_id":self.installation_id,
            "name":self.name,"state":self.state,"credential_generation":self.credential_generation,
            "last_seen_at":self.last_seen_at,"capabilities_sequence":self.capabilities_sequence}),
            "machine",
        )
    }
}

async fn lock_machine(db: &mut PgConnection, company: Uuid, id: MachineId) -> AppResult<MachineDb> {
    sqlx::query_as::<_,MachineDb>(&format!("SELECT {MACHINE_COLUMNS} FROM local_machines AS machine WHERE company_id=$1 AND id=$2 FOR UPDATE"))
        .bind(company).bind(id.as_uuid()).fetch_optional(db).await?.ok_or_else(missing)
}

#[derive(sqlx::FromRow)]
struct PairingDb {
    id: Uuid,
    company_id: Uuid,
    actor_id: Option<Uuid>,
    expires_at: DateTime<Utc>,
    consumed_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
    installation_id: Option<Uuid>,
    machine_id: Option<Uuid>,
    claim_verifier: Option<Vec<u8>>,
    credential_verifier: Option<Vec<u8>>,
}
const PAIRING_COLUMNS: &str = "id,company_id,actor_id,expires_at,consumed_at,revoked_at,installation_id,machine_id,claim_verifier,credential_verifier";
impl PairingDb {
    fn projection(&self) -> AppResult<PairingAttempt> {
        Ok(PairingAttempt {
            id: PairingId::new(self.id).map_err(|_| invalid("pairing ID"))?,
            company_id: self.company_id,
            expires_at: self.expires_at,
            consumed_at: self.consumed_at,
            revoked_at: self.revoked_at,
            machine_id: self
                .machine_id
                .map(MachineId::new)
                .transpose()
                .map_err(|_| invalid("machine ID"))?,
        })
    }
    fn matches(&self, claim: &PairingClaim) -> bool {
        self.installation_id == Some(claim.installation_id.as_uuid())
            && self.claim_verifier.as_deref() == Some(claim.claim_verifier.as_bytes().as_slice())
            && self.credential_verifier.as_deref()
                == Some(claim.credential_verifier.as_bytes().as_slice())
    }
}

async fn receipt(
    db: &mut PgConnection,
    company: Uuid,
    machine: Uuid,
) -> AppResult<Option<PairingReceipt>> {
    let Some(row) = sqlx::query_as::<_,MachineDb>(&format!("SELECT {MACHINE_COLUMNS} FROM local_machines AS machine \
        JOIN local_machine_credentials AS credential ON credential.company_id=machine.company_id AND credential.machine_id=machine.id \
        AND credential.generation=machine.credential_generation AND credential.revoked_at IS NULL \
        WHERE machine.company_id=$1 AND machine.id=$2 AND machine.state <> 'revoked'"))
        .bind(company).bind(machine).fetch_optional(&mut *db).await? else { return Ok(None); };
    let name = sqlx::query_scalar("SELECT name FROM companies WHERE id=$1")
        .bind(company)
        .fetch_one(db)
        .await?;
    Ok(Some(PairingReceipt {
        company_id: company,
        company_name: name,
        machine: row.entity()?,
    }))
}

#[async_trait]
impl LocalAgentRead for PostgresPersistence {
    async fn machine(&self, company: Uuid, id: MachineId) -> AppResult<Option<LocalMachine>> {
        let mut tx = begin(&self.pool).await?;
        let row = sqlx::query_as::<_, MachineDb>(&format!(
            "SELECT {MACHINE_COLUMNS} FROM local_machines AS machine WHERE company_id=$1 AND id=$2"
        ))
        .bind(company)
        .bind(id.as_uuid())
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        row.map(MachineDb::entity).transpose()
    }

    async fn profile(&self, company: Uuid, id: ProfileId) -> AppResult<Option<LocalAgentProfile>> {
        let mut tx = begin(&self.pool).await?;
        let profile = shared::profile_on(&mut tx, CompanyId::new(company), id).await?;
        tx.commit().await?;
        Ok(profile)
    }

    async fn session(&self, company: Uuid, id: SessionId) -> AppResult<Option<LocalAgentSession>> {
        let mut tx = begin(&self.pool).await?;
        let session = shared::session_on(&mut tx, CompanyId::new(company), id).await?;
        tx.commit().await?;
        Ok(session)
    }

    async fn capabilities(
        &self,
        company: Uuid,
        machine: MachineId,
        sequence: EventSequence,
    ) -> AppResult<Option<CapabilitySnapshot>> {
        let mut tx = begin(&self.pool).await?;
        let row = sqlx::query_as::<_,(Uuid,DateTime<Utc>,Value)>(
            "SELECT capability.installation_id,capability.observed_at,capability.catalog FROM local_machine_capabilities AS capability \
             JOIN local_machines AS machine ON machine.company_id=capability.company_id AND machine.id=capability.machine_id AND machine.installation_id=capability.installation_id \
             WHERE capability.company_id=$1 AND capability.machine_id=$2 AND capability.sequence=$3")
            .bind(company).bind(machine.as_uuid()).bind(sequence.get() as i64).fetch_optional(&mut *tx).await?;
        tx.commit().await?;
        row.map(|(installation, time, catalog)| {
            decode(
                json!({"machine_id":machine,"installation_id":installation,
            "sequence":sequence,"observed_at":time,"catalog":catalog}),
                "capability snapshot",
            )
        })
        .transpose()
    }
}

async fn revoke(db: &mut PgConnection, company: Uuid, id: MachineId) -> AppResult<()> {
    let machine = lock_machine(db, company, id).await?;
    if machine.state == "revoked" {
        return Ok(());
    }
    sqlx::query("UPDATE local_machines SET state='revoked',revoked_at=clock_timestamp(),credential_generation=credential_generation+1,connection_generation=connection_generation+1,connection_owner=NULL,connection_expires_at=NULL WHERE company_id=$1 AND id=$2")
        .bind(company).bind(id.as_uuid()).execute(&mut *db).await?;
    sqlx::query("UPDATE local_machine_credentials SET revoked_at=clock_timestamp() WHERE company_id=$1 AND machine_id=$2 AND revoked_at IS NULL")
        .bind(company).bind(id.as_uuid()).execute(&mut *db).await?;
    sqlx::query("UPDATE local_pairings SET revoked_at=clock_timestamp() WHERE company_id=$1 AND machine_id=$2 AND revoked_at IS NULL")
        .bind(company).bind(id.as_uuid()).execute(&mut *db).await?;
    sqlx::query("UPDATE local_agent_operations SET outcome=CASE WHEN dispatch->>'kind'='not_dispatched' AND delivery_attempted_at IS NULL THEN 'cancelled' ELSE 'uncertain' END WHERE company_id=$1 AND machine_id=$2 AND outcome IS NULL")
        .bind(company).bind(id.as_uuid()).execute(db).await?;
    Ok(())
}

async fn current_catalog(
    db: &mut PgConnection,
    company: Uuid,
    machine: MachineId,
) -> AppResult<LocalCatalog> {
    let catalog = sqlx::query_scalar::<_,Value>("SELECT capability.catalog FROM local_machines AS machine JOIN local_machine_capabilities AS capability \
        ON capability.company_id=machine.company_id AND capability.machine_id=machine.id AND capability.installation_id=machine.installation_id AND capability.sequence=machine.capabilities_sequence \
        WHERE machine.company_id=$1 AND machine.id=$2")
        .bind(company).bind(machine.as_uuid()).fetch_optional(db).await?.ok_or_else(||AppError::Conflict("machine has no current catalog".into()))?;
    decode(catalog, "catalog")
}

pub(in crate::adapters::persistence) mod protocol;
#[cfg(test)]
pub(crate) use protocol::test_support as protocol_test_support;
mod management;
mod retention;
#[cfg(test)]
mod retention_tests;
mod store;

#[cfg(test)]
mod machine_tests;
#[cfg(test)]
mod pairing_tests;
#[cfg(test)]
pub(crate) mod test_support;
