use super::*;
use crate::application::local_agents::workflow::{LocalRunProgress, MAX_WORKFLOW_TEXT_BYTES};
use crate::application::workflow::{completion::CommitDisposition, waits::WaitProgress};

pub(in crate::adapters::persistence::workflow) async fn prelock_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<()> {
    let owned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_local_agent_calls WHERE company_id=$1 AND run_id=$2 AND execution_id=$3)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).fetch_one(&mut *db).await?;
    if owned {
        action_authority::prelock_on(db, action_scope(scope)).await?;
    }
    Ok(())
}

/// The same recorded local-call readiness for observation and the actual owner.
/// Return the query future directly so this reader adds no async forwarding frame.
pub(in crate::adapters::persistence::workflow) fn due_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    wait: Uuid,
) -> impl std::future::Future<Output = Result<Option<bool>, sqlx::Error>> + '_ {
    sqlx::query_scalar("SELECT call.ready_at IS NOT NULL OR operation.deadline<=clock_timestamp() FROM workflow_local_agent_calls AS call JOIN local_agent_operations AS operation ON operation.company_id=call.company_id AND operation.id=call.operation_id WHERE call.company_id=$1 AND call.run_id=$2 AND call.execution_id=$3 AND call.wait_id=$4")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(wait).fetch_optional(db)
}

pub(in crate::adapters::persistence::workflow) async fn resume_on(
    tx: &mut Transaction<'_, Postgres>,
    scope: ActivationRequest,
    wait: Uuid,
) -> AppResult<Option<WaitProgress>> {
    let due = due_on(tx, scope, wait).await?;
    let Some(due) = due else {
        return Ok(None);
    };
    if !due {
        return Ok(Some(WaitProgress::Pending));
    }
    let call = saved_on(tx, action_scope(scope))
        .await?
        .ok_or_else(invalid)?;
    let action = call.action()?;
    // prelock_on already locks the account/company before wait_commit locks the run.
    let actor = WorkflowActor::authenticated(action.request().context.actor)?;
    let current =
        delivery::current_authority_on(tx, action_scope(scope), &call.subject()?, Some(actor))
            .await;
    match current {
        Ok(()) => {}
        Err(
            AppError::NotFound(_)
            | AppError::Forbidden(_)
            | AppError::BadRequest(_)
            | AppError::Conflict(_),
        ) => {
            let failure = StepFailure::new(
                FailureClass::Terminal,
                FailureCode::parse("local_agent.refused").map_err(|_| invalid())?,
                None,
            )
            .map_err(|_| invalid())?;
            return refused_on(tx, scope, wait, &failure).await.map(Some);
        }
        Err(AppError::KnowledgeRelease {
            content,
            destination,
        }) => {
            return refused_on(
                tx,
                scope,
                wait,
                &StepFailure::from_knowledge_refusal(crate::domain::workflow::KnowledgeRefusal {
                    content,
                    destination,
                }),
            )
            .await
            .map(Some);
        }
        Err(error) => return Err(error),
    }
    let expiration = crate::adapters::persistence::local_agents::protocol::EXPIRATION_OUTCOME_SQL;
    sqlx::query(&format!("UPDATE local_agent_operations AS operation SET outcome={expiration} WHERE company_id=$1 AND id=$2 AND outcome IS NULL AND deadline<=clock_timestamp()"))
        .bind(scope.company.as_uuid()).bind(call.invocation_id).execute(&mut **tx).await?;
    wait_commit::continue_delivery(tx, scope, wait)
        .await
        .map(Some)
}

async fn refused_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    wait: Uuid,
    failure: &StepFailure,
) -> AppResult<WaitProgress> {
    sqlx::query("UPDATE workflow_waits SET state='completed',settled_at=clock_timestamp() WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND id=$4 AND state='waiting'")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(wait).execute(&mut *db).await?;
    sqlx::query("UPDATE workflow_runs SET state='running',waiting_reason=NULL WHERE company_id=$1 AND id=$2 AND state='waiting'")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut *db).await?;
    let routed = recovery::route_parked_failure(db, scope, failure).await?;
    waits::audit_wait(db, scope, wait, "wait_completed").await?;
    if !routed {
        // The refusal's own typed audit is what the default ladder's case proves (D1.3).
        waits::audit(db, scope, failure.code().as_str()).await?;
        Box::pin(super::super::escalations::run_failed_on(
            db,
            scope,
            failure.code(),
        ))
        .await?;
    }
    wait_commit::saved(db, scope, CommitDisposition::Committed, true).await
}

pub(super) async fn confirm_on(
    db: &mut PgConnection,
    fence: WorkflowFence,
    wait: Uuid,
) -> AppResult<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_local_agent_calls AS call JOIN workflow_action_dispatches AS marker ON marker.company_id=call.company_id AND marker.id=call.dispatch_id JOIN task_attempts AS attempt ON attempt.task_id=marker.job_id AND attempt.attempt_number=marker.attempt_number WHERE call.company_id=$1 AND call.run_id=$2 AND call.execution_id=$3 AND call.wait_id=$4 AND marker.job_id=$5 AND marker.attempt_number=$6 AND marker.worker_id=$7 AND marker.execution_generation=$8 AND marker.effect_kind='local_agent' AND attempt.status='completed' AND attempt.stop_reason='local_agent_parked' AND attempt.worker_id=marker.worker_id AND attempt.execution_generation=marker.execution_generation)")
        .bind(fence.scope.company.as_uuid()).bind(fence.scope.run.as_uuid()).bind(fence.scope.execution.as_uuid()).bind(wait)
        .bind(fence.scope.job.0).bind(fence.attempt.0).bind(fence.worker.0).bind(fence.generation.0).fetch_one(db).await?)
}

#[derive(sqlx::FromRow)]
struct NativeResult {
    operation_id: Uuid,
    session_id: Uuid,
    thread_id: Option<String>,
    workflow_outcome: Option<String>,
    output: Option<String>,
    output_truncated: bool,
    measured_usage: Option<Value>,
    wait_state: String,
}

pub(super) async fn result_on(
    db: &mut PgConnection,
    request: &ActionDispatchRequest,
    action: &FrozenAction,
    call: &SavedCall,
) -> AppResult<LocalRunProgress> {
    if let Some(saved) = saved_result_on(db, request, action).await? {
        return Ok(saved);
    }
    let scope = request.scope();
    let result = sqlx::query_as::<_, NativeResult>("SELECT operation.id AS operation_id,call.session_id,session.thread_id,call.workflow_outcome,operation.output,operation.output_truncated,operation.measured_usage,wait.state AS wait_state FROM workflow_local_agent_calls AS call JOIN local_agent_operations AS operation ON operation.company_id=call.company_id AND operation.id=call.operation_id JOIN local_agent_sessions AS session ON session.company_id=call.company_id AND session.id=call.session_id JOIN workflow_waits AS wait ON wait.company_id=call.company_id AND wait.id=call.wait_id WHERE call.company_id=$1 AND call.run_id=$2 AND call.execution_id=$3 AND call.invocation_id=$4")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(call.invocation_id).fetch_one(&mut *db).await?;
    if result.wait_state != "completed" {
        return Err(conflict());
    }
    let outcome: OperationOutcome =
        serde_json::from_value(json!(result.workflow_outcome.as_ref().ok_or_else(invalid)?))
            .map_err(|_| invalid())?;
    let result = bounded_result(result, outcome)?;
    let result = validate_action_result(action, result)?;
    sqlx::query("INSERT INTO workflow_action_receipts(company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,effect_kind,result) VALUES($1,$2,$3,$4,$5,$6,'local_agent',$7) ON CONFLICT (company_id,invocation_id) DO NOTHING")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(call.invocation_id)
        .bind(request.subject.argument_digest.as_str()).bind(call.dispatch_id.ok_or_else(invalid)?).bind(&result).execute(db).await?;
    Ok(match outcome {
        OperationOutcome::Succeeded => LocalRunProgress::Completed(result),
        other => LocalRunProgress::Failed(other),
    })
}

pub(super) async fn saved_result_on(
    db: &mut PgConnection,
    request: &ActionDispatchRequest,
    action: &FrozenAction,
) -> AppResult<Option<LocalRunProgress>> {
    let scope = request.scope();
    let saved: Option<Value> = sqlx::query_scalar("SELECT result FROM workflow_action_receipts WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND invocation_id=$4 AND argument_digest=$5 AND effect_kind='local_agent'")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(request.subject.invocation.as_uuid())
        .bind(request.subject.argument_digest.as_str()).fetch_optional(db).await?;
    saved
        .map(|saved| {
            let result = validate_action_result(action, saved)?;
            let outcome: OperationOutcome =
                serde_json::from_value(result["status"].clone()).map_err(|_| invalid())?;
            Ok(match outcome {
                OperationOutcome::Succeeded => LocalRunProgress::Completed(result),
                other => LocalRunProgress::Failed(other),
            })
        })
        .transpose()
}

fn bounded_result(result: NativeResult, outcome: OperationOutcome) -> AppResult<Value> {
    let mut text = result.output.unwrap_or_default();
    let mut length = text.len().min(MAX_WORKFLOW_TEXT_BYTES);
    while !text.is_char_boundary(length) {
        length -= 1;
    }
    let truncated = result.output_truncated || length < text.len();
    text.truncate(length);
    if let Some(usage) = &result.measured_usage {
        let _: MeasuredUsage = serde_json::from_value(usage.clone()).map_err(|_| invalid())?;
    }
    Ok(
        json!({"status":outcome,"operation_id":result.operation_id,"session_id":result.session_id,
        "thread_id":result.thread_id,"text":text,"truncated":truncated,"usage":result.measured_usage}),
    )
}
