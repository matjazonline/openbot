//! Single atomic progression writer shared by pure and fenced I/O completions.
use super::*;
use crate::application::workflow::{activation::*, batch::PureCompletion};

/// The execution being completed and the frozen graph it belongs to. The graph names the
/// step's kind, which decides whether its committed output keeps the run exact (phase 05 E3):
/// this writer is the one funnel every step output passes, so the hook lives here.
#[derive(Clone, Copy)]
pub(super) struct CompletedStep<'a> {
    pub activated: &'a ActivatedExecution,
    pub graph: &'a crate::domain::workflow::ValidatedWorkflow,
}
impl CompletedStep<'_> {
    fn output_exact(self, output: &serde_json::Value) -> AppResult<bool> {
        let definition = self
            .graph
            .definition()
            .steps
            .get(&self.activated.step)
            .ok_or_else(invalid)?;
        Ok(crate::application::workflow::exactness::step_output_exact(
            definition.step_type.as_str(),
            output,
        ))
    }
}

pub(super) enum CompletionProgress {
    Advanced(Option<ActivationRequest>),
    Parked,
    Refused,
}

pub(super) async fn complete(
    db: &mut PgConnection,
    request: ActivationRequest,
    step: CompletedStep<'_>,
    completion: &PureCompletion,
    max_steps: i32,
    owner: completion_job::CompletionOwner,
    notices: Option<&crate::application::workflow::decisions::DecisionNotices>,
) -> AppResult<CompletionProgress> {
    check_deadline(db, request).await?;
    let activated = step.activated;
    let exact = step.output_exact(&completion.output)?;
    // Box the proposal/approval boundary: every completion family reaches this writer.
    let Some(final_reply) = Box::pin(channel_reply_refusal::prepare_on(
        db, request, completion, owner, notices,
    ))
    .await?
    else {
        return Ok(CompletionProgress::Refused);
    };
    if matches!(final_reply, channel_reply_completion::PreparedFinal::Parked) {
        return Ok(CompletionProgress::Parked);
    }
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
         committed_route = $5, route_target = $6, successor_execution_id = $7, response_disposition = $8 \
         WHERE company_id = $1 AND run_id = $2 AND id = $3 AND completed_at IS NULL",
    )
    .bind(request.company.as_uuid())
    .bind(request.run.as_uuid())
    .bind(request.execution.as_uuid())
    .bind(&completion.output)
    .bind(route)
    .bind(target)
    .bind(next.map(|n| n.execution.as_uuid()))
    .bind(completion.response_disposition.as_str())
    .execute(&mut *db)
    .await?;
    if changed.rows_affected() != 1 {
        return Err(invalid());
    }
    let changed = sqlx::query(
        "UPDATE workflow_runs SET state = $3, terminal_execution_id = $4, content_exact = content_exact AND $5 \
         WHERE company_id = $1 AND id = $2 AND state IN ('queued','running') AND deadline > clock_timestamp()",
    ).bind(request.company.as_uuid()).bind(request.run.as_uuid()).bind(match completion.state { RunState::Running => "running", RunState::Succeeded => "succeeded", RunState::Failed => "failed", _ => return Err(invalid()) })
        .bind(if next.is_none() { Some(request.execution.as_uuid()) } else { None }).bind(exact).execute(&mut *db).await?;
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
    channel_reply_completion::commit_on(db, request, final_reply).await?;
    Ok(CompletionProgress::Advanced(next))
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
        "INSERT INTO background_tasks (id,company_id,channel_id,thread_id,correlation_id,workflow_execution_id) \
         SELECT $3,run.company_id,run.channel_id,run.thread_id,run.correlation_id,$4 \
         FROM workflow_runs AS run WHERE run.company_id = $1 AND run.id = $2",
    ).bind(request.company.as_uuid()).bind(request.run.as_uuid()).bind(next.job.0)
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
