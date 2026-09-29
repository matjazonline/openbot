//! Single atomic progression writer shared by pure and fenced I/O completions.
use super::*;
use crate::application::workflow::{activation::*, batch::PureCompletion};

pub(super) async fn complete(
    db: &mut PgConnection,
    request: ActivationRequest,
    activated: &ActivatedExecution,
    completion: &PureCompletion,
    max_steps: i32,
    owner: completion_job::CompletionOwner,
) -> AppResult<Option<ActivationRequest>> {
    check_deadline(db, request).await?;
    let next = match &completion.target {
        TransitionTarget::End => None,
        TransitionTarget::Step(step) => {
            if activated.ordinal >= u64::try_from(max_steps).map_err(|_| invalid())? {
                return Err(AppError::Conflict("Workflow step limit reached".into()));
            }
            Some(successor(db, request, activated.ordinal + 1, step).await?)
        }
    };
    let route = match &completion.route {
        RouteSelection::Success => "success".to_owned(),
        RouteSelection::Choice(choice) => format!("choice:{}", choice.as_str()),
        RouteSelection::FinalError => "final_error".to_owned(),
    };
    let target = match &completion.target {
        TransitionTarget::End => "$end",
        TransitionTarget::Step(step) => step.as_str(),
    };
    let changed = sqlx::query(
        "UPDATE workflow_executions SET committed_output = $4, completed_at = clock_timestamp(), \
         committed_route = $5, route_target = $6, successor_execution_id = $7 \
         WHERE company_id = $1 AND run_id = $2 AND id = $3 AND completed_at IS NULL",
    )
    .bind(request.company.as_uuid())
    .bind(request.run.as_uuid())
    .bind(request.execution.as_uuid())
    .bind(&completion.output)
    .bind(route)
    .bind(target)
    .bind(next.map(|n| n.execution.as_uuid()))
    .execute(&mut *db)
    .await?;
    if changed.rows_affected() != 1 {
        return Err(invalid());
    }
    let changed = sqlx::query(
        "UPDATE workflow_runs SET state = $3, terminal_execution_id = $4 WHERE company_id = $1 AND id = $2 \
         AND state IN ('queued','running') AND deadline > clock_timestamp()",
    ).bind(request.company.as_uuid()).bind(request.run.as_uuid()).bind(match completion.state { RunState::Running => "running", RunState::Succeeded => "succeeded", RunState::Failed => "failed", _ => return Err(invalid()) })
        .bind(if next.is_none() { Some(request.execution.as_uuid()) } else { None }).execute(&mut *db).await?;
    if changed.rows_affected() != 1 {
        return Err(AppError::Conflict(
            "Workflow deadline or state changed".into(),
        ));
    }
    sqlx::query(
        "INSERT INTO workflow_run_events (company_id,run_id,sequence,event_kind,actor_id,execution_id) \
         SELECT run.company_id,run.id, \
         (SELECT COALESCE(MAX(event.sequence),0)+1 FROM workflow_run_events AS event WHERE event.company_id = $1 AND event.run_id = $2), \
         $4,run.actor_id,$3 FROM workflow_runs AS run WHERE run.company_id = $1 AND run.id = $2",
    ).bind(request.company.as_uuid()).bind(request.run.as_uuid()).bind(request.execution.as_uuid()).bind(owner.event_kind()).execute(&mut *db).await?;
    completion_job::finish(db, request, owner).await?;
    Ok(next)
}

async fn successor(
    db: &mut PgConnection,
    request: ActivationRequest,
    ordinal: u64,
    step: &StepId,
) -> AppResult<ActivationRequest> {
    let next = ActivationRequest {
        execution: ExecutionId::new(Uuid::new_v4()),
        job: WorkflowJobId(Uuid::new_v4()),
        ..request
    };
    sqlx::query("INSERT INTO workflow_executions (company_id,run_id,id,step_id,activation) VALUES ($1,$2,$3,$4,$5)")
        .bind(request.company.as_uuid()).bind(request.run.as_uuid()).bind(next.execution.as_uuid()).bind(step.as_str())
        .bind(i64::try_from(ordinal).map_err(|_| invalid())?).execute(&mut *db).await?;
    sqlx::query(
        "INSERT INTO background_tasks (id,company_id,channel_id,thread_id,correlation_id,task_type,payload,queue_kind,workflow_execution_id) \
         SELECT $3,run.company_id,run.channel_id,run.thread_id,run.correlation_id,'workflow_execution',$4,'workflow',$5 \
         FROM workflow_runs AS run WHERE run.company_id = $1 AND run.id = $2",
    ).bind(request.company.as_uuid()).bind(request.run.as_uuid()).bind(next.job.0).bind(job_payload(next.execution))
        .bind(next.execution.as_uuid()).execute(db).await?;
    Ok(next)
}

pub(super) async fn check_deadline(
    db: &mut PgConnection,
    request: ActivationRequest,
) -> AppResult<()> {
    let valid: bool = sqlx::query_scalar(
        "SELECT deadline > clock_timestamp() FROM workflow_runs WHERE company_id = $1 AND id = $2",
    )
    .bind(request.company.as_uuid())
    .bind(request.run.as_uuid())
    .fetch_one(db)
    .await?;
    if !valid {
        return Err(AppError::Conflict("Workflow run deadline reached".into()));
    }
    Ok(())
}
