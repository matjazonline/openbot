use super::*;
use crate::application::workflow::{activation::*, completion::*, waits::*};
use serde_json::{Value, json};

pub(super) async fn resume_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<WaitProgress> {
    if maintenance::expire_on(db, scope).await? {
        return Ok(WaitProgress::Expired(CommitDisposition::Committed));
    }
    let Some(run) = waits::lock(db, scope).await? else {
        return Ok(WaitProgress::Refused);
    };
    let Some(wait) = waits::row(db, scope).await? else {
        return Ok(WaitProgress::Refused);
    };
    match wait.state.as_str() {
        "completed" => return replay(db, scope).await,
        "cancelled" => return Ok(WaitProgress::Refused),
        "expired" => return Ok(WaitProgress::Expired(CommitDisposition::Replayed)),
        "failed" => {
            return Ok(WaitProgress::Failed {
                disposition: CommitDisposition::Replayed,
                reason: match wait.failure_code.as_deref() {
                    Some("activation_limit") => WaitFailureReason::ActivationLimit,
                    Some("invalid_output") => WaitFailureReason::InvalidOutput,
                    _ => return Err(invalid()),
                },
            });
        }
        "waiting" => {}
        _ => return Err(invalid()),
    }
    let now = waits::now(db).await?;
    if run.deadline <= now && maintenance::expire_on(db, scope).await? {
        return Ok(WaitProgress::Expired(CommitDisposition::Committed));
    }
    if run.state != "waiting" || (wait.reason == "event" && wait.deadline <= now) {
        return expire(db, scope, &run.state).await;
    }
    let event = if wait.reason == "timer" {
        if wait.deadline > now {
            return Ok(WaitProgress::Pending);
        }
        None
    } else {
        let event:Option<ReceivedEvent>=sqlx::query_as("SELECT id,payload FROM workflow_wait_events WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND event_name=$4 AND correlation=$5 ORDER BY created_at,id LIMIT 1 FOR UPDATE")
            .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(&wait.event_name).bind(&wait.correlation).fetch_optional(&mut *db).await?;
        let Some(event) = event else {
            return Ok(WaitProgress::Pending);
        };
        Some(event)
    };
    // Box the transactional continuation to keep the debug async stack shallow.
    Box::pin(advance(db, scope, run, event)).await
}

async fn advance(
    db: &mut PgConnection,
    scope: ActivationRequest,
    run: waits::WaitRun,
    event: Option<ReceivedEvent>,
) -> AppResult<WaitProgress> {
    let binding = admission_binding::read_saved_binding(
        db,
        scope.company,
        WorkflowBindingId::new(run.binding_id),
        scope.run.as_uuid(),
    )
    .await?;
    let activation = activation::activate_on(db, scope)
        .await?
        .ok_or_else(invalid)?;
    let output = event
        .as_ref()
        .map(|event| event.payload.clone())
        .unwrap_or_else(|| json!({"deadline":activation.inputs["deadline"]}));
    let completion = match prepare(binding.bundle(), &activation, output) {
        Ok(completion) => completion,
        Err(AppError::BadRequest(_)) => {
            return fail(db, scope, WaitFailureReason::InvalidOutput).await;
        }
        Err(error) => return Err(error),
    };
    let max_steps = u64::try_from(run.max_steps).map_err(|_| invalid())?;
    if activation.ordinal > max_steps
        || (activation.ordinal >= max_steps
            && matches!(completion.target, TransitionTarget::Step(_)))
    {
        return fail(db, scope, WaitFailureReason::ActivationLimit).await;
    }
    sqlx::query("UPDATE workflow_waits SET state='completed',settled_at=clock_timestamp(),consumed_event_id=$4 WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND state='waiting'")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(event.map(|event|event.id)).execute(&mut *db).await?;
    sqlx::query("UPDATE workflow_runs SET state='running',waiting_reason=NULL WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut *db).await?;
    let successor = batch_commit::complete(
        db,
        scope,
        &activation,
        &completion,
        run.max_steps,
        completion_job::CompletionOwner::Parked,
    )
    .await?;
    Ok(WaitProgress::Completed(Box::new(CommittedWorkflowStep {
        disposition: CommitDisposition::Committed,
        output: completion.output,
        route: completion.route,
        target: completion.target,
        successor,
    })))
}
#[derive(sqlx::FromRow)]
struct ReceivedEvent {
    id: Uuid,
    payload: Value,
}
async fn expire(
    db: &mut PgConnection,
    scope: ActivationRequest,
    run_state: &str,
) -> AppResult<WaitProgress> {
    sqlx::query("UPDATE workflow_waits SET state='expired',settled_at=clock_timestamp() WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND state='waiting'")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).execute(&mut *db).await?;
    if run_state == "waiting" {
        sqlx::query("UPDATE workflow_runs SET state='failed',waiting_reason=NULL WHERE company_id=$1 AND id=$2")
            .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut *db).await?;
    }
    waits::audit(db, scope, "wait_expired").await?;
    Ok(WaitProgress::Expired(CommitDisposition::Committed))
}
#[derive(sqlx::FromRow)]
struct SavedResult {
    committed_output: Value,
    route_target: String,
    successor_execution_id: Option<Uuid>,
}
async fn replay(db: &mut PgConnection, scope: ActivationRequest) -> AppResult<WaitProgress> {
    let saved=sqlx::query_as::<_,SavedResult>("SELECT committed_output,route_target,successor_execution_id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3 AND completed_at IS NOT NULL AND committed_route='success'")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).fetch_one(&mut *db).await?;
    let target = if saved.route_target == "$end" {
        TransitionTarget::End
    } else {
        TransitionTarget::Step(StepId::parse(saved.route_target).map_err(|_| invalid())?)
    };
    let successor = if let Some(id) = saved.successor_execution_id {
        let job=sqlx::query_scalar("SELECT id FROM background_tasks WHERE company_id=$1 AND workflow_execution_id=$2 AND queue_kind='workflow'")
            .bind(scope.company.as_uuid()).bind(id).fetch_one(db).await?;
        Some(ActivationRequest {
            execution: ExecutionId::new(id),
            job: WorkflowJobId(job),
            ..scope
        })
    } else {
        None
    };
    Ok(WaitProgress::Completed(Box::new(CommittedWorkflowStep {
        disposition: CommitDisposition::Replayed,
        output: saved.committed_output,
        route: RouteSelection::Success,
        target,
        successor,
    })))
}

async fn fail(
    db: &mut PgConnection,
    scope: ActivationRequest,
    reason: WaitFailureReason,
) -> AppResult<WaitProgress> {
    let code = match reason {
        WaitFailureReason::ActivationLimit => "activation_limit",
        WaitFailureReason::InvalidOutput => "invalid_output",
    };
    sqlx::query("UPDATE workflow_waits SET state='failed',failure_code=$4,settled_at=clock_timestamp() WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND state='waiting'")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(code).execute(&mut *db).await?;
    sqlx::query(
        "UPDATE workflow_runs SET state='failed',waiting_reason=NULL WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .execute(&mut *db)
    .await?;
    waits::audit(db, scope, "wait_failed").await?;
    Ok(WaitProgress::Failed {
        disposition: CommitDisposition::Committed,
        reason,
    })
}
