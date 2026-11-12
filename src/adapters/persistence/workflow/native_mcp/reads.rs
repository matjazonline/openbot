use super::*;
use crate::application::local_agents::mcp::{
    MAX_MACHINE_UNFINISHED, MAX_OPERATION_CALLS, MAX_RESULT_BYTES, NativeMcpState,
};

#[derive(sqlx::FromRow)]
struct Call {
    id: Uuid,
    broker_id: Uuid,
    operation_id: Uuid,
    machine_id: Uuid,
    installation_id: Uuid,
    session_id: Uuid,
    native_thread_id: String,
    native_turn_id: String,
    client_request_id: String,
    model_call_id: String,
    actor_id: Uuid,
    harness_digest: String,
    argument_digest: String,
    original_run_id: Uuid,
    deadline: DateTime<Utc>,
    retired_at: Option<DateTime<Utc>>,
    state: Option<String>,
    waiting_reason: Option<String>,
    unknown: bool,
    processing: bool,
    source_live: bool,
    content_level: Option<i16>,
    result: Option<Value>,
}

const CALL_COLUMNS: &str = "call.id,call.broker_id,call.operation_id,call.machine_id,call.installation_id,
    call.session_id,call.native_thread_id,call.native_turn_id,call.client_request_id,call.model_call_id,call.actor_id,call.harness_digest,
    call.argument_digest,call.original_run_id,call.deadline,call.retired_at,run.state,run.waiting_reason,
    public.native_mcp_has_unknown(call.company_id,call.run_id) AS unknown,COALESCE(run.content_level,call.retained_content_level)::smallint AS content_level,
    COALESCE(public.native_mcp_run_live(call.company_id,call.run_id),false) AS source_live,
    EXISTS(SELECT 1 FROM background_tasks AS job JOIN workflow_executions AS execution
        ON execution.company_id=job.company_id AND execution.id=job.workflow_execution_id
        WHERE execution.company_id=call.company_id AND execution.run_id=call.run_id AND execution.completed_at IS NULL
            AND job.status='processing' AND job.lock_expires_at>clock_timestamp()) AS processing,
    (SELECT receipt.result FROM workflow_action_receipts AS receipt
        WHERE receipt.company_id=call.company_id AND receipt.run_id=call.run_id LIMIT 1) AS result";
const CALL_FROM: &str = "FROM native_mcp_calls AS call LEFT JOIN workflow_runs AS run
    ON run.company_id=call.company_id AND run.id=call.run_id";

/// The caller holds the current operator's company/membership authority. Historical
/// access uses that operator's knowledge, never the original native owner's liveness.
pub(in crate::adapters::persistence::workflow) async fn authorize_history_on(
    db: &mut PgConnection,
    company: CompanyId,
    actor: WorkflowActor,
    run: RunId,
) -> AppResult<()> {
    let content: Option<i16> = sqlx::query_scalar(
        "SELECT content_level::smallint FROM workflow_runs WHERE company_id=$1 AND id=$2 AND native_mcp_call_id IS NOT NULL",
    ).bind(company.as_uuid()).bind(run.as_uuid()).fetch_optional(&mut *db).await?;
    if let Some(content) = content
        && super::super::knowledge::actor_level_on(db, company, Some(actor.user_id()))
            .await?
            .as_i16()
            < content
    {
        return Err(missing());
    }
    Ok(())
}

pub(super) async fn replay_on(
    db: &mut PgConnection,
    owner: &NativeMcpOwner,
    broker: &registry::Broker,
    prepared: &PreparedNativeMcp,
) -> AppResult<Option<NativeMcpResponse>> {
    let Some(call) = invocation_on(db, owner).await? else {
        return Ok(None);
    };
    check_owner(&call, owner)?;
    if call.operation_id != owner.operation.as_uuid()
        || call.native_turn_id != owner.invocation.turn_id.as_str()
        || call.broker_id != broker.id
        || call.argument_digest != prepared.argument_digest()
    {
        return Err(conflict());
    }
    response(db, owner.company, call).await.map(Some)
}

async fn invocation_on(db: &mut PgConnection, owner: &NativeMcpOwner) -> AppResult<Option<Call>> {
    sqlx::query_as::<_,Call>(&format!("SELECT {CALL_COLUMNS} {CALL_FROM} WHERE call.company_id=$1
        AND call.installation_id=$2 AND call.native_thread_id=$3 AND call.client_request_id=$4 AND call.model_call_id=$5"))
        .bind(owner.company.as_uuid()).bind(owner.installation.as_uuid()).bind(owner.invocation.thread_id.as_str())
        .bind(owner.invocation.client_request_id.as_str()).bind(owner.invocation.model_call_id.as_str())
        .fetch_optional(db).await.map_err(Into::into)
}

pub(in crate::adapters::persistence) async fn invocation_id_on(
    db: &mut PgConnection,
    owner: &NativeMcpOwner,
) -> AppResult<NativeMcpCallId> {
    let call = invocation_on(db, owner).await?.ok_or_else(missing)?;
    check_owner(&call, owner)?;
    if call.operation_id != owner.operation.as_uuid()
        || call.native_turn_id != owner.invocation.turn_id.as_str()
    {
        return Err(missing());
    }
    NativeMcpCallId::new(call.id).map_err(|_| invalid())
}

pub(in crate::adapters::persistence) async fn recover_on(
    db: &mut PgConnection,
    owner: &NativeMcpOwner,
    tool: &ResolvedAgentTool,
    arguments: &Value,
) -> AppResult<Option<NativeMcpResponse>> {
    let Some(call) = invocation_on(db, owner).await? else {
        return Ok(None);
    };
    check_owner(&call, owner)?;
    let contract: Option<Value> =
        sqlx::query_scalar("SELECT contract FROM native_mcp_brokers WHERE company_id=$1 AND id=$2")
            .bind(owner.company.as_uuid())
            .bind(call.broker_id)
            .fetch_optional(&mut *db)
            .await?;
    if call.operation_id != owner.operation.as_uuid()
        || call.native_turn_id != owner.invocation.turn_id.as_str()
        || contract != Some(serde_json::to_value(tool).map_err(|_| invalid())?)
        || call.argument_digest
            != crate::application::workflow::native_mcp::argument_digest(arguments)?
    {
        return Err(conflict());
    }
    response(db, owner.company, call).await.map(Some)
}

pub(in crate::adapters::persistence) async fn response_on(
    db: &mut PgConnection,
    company: CompanyId,
    id: NativeMcpCallId,
    owner: Option<&NativeMcpOwner>,
) -> AppResult<NativeMcpResponse> {
    let call = sqlx::query_as::<_, Call>(&format!(
        "SELECT {CALL_COLUMNS} {CALL_FROM} WHERE call.company_id=$1 AND call.id=$2"
    ))
    .bind(company.as_uuid())
    .bind(id.as_uuid())
    .fetch_optional(&mut *db)
    .await?
    .ok_or_else(missing)?;
    let owner = owner.ok_or_else(missing)?;
    check_owner(&call, owner)?;
    response(db, company, call).await
}

fn check_owner(call: &Call, owner: &NativeMcpOwner) -> AppResult<()> {
    let NativeHarnessSelection::Agent { digest, .. } = &owner.harness else {
        return Err(missing());
    };
    if call.actor_id != owner.actor.user_id()
        || call.session_id != owner.session.as_uuid()
        || call.machine_id != owner.machine.as_uuid()
        || call.installation_id != owner.installation.as_uuid()
        || call.native_thread_id != owner.invocation.thread_id.as_str()
        || call.harness_digest != digest.as_str()
    {
        return Err(missing());
    }
    if call
        .content_level
        .is_some_and(|level| owner.ceiling.as_i16() < level)
    {
        return Err(missing());
    }
    Ok(())
}

async fn response(
    db: &mut PgConnection,
    company: CompanyId,
    call: Call,
) -> AppResult<NativeMcpResponse> {
    // No owner locks or liveness checks: this is a structural historical read.
    sqlx::query("SELECT public.native_mcp_witness($1,$2)")
        .bind(company.as_uuid())
        .bind(call.id)
        .execute(db)
        .await?;
    let terminal = matches!(
        call.state.as_deref(),
        Some("succeeded" | "failed" | "cancelled")
    );
    let state = if call.retired_at.is_some() {
        NativeMcpState::Collected
    } else if call.unknown
        && (terminal
            || !call.processing
            || !call.source_live
            || call.waiting_reason.as_deref() == Some("reconciliation"))
    {
        NativeMcpState::ReconciliationRequired
    } else if !terminal {
        // A healthy in-flight marker is pending, not a failed/unknown effect. Keep
        // the native promise open until the actual worker has retired its ownership.
        NativeMcpState::Pending
    } else if let Some(result) = call.result {
        if serde_json::to_vec(&result).map_err(|_| invalid())?.len() > MAX_RESULT_BYTES {
            NativeMcpState::Failed {
                code: "result_too_large",
            }
        } else if result.get("isError").and_then(Value::as_bool) == Some(true) {
            NativeMcpState::ToolError { result }
        } else {
            NativeMcpState::Completed { result }
        }
    } else {
        match call.state.as_deref() {
            Some("queued" | "running" | "waiting") => NativeMcpState::Pending,
            Some("cancelled") => NativeMcpState::Cancelled,
            Some("failed") => NativeMcpState::Failed { code: "run_failed" },
            Some("succeeded") => NativeMcpState::Failed {
                code: "missing_mcp_receipt",
            },
            _ => return Err(invalid()),
        }
    };
    Ok(NativeMcpResponse {
        call_id: NativeMcpCallId::new(call.id).map_err(|_| invalid())?,
        run_id: RunId::new(call.original_run_id),
        deadline: call.deadline,
        origin: crate::application::local_agents::mcp::NativeMcpOrigin {
            operation_id: OperationId::new(call.operation_id).map_err(|_| invalid())?,
            invocation: NativeMcpInvocation {
                thread_id: call.native_thread_id.try_into().map_err(|_| invalid())?,
                turn_id: call.native_turn_id.try_into().map_err(|_| invalid())?,
                client_request_id: call.client_request_id.try_into().map_err(|_| invalid())?,
                model_call_id: call.model_call_id.try_into().map_err(|_| invalid())?,
            },
        },
        state,
    })
}

pub(super) async fn capacity_on(db: &mut PgConnection, owner: &NativeMcpOwner) -> AppResult<()> {
    let (operation,unfinished,unknown):(i64,i64,bool) = sqlx::query_as("SELECT
        count(*) FILTER(WHERE call.operation_id=$3),
        count(*) FILTER(WHERE call.retired_at IS NULL AND (run.state NOT IN ('succeeded','failed','cancelled') OR public.native_mcp_has_unknown(call.company_id,call.run_id))),
        COALESCE(bool_or(call.session_id=$4 AND public.native_mcp_has_unknown(call.company_id,call.run_id)),false)
        FROM native_mcp_calls AS call LEFT JOIN workflow_runs AS run ON run.company_id=call.company_id AND run.id=call.run_id
        WHERE call.company_id=$1 AND call.machine_id=$2")
        .bind(owner.company.as_uuid()).bind(owner.machine.as_uuid()).bind(owner.operation.as_uuid()).bind(owner.session.as_uuid())
        .fetch_one(db).await?;
    if unknown {
        return Err(AppError::Conflict(
            "Resolve the session's unknown MCP effect before another call".into(),
        ));
    }
    if operation >= MAX_OPERATION_CALLS as i64 || unfinished >= MAX_MACHINE_UNFINISHED as i64 {
        return Err(AppError::ServiceUnavailable(
            "Native MCP call capacity exhausted".into(),
        ));
    }
    Ok(())
}
