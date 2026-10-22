//! Consuming a child's terminal fact (07-D1 §D1.4). The caller locked the parent run, execution,
//! job and wait; the child is only read, through its immutable `parent_wakeup` fact, so a
//! child transition and its parent's consumption never lock each other.
use super::*;
use crate::application::workflow::tools::{CHILD_CANCELLED, CHILD_FAILED, CHILD_INVALID_INPUT};
use crate::application::workflow::{
    activation::*, budget::BudgetDisposition, children::*, compiler::StepControl,
    completion::CommitDisposition, waits::*,
};
use children::ChildStart;
use serde_json::Value;

/// The wait's child and, once it is terminal, its single wakeup fact, found by the sequence the
/// child's terminal transition allocated (a primary-key lookup).
#[derive(sqlx::FromRow)]
struct ChildFact {
    child_run_id: Uuid,
    child_ordinal: Option<i32>,
    child_tool_call_id: Option<Uuid>,
    terminal_state: Option<String>,
    terminal_execution_id: Option<Uuid>,
}

pub(super) async fn resume_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    run: waits::WaitRun,
    wait: Uuid,
) -> AppResult<WaitProgress> {
    let fact = sqlx::query_as::<_, ChildFact>(
        "SELECT wait.child_run_id, wait.child_ordinal, wait.child_tool_call_id, event.terminal_state, event.terminal_execution_id \
         FROM workflow_waits AS wait \
         JOIN workflow_runs AS child ON child.company_id = wait.company_id AND child.id = wait.child_run_id \
         LEFT JOIN workflow_run_events AS event ON event.company_id = child.company_id \
           AND event.run_id = child.id AND event.sequence = child.terminal_wakeup_sequence \
           AND event.event_kind = 'parent_wakeup' \
         WHERE wait.company_id = $1 AND wait.id = $2 AND wait.reason = 'child_run'",
    )
    .bind(scope.company.as_uuid())
    .bind(wait)
    .fetch_one(&mut *db)
    .await?;
    let Some(state) = fact.terminal_state.as_deref() else {
        return Ok(WaitProgress::Pending);
    };
    // A workflow tool call continues its agent job instead of completing a step (07-D2 §D2.4).
    if fact.child_tool_call_id.is_some() {
        return workflow_tool_calls::continue_call(db, scope, wait, fact.child_run_id).await;
    }
    let round = fact
        .child_ordinal
        .and_then(|ordinal| u32::try_from(ordinal).ok())
        .and_then(ChildOrdinal::new)
        .ok_or_else(invalid)?;
    match state {
        "succeeded" => Box::pin(succeeded(db, scope, &run, wait, &fact, round)).await,
        "failed" => child_failed(db, scope, wait, CHILD_FAILED, &fact).await,
        "cancelled" => child_failed(db, scope, wait, CHILD_CANCELLED, &fact).await,
        _ => Err(invalid()),
    }
}

async fn succeeded(
    db: &mut PgConnection,
    scope: ActivationRequest,
    run: &waits::WaitRun,
    wait: Uuid,
    fact: &ChildFact,
    round: ChildOrdinal,
) -> AppResult<WaitProgress> {
    let output: Value = sqlx::query_scalar(
        "SELECT committed_output FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3 AND completed_at IS NOT NULL",
    )
    .bind(scope.company.as_uuid())
    .bind(fact.child_run_id)
    .bind(fact.terminal_execution_id.ok_or_else(invalid)?)
    .fetch_optional(&mut *db)
    .await?
    .ok_or_else(invalid)?;
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
    let control = binding
        .bundle()
        .compiled()
        .child_control(&activation.step)
        .ok_or_else(invalid)?;
    let child = binding
        .bundle()
        .children()
        .get(&control.child_version_id)
        .cloned()
        .ok_or_else(invalid)?;
    // The child validated its own output at `$end`; a mismatch here is a defect.
    if child.compiled().validate_output(&output).is_err() {
        return wait_commit::fail(db, scope, WaitFailureReason::InvalidOutput).await;
    }
    let Some(bindings) = control.repeat.as_ref() else {
        return finish(
            db,
            scope,
            run,
            &binding,
            &activation,
            output,
            RouteSelection::Success,
        )
        .await;
    };
    let provisional = provisional_output(output, round);
    let context =
        activation::round_context(db, scope, &binding, &activation, provisional.clone()).await?;
    let rule = RepeatRule {
        bindings,
        max_iterations: control.max_iterations.ok_or_else(invalid)?,
        child: &child,
    };
    let limits = binding.bundle().compiled().graph().context_limits();
    match decide_round(&rule, &context.context(), limits, round) {
        Ok(RoundDecision::Done) => {
            finish(
                db,
                scope,
                run,
                &binding,
                &activation,
                provisional,
                choice("done")?,
            )
            .await
        }
        Ok(RoundDecision::Exhausted) => {
            let exhausted = choice("exhausted")?;
            finish(
                db,
                scope,
                run,
                &binding,
                &activation,
                provisional,
                exhausted,
            )
            .await
        }
        Ok(RoundDecision::Next(input)) => {
            let next = NextRound {
                binding: &binding,
                activation: &activation,
                control,
                input,
                round,
            };
            Box::pin(next_round(db, scope, run, wait, next)).await
        }
        Err(InvalidRound) => {
            terminal(
                db,
                scope,
                wait,
                CHILD_INVALID_INPUT,
                "next round undecidable",
            )
            .await
        }
    }
}

fn choice(name: &str) -> AppResult<RouteSelection> {
    Ok(RouteSelection::Choice(
        ChoiceName::parse(name).map_err(|_| invalid())?,
    ))
}

async fn finish(
    db: &mut PgConnection,
    scope: ActivationRequest,
    run: &waits::WaitRun,
    binding: &crate::application::workflow::binding::ConfiguredBinding,
    activation: &ActivatedExecution,
    output: Value,
    route: RouteSelection,
) -> AppResult<WaitProgress> {
    let completion = match child_completion(binding.bundle(), activation, output, route) {
        Ok(completion) => completion,
        Err(AppError::BadRequest(_)) => {
            return wait_commit::fail(db, scope, WaitFailureReason::InvalidOutput).await;
        }
        Err(error) => return Err(error),
    };
    wait_commit::commit_wake(db, scope, run, activation, completion, None).await
}

struct NextRound<'a> {
    binding: &'a crate::application::workflow::binding::ConfiguredBinding,
    activation: &'a ActivatedExecution,
    control: &'a StepControl,
    input: Value,
    round: ChildOrdinal,
}

/// Settle this round's wait and start the next round's child and wait in the same transaction;
/// the parent stays parked (07-D1 §D1.4). Its parameters are the step's frozen ones.
async fn next_round(
    db: &mut PgConnection,
    scope: ActivationRequest,
    run: &waits::WaitRun,
    wait: Uuid,
    next: NextRound<'_>,
) -> AppResult<WaitProgress> {
    let round = next.round.next().ok_or_else(invalid)?;
    if budget::repetition(db, scope, round).await? == BudgetDisposition::Exhausted {
        return budget_exhausted(db, scope).await;
    }
    let params = next
        .activation
        .inputs
        .get("params")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    // The same configuration already started round one; a refusal now is a defect.
    let child = children::child_binding(next.binding, next.control.child_version_id, params)?
        .ok_or_else(invalid)?;
    settle_wait(db, scope, wait).await?;
    let parked = children::create_on(
        db,
        scope,
        &ChildStart {
            step: &next.activation.step,
            origin: children::ChildOrigin::Round(round),
            binding: &child,
            input: &next.input,
            deadline: run.deadline,
        },
    )
    .await?;
    waits::audit_wait(db, scope, wait, "wait_completed").await?;
    waits::audit_wait(db, scope, parked.wait.as_uuid(), "wait_parked").await?;
    Ok(WaitProgress::NextRound)
}

async fn settle_wait(db: &mut PgConnection, scope: ActivationRequest, wait: Uuid) -> AppResult<()> {
    let settled = sqlx::query("UPDATE workflow_waits SET state='completed',settled_at=clock_timestamp() WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND id=$4 AND reason='child_run' AND state='waiting'")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(wait).execute(&mut *db).await?;
    if settled.rows_affected() != 1 {
        return Err(invalid());
    }
    Ok(())
}

/// The root budget refused another round: the run fails, never routed and never approved
/// (07-D1 §D1.6). The wait keeps the failure code so a replay answers the same way.
async fn budget_exhausted(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<WaitProgress> {
    let progress = wait_commit::fail(db, scope, WaitFailureReason::RootBudgetExhausted).await?;
    sqlx::query("UPDATE workflow_runs SET terminal_execution_id=$3 WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .bind(scope.execution.as_uuid())
        .execute(&mut *db)
        .await?;
    let closed = sqlx::query("UPDATE background_tasks SET status='failed',updated_at=clock_timestamp() WHERE company_id=$1 AND id=$2 AND workflow_execution_id=$3 AND queue_kind='workflow' AND status='completed'")
        .bind(scope.company.as_uuid()).bind(scope.job.0).bind(scope.execution.as_uuid()).execute(&mut *db).await?;
    if closed.rows_affected() != 1 {
        return Err(invalid());
    }
    waits::audit(db, scope, "workflow.root_budget_exhausted").await?;
    Ok(progress)
}

async fn child_failed(
    db: &mut PgConnection,
    scope: ActivationRequest,
    wait: Uuid,
    code: &'static str,
    fact: &ChildFact,
) -> AppResult<WaitProgress> {
    let diagnostic = format!(
        "child run {} {}",
        fact.child_run_id,
        fact.terminal_state.as_deref().unwrap_or("unknown")
    );
    terminal(db, scope, wait, code, &diagnostic).await
}

/// A Terminal step failure from a child outcome or an undecidable round: the wait completes and
/// the failure takes the declared error route (successor gets `null`) or fails the run, through
/// the shared final-error writer. Never a fabricated success (07-D1 §D1.4, §D1.9).
async fn terminal(
    db: &mut PgConnection,
    scope: ActivationRequest,
    wait: Uuid,
    code: &'static str,
    diagnostic: &str,
) -> AppResult<WaitProgress> {
    settle_wait(db, scope, wait).await?;
    sqlx::query("UPDATE workflow_runs SET state='running',waiting_reason=NULL WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut *db).await?;
    waits::audit(db, scope, code).await?;
    let failure = StepFailure::new(
        FailureClass::Terminal,
        FailureCode::parse(code).map_err(|_| invalid())?,
        Some(diagnostic.to_owned()),
    )
    .map_err(|_| invalid())?;
    recovery::route_parked_failure(db, scope, &failure).await?;
    wait_commit::saved(db, scope, CommitDisposition::Committed, true).await
}
