use super::*;
use crate::adapters::persistence::workflow::action_dispatch::SqlActionAuthority;

/// A parked call authorizes only its saved operation. This check precedes machine and
/// operation locks; reconciliation after any attempted delivery retains protocol semantics.
pub(in crate::adapters::persistence) async fn authorize_operation_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    operation: OperationId,
    actor: WorkflowActor,
) -> AppResult<()> {
    let (scope, subject) = parked_subject_on(tx, company, operation)
        .await?
        .ok_or_else(missing)?;
    let current = action_authority::prelock_on(tx, scope).await?;
    if current != Some(actor) {
        return Err(missing());
    }
    current_authority_on(tx, scope, &subject, current).await?;
    // This last proof also checks cancellation/deadline after lock waits.
    if parked_subject_on(tx, company, operation).await?.is_none() {
        return Err(conflict());
    }
    Ok(())
}

pub(super) async fn current_authority_on(
    tx: &mut Transaction<'_, Postgres>,
    scope: ActionScope,
    subject: &ApprovalSubject,
    actor: Option<WorkflowActor>,
) -> AppResult<()> {
    let authority = action_authority::load_on(
        tx,
        scope,
        subject,
        actor,
        action_authority::RunPhase::LocalAgentWait,
    )
    .await?;
    validate_run_authority(scope, subject, &authority)?;
    let current = LocalActionAuthority.lock_current(tx, &authority).await?;
    validate_resource(authority.action.request(), &current.resource)?;
    let policy = validate_current_authority(authority.action.request(), &current.policy)?;
    let activation = activation_on(tx, scope).await?;
    if policy == ActionAccessDecision::ApprovalRequired
        && decisions::approvals::approved(
            tx,
            activation,
            subject,
            authority.action.operation_digest().as_str(),
        )
        .await?
        .is_none()
    {
        return Err(missing());
    }
    knowledge_release::evaluate_on(
        tx,
        &authority.action,
        knowledge_release::MessageRecipients::Current,
    )
    .await?;
    Ok(())
}

async fn activation_on(db: &mut PgConnection, scope: ActionScope) -> AppResult<ActivationRequest> {
    let job: Uuid = sqlx::query_scalar("SELECT job.id FROM background_tasks AS job JOIN workflow_executions AS execution ON execution.company_id=job.company_id AND execution.id=job.workflow_execution_id WHERE job.company_id=$1 AND execution.run_id=$2 AND job.workflow_execution_id=$3")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).fetch_optional(db).await?.ok_or_else(missing)?;
    Ok(ActivationRequest {
        company: scope.company,
        run: scope.run,
        execution: scope.execution,
        job: crate::application::workflow::activation::WorkflowJobId(job),
    })
}

#[derive(sqlx::FromRow)]
struct ParkedCall {
    run_id: Uuid,
    execution_id: Uuid,
    invocation_id: Uuid,
    argument_digest: String,
}

pub(super) async fn parked_subject_on(
    db: &mut PgConnection,
    company: CompanyId,
    operation: OperationId,
) -> AppResult<Option<(ActionScope, ApprovalSubject)>> {
    let row = sqlx::query_as::<_, ParkedCall>("SELECT call.run_id,call.execution_id,call.invocation_id,call.argument_digest FROM workflow_local_agent_calls AS call JOIN workflow_waits AS wait ON wait.company_id=call.company_id AND wait.id=call.wait_id JOIN workflow_runs AS run ON run.company_id=call.company_id AND run.id=call.run_id JOIN workflow_executions AS execution ON execution.company_id=call.company_id AND execution.id=call.execution_id WHERE call.company_id=$1 AND call.operation_id=$2 AND call.dispatch_id IS NOT NULL AND wait.state='waiting' AND wait.reason='effect' AND wait.deadline>clock_timestamp() AND run.state='waiting' AND run.waiting_reason='effect' AND run.deadline>clock_timestamp() AND execution.completed_at IS NULL AND execution.committed_route IS NULL")
        .bind(company.as_uuid()).bind(operation.as_uuid()).fetch_optional(db).await?;
    row.map(|row| {
        Ok((
            ActionScope {
                company,
                run: RunId::new(row.run_id),
                execution: ExecutionId::new(row.execution_id),
            },
            ApprovalSubject {
                invocation: ActionInvocationId::new(row.invocation_id),
                argument_digest: serde_json::from_value(json!(row.argument_digest))
                    .map_err(|_| invalid())?,
            },
        ))
    })
    .transpose()
}
