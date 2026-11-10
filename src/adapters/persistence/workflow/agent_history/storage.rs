use super::*;
use crate::application::workflow::agent::PreparedAgent;
use serde_json::Value;

#[derive(sqlx::FromRow)]
struct SavedExchange {
    id: Uuid,
    conversation_id: Uuid,
    agent_id: Uuid,
    revision_id: Uuid,
    manifest_id: Uuid,
    run_id: Uuid,
    job_id: Uuid,
    input_matches: bool,
    provenance: Value,
}

fn provenance(prepared: &PreparedAgent) -> AppResult<Value> {
    let selection = prepared.selection();
    serde_json::to_value(AgentExchangeProvenance {
        agent: selection.agent().agent_id,
        revision: selection.revision().id,
        model: selection.agent().model.clone(),
        output_contract: prepared.response_contract()?,
    })
    .map_err(|_| invalid())
}

pub(super) async fn matching_on(
    db: &mut PgConnection,
    fence: WorkflowFence,
    conversation: Uuid,
    prepared: &PreparedAgent,
) -> AppResult<Option<AgentExchangeId>> {
    let saved = sqlx::query_as::<_, SavedExchange>(
        "SELECT id,conversation_id,agent_id,revision_id,manifest_id,run_id,job_id,provenance, \
         input_digest=sha256(convert_to($3::jsonb::text,'UTF8')) AS input_matches \
         FROM agent_history_exchanges WHERE company_id=$1 AND execution_id=$2",
    )
    .bind(fence.scope.company.as_uuid())
    .bind(fence.scope.execution.as_uuid())
    .bind(prepared.logical_input().value())
    .fetch_optional(db)
    .await?;
    let Some(saved) = saved else { return Ok(None) };
    let selection = prepared.selection();
    if saved.id != fence.scope.execution.as_uuid()
        || saved.conversation_id != conversation
        || saved.agent_id != selection.agent().agent_id.as_uuid()
        || saved.revision_id != selection.revision().id.as_uuid()
        || saved.manifest_id != selection.manifest_id().as_uuid()
        || saved.run_id != fence.scope.run.as_uuid()
        || saved.job_id != fence.scope.job.0
        || !saved.input_matches
        || saved.provenance != provenance(prepared)?
    {
        return Err(AppError::Conflict(
            "Agent exchange differs from its logical activation".into(),
        ));
    }
    Ok(Some(AgentExchangeId::new(saved.id)))
}

pub(super) async fn enqueue_on(
    db: &mut PgConnection,
    fence: WorkflowFence,
    conversation: Uuid,
    prepared: &PreparedAgent,
) -> AppResult<Option<AgentExchangeId>> {
    if let Some(saved) = matching_on(db, fence, conversation, prepared).await? {
        return Ok(Some(saved));
    }
    let available: bool = sqlx::query_scalar(
        "SELECT agent_history_storage_available($1,$2) AND agent_history_admit($1,$2)",
    )
    .bind(fence.scope.company.as_uuid())
    .bind(conversation)
    .fetch_one(&mut *db)
    .await?;
    if !available {
        return Ok(None);
    }
    let last: i64 = sqlx::query_scalar(
        "SELECT last_exchange_order FROM agent_history_conversations \
        WHERE company_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(fence.scope.company.as_uuid())
    .bind(conversation)
    .fetch_one(&mut *db)
    .await?;
    let position = last
        .checked_add(1)
        .ok_or_else(|| AppError::Conflict("Agent conversation order is exhausted".into()))?;
    let selection = prepared.selection();
    let inserted=sqlx::query_scalar::<_,Uuid>(
        "INSERT INTO agent_history_exchanges \
         (company_id,id,conversation_id,agent_id,revision_id,manifest_id,run_id,execution_id,job_id, \
          position,input_digest,provenance,access_level,content_exact,state) \
         SELECT $1,$2,$3,$4,$5,$6,$7,$2,$8,$9,sha256(convert_to($10::jsonb::text,'UTF8')),$11,run.content_level,run.content_exact,'queued' \
         FROM workflow_runs AS run JOIN workflow_executions AS execution \
           ON execution.company_id=run.company_id AND execution.run_id=run.id \
         WHERE run.company_id=$1 AND run.id=$7 AND execution.id=$2 \
           AND sha256(convert_to((execution.frozen_inputs->'context')::text,'UTF8'))=sha256(convert_to($10::jsonb::text,'UTF8')) \
         RETURNING id",
    ).bind(fence.scope.company.as_uuid()).bind(fence.scope.execution.as_uuid()).bind(conversation)
        .bind(selection.agent().agent_id.as_uuid()).bind(selection.revision().id.as_uuid())
        .bind(selection.manifest_id().as_uuid()).bind(fence.scope.run.as_uuid()).bind(fence.scope.job.0)
        .bind(position).bind(prepared.logical_input().value()).bind(provenance(prepared)?)
        .fetch_optional(db).await?.ok_or_else(||AppError::Conflict("Agent exchange input changed".into()))?;
    tracing::info!(company_id=%fence.scope.company.as_uuid(),run_id=%fence.scope.run.as_uuid(),
        exchange_id=%inserted, position, durability="staged", event="agent_history.queued", "Agent exchange queued in current transaction");
    Ok(Some(AgentExchangeId::new(inserted)))
}
