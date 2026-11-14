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

#[path = "child_completion_plan.rs"]
mod completion_plan;

/// The wait's child and, once it is terminal, its single wakeup fact, found by the sequence the
/// child's terminal transition allocated (a primary-key lookup).
#[derive(sqlx::FromRow)]
pub(super) struct ChildFact {
    child_run_id: Uuid,
    content_level: i16,
    content_exact: bool,
    child_ordinal: Option<i32>,
    child_tool_call_id: Option<Uuid>,
    terminal_state: Option<String>,
    terminal_execution_id: Option<Uuid>,
}

impl ChildFact {
    pub(super) fn terminal_state(&self) -> Option<&str> {
        self.terminal_state.as_deref()
    }
}

pub(super) enum ResumePurpose {
    WithoutReplyAccounts,
    Continue,
}

pub(super) fn fact_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    wait: Uuid,
) -> impl std::future::Future<Output = Result<ChildFact, sqlx::Error>> + '_ {
    sqlx::query_as::<_, ChildFact>(
        "SELECT wait.child_run_id, child.content_level::smallint AS content_level, child.content_exact, wait.child_ordinal, wait.child_tool_call_id, event.terminal_state, event.terminal_execution_id \
         FROM workflow_waits AS wait \
         JOIN workflow_runs AS child ON child.company_id = wait.company_id AND child.id = wait.child_run_id \
         LEFT JOIN workflow_run_events AS event ON event.company_id = child.company_id \
           AND event.run_id = child.id AND event.sequence = child.terminal_wakeup_sequence \
           AND event.event_kind = 'parent_wakeup' \
         WHERE wait.company_id = $1 AND wait.id = $2 AND wait.reason = 'child_run'",
    )
    .bind(scope.company.as_uuid())
    .bind(wait)
    .fetch_one(db)
}

pub(super) async fn resume_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    run: waits::WaitRun,
    wait: Uuid,
    purpose: ResumePurpose,
    notices: Option<&crate::application::workflow::decisions::DecisionNotices>,
) -> AppResult<WaitProgress> {
    let fact = fact_on(db, scope, wait).await?;
    let Some(state) = fact.terminal_state() else {
        return Ok(WaitProgress::Pending);
    };
    // A workflow tool call continues its agent job instead of completing a step (07-D2 §D2.4).
    if fact.child_tool_call_id.is_some() {
        return workflow_tool_calls::continue_call(db, scope, wait, fact.child_run_id).await;
    }
    let level = crate::entities::access_level::AccessLevel::try_from(fact.content_level)
        .map_err(|_| invalid())?;
    // An inexact child lowers its parent with its result (E3).
    super::knowledge::absorb_content_on(db, scope.company, scope.run, level, fact.content_exact)
        .await?;
    let round = round(&fact)?;
    match state {
        "succeeded" => {
            Box::pin(succeeded(
                db,
                scope,
                &run,
                wait,
                &fact,
                ChildResume {
                    round,
                    purpose,
                    notices,
                },
            ))
            .await
        }
        "failed" => child_failed(db, scope, wait, CHILD_FAILED, &fact).await,
        "cancelled" => child_failed(db, scope, wait, CHILD_CANCELLED, &fact).await,
        _ => Err(invalid()),
    }
}

pub(super) enum ReplyUse {
    None,
    Required,
}

pub(super) async fn reply_use_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    run: &waits::WaitRun,
    wait: Uuid,
) -> AppResult<ReplyUse> {
    let fact = fact_on(db, scope, wait).await?;
    let Some(state) = fact.terminal_state() else {
        return Ok(ReplyUse::None);
    };
    // A received tool result only reopens its parked agent job. Admission of
    // another provider effect remains with the next fenced owner.
    if fact.child_tool_call_id.is_some() || matches!(state, "failed" | "cancelled") {
        return Ok(ReplyUse::None);
    }
    if state != "succeeded" {
        return Err(invalid());
    }
    // Share the actual child-output/round/completion preparation; never run a second evaluator.
    let prepared = Box::pin(completion_plan::prepare_on(
        db,
        scope,
        run,
        &fact,
        round(&fact)?,
        completion_plan::Read::Observe,
    ))
    .await?;
    Ok(if prepared.requires_reply(run.max_steps)? {
        ReplyUse::Required
    } else {
        ReplyUse::None
    })
}

fn round(fact: &ChildFact) -> AppResult<ChildOrdinal> {
    fact.child_ordinal
        .and_then(|ordinal| u32::try_from(ordinal).ok())
        .and_then(ChildOrdinal::new)
        .ok_or_else(invalid)
}

struct ChildResume<'a> {
    round: ChildOrdinal,
    purpose: ResumePurpose,
    notices: Option<&'a crate::application::workflow::decisions::DecisionNotices>,
}

async fn succeeded(
    db: &mut PgConnection,
    scope: ActivationRequest,
    run: &waits::WaitRun,
    wait: Uuid,
    fact: &ChildFact,
    resume: ChildResume<'_>,
) -> AppResult<WaitProgress> {
    // Box shared preparation and its owned binding so the stock async stack stays shallow.
    let prepared = Box::pin(completion_plan::prepare_on(
        db,
        scope,
        run,
        fact,
        resume.round,
        completion_plan::Read::Lock,
    ))
    .await?;
    if matches!(resume.purpose, ResumePurpose::WithoutReplyAccounts)
        && prepared.requires_reply(run.max_steps)?
    {
        return Err(AppError::Conflict(
            "Workflow child purpose changed; retry".into(),
        ));
    }
    let completion_plan::Prepared {
        binding,
        activation,
        outcome,
    } = *prepared;
    match outcome {
        completion_plan::Outcome::InvalidOutput => {
            wait_commit::fail(db, scope, WaitFailureReason::InvalidOutput).await
        }
        completion_plan::Outcome::InvalidRound => {
            terminal(
                db,
                scope,
                wait,
                CHILD_INVALID_INPUT,
                "next round undecidable",
            )
            .await
        }
        completion_plan::Outcome::Finished(completion) => {
            wait_commit::commit_wake(
                db,
                scope,
                run,
                super::batch_commit::CompletedStep {
                    activated: &activation,
                    graph: binding.bundle().compiled().graph(),
                },
                *completion,
                None,
                resume.notices,
            )
            .await
        }
        completion_plan::Outcome::Next(input) => {
            let next = NextRound::new(&binding, &activation, input, resume.round)?;
            Box::pin(next_round(db, scope, run, wait, next)).await
        }
    }
}

fn choice(name: &str) -> AppResult<RouteSelection> {
    Ok(RouteSelection::Choice(
        ChoiceName::parse(name).map_err(|_| invalid())?,
    ))
}

struct NextRound<'a> {
    binding: &'a crate::application::workflow::binding::ConfiguredBinding,
    activation: &'a ActivatedExecution,
    control: &'a StepControl,
    input: Value,
    round: ChildOrdinal,
}

impl<'a> NextRound<'a> {
    fn new(
        binding: &'a crate::application::workflow::binding::ConfiguredBinding,
        activation: &'a ActivatedExecution,
        input: Value,
        round: ChildOrdinal,
    ) -> AppResult<Self> {
        let control = binding
            .bundle()
            .compiled()
            .child_control(&activation.step)
            .ok_or_else(invalid)?;
        Ok(Self {
            binding,
            activation,
            control,
            input,
            round,
        })
    }
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
    let closed = sqlx::query("UPDATE background_tasks SET status='failed',updated_at=clock_timestamp() WHERE company_id=$1 AND id=$2 AND workflow_execution_id=$3  AND status='completed'")
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
    // The child never escalates itself; the parent's propagation decides (D1.3). Unrouted, the
    // parent is failed here, and a failed root starts the default ladder once.
    if !recovery::route_parked_failure(db, scope, &failure).await? {
        Box::pin(super::escalations::run_failed_on(db, scope, failure.code())).await?;
    }
    wait_commit::saved(db, scope, CommitDisposition::Committed, true).await
}
