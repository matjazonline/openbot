//! All worker mutations lock task -> run -> invocation. Never hold these locks over provider I/O.
use super::super::PostgresPersistence;
use crate::{
    app_error::{AppError, AppResult},
    services::harness::{AgentExecutionOutput, runs::*},
};
use async_trait::async_trait;
use sqlx::PgConnection;
use uuid::Uuid;

pub(crate) async fn lock_task_execution_on(
    tx: &mut PgConnection,
    company_id: Uuid,
    lease: crate::entities::task::TaskLeaseRef,
) -> AppResult<bool> {
    let owner = lease.claimed_owner.principal_id().map(|id| id.as_uuid());
    let version = i64::try_from(lease.ownership_version)
        .map_err(|_| AppError::Conflict("Ownership version exhausted".into()))?;
    let task = sqlx::query_scalar::<_, Uuid>(
        r#"SELECT id FROM background_tasks
           WHERE company_id = $1 AND id = $2 AND status = 'processing'
             AND worker_id = $3 AND execution_generation = $4
             AND owner_principal_id = $5 AND ownership_version = $6
             AND lock_expires_at > CURRENT_TIMESTAMP FOR UPDATE"#,
    )
    .bind(company_id)
    .bind(lease.task_id)
    .bind(lease.worker_id)
    .bind(lease.execution_generation)
    .bind(owner)
    .bind(version)
    .fetch_optional(&mut *tx)
    .await?;
    Ok(task.is_some())
}

pub(super) async fn load_on(
    tx: &mut PgConnection,
    company: Uuid,
    task: Uuid,
    run: RunId,
) -> AppResult<Option<RunCheckpoint>> {
    let value = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT checkpoint FROM task_harness_runs WHERE company_id = $1 AND task_id = $2 AND id = $3 FOR UPDATE",
    ).bind(company).bind(task).bind(run.0).fetch_optional(&mut *tx).await?;
    value.map(decode).transpose()
}

fn decode(value: serde_json::Value) -> AppResult<RunCheckpoint> {
    let run: RunCheckpoint = serde_json::from_value(value)
        .map_err(|_| AppError::BadRequest("Malformed harness checkpoint".into()))?;
    run.validate()?;
    if let Some(output) = &run.final_output
        && let Some(response) = &output.structured
    {
        response.verify(
            &output.content,
            &crate::adapters::response_schema::JsonResponseValidator,
        )?;
    }
    Ok(run)
}

fn state_name(state: &RunState) -> &'static str {
    match state {
        RunState::Active => "active",
        RunState::InvalidOutput => "invalid_output",
        RunState::Waiting => "waiting",
        RunState::Completed => "completed",
        RunState::Superseded => "superseded",
    }
}

pub(super) async fn persist_checkpoint_on(
    tx: &mut PgConnection,
    company_id: Uuid,
    task_id: Uuid,
    previous: &RunCheckpoint,
    run: &RunCheckpoint,
) -> AppResult<()> {
    let checkpoint =
        serde_json::to_value(run).map_err(|_| AppError::BadRequest("Invalid checkpoint".into()))?;
    sqlx::query(
        "UPDATE task_harness_runs SET revision = $4, state = $5, checkpoint = $6, updated_at = CURRENT_TIMESTAMP WHERE company_id = $1 AND task_id = $2 AND id = $3",
    ).bind(company_id).bind(task_id).bind(run.run_id.0)
    .bind(i64::try_from(run.revision.0).map_err(|_| AppError::Conflict("Checkpoint revision exhausted".into()))?)
    .bind(state_name(&run.state)).bind(checkpoint).execute(&mut *tx).await?;
    persist_changed_invocations_on(tx, company_id, task_id, previous, run).await?;
    Ok(())
}

/// The checkpoint remains the conversation snapshot; receipts are a transactional projection.
/// Only changed receipts travel to Postgres, in one statement, while the caller holds the run lock.
async fn persist_changed_invocations_on(
    tx: &mut PgConnection,
    company_id: Uuid,
    task_id: Uuid,
    previous: &RunCheckpoint,
    run: &RunCheckpoint,
) -> AppResult<()> {
    let previous: std::collections::HashMap<_, _> = previous
        .invocations
        .iter()
        .map(|invocation| (invocation.call.invocation_id.0, invocation))
        .collect();
    let changed: Vec<_> = run
        .invocations
        .iter()
        .filter(|invocation| {
            previous.get(&invocation.call.invocation_id.0).copied() != Some(*invocation)
        })
        .collect();
    if changed.is_empty() {
        return Ok(());
    }
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new(
        "INSERT INTO task_harness_invocations (id, company_id, task_id, run_id, model_turn, call_ordinal, state, invocation) ",
    );
    query.push_values(changed, |mut row, invocation| {
        let state = match invocation.state {
            InvocationState::Prepared => "prepared",
            InvocationState::Ready => "ready",
            InvocationState::Waiting => "waiting",
            InvocationState::Completed => "completed",
            InvocationState::Failed => "failed",
            InvocationState::Indeterminate => "indeterminate",
        };
        row.push_bind(invocation.call.invocation_id.0)
            .push_bind(company_id)
            .push_bind(task_id)
            .push_bind(run.run_id.0)
            .push_bind(invocation.turn as i16)
            .push_bind(invocation.ordinal as i16)
            .push_bind(state)
            .push_bind(sqlx::types::Json(invocation));
    });
    query.push(" ON CONFLICT (run_id, model_turn, call_ordinal) DO UPDATE SET state = EXCLUDED.state, invocation = EXCLUDED.invocation, updated_at = CURRENT_TIMESTAMP WHERE task_harness_invocations.invocation IS DISTINCT FROM EXCLUDED.invocation");
    query.build().execute(tx).await?;
    Ok(())
}

impl PostgresPersistence {
    async fn mutate_harness_run(
        &self,
        write: &RunWrite,
        mutation: Mutation,
    ) -> AppResult<WriteOutcome<RunCheckpoint>> {
        let mut tx = self.pool.begin().await?;
        if !lock_task_execution_on(&mut tx, write.company_id, write.lease).await? {
            return Ok(WriteOutcome::OwnershipLost);
        }
        let Some(current) =
            load_on(&mut tx, write.company_id, write.lease.task_id, write.run_id).await?
        else {
            return Err(AppError::NotFound("Harness run".into()));
        };
        let scope_matches = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM task_harness_runs WHERE id = $1 AND owner_principal_id = $2 AND ownership_version = $3 AND state <> 'superseded')",
        ).bind(write.run_id.0).bind(write.lease.claimed_owner.principal_id().map(|id| id.as_uuid()))
            .bind(write.lease.ownership_version as i64).fetch_one(&mut *tx).await?;
        if !scope_matches {
            return Ok(WriteOutcome::OwnershipLost);
        }
        if current.revision != write.expected_revision {
            // A lost acknowledgement may replay a committed operation with its original revision.
            // Only an exact, side-effect-free ledger replay can cross this revision fence.
            return Ok(match current.apply(mutation) {
                Ok((_, false)) => WriteOutcome::AlreadyApplied(current),
                _ => WriteOutcome::RevisionConflict,
            });
        }
        let (next, changed) = current.apply(mutation)?;
        if !changed {
            return Ok(WriteOutcome::AlreadyApplied(next));
        }
        persist_checkpoint_on(
            &mut tx,
            write.company_id,
            write.lease.task_id,
            &current,
            &next,
        )
        .await?;
        tx.commit().await?;
        Ok(WriteOutcome::Applied(next))
    }
}

#[async_trait]
impl HarnessRunStore for PostgresPersistence {
    fn mcp_journal(
        &self,
        lease: crate::entities::task::TaskLeaseRef,
    ) -> std::sync::Arc<dyn crate::services::mcp_runtime::McpInvocationJournal> {
        std::sync::Arc::new(super::mcp_journal::PostgresMcpJournal::new(
            self.pool.clone(),
            lease,
        ))
    }
    async fn reserve_execution(
        &self,
        write: &RunWrite,
        allowance_ms: u64,
    ) -> AppResult<WriteOutcome<RunCheckpoint>> {
        self.mutate_harness_run(
            write,
            Mutation::Execution(ExecutionReservation {
                generation: write.lease.execution_generation,
                allowance_ms,
                started_at: chrono::Utc::now(),
                replayed_invocations: 0,
            }),
        )
        .await
    }
    async fn bind_tool_catalogue(
        &self,
        write: &RunWrite,
        fingerprint: String,
    ) -> AppResult<WriteOutcome<RunCheckpoint>> {
        self.mutate_harness_run(write, Mutation::BindTools(fingerprint))
            .await
    }

    async fn open_run(&self, request: OpenRun<'_>) -> AppResult<WriteOutcome<RunCheckpoint>> {
        let run = RunCheckpoint::new(
            request.identity.clone(),
            request.initial_prompt.into(),
            request.policy,
        )?;
        if request.identity.task_id != request.lease.task_id {
            return Err(AppError::BadRequest("Continuation task mismatch".into()));
        }
        let write = RunWrite {
            company_id: request.identity.company_id,
            run_id: run.run_id,
            lease: request.lease,
            expected_revision: CheckpointRevision(0),
        };
        let mut tx = self.pool.begin().await?;
        if !lock_task_execution_on(&mut tx, write.company_id, write.lease).await? {
            return Ok(WriteOutcome::OwnershipLost);
        }
        let owner = request
            .lease
            .claimed_owner
            .principal_id()
            .map(|id| id.as_uuid());
        let authorized = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM principals WHERE company_id = $1 AND id = $2 AND agent_id = $3)",
        ).bind(write.company_id).bind(owner).bind(request.identity.agent_id).fetch_one(&mut *tx).await?;
        if !authorized {
            return Ok(WriteOutcome::OwnershipLost);
        }
        let existing = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT checkpoint FROM task_harness_runs WHERE company_id = $1 AND task_id = $2 AND agent_id = $3 AND ownership_version = $4 ORDER BY created_at DESC LIMIT 1 FOR UPDATE",
        ).bind(write.company_id).bind(write.lease.task_id).bind(request.identity.agent_id)
        .bind(request.lease.ownership_version as i64).fetch_optional(&mut *tx).await?;
        if let Some(value) = existing {
            let saved = decode(value)?;
            if saved.state == RunState::InvalidOutput {
                return Err(crate::services::response_contract::invalid_output());
            }
            if saved.state == RunState::Superseded {
                return Err(AppError::Execution(
                    crate::app_error::ExecutionFailure::CheckpointDenied,
                ));
            }
            if saved.identity != *request.identity || saved.policy != request.policy {
                return Err(AppError::Conflict(
                    "Execution configuration changed during a durable run".into(),
                ));
            }
            return Ok(WriteOutcome::AlreadyApplied(saved));
        }
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM task_harness_runs WHERE company_id = $1 AND task_id = $2",
        )
        .bind(write.company_id)
        .bind(write.lease.task_id)
        .fetch_one(&mut *tx)
        .await?;
        if count >= i64::from(MAX_CONTINUATIONS) {
            return Err(AppError::BadRequest("Continuation count exhausted".into()));
        }
        sqlx::query(
            r#"INSERT INTO task_harness_runs (id, company_id, task_id, agent_id, owner_principal_id, ownership_version, schema_version, revision, state, checkpoint)
               VALUES ($1,$2,$3,$4,$5,$6,1,0,'active',$7)"#,
        ).bind(run.run_id.0).bind(write.company_id).bind(write.lease.task_id).bind(request.identity.agent_id)
        .bind(owner).bind(request.lease.ownership_version as i64)
        .bind(serde_json::to_value(&run).map_err(|_| AppError::BadRequest("Invalid checkpoint".into()))?)
        .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(WriteOutcome::Applied(run))
    }

    async fn diagnostics(
        &self,
        company_id: Uuid,
        task_id: Uuid,
    ) -> AppResult<Option<RunDiagnostics>> {
        let value = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT checkpoint FROM task_harness_runs WHERE company_id = $1 AND task_id = $2 ORDER BY created_at DESC, id DESC LIMIT 1",
        ).bind(company_id).bind(task_id).fetch_optional(&self.pool).await?;
        value
            .map(|value| decode(value).map(|run| RunDiagnostics::from(&run)))
            .transpose()
    }

    async fn load_run(
        &self,
        company_id: Uuid,
        task_id: Uuid,
        run_id: RunId,
    ) -> AppResult<Option<RunCheckpoint>> {
        let value = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT checkpoint FROM task_harness_runs WHERE company_id = $1 AND task_id = $2 AND id = $3",
        ).bind(company_id).bind(task_id).bind(run_id.0).fetch_optional(&self.pool).await?;
        value.map(decode).transpose()
    }

    async fn reserve_model(
        &self,
        write: &RunWrite,
        reservation: ModelReservation,
    ) -> AppResult<WriteOutcome<RunCheckpoint>> {
        self.mutate_harness_run(write, Mutation::Reserve(reservation))
            .await
    }
    async fn commit_model_turn(
        &self,
        write: &RunWrite,
        turn: SavedModelTurn,
    ) -> AppResult<WriteOutcome<RunCheckpoint>> {
        self.mutate_harness_run(write, Mutation::Model(turn)).await
    }
    async fn prepare_invocation(
        &self,
        write: &RunWrite,
        id: InvocationId,
    ) -> AppResult<WriteOutcome<RunCheckpoint>> {
        self.mutate_harness_run(write, Mutation::Prepare(id)).await
    }
    async fn record_result(
        &self,
        write: &RunWrite,
        id: InvocationId,
        result: serde_json::Value,
    ) -> AppResult<WriteOutcome<RunCheckpoint>> {
        self.mutate_harness_run(write, Mutation::Result(id, result))
            .await
    }
    async fn fail_invalid_output(
        &self,
        write: &RunWrite,
    ) -> AppResult<WriteOutcome<RunCheckpoint>> {
        self.mutate_harness_run(write, Mutation::InvalidOutput)
            .await
    }

    async fn save_final_output(
        &self,
        write: &RunWrite,
        output: AgentExecutionOutput,
    ) -> AppResult<WriteOutcome<RunCheckpoint>> {
        if let Some(structured) = &output.structured {
            structured.verify(
                &output.content,
                &crate::adapters::response_schema::JsonResponseValidator,
            )?;
        }
        self.mutate_harness_run(write, Mutation::Final(output))
            .await
    }
}

#[cfg(test)]
#[path = "harness_runs_tests.rs"]
mod tests;

pub(crate) async fn park_harness_approval_on(
    tx: &mut PgConnection,
    write: &RunWrite,
    wait: ApprovalWait,
) -> AppResult<CheckpointRevision> {
    if !lock_task_execution_on(tx, write.company_id, write.lease).await? {
        return Err(AppError::Execution(
            crate::app_error::ExecutionFailure::OwnershipLost,
        ));
    }
    let run = load_on(tx, write.company_id, write.lease.task_id, write.run_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Approval continuation".into()))?;
    if run.revision != write.expected_revision {
        return Err(AppError::Conflict(
            "Approval checkpoint revision changed".into(),
        ));
    }
    let parked = run.park_approval(wait)?;
    persist_checkpoint_on(tx, write.company_id, write.lease.task_id, &run, &parked).await?;
    Ok(parked.revision)
}

#[derive(sqlx::FromRow)]
struct ApprovalLink {
    run_id: Uuid,
    invocation_id: Uuid,
    cycle_id: Uuid,
    checkpoint_revision: i64,
}

pub(crate) async fn resolve_harness_approval_on(
    tx: &mut PgConnection,
    company_id: Uuid,
    task_id: Uuid,
    approval_id: Uuid,
    approved: bool,
) -> AppResult<()> {
    let link = sqlx::query_as::<_, ApprovalLink>(
        "SELECT run_id, invocation_id, cycle_id, checkpoint_revision FROM task_approval_waits WHERE company_id = $1 AND task_id = $2 AND approval_id = $3 AND state = 'waiting' AND run_id IS NOT NULL FOR UPDATE",
    ).bind(company_id).bind(task_id).bind(approval_id).fetch_optional(&mut *tx).await?;
    let Some(ApprovalLink {
        run_id,
        invocation_id,
        cycle_id,
        checkpoint_revision: revision,
    }) = link
    else {
        return Ok(());
    };
    let run = load_on(tx, company_id, task_id, RunId(run_id))
        .await?
        .ok_or_else(|| AppError::NotFound("Approval continuation".into()))?;
    if run.revision.0 != revision as u64 {
        return Err(AppError::Conflict(
            "Approval continuation revision changed".into(),
        ));
    }
    let wait = ApprovalWait {
        approval_id,
        cycle_id,
        invocation_id: InvocationId(invocation_id),
    };
    let next = run.resolve_approval(&wait, approved)?;
    // The decision transaction has already locked and checked the task's wait and ownership.
    // No worker lease exists here; persistence takes only the already-authorized scope.
    persist_checkpoint_on(tx, company_id, task_id, &run, &next).await
}

/// Database effects call this before committing their own transaction. A failed revision or
/// lease check rolls the effect back together with its receipt.
pub(crate) async fn record_harness_result_on(
    tx: &mut PgConnection,
    company_id: Uuid,
    lease: crate::entities::task::TaskLeaseRef,
    reference: InvocationRef,
    result: serde_json::Value,
) -> AppResult<()> {
    if !lock_task_execution_on(tx, company_id, lease).await? {
        return Err(AppError::Execution(
            crate::app_error::ExecutionFailure::OwnershipLost,
        ));
    }
    let run = load_on(tx, company_id, lease.task_id, reference.run_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Effect continuation".into()))?;
    if run.revision != reference.expected_revision {
        return Err(AppError::Conflict(
            "Effect checkpoint revision changed".into(),
        ));
    }
    let (next, _) = run.apply(Mutation::Result(reference.invocation_id, result))?;
    persist_checkpoint_on(tx, company_id, lease.task_id, &run, &next).await
}

/// Ownership event authority: the caller holds the task row lock and has authorized the
/// transition from `previous_version`. New owners never inherit these conversations.
pub(crate) async fn supersede_harness_runs_on(
    tx: &mut PgConnection,
    company_id: Uuid,
    task_id: Uuid,
    previous_version: u64,
) -> AppResult<()> {
    let values = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT checkpoint FROM task_harness_runs WHERE company_id = $1 AND task_id = $2 AND ownership_version = $3 AND state <> 'superseded' ORDER BY id FOR UPDATE",
    ).bind(company_id).bind(task_id).bind(previous_version as i64).fetch_all(&mut *tx).await?;
    for value in values {
        let run = decode(value)?;
        let next = run.supersede()?;
        persist_checkpoint_on(tx, company_id, task_id, &run, &next).await?;
    }
    Ok(())
}

pub(crate) async fn park_harness_outreach_on(
    tx: &mut PgConnection,
    company_id: Uuid,
    lease: crate::entities::task::TaskLeaseRef,
    reference: InvocationRef,
    outreach_id: Uuid,
) -> AppResult<()> {
    if !lock_task_execution_on(tx, company_id, lease).await? {
        return Err(AppError::Execution(
            crate::app_error::ExecutionFailure::OwnershipLost,
        ));
    }
    let run = load_on(tx, company_id, lease.task_id, reference.run_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Outreach continuation".into()))?;
    if run.revision != reference.expected_revision {
        return Err(AppError::Conflict(
            "Outreach checkpoint revision changed".into(),
        ));
    }
    let next = run.park_outreach(OutreachWait {
        outreach_id,
        invocation_id: reference.invocation_id,
    })?;
    persist_checkpoint_on(tx, company_id, lease.task_id, &run, &next).await?;
    sqlx::query("UPDATE task_outreaches SET harness_run_id = $3, harness_invocation_id = $4, checkpoint_revision = $5 WHERE company_id = $1 AND id = $2")
        .bind(company_id).bind(outreach_id).bind(reference.run_id.0).bind(reference.invocation_id.0).bind(next.revision.0 as i64)
        .execute(&mut *tx).await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct OutreachLink {
    company_id: Uuid,
    task_id: Uuid,
    harness_run_id: Uuid,
    harness_invocation_id: Uuid,
    checkpoint_revision: i64,
    status: String,
    task_status: String,
}

/// Called inside quorum/timeout/ownership-authorized event transactions after locking outreach
/// then task. The saved ownership and exact awaited outreach must still match.
pub(crate) async fn resolve_harness_outreach_on(
    tx: &mut PgConnection,
    outreach_id: Uuid,
) -> AppResult<()> {
    let link = sqlx::query_as::<_, OutreachLink>(
        r#"SELECT outreach.company_id, outreach.task_id, outreach.harness_run_id,
                  outreach.harness_invocation_id, outreach.checkpoint_revision, outreach.status, task.status AS task_status
           FROM task_outreaches AS outreach JOIN background_tasks AS task ON task.id = outreach.task_id
           WHERE outreach.id = $1 AND outreach.harness_run_id IS NOT NULL
             AND task.owner_principal_id IS NOT DISTINCT FROM outreach.created_by_principal_id
             AND task.ownership_version = outreach.ownership_version
             AND task.awaited_outreach_id = outreach.id
           FOR UPDATE OF task"#,
    ).bind(outreach_id).fetch_optional(&mut *tx).await?;
    let Some(link) = link else {
        return Ok(());
    };
    let run = load_on(
        tx,
        link.company_id,
        link.task_id,
        RunId(link.harness_run_id),
    )
    .await?
    .ok_or_else(|| AppError::NotFound("Outreach continuation".into()))?;
    if run.state == RunState::Superseded || run.outreach_wait.is_none() {
        return Ok(());
    }
    if run.revision.0 != link.checkpoint_revision as u64 {
        return Err(AppError::Conflict("Outreach wait revision changed".into()));
    }
    let next = match link.status.as_str() {
        "threshold_met" | "proceed_partial" => {
            let replies = sqlx::query_scalar::<_, String>(
                r#"SELECT LEFT(message.clean_text_body, 256) FROM task_outreach_targets AS target
                   JOIN thread_messages AS association ON association.id = target.response_association_id
                   JOIN messages AS message ON message.id = association.message_id AND message.company_id = target.company_id
                   WHERE target.outreach_id = $1 AND target.status = 'responded'
                   ORDER BY target.responded_at, target.id LIMIT 32"#,
            ).bind(outreach_id).fetch_all(&mut *tx).await?;
            let result = serde_json::json!({"accepted":true,"outreach_id":outreach_id,"status":link.status,
                "responses":replies,"response_excerpt_limit":256,"response_count_limit":32});
            run.resolve_outreach(
                &OutreachWait {
                    outreach_id,
                    invocation_id: InvocationId(link.harness_invocation_id),
                },
                result,
            )?
        }
        "cancelled" if link.task_status != "stopped" => run.resolve_outreach(
            &OutreachWait {
                outreach_id,
                invocation_id: InvocationId(link.harness_invocation_id),
            },
            serde_json::json!({"accepted":false,"outreach_id":outreach_id,"status":"cancelled"}),
        )?,
        "cancelled" | "completed" => run.supersede()?,
        _ => return Ok(()),
    };
    persist_checkpoint_on(tx, link.company_id, link.task_id, &run, &next).await?;
    sqlx::query("UPDATE background_tasks SET awaited_outreach_id = NULL WHERE company_id = $1 AND id = $2 AND awaited_outreach_id = $3")
        .bind(link.company_id).bind(link.task_id).bind(outreach_id).execute(&mut *tx).await?;
    sqlx::query("UPDATE task_approval_waits AS wait SET state = 'expired' FROM human_approvals AS approval WHERE wait.approval_id = approval.id AND approval.company_id = $1 AND approval.task_id = $2 AND approval.payload->>'outreach_id' = $3 AND wait.state = 'waiting'")
        .bind(link.company_id).bind(link.task_id).bind(outreach_id.to_string()).execute(&mut *tx).await?;
    Ok(())
}

/// The same transaction consumes the validated generation and publishes exactly that answer.
pub(super) async fn validate_final_publication_on(
    tx: &mut PgConnection,
    lease: crate::entities::task::TaskLeaseRef,
    message: &crate::use_cases::thread::MessageWrite,
) -> AppResult<()> {
    let saved: Option<sqlx::types::Json<AgentExecutionOutput>> = sqlx::query_scalar(
        "SELECT checkpoint->'final_output' FROM task_harness_runs WHERE task_id = $1 AND owner_principal_id = $2 AND ownership_version = $3 AND state = 'completed'",
    ).bind(lease.task_id).bind(lease.claimed_owner.principal_id().map(|id| id.as_uuid()))
        .bind(lease.ownership_version as i64).fetch_optional(&mut *tx).await?;
    if let Some(saved) = saved
        && (saved.structured != message.structured
            || (saved.structured.is_some() && saved.content != message.clean_text_body))
    {
        return Err(crate::services::response_contract::invalid_output());
    }
    Ok(())
}
