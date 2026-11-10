//! Lock order: company/association authority → machine fence → operation/session.
use super::*;
use crate::application::local_agents::protocol::*;

#[derive(sqlx::FromRow)]
struct FenceRow {
    state: String,
    journal_id: Option<Uuid>,
    acknowledged_sequence: i64,
    reconciled_generation: i64,
    journal_generation: i64,
    connection_expires_at: Option<DateTime<Utc>>,
}

async fn lock_fence(db: &mut PgConnection, fence: ConnectionFence) -> AppResult<FenceRow> {
    let row=sqlx::query_as::<_,FenceRow>("SELECT state,journal_id,acknowledged_sequence,reconciled_generation,journal_generation,connection_expires_at FROM local_machines \
        WHERE company_id=$1 AND id=$2 AND installation_id=$3 AND credential_generation=$4 AND connection_generation=$5 AND connection_owner=$6 AND state<>'revoked' FOR UPDATE")
        .bind(fence.machine.company_id).bind(fence.machine.machine_id.as_uuid()).bind(fence.machine.installation_id.as_uuid())
        .bind(fence.machine.credential_generation.get() as i64).bind(fence.generation.get() as i64).bind(fence.owner_id)
        .fetch_optional(&mut *db).await?.ok_or(AppError::InvalidCredentials)?;
    let live: bool = sqlx::query_scalar("SELECT $1::timestamptz>clock_timestamp()")
        .bind(row.connection_expires_at)
        .fetch_one(db)
        .await?;
    if !live {
        return Err(AppError::InvalidCredentials);
    }
    Ok(row)
}

async fn commit_cursor(
    db: &mut PgConnection,
    fence: ConnectionFence,
    sequence: u64,
) -> AppResult<()> {
    let result=sqlx::query("UPDATE local_machines SET acknowledged_sequence=$7,last_seen_at=clock_timestamp() \
        WHERE company_id=$1 AND id=$2 AND installation_id=$3 AND credential_generation=$4 AND connection_generation=$5 AND connection_owner=$6 AND state<>'revoked' AND connection_expires_at>clock_timestamp()")
        .bind(fence.machine.company_id).bind(fence.machine.machine_id.as_uuid()).bind(fence.machine.installation_id.as_uuid())
        .bind(fence.machine.credential_generation.get() as i64).bind(fence.generation.get() as i64).bind(fence.owner_id)
        .bind(sequence as i64).execute(db).await?;
    if result.rows_affected() != 1 {
        return Err(AppError::InvalidCredentials);
    }
    Ok(())
}

#[derive(sqlx::FromRow)]
struct OperationDb {
    id: Uuid,
    origin: String,
    command: Value,
    deadline: DateTime<Utc>,
    payload_digest: String,
    dispatch: Value,
    outcome: Option<String>,
    session_id: Option<Uuid>,
    actor_id: Option<Uuid>,
    delivery_attempted_at: Option<DateTime<Utc>>,
    outstanding: bool,
}
// A server wait can end while native work remains physically unsettled. Every
// queue, reconnect barrier and delivery read uses this same definition.
pub(in crate::adapters::persistence) const OUTSTANDING_OPERATION_SQL: &str =
    "public.local_operation_is_outstanding(command,outcome,observed_turn_state)";

// Every expiry producer preserves possible physical work until native terminal proof.
pub(in crate::adapters::persistence) const EXPIRATION_OUTCOME_SQL: &str = "CASE WHEN operation.delivery_attempted_at IS NULL AND operation.dispatch->>'kind'='not_dispatched' THEN 'expired' ELSE 'uncertain' END";

fn operation_columns() -> String {
    format!(
        "id,origin,command,deadline,payload_digest,dispatch,outcome,session_id,actor_id,delivery_attempted_at, \
        {OUTSTANDING_OPERATION_SQL} AS outstanding"
    )
}
impl OperationDb {
    fn origin(&self) -> AppResult<OperationOrigin> {
        OperationOrigin::try_from(self.origin.as_str()).map_err(|_| invalid("operation origin"))
    }
    fn operation(&self) -> AppResult<LocalAgentOperation> {
        self.origin()?;
        let operation: LocalAgentOperation = decode(
            json!({"id":self.id,"command":self.command,"deadline":self.deadline,"payload_digest":self.payload_digest,
            "dispatch":self.dispatch,"outcome":self.outcome}),
            "operation",
        )?;
        if command_session(operation.command()).map(SessionId::as_uuid) != self.session_id {
            return Err(invalid("operation session binding"));
        }
        Ok(operation)
    }
}

fn command_session(command: &LocalCommand) -> Option<SessionId> {
    match command {
        LocalCommand::Create { session_id, .. }
        | LocalCommand::Send { session_id, .. }
        | LocalCommand::Inspect { session_id, .. }
        | LocalCommand::StopThread { session_id, .. }
        | LocalCommand::ArchiveThread { session_id, .. } => Some(*session_id),
        LocalCommand::RefreshCatalog {} => None,
    }
}

fn target_thread(command: &LocalCommand) -> Option<&CloudroomThreadId> {
    match command {
        LocalCommand::Send { thread_id, .. }
        | LocalCommand::Inspect { thread_id, .. }
        | LocalCommand::StopThread { thread_id, .. }
        | LocalCommand::ArchiveThread { thread_id, .. } => Some(thread_id),
        _ => None,
    }
}

#[derive(sqlx::FromRow)]
pub(super) struct Capacity {
    pub(super) unsettled: i64,
    pub(super) queued: i64,
}

pub(super) async fn capacity(
    db: &mut PgConnection,
    company: Uuid,
    machine: MachineId,
) -> AppResult<Capacity> {
    sqlx::query_as(&format!("SELECT count(*) FILTER(WHERE command->>'kind' IN ('create','send')) AS unsettled,count(*) AS queued \
        FROM local_agent_operations WHERE company_id=$1 AND machine_id=$2 AND {OUTSTANDING_OPERATION_SQL}"))
        .bind(company).bind(machine.as_uuid()).fetch_one(db).await.map_err(Into::into)
}

#[async_trait]
impl LocalProtocolStore for PostgresPersistence {
    async fn retired_mappings(
        &self,
        fence: ConnectionFence,
        request: &MappingRetirementRequest,
    ) -> AppResult<MappingRetirementSessions> {
        request.validate()?;
        let mut tx = begin(&self.pool).await?;
        let machine = lock_fence(&mut tx, fence).await?;
        if machine.journal_generation != fence.generation.get() as i64
            || machine.reconciled_generation != fence.generation.get() as i64
        {
            return Err(AppError::InvalidCredentials);
        }
        let ids: Vec<Uuid> = request.sessions.iter().map(|id| id.as_uuid()).collect();
        let retired:Vec<Uuid>=sqlx::query_scalar("SELECT requested.id FROM unnest($4::uuid[]) AS requested(id)
            WHERE NOT EXISTS(SELECT 1 FROM local_agent_sessions AS session WHERE session.company_id=$1
              AND session.machine_id=$2 AND session.installation_id=$3 AND session.id=requested.id)
            ORDER BY requested.id")
            .bind(fence.machine.company_id).bind(fence.machine.machine_id.as_uuid())
            .bind(fence.machine.installation_id.as_uuid()).bind(&ids).fetch_all(&mut *tx).await?;
        commit_cursor(&mut tx, fence, machine.acknowledged_sequence as u64).await?;
        tx.commit().await?;
        let retired = retired
            .into_iter()
            .map(SessionId::new)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| invalid("retired session identity"))?;
        MappingRetirementSessions::parse("retired mappings", retired)
            .map_err(|_| invalid("retired session page"))
    }
    async fn record_connection_issue(
        &self,
        fence: ConnectionFence,
        issue: ConnectionIssue,
    ) -> AppResult<bool> {
        let mut tx = begin(&self.pool).await?;
        let result = sqlx::query("UPDATE local_machines SET last_connection_issue=$7 WHERE last_connection_issue IS NULL AND company_id=$1 AND id=$2 AND installation_id=$3 AND credential_generation=$4 AND connection_generation=$5 AND connection_owner=$6 AND connection_expires_at>clock_timestamp() AND state<>'revoked'")
            .bind(fence.machine.company_id).bind(fence.machine.machine_id.as_uuid()).bind(fence.machine.installation_id.as_uuid())
            .bind(fence.machine.credential_generation.get() as i64).bind(fence.generation.get() as i64).bind(fence.owner_id).bind(issue.as_str()).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(result.rows_affected() == 1)
    }
    async fn connect_journal(
        &self,
        fence: ConnectionFence,
        hello: &JournalHello,
    ) -> AppResult<Welcome> {
        hello.validate(fence)?;
        let identity = ConnectionIdentity::from_fence(fence)?;
        let mut tx = begin(&self.pool).await?;
        let row = lock_fence(&mut tx, fence).await?;
        let acknowledged = row.acknowledged_sequence as u64;
        let mut recovery = acknowledged > hello.journal_sequence || acknowledged > MAX_SAFE_INTEGER;
        match row.journal_id {
            Some(id) => recovery |= id != hello.journal_id,
            None => {
                let prior:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM local_agent_operations WHERE company_id=$1 AND machine_id=$2 AND delivery_attempted_at IS NOT NULL) \
                    OR EXISTS(SELECT 1 FROM local_agent_sessions WHERE company_id=$1 AND machine_id=$2 AND thread_id IS NOT NULL)")
                    .bind(fence.machine.company_id).bind(fence.machine.machine_id.as_uuid()).fetch_one(&mut *tx).await?;
                recovery |= prior;
                if !recovery {
                    sqlx::query(
                        "UPDATE local_machines SET journal_id=$3 WHERE company_id=$1 AND id=$2",
                    )
                    .bind(fence.machine.company_id)
                    .bind(fence.machine.machine_id.as_uuid())
                    .bind(hello.journal_id)
                    .execute(&mut *tx)
                    .await?;
                }
            }
        }
        if !recovery {
            sqlx::query(
                "UPDATE local_machines SET journal_generation=$3 WHERE company_id=$1 AND id=$2",
            )
            .bind(fence.machine.company_id)
            .bind(fence.machine.machine_id.as_uuid())
            .bind(fence.generation.get() as i64)
            .execute(&mut *tx)
            .await?;
        }
        if !recovery && row.state == "enabled" {
            bootstrap_catalog(&mut tx, fence).await?;
        }
        sqlx::query(
            "UPDATE local_machines SET last_connection_issue=$3 WHERE company_id=$1 AND id=$2",
        )
        .bind(fence.machine.company_id)
        .bind(fence.machine.machine_id.as_uuid())
        .bind(if recovery {
            Some(ConnectionIssue::RecoveryRequired.as_str())
        } else {
            None
        })
        .execute(&mut *tx)
        .await?;
        commit_cursor(&mut tx, fence, acknowledged).await?;
        tx.commit().await?;
        Ok(Welcome {
            identity,
            acknowledged_sequence: acknowledged,
            recovery_required: recovery,
        })
    }

    async fn finish_reconciliation(&self, fence: ConnectionFence) -> AppResult<bool> {
        let mut tx = begin(&self.pool).await?;
        let row = lock_fence(&mut tx, fence).await?;
        if row.journal_id.is_none() || row.journal_generation != fence.generation.get() as i64 {
            return Err(AppError::InvalidCredentials);
        }
        // Reconciliation is a one-time barrier for this connection generation.
        // New dispatches after it opens cannot close it before their receipts arrive.
        let outstanding = if row.reconciled_generation == fence.generation.get() as i64 {
            false
        } else {
            sqlx::query_scalar::<_, bool>(&format!("SELECT EXISTS(SELECT 1 FROM local_agent_operations WHERE company_id=$1 AND machine_id=$2 AND {OUTSTANDING_OPERATION_SQL} \
            AND delivery_attempted_at IS NOT NULL AND last_observed_generation<>$3)"))
                .bind(fence.machine.company_id).bind(fence.machine.machine_id.as_uuid()).bind(fence.generation.get() as i64).fetch_one(&mut *tx).await?
        };
        if !outstanding {
            sqlx::query(
                "UPDATE local_machines SET reconciled_generation=$3 WHERE company_id=$1 AND id=$2",
            )
            .bind(fence.machine.company_id)
            .bind(fence.machine.machine_id.as_uuid())
            .bind(fence.generation.get() as i64)
            .execute(&mut *tx)
            .await?;
        }
        commit_cursor(&mut tx, fence, row.acknowledged_sequence as u64).await?;
        tx.commit().await?;
        Ok(!outstanding)
    }

    async fn pending_commands(&self, fence: ConnectionFence) -> AppResult<Vec<CommandDelivery>> {
        commands::pending(self, fence).await
    }
    async fn enqueue_operation(
        &self,
        admission: &OperationAdmission,
    ) -> AppResult<LocalAgentOperation> {
        commands::enqueue(self, admission).await
    }
    async fn accept_observation(
        &self,
        fence: ConnectionFence,
        observation: &JournalObservation,
    ) -> AppResult<u64> {
        observations::accept(self, fence, observation).await
    }
    async fn expire_operations(&self, limit: u16) -> AppResult<u16> {
        if limit == 0 || limit > 100 {
            return Err(AppError::BadRequest("invalid expiration batch".into()));
        }
        let mut tx = begin(&self.pool).await?;
        let result=sqlx::query(&format!("WITH due AS (SELECT company_id,id FROM local_agent_operations WHERE outcome IS NULL AND deadline<=clock_timestamp() ORDER BY deadline,id LIMIT $1 FOR UPDATE SKIP LOCKED) \
            UPDATE local_agent_operations AS operation SET outcome={EXPIRATION_OUTCOME_SQL} \
            FROM due WHERE operation.company_id=due.company_id AND operation.id=due.id AND operation.outcome IS NULL"))
            .bind(i64::from(limit)).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(result.rows_affected() as u16)
    }
}

#[cfg(test)]
mod capacity_tests;
#[cfg(test)]
mod command_tests;
pub(in crate::adapters::persistence) mod commands;
#[cfg(test)]
mod observation_tests;
mod observations;
#[cfg(test)]
mod operation_record_tests;
#[cfg(test)]
mod recovery_tests;
#[cfg(test)]
mod submission_tests;
#[cfg(test)]
pub(crate) mod test_support;

/// A connection may observe its own catalog, never manufacture native-agent work or user authority.
async fn bootstrap_catalog(db: &mut PgConnection, fence: ConnectionFence) -> AppResult<()> {
    let pending: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM local_agent_operations WHERE company_id=$1 AND machine_id=$2 AND command->>'kind'='refresh_catalog' AND outcome IS NULL)")
        .bind(fence.machine.company_id).bind(fence.machine.machine_id.as_uuid()).fetch_one(&mut *db).await?;
    let counts = capacity(db, fence.machine.company_id, fence.machine.machine_id).await?;
    if pending || counts.queued >= MAX_QUEUED_COMMANDS as i64 {
        return Ok(());
    }
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *db)
        .await?;
    let operation = LocalAgentOperation::queued(
        OperationId::new(Uuid::new_v4()).map_err(|_| invalid("operation identity"))?,
        LocalCommand::RefreshCatalog {},
        now + chrono::Duration::seconds(DEFAULT_DEADLINE_SECONDS),
    )
    .map_err(|_| invalid("catalog operation"))?;
    sqlx::query("INSERT INTO local_agent_operations(company_id,id,machine_id,installation_id,logical_key,command,payload_digest,deadline) VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT(company_id,machine_id,logical_key) DO NOTHING")
        .bind(fence.machine.company_id).bind(operation.id().as_uuid()).bind(fence.machine.machine_id.as_uuid()).bind(fence.machine.installation_id.as_uuid())
        .bind(format!("catalog:{}",fence.generation.get())).bind(encode(operation.command())?).bind(operation.payload_digest().as_str()).bind(operation.deadline()).execute(db).await?;
    Ok(())
}
