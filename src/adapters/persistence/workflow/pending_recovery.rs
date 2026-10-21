//! Deterministic failures before an I/O attempt must leave the runnable set.
use super::*;
use crate::application::workflow::activation::*;

#[derive(Clone, Copy)]
pub(super) enum PendingFailure {
    InvalidInput,
    InvalidOutput,
    ActivationLimit,
    AttemptsExhausted,
    RootBudgetExhausted,
    TokenRefused(crate::domain::workflow::TokenRefusal),
    /// A listed decision reviewer cannot be assigned or given a provable notice. Never routed:
    /// a configuration failure is not presented as a business timeout (05-D2 §7.1, 05-D4 §2).
    ReviewerUnavailable,
}
impl PendingFailure {
    fn code(self) -> &'static str {
        match self {
            Self::InvalidInput => "workflow.invalid_input",
            Self::InvalidOutput => "workflow.invalid_output",
            Self::ActivationLimit => "workflow.activation_limit",
            Self::AttemptsExhausted => "workflow.attempts_exhausted",
            Self::RootBudgetExhausted => "workflow.root_budget_exhausted",
            Self::TokenRefused(reason) => reason.failure_code(),
            Self::ReviewerUnavailable => "decision.reviewer_unavailable",
        }
    }
}

pub(super) enum Pending<T> {
    Ready(T),
    Deferred,
    Settled,
}

/// Call only after the run, execution and pending job have been locked and scoped.
/// SQL/storage failures propagate; only deterministic input validation is settled.
pub(super) async fn activate(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<Pending<ActivatedExecution>> {
    match token_budget::pending_gate(db, scope).await? {
        token_budget::Progression::Open => {}
        token_budget::Progression::AwaitingAllowance => return Ok(Pending::Deferred),
        token_budget::Progression::Refused(reason) => {
            settle(db, scope, PendingFailure::TokenRefused(reason)).await?;
            return Ok(Pending::Settled);
        }
    }
    let over: bool = sqlx::query_scalar("SELECT execution.activation > run.max_steps FROM workflow_executions AS execution JOIN workflow_runs AS run ON run.company_id=execution.company_id AND run.id=execution.run_id WHERE execution.company_id=$1 AND execution.run_id=$2 AND execution.id=$3")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).fetch_one(&mut *db).await?;
    if over {
        settle(db, scope, PendingFailure::ActivationLimit).await?;
        return Ok(Pending::Settled);
    }
    match activation::activate_on(db, scope).await {
        Ok(Some(activation)) => Ok(Pending::Ready(activation)),
        Ok(None) => Ok(Pending::Settled),
        Err(AppError::BadRequest(_)) => {
            settle(db, scope, PendingFailure::InvalidInput).await?;
            Ok(Pending::Settled)
        }
        Err(error) => Err(error),
    }
}

pub(super) async fn settle(
    db: &mut PgConnection,
    scope: ActivationRequest,
    failure: PendingFailure,
) -> AppResult<()> {
    let failure = token_budget::refusal_on(db, scope)
        .await?
        .map(PendingFailure::TokenRefused)
        .unwrap_or(failure);
    // Pre-activation failures cannot fabricate frozen inputs. Already activated
    // failures can take their immutable error edge through the one progression writer.
    let action_safe: Option<bool> = sqlx::query_scalar("SELECT workflow_action_retry_safe($1,$2)")
        .bind(scope.company.as_uuid())
        .bind(scope.execution.as_uuid())
        .fetch_one(&mut *db)
        .await?;
    let reconcile = match action_safe {
        Some(safe) => !safe,
        None => {
            matches!(failure, PendingFailure::AttemptsExhausted)
                && unknown_effect(db, scope).await?
        }
    };
    let routed = if !reconcile
        && matches!(
            failure,
            PendingFailure::InvalidInput
                | PendingFailure::InvalidOutput
                | PendingFailure::AttemptsExhausted
        ) {
        route(db, scope).await?
    } else {
        false
    };
    if !routed {
        sqlx::query("UPDATE workflow_runs SET state=$4,waiting_reason=$5,terminal_execution_id=CASE WHEN $4='failed' THEN $3 ELSE NULL END WHERE company_id=$1 AND id=$2 AND state IN ('queued','running')")
            .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(if reconcile {"waiting"} else {"failed"}).bind(if reconcile {Some("reconciliation")} else {None}).execute(&mut *db).await?;
    }
    let changed = sqlx::query("UPDATE background_tasks SET status='failed',updated_at=clock_timestamp() WHERE company_id=$1 AND id=$2 AND workflow_execution_id=$3 AND queue_kind='workflow' AND status='pending' AND worker_id IS NULL AND execution_generation IS NULL AND lock_expires_at IS NULL")
        .bind(scope.company.as_uuid()).bind(scope.job.0).bind(scope.execution.as_uuid()).execute(&mut *db).await?;
    if changed.rows_affected() != 1 {
        return Err(invalid());
    }
    // Pure/activation validation has no worker attempt. Do not invent an attempt
    // or alter the already-consumed allowance when retiring exhausted pending work.
    waits::audit(db, scope, failure.code()).await?;
    if reconcile {
        let code =
            crate::domain::workflow::FailureCode::parse(failure.code()).map_err(|_| invalid())?;
        action_uncertainty::record(db, scope.company, scope.run, Some(scope.execution), &code)
            .await?;
    }
    Ok(())
}

async fn route(db: &mut PgConnection, scope: ActivationRequest) -> AppResult<bool> {
    let activated: bool = sqlx::query_scalar("SELECT activated_at IS NOT NULL FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).fetch_one(&mut *db).await?;
    if !activated {
        return Ok(false);
    }
    recovery::route_pending_failure(db, scope).await
}

async fn unknown_effect(db: &mut PgConnection, scope: ActivationRequest) -> AppResult<bool> {
    let binding_id: Uuid =
        sqlx::query_scalar("SELECT binding_id FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(scope.company.as_uuid())
            .bind(scope.run.as_uuid())
            .fetch_one(&mut *db)
            .await?;
    let binding = admission_binding::read_saved_binding(
        db,
        scope.company,
        WorkflowBindingId::new(binding_id),
        scope.run.as_uuid(),
    )
    .await?;
    let step: String = sqlx::query_scalar(
        "SELECT step_id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .fetch_one(db)
    .await?;
    let step = StepId::parse(step).map_err(|_| invalid())?;
    let definition = binding
        .bundle()
        .compiled()
        .graph()
        .definition()
        .steps
        .get(&step)
        .ok_or_else(invalid)?;
    // Called only when the execution has no marker: an action-owned kind cannot then
    // have sent anything (see `retry_safe_without_marker`).
    Ok(
        !crate::application::workflow::actions::retry_safe_without_marker(
            definition.step_type.as_str(),
        ),
    )
}

pub(super) async fn prepare_pure(
    db: &mut PgConnection,
    scope: ActivationRequest,
    bundle: &PublishedBundle,
    max_steps: i32,
) -> AppResult<
    Pending<(
        ActivatedExecution,
        crate::application::workflow::batch::PureCompletion,
    )>,
> {
    let activated = match activate(db, scope).await? {
        Pending::Ready(activated) => activated,
        Pending::Deferred => return Ok(Pending::Deferred),
        Pending::Settled => return Ok(Pending::Settled),
    };
    let completion = match crate::application::workflow::batch::execute_pure(bundle, &activated) {
        Ok(Some(completion)) => completion,
        // This call is pure validation: no database/provider error can enter here.
        Err(_) => {
            settle(db, scope, PendingFailure::InvalidOutput).await?;
            return Ok(Pending::Settled);
        }
        Ok(None) => return Err(invalid()),
    };
    if completion.target != TransitionTarget::End
        && activated.ordinal >= u64::try_from(max_steps).map_err(|_| invalid())?
    {
        settle(db, scope, PendingFailure::ActivationLimit).await?;
        return Ok(Pending::Settled);
    }
    Ok(Pending::Ready((activated, completion)))
}
