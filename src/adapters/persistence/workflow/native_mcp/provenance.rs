//! Historical origin restoration intentionally acquires no live native owner locks.
use super::*;
use crate::application::agent::ToolCallOrigin;
use crate::application::workflow::actions::{ActionRequest, ActionTarget};
use crate::entities::agent_capability::AgentBindingSource;

#[derive(sqlx::FromRow)]
struct FrozenOrigin {
    id: Uuid,
    actor_id: Uuid,
    contract: Value,
    workflow_id: Uuid,
    version_id: Uuid,
    workflow_binding_id: Uuid,
    input: Value,
    argument_digest: String,
    deadline: DateTime<Utc>,
    resources: Value,
    params: Value,
}

pub(in crate::adapters::persistence::workflow) async fn restore_on(
    db: &mut PgConnection,
    request: &ActionRequest,
    actor: WorkflowActor,
    bundle: &PublishedBundle,
) -> AppResult<bool> {
    let (call,private):(Option<Uuid>,bool) = sqlx::query_as("SELECT run.native_mcp_call_id,
        EXISTS(SELECT 1 FROM native_mcp_brokers AS broker WHERE broker.company_id=run.company_id AND broker.version_id=run.version_id)
        FROM workflow_runs AS run WHERE run.company_id=$1 AND run.id=$2")
        .bind(request.scope.company.as_uuid()).bind(request.scope.run.as_uuid())
        .fetch_optional(&mut *db).await?.ok_or_else(missing)?;
    let Some(call) = call else {
        if private {
            return Err(missing());
        }
        return Ok(false);
    };
    if !private || request.context.origin != ToolCallOrigin::Workflow {
        return Err(missing());
    }
    let origin=sqlx::query_as::<_,FrozenOrigin>("SELECT call.id,call.actor_id,broker.contract,broker.workflow_id,broker.version_id,
        broker.workflow_binding_id,call.input,call.argument_digest,call.deadline,run.resources,run.params
        FROM native_mcp_calls AS call JOIN native_mcp_brokers AS broker ON broker.company_id=call.company_id AND broker.id=call.broker_id
        JOIN workflow_runs AS run ON run.company_id=call.company_id AND run.id=call.run_id
        WHERE call.company_id=$1 AND call.id=$2 AND run.id=$3 AND call.retired_at IS NULL")
        .bind(request.scope.company.as_uuid()).bind(call).bind(request.scope.run.as_uuid())
        .fetch_optional(&mut *db).await?.ok_or_else(missing)?;
    sqlx::query("SELECT public.native_mcp_witness($1,$2)")
        .bind(request.scope.company.as_uuid())
        .bind(origin.id)
        .execute(&mut *db)
        .await?;
    validate(origin, request, actor, bundle)?;
    Ok(true)
}

fn validate(
    origin: FrozenOrigin,
    request: &ActionRequest,
    actor: WorkflowActor,
    bundle: &PublishedBundle,
) -> AppResult<()> {
    if actor.user_id() != origin.actor_id || request.context.actor != origin.actor_id {
        return Err(missing());
    }
    let tool: ResolvedAgentTool = serde_json::from_value(origin.contract).map_err(|_| invalid())?;
    let prepared = PreparedNativeMcp::compile(
        &crate::adapters::workflow_source::WorkflowSourceDecoder,
        NativeBrokerIdentity {
            company: request.scope.company,
            actor,
            workflow: WorkflowId::new(origin.workflow_id),
            version: VersionId::new(origin.version_id),
            binding: WorkflowBindingId::new(origin.workflow_binding_id),
        },
        &tool,
        &origin.input["arguments"],
        origin.deadline,
    )?;
    let AgentBindingSource::Mcp {
        connection,
        operation,
        ..
    } = tool.binding.source
    else {
        return Err(missing());
    };
    let expected = ActionTarget::Connection {
        resource: connection,
        resource_kind: TypeName::parse("mcp").map_err(|_| invalid())?,
    };
    if prepared.binding().bundle().content_hash() != bundle.content_hash()
        || prepared.input() != &origin.input
        || prepared.argument_digest() != origin.argument_digest
        || request.arguments != origin.input["arguments"]
        || request.operation_key != operation
        || request.target != expected
        || serde_json::to_value(&request.contract).map_err(|_| invalid())?
            != serde_json::to_value(&prepared.binding().bundle().snapshots().tools[0])
                .map_err(|_| invalid())?
        || serde_json::to_value(prepared.binding().resources()).map_err(|_| invalid())?
            != origin.resources
        || prepared.binding().params() != &origin.params
    {
        return Err(AppError::Conflict(
            "Native MCP action provenance does not match its frozen call".into(),
        ));
    }
    Ok(())
}
