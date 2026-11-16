use super::persistence_bound::{Attempt, End, Evidence, WriteBound};
use super::*;

#[path = "completion_accounts.rs"]
mod accounts;
use crate::application::workflow::{activation::*, completion::*, lease::*};
use crate::domain::workflow::{ChoiceName, RouteSelection, TransitionTarget};
use serde_json::Value;

#[derive(sqlx::FromRow)]
struct CompletionRun {
    binding_id: Uuid,
    max_steps: i32,
}
#[derive(sqlx::FromRow)]
struct CompletionExecution {
    response_disposition: String,
    completed_at: Option<chrono::DateTime<chrono::Utc>>,
    committed_output: Option<Value>,
    committed_route: Option<String>,
    route_target: Option<String>,
    successor_execution_id: Option<Uuid>,
}

#[derive(Clone, Copy)]
enum CompletionRead {
    Observe,
    Lock,
}

impl CompletionRead {
    fn lock_clause(self) -> &'static str {
        match self {
            Self::Observe => "",
            Self::Lock => " FOR UPDATE",
        }
    }
}

async fn run_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    read: CompletionRead,
) -> AppResult<Option<CompletionRun>> {
    let query = format!(
        "SELECT binding_id,max_steps FROM workflow_runs WHERE company_id=$1 AND id=$2{}",
        read.lock_clause(),
    );
    Ok(sqlx::query_as(&query)
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .fetch_optional(db)
        .await?)
}

async fn execution_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    read: CompletionRead,
) -> AppResult<Option<CompletionExecution>> {
    let query = format!(
        "SELECT response_disposition,completed_at,committed_output,committed_route,route_target,successor_execution_id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3{}",
        read.lock_clause(),
    );
    Ok(sqlx::query_as(&query)
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .bind(scope.execution.as_uuid())
        .fetch_optional(db)
        .await?)
}

#[async_trait]
impl WorkflowCompletion for PostgresPersistence {
    async fn complete_io(
        &self,
        result: FencedWorkflowResult,
    ) -> AppResult<Option<CommittedWorkflowStep>> {
        let bound = WriteBound::start("completion", result.policy);
        let (fence, output, policy) = (result.fence, result.output.clone(), result.policy);
        let attempt = bound
            .attempt(Box::pin(async {
                let mut conn = bound.acquire(&self.pool).await?;
                let mut tx = bound.begin(&mut conn).await?;
                // Box at the activation/restore boundary to preserve stock-stack tests.
                let saved =
                    Box::pin(complete_on(&mut tx, result, self.channel_reply_notices())).await;
                bound.end(tx, saved.map(End::Commit)).await
            }))
            .await;
        match attempt {
            Attempt::Done(saved) => Ok(saved),
            Attempt::Failed(error) => Err(error),
            // complete_on replays an already completed execution without writing, so under
            // the same lock order it is exactly the evidence of the original. Otherwise our
            // attempt row tells a committed retirement from nothing written.
            Attempt::Ambiguous => {
                bound
                    .resolve(Box::pin(async {
                        let mut conn = bound.acquire(&self.pool).await?;
                        let mut tx = bound.begin(&mut conn).await?;
                        let retry = FencedWorkflowResult {
                            fence,
                            output,
                            policy,
                        };
                        let evidence = async {
                            Ok(match completed(&mut tx, fence).await? {
                                Settled::Completed => Evidence::Applied(
                                    Box::pin(complete_on(
                                        &mut tx,
                                        retry,
                                        self.channel_reply_notices(),
                                    ))
                                    .await?,
                                ),
                                Settled::Retired => Evidence::Applied(None),
                                Settled::Owned => Evidence::NotApplied,
                            })
                        }
                        .await;
                        bound.roll_back(tx, evidence).await
                    }))
                    .await
            }
        }
    }
}

pub(super) async fn complete_on(
    db: &mut PgConnection,
    result: FencedWorkflowResult,
    notices: Option<&crate::application::workflow::decisions::DecisionNotices>,
) -> AppResult<Option<CommittedWorkflowStep>> {
    // The caller's transaction already carries the server lock/statement bound.
    let scope = result.fence.scope;
    // Box purpose preparation/restore rather than adding it to the worker's debug frame.
    let accounts = Box::pin(accounts::prelock_on(db, &result)).await?;
    let run = Box::pin(run_on(db, scope, CompletionRead::Lock)).await?;
    let Some(run) = run else {
        return Ok(None);
    };
    let row = Box::pin(execution_on(db, scope, CompletionRead::Lock)).await?;
    let Some(row) = row else {
        return Ok(None);
    };
    if !job_on(db, result.fence, &row, CompletionRead::Lock).await? {
        return Ok(None);
    }
    super::persistence_bound::locks_held();
    if row.completed_at.is_some() {
        return replay(db, result, row).await;
    }
    if lease::live_window(db, result.fence, tokio::time::Instant::now())
        .await?
        .is_none()
    {
        return Ok(None);
    }
    // Box the retirement boundary to keep the debug future stack bounded.
    if !Box::pin(progression_allowed(db, result.fence)).await? {
        return Ok(None);
    }
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
    let completion =
        match prepare_result(binding.bundle(), &activation, result.output, run.max_steps) {
            Ok(completion) => completion,
            Err(cause) => {
                // Pure schema/route validation failed after the handler returned. The
                // retirement owner decides effect safety from the frozen step kind.
                recovery::retire_on(db, result.fence, lease::Retirement::Interrupted(cause))
                    .await?;
                return Ok(None);
            }
        };
    // Check the actual writer purpose under its row locks without new account acquisition.
    Box::pin(accounts::ensure_on(db, scope, accounts, &completion)).await?;
    let successor = batch_commit::complete(
        db,
        scope,
        batch_commit::CompletedStep {
            activated: &activation,
            graph: binding.bundle().compiled().graph(),
        },
        &completion,
        run.max_steps,
        completion_job::CompletionOwner::Fenced(result.fence),
        notices,
    )
    .await?;
    let batch_commit::CompletionProgress::Advanced(successor) = successor else {
        return Ok(None);
    };
    Ok(Some(committed(completion, successor)))
}

fn committed(
    completion: crate::application::workflow::batch::PureCompletion,
    successor: Option<ActivationRequest>,
) -> CommittedWorkflowStep {
    CommittedWorkflowStep {
        disposition: CommitDisposition::Committed,
        response_disposition: completion.response_disposition,
        output: completion.output,
        route: completion.route,
        target: completion.target,
        successor,
    }
}

async fn progression_allowed(db: &mut PgConnection, fence: WorkflowFence) -> AppResult<bool> {
    let scope = fence.scope;
    if token_budget::progression_on(db, scope).await? != token_budget::Progression::Open {
        Box::pin(recovery::retire_on(
            db,
            fence,
            lease::Retirement::Interrupted(
                crate::application::workflow::lease::LeaseReleaseCause::HandlerFailure,
            ),
        ))
        .await?;
        return Ok(false);
    }
    if action_uncertainty::conflicted_on(
        db,
        crate::application::workflow::actions::ActionScope {
            company: scope.company,
            run: scope.run,
            execution: scope.execution,
        },
    )
    .await?
    {
        let failure = crate::application::workflow::lease::WorkflowFailure {
            failure: crate::domain::workflow::StepFailure::new(
                crate::domain::workflow::FailureClass::Terminal,
                crate::domain::workflow::FailureCode::parse("action.evidence_conflict")
                    .map_err(|_| invalid())?,
                None,
            )
            .map_err(|_| invalid())?,
            safety: crate::domain::workflow::RetrySafety::EffectOutcomeUnknown,
        };
        recovery::retire_on(
            db,
            fence,
            lease::Retirement::Interrupted(
                crate::application::workflow::lease::LeaseReleaseCause::Classified(failure),
            ),
        )
        .await?;
        return Ok(false);
    }
    Ok(true)
}

/// What an ambiguous completion left behind.
enum Settled {
    /// The execution completed: replay classifies the saved step.
    Completed,
    /// Not completed, and our attempt is no longer processing (or is gone): the original
    /// retired it (evidence conflict, invalid result) or a third party did. Nothing is owned.
    Retired,
    /// Not completed and our attempt still processing: nothing was written.
    Owned,
}

/// Waits on the original's run and execution locks, in complete_on's order, then reads the
/// execution and our attempt row. complete_on can commit a retirement without completing.
async fn completed(db: &mut PgConnection, fence: WorkflowFence) -> AppResult<Settled> {
    let scope = fence.scope;
    lease::lock_lease_company_on(db, scope.company).await?;
    sqlx::query("SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .execute(&mut *db)
        .await?;
    let done: Option<bool> = sqlx::query_scalar("SELECT completed_at IS NOT NULL FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3 FOR UPDATE")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).fetch_optional(&mut *db).await?;
    if done == Some(true) {
        return Ok(Settled::Completed);
    }
    let processing: Option<bool> = sqlx::query_scalar("SELECT status='processing' FROM task_attempts WHERE task_id=$1 AND attempt_number=$2 AND worker_id=$3 AND execution_generation=$4")
        .bind(scope.job.0).bind(fence.attempt.0).bind(fence.worker.0).bind(fence.generation.0).fetch_optional(db).await?;
    Ok(if processing == Some(true) {
        Settled::Owned
    } else {
        Settled::Retired
    })
}

async fn job_on(
    db: &mut PgConnection,
    fence: WorkflowFence,
    row: &CompletionExecution,
    read: CompletionRead,
) -> AppResult<bool> {
    let scope = fence.scope;
    let job_query = format!(
        "SELECT workflow_execution_id FROM background_tasks WHERE company_id=$1 AND id=$2 AND workflow_execution_id=$3  AND status=$4{}",
        read.lock_clause()
    );
    let execution: Option<Uuid> = sqlx::query_scalar(&job_query)
        .bind(scope.company.as_uuid())
        .bind(scope.job.0)
        .bind(scope.execution.as_uuid())
        .bind(if row.completed_at.is_some() {
            "completed"
        } else {
            "processing"
        })
        .fetch_optional(&mut *db)
        .await?;
    let Some(_) = execution else {
        return Ok(false);
    };
    let attempt_query = format!(
        "SELECT id FROM task_attempts WHERE task_id=$1 AND attempt_number=$2 AND worker_id=$3 AND execution_generation=$4 AND status=$5{}",
        read.lock_clause()
    );
    let attempt: Option<Uuid> = sqlx::query_scalar(&attempt_query)
        .bind(scope.job.0)
        .bind(fence.attempt.0)
        .bind(fence.worker.0)
        .bind(fence.generation.0)
        .bind(if row.completed_at.is_some() {
            "completed"
        } else {
            "processing"
        })
        .fetch_optional(db)
        .await?;
    Ok(attempt.is_some())
}

async fn replay(
    db: &mut PgConnection,
    result: FencedWorkflowResult,
    row: CompletionExecution,
) -> AppResult<Option<CommittedWorkflowStep>> {
    if row.committed_output.as_ref() != Some(&result.output) {
        return Ok(None);
    }
    let route = match row.committed_route.as_deref() {
        Some("success") => RouteSelection::Success,
        Some(value) if value.starts_with("choice:") => {
            RouteSelection::Choice(ChoiceName::parse(&value[7..]).map_err(|_| invalid())?)
        }
        _ => return Err(invalid()),
    };
    let target = match row.route_target.as_deref() {
        Some("$end") => TransitionTarget::End,
        Some(step) => TransitionTarget::Step(StepId::parse(step).map_err(|_| invalid())?),
        None => return Err(invalid()),
    };
    let scope = result.fence.scope;
    let successor = if let Some(execution) = row.successor_execution_id {
        let job = sqlx::query_scalar("SELECT job.id FROM background_tasks AS job JOIN workflow_executions AS execution ON execution.company_id=job.company_id AND execution.id=job.workflow_execution_id WHERE job.company_id=$1 AND execution.run_id=$2 AND execution.id=$3 ")
            .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(execution).fetch_one(db).await?;
        Some(ActivationRequest {
            execution: ExecutionId::new(execution),
            job: WorkflowJobId(job),
            ..scope
        })
    } else {
        None
    };
    Ok(Some(CommittedWorkflowStep {
        disposition: CommitDisposition::Replayed,
        response_disposition: row.response_disposition.parse().map_err(|_| invalid())?,
        output: result.output,
        route,
        target,
        successor,
    }))
}

fn prepare_result(
    bundle: &PublishedBundle,
    activation: &crate::application::workflow::activation::ActivatedExecution,
    output: serde_json::Value,
    max_steps: i32,
) -> Result<
    crate::application::workflow::batch::PureCompletion,
    crate::application::workflow::lease::LeaseReleaseCause,
> {
    use crate::application::workflow::lease::LeaseReleaseCause;
    let limit = u64::try_from(max_steps).map_err(|_| LeaseReleaseCause::ActivationLimit)?;
    if activation.ordinal > limit {
        return Err(LeaseReleaseCause::ActivationLimit);
    }
    let completion =
        prepare_io(bundle, activation, output).map_err(|_| LeaseReleaseCause::InvalidResult)?;
    if completion.target != TransitionTarget::End && activation.ordinal >= limit {
        return Err(LeaseReleaseCause::ActivationLimit);
    }
    Ok(completion)
}
