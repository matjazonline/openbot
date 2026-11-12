use super::*;
use crate::application::local_agents::protocol::{LogicalInvocationKey, OperationAdmission};
use crate::application::local_agents::workflow::LocalRunProgress;
use crate::application::workflow::decisions::DecisionNotices;

pub(super) async fn run_on(
    tx: &mut Transaction<'_, Postgres>,
    claim: &ClaimedWorkflow,
    policy: LeasePolicy,
    dispatch: &Dispatch,
    notices: Option<&DecisionNotices>,
) -> AppResult<LocalRunProgress> {
    let scope = action_scope(claim.fence.scope);
    // A repeated handler call after COMMIT can prove the exact closed attempt's park.
    // It cannot manufacture authority for another attempt or turn a wake into a result.
    if let Some(call) = saved_on(tx, scope).await?
        && let Some(wait) = call.wait_id
        && continuation::confirm_on(tx, claim.fence, wait).await?
    {
        return Ok(LocalRunProgress::Suspended(WaitId::new(wait)));
    }
    let call = preparation::prepare_on(tx, claim, policy).await?;
    let request = ActionDispatchRequest {
        subject: call.subject()?,
        fence: claim.fence,
        lease: policy,
    };
    let authorized = dispatch
        .authorize_on(tx, &request, policy.attempt_bound())
        .await?;
    token_budget::require_progression(tx, claim.fence.scope).await?;
    if authorized.approval_required {
        return decisions::approvals::park(tx, &request, &authorized.action, notices)
            .await?
            .map(LocalRunProgress::Suspended)
            .ok_or_else(|| AppError::Forbidden("Local execution requires approval".into()));
    }
    if action_uncertainty::conflicted_on(tx, scope).await? {
        return Err(conflict());
    }
    if call.dispatch_id.is_some() {
        let result = continuation::result_on(tx, &request, &authorized.action, &call).await?;
        lease::live_window(tx, claim.fence, tokio::time::Instant::now())
            .await?
            .ok_or_else(conflict)?;
        return Ok(result);
    }
    knowledge_release::evaluate_on(
        tx,
        &authorized.action,
        knowledge_release::MessageRecipients::Current,
    )
    .await?;
    let args: FrozenLocalRun =
        serde_json::from_value(authorized.action.request().arguments.clone())
            .map_err(|_| invalid())?;
    let session = session_on(tx, scope, call.invocation_id, &args).await?;
    let admission = operation_admission(
        scope,
        call.invocation_id,
        authorized.action.request().context.actor,
        args,
        &session,
    )?;
    let now = waits::now(tx).await?;
    crate::application::local_agents::bounds::validate_deadline(
        now,
        admission.operation.deadline(),
    )
    .map_err(|_| conflict())?;
    shared::validate_capacity_on(
        tx,
        scope.company,
        session.selection.machine_id,
        admission.operation.command(),
    )
    .await?;
    let (marker, wait) = (Uuid::new_v4(), Uuid::new_v4());
    Dispatch::insert(
        tx,
        &request,
        marker,
        "local_agent",
        authorized.current_policy,
    )
    .await?;
    crate::adapters::persistence::local_agents::protocol::commands::insert_admission_on(
        tx,
        &admission,
        session.installation_id,
        OperationOrigin::Workflow,
    )
    .await?;
    park_on(tx, &request, &session, &admission, marker, wait).await?;
    Ok(LocalRunProgress::Suspended(WaitId::new(wait)))
}

fn operation_admission(
    scope: ActionScope,
    invocation: Uuid,
    actor: Uuid,
    args: FrozenLocalRun,
    session: &LocalAgentSession,
) -> AppResult<OperationAdmission> {
    let command = match &session.thread_id {
        Some(thread) => LocalCommand::Send {
            session_id: session.id,
            thread_id: thread.clone(),
            prompt: args.prompt,
        },
        None => LocalCommand::Create {
            session_id: session.id,
            selection: args.selection,
            harness: args.harness,
            prompt: args.prompt,
        },
    };
    let operation = LocalAgentOperation::queued(
        OperationId::new(invocation).map_err(|_| invalid())?,
        command,
        args.deadline,
    )
    .map_err(|_| contract::invalid())?;
    Ok(OperationAdmission {
        company_id: scope.company.as_uuid(),
        actor_id: actor,
        machine_id: session.selection.machine_id,
        logical_key: LogicalInvocationKey::parse(format!(
            "workflow:{}:{}",
            scope.run.as_uuid(),
            scope.execution.as_uuid()
        ))?,
        operation,
    })
}

async fn session_on(
    db: &mut PgConnection,
    scope: ActionScope,
    invocation: Uuid,
    args: &FrozenLocalRun,
) -> AppResult<LocalAgentSession> {
    if let Some(id) = args.session_id {
        let session = shared::session_on(db, scope.company, id)
            .await?
            .ok_or_else(missing)?;
        // prepare_on has authorized the original run before machine locking. Recheck
        // immutable profile/selection here without acquiring a later workflow lock.
        if session.profile_id != args.profile_id
            || session.selection != args.selection
            || session.harness != args.harness
            || session.thread_id.is_none()
            || !matches!(session.association, SessionAssociation::Workflow { .. })
        {
            return Err(missing());
        }
        return Ok(session);
    }
    let installation: Uuid = sqlx::query_scalar("SELECT installation_id FROM local_machines WHERE company_id=$1 AND id=$2 AND state='enabled'")
        .bind(scope.company.as_uuid()).bind(args.selection.machine_id.as_uuid()).fetch_optional(&mut *db).await?.ok_or_else(missing)?;
    let session = LocalAgentSession {
        id: SessionId::new(invocation).map_err(|_| invalid())?,
        profile_id: args.profile_id,
        company_id: scope.company.as_uuid(),
        installation_id: InstallationId::new(installation).map_err(|_| invalid())?,
        selection: args.selection.clone(),
        harness: args.harness.clone(),
        association: SessionAssociation::Workflow {
            run_id: scope.run.as_uuid(),
        },
        thread_id: None,
    };
    shared::insert_session_on(db, &session).await?;
    Ok(session)
}

async fn park_on(
    db: &mut PgConnection,
    request: &ActionDispatchRequest,
    session: &LocalAgentSession,
    admission: &OperationAdmission,
    marker: Uuid,
    wait: Uuid,
) -> AppResult<()> {
    let scope = request.fence.scope;
    let fence = request.fence;
    let linked = sqlx::query("UPDATE workflow_local_agent_calls SET machine_id=$4,installation_id=$5,session_id=$6,operation_id=$7,wait_id=$8,dispatch_id=$9,fence_expires_at=job.lock_expires_at FROM background_tasks AS job WHERE workflow_local_agent_calls.company_id=$1 AND workflow_local_agent_calls.run_id=$2 AND workflow_local_agent_calls.execution_id=$3 AND operation_id IS NULL AND job.company_id=$1 AND job.id=$10 AND job.continuation_count<workflow_job_continuation_limit()")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(session.selection.machine_id.as_uuid())
        .bind(session.installation_id.as_uuid()).bind(session.id.as_uuid()).bind(admission.operation.id().as_uuid()).bind(wait).bind(marker).bind(scope.job.0)
        .execute(&mut *db).await?;
    sqlx::query("INSERT INTO workflow_waits(company_id,run_id,execution_id,id,reason,deadline) VALUES($1,$2,$3,$4,'effect',$5)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(wait).bind(admission.operation.deadline()).execute(&mut *db).await?;
    let attempt = sqlx::query("UPDATE task_attempts SET status='completed',stop_reason='local_agent_parked',finished_at=clock_timestamp() WHERE task_id=$1 AND attempt_number=$2 AND worker_id=$3 AND execution_generation=$4 AND status='processing'")
        .bind(scope.job.0).bind(fence.attempt.0).bind(fence.worker.0).bind(fence.generation.0).execute(&mut *db).await?;
    let job = sqlx::query("UPDATE background_tasks SET status='completed',updated_at=clock_timestamp(),worker_id=NULL,execution_generation=NULL,locked_at=NULL,lock_expires_at=NULL WHERE company_id=$1 AND id=$2 AND workflow_execution_id=$3 AND status='processing' AND worker_id=$4 AND execution_generation=$5 AND attempt_ordinal=$6 AND lock_expires_at>clock_timestamp()")
        .bind(scope.company.as_uuid()).bind(scope.job.0).bind(scope.execution.as_uuid()).bind(fence.worker.0).bind(fence.generation.0).bind(fence.attempt.0).execute(&mut *db).await?;
    if linked.rows_affected() != 1 || attempt.rows_affected() != 1 || job.rows_affected() != 1 {
        return Err(conflict());
    }
    sqlx::query("UPDATE workflow_runs SET state='waiting',waiting_reason='effect' WHERE company_id=$1 AND id=$2 AND state IN ('queued','running')")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut *db).await?;
    waits::audit_wait(db, scope, wait, "wait_parked").await
}
