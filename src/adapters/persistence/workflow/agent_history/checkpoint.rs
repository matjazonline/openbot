use super::*;
use crate::application::workflow::{
    agent_contract::AgentSelectionIdentity, publication::AgentUseMode,
};
use crate::services::harness::runs::{RunCheckpoint, RunState};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::adapters::persistence::workflow) enum LinkChange {
    Linked,
    Unchanged,
}

#[derive(sqlx::FromRow)]
struct Record {
    id: Uuid,
    job_id: Uuid,
    agent_id: Uuid,
    revision_id: Uuid,
    manifest_id: Uuid,
    checkpoint_id: Option<Uuid>,
    #[sqlx(try_from = "String")]
    state: AgentExchangeState,
    input_matches: bool,
    output_contract: Value,
    frozen_preparation: Option<Value>,
}

fn reference(prepared: &Value) -> AppResult<Option<AgentExchangeId>> {
    serde_json::from_value(prepared["history_exchange"].clone()).map_err(|_| invalid())
}

async fn record_on(
    db: &mut PgConnection,
    scope: ActionScope,
    actual_mode: AgentUseMode,
    prepared: &Value,
) -> AppResult<Option<Record>> {
    let reference = reference(prepared)?;
    if actual_mode == AgentUseMode::ModelOnly {
        return if reference.is_none() && prepared.get("history") == Some(&Value::Null) {
            Ok(None)
        } else {
            Err(invalid())
        };
    }
    if prepared["version"] != 5 {
        return Err(invalid());
    }
    let reference =
        reference.ok_or_else(|| AppError::Conflict("Agent input was not durably queued".into()))?;
    if reference != AgentExchangeId::for_execution(scope.execution) {
        return Err(invalid());
    }
    let selection: AgentSelectionIdentity =
        serde_json::from_value(prepared["selection"].clone()).map_err(|_| invalid())?;
    let input = prepared.get("context").ok_or_else(invalid)?;
    let row = sqlx::query_as::<_, Record>(
        "SELECT id,job_id,agent_id,revision_id,manifest_id,checkpoint_id,state, \
         provenance->'output_contract' AS output_contract,frozen_preparation, \
         input_digest=sha256(convert_to($4::jsonb::text,'UTF8')) AS input_matches \
         FROM agent_history_exchanges WHERE company_id=$1 AND run_id=$2 AND execution_id=$3",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(input)
    .fetch_optional(db)
    .await?
    .ok_or_else(missing)?;
    if row.id != reference.as_uuid()
        || row.agent_id != selection.agent.as_uuid()
        || row.revision_id != selection.revision.as_uuid()
        || row.manifest_id != selection.manifest.as_uuid()
        || selection.mode != actual_mode
        || !row.input_matches
        || row.output_contract["schema"] != prepared["output_schema"]
        || row.frozen_preparation.as_ref() != Some(prepared)
    {
        return Err(AppError::Conflict(
            "Checkpoint preparation differs from its queued input".into(),
        ));
    }
    Ok(Some(row))
}

#[derive(sqlx::FromRow)]
struct StepSource {
    bundle: Vec<u8>,
    step_id: String,
}

/// Generic native checkpoint fixtures may be non-model work. The immutable program, rather
/// than a caller's prepared label, decides whether this is an eligible production agent.
async fn eligible_on(db: &mut PgConnection, scope: ActionScope) -> AppResult<AgentUseMode> {
    let source=sqlx::query_as::<_,StepSource>(
        "SELECT stored.bundle,execution.step_id FROM workflow_runs AS run \
         JOIN workflow_bundles AS stored ON stored.company_id=run.company_id AND stored.digest=run.bundle_digest \
         JOIN workflow_executions AS execution ON execution.company_id=run.company_id AND execution.run_id=run.id \
         WHERE run.company_id=$1 AND run.id=$2 AND execution.id=$3 AND octet_length(stored.bundle)<=$4",
    ).bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .bind(MAX_BUNDLE_BYTES as i32).fetch_optional(db).await?.ok_or_else(missing)?;
    let bundle = restore_bundle(
        &source.bundle,
        &crate::adapters::workflow_source::WorkflowSourceDecoder,
    )?;
    let step = StepId::parse(source.step_id).map_err(|_| invalid())?;
    let kind = bundle
        .compiled()
        .graph()
        .definition()
        .steps
        .get(&step)
        .ok_or_else(invalid)?;
    Ok(if kind.step_type.as_str() == "agent.run" {
        AgentUseMode::Tools
    } else {
        AgentUseMode::ModelOnly
    })
}

pub(in crate::adapters::persistence::workflow) async fn check_prepared_on(
    db: &mut PgConnection,
    scope: ActionScope,
    actual_mode: AgentUseMode,
    prepared: &Value,
) -> AppResult<()> {
    if prepared["version"] != 5 {
        return Err(invalid());
    }
    record_on(db, scope, actual_mode, prepared).await?;
    Ok(())
}

pub(in crate::adapters::persistence::workflow) async fn attach_on(
    db: &mut PgConnection,
    fence: WorkflowFence,
    prepared: &Value,
    checkpoint: Uuid,
) -> AppResult<LinkChange> {
    let scope = ActionScope {
        company: fence.scope.company,
        run: fence.scope.run,
        execution: fence.scope.execution,
    };
    let mode = eligible_on(db, scope).await?;
    let Some(row) = record_on(db, scope, mode, prepared).await? else {
        return Ok(LinkChange::Unchanged);
    };
    if !turns::owned_on(db, scope.company, AgentExchangeId::new(row.id)).await? {
        return Err(invalid());
    }
    if row.job_id != fence.scope.job.0 || row.checkpoint_id.is_some_and(|saved| saved != checkpoint)
    {
        return Err(invalid());
    }
    if row.state != AgentExchangeState::Queued {
        return if row.checkpoint_id == Some(checkpoint) {
            Ok(LinkChange::Unchanged)
        } else {
            Err(invalid())
        };
    }
    let changed = sqlx::query(
        "UPDATE agent_history_exchanges AS exchange SET state='active',checkpoint_id=$4, \
         activated_at=COALESCE(exchange.activated_at,clock_timestamp()), \
         access_level=GREATEST(exchange.access_level,run.content_level) \
         FROM workflow_runs AS run WHERE exchange.company_id=$1 AND exchange.run_id=$2 AND exchange.id=$3 \
           AND run.company_id=exchange.company_id AND run.id=exchange.run_id AND exchange.state='queued'",
    ).bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(row.id).bind(checkpoint).execute(db).await?;
    if changed.rows_affected() != 1 {
        return Err(missing());
    }
    Ok(LinkChange::Linked)
}

/// Read-only evidence for checkpoint open, including its additional reactivation effect.
pub(in crate::adapters::persistence::workflow) async fn linked_on(
    db: &mut PgConnection,
    fence: WorkflowFence,
    prepared: &Value,
    checkpoint: Uuid,
) -> AppResult<bool> {
    let scope = ActionScope {
        company: fence.scope.company,
        run: fence.scope.run,
        execution: fence.scope.execution,
    };
    let mode = eligible_on(db, scope).await?;
    let Some(row) = record_on(db, scope, mode, prepared).await? else {
        return Ok(true);
    };
    Ok(row.checkpoint_id == Some(checkpoint)
        && row.state != AgentExchangeState::Queued
        && turns::owned_on(db, scope.company, AgentExchangeId::new(row.id)).await?)
}

pub(in crate::adapters::persistence::workflow) async fn require_active_on(
    db: &mut PgConnection,
    fence: WorkflowFence,
    prepared: &Value,
    checkpoint: Uuid,
) -> AppResult<()> {
    let scope = ActionScope {
        company: fence.scope.company,
        run: fence.scope.run,
        execution: fence.scope.execution,
    };
    let mode = eligible_on(db, scope).await?;
    let Some(row) = record_on(db, scope, mode, prepared).await? else {
        return Ok(());
    };
    if row.state != AgentExchangeState::Active
        || row.checkpoint_id != Some(checkpoint)
        || !turns::owned_on(db, scope.company, AgentExchangeId::new(row.id)).await?
    {
        return Err(AppError::Execution(
            crate::app_error::ExecutionFailure::CheckpointDenied,
        ));
    }
    Ok(())
}

pub(in crate::adapters::persistence::workflow) async fn accepted_on(
    db: &mut PgConnection,
    fence: WorkflowFence,
    prepared: &Value,
    checkpoint: &RunCheckpoint,
) -> AppResult<()> {
    if !matches!(
        checkpoint.state,
        RunState::Completed | RunState::InvalidOutput
    ) {
        return Ok(());
    }
    let scope = ActionScope {
        company: fence.scope.company,
        run: fence.scope.run,
        execution: fence.scope.execution,
    };
    let mode = eligible_on(db, scope).await?;
    let Some(row) = record_on(db, scope, mode, prepared).await? else {
        return Ok(());
    };
    if row.checkpoint_id != Some(checkpoint.run_id.0) {
        return Err(invalid());
    }
    let output = if checkpoint.state == RunState::Completed {
        let structured = checkpoint
            .final_output
            .as_ref()
            .and_then(|output| output.structured.as_ref())
            .ok_or_else(invalid)?;
        if serde_json::to_value(structured.contract()).map_err(|_| invalid())?
            != row.output_contract
        {
            return Err(invalid());
        }
        Some(
            AgentLogicalOutput::new(
                serde_json::from_str(structured.body()).map_err(|_| invalid())?,
            )?
            .into_value(),
        )
    } else {
        None
    };
    let changed = sqlx::query(
        "UPDATE agent_history_exchanges AS exchange SET state=$4,output=$5,finished_at=clock_timestamp(), \
         access_level=GREATEST(exchange.access_level,run.content_level) \
         FROM workflow_runs AS run WHERE exchange.company_id=$1 AND exchange.run_id=$2 AND exchange.id=$3 \
           AND run.company_id=exchange.company_id AND run.id=exchange.run_id AND exchange.state='active'",
    ).bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(row.id)
        .bind(if output.is_some() {"completed"} else {"failed"}).bind(output).execute(db).await?;
    if changed.rows_affected() != 1 {
        return Err(missing());
    }
    tracing::info!(company_id=%scope.company.as_uuid(),run_id=%scope.run.as_uuid(),exchange_id=%row.id,
        durability="staged", state=if checkpoint.state==RunState::Completed {"completed"} else {"failed"},
        event="agent_history.completed", "Agent exchange terminal result staged in current transaction");
    tracing::info!(company_id=%scope.company.as_uuid(),exchange_id=%row.id,
        durability="staged", event="agent_history.released", "Agent turn released in current transaction");
    Ok(())
}
