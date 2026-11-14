//! Shared actual child-result preparation for observation and its transactional owner.
use super::*;
use crate::application::workflow::{batch::PureCompletion, binding::ConfiguredBinding};

pub(super) enum Read {
    Observe,
    Lock,
}

pub(super) struct Prepared {
    pub binding: std::sync::Arc<ConfiguredBinding>,
    pub activation: ActivatedExecution,
    pub outcome: Outcome,
}

pub(super) enum Outcome {
    Finished(Box<PureCompletion>),
    Next(Value),
    InvalidOutput,
    InvalidRound,
}

impl Prepared {
    pub(super) fn requires_reply(&self, max_steps: i32) -> AppResult<bool> {
        let Outcome::Finished(completion) = &self.outcome else {
            return Ok(false);
        };
        Ok(
            !wait_commit::activation_limited(self.activation.ordinal, completion, max_steps)?
                && channel_reply_completion::final_response_disposition(completion)
                    == Some(crate::domain::workflow::ResponseDisposition::Automatic),
        )
    }
}

pub(super) async fn prepare_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    run: &waits::WaitRun,
    fact: &ChildFact,
    round: ChildOrdinal,
    read: Read,
) -> AppResult<Box<Prepared>> {
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
    let activation = match read {
        Read::Observe => Box::pin(activation::preview_on(db, scope)).await?,
        Read::Lock => Box::pin(activation::activate_on(db, scope)).await?,
    }
    .ok_or_else(invalid)?;
    // Box the existing round-context phase instead of growing the parent's future.
    let outcome = Box::pin(outcome_on(db, scope, &binding, &activation, output, round)).await?;
    Ok(Box::new(Prepared {
        binding,
        activation,
        outcome,
    }))
}

async fn outcome_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    binding: &ConfiguredBinding,
    activation: &ActivatedExecution,
    output: Value,
    round: ChildOrdinal,
) -> AppResult<Outcome> {
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
    if child.compiled().validate_output(&output).is_err() {
        return Ok(Outcome::InvalidOutput);
    }
    let Some(bindings) = control.repeat.as_ref() else {
        return finished(binding, activation, output, RouteSelection::Success);
    };
    let provisional = provisional_output(output, round);
    let context = Box::pin(activation::round_context(
        db,
        scope,
        binding,
        activation,
        provisional.clone(),
    ))
    .await?;
    let rule = RepeatRule {
        bindings,
        max_iterations: control.max_iterations.ok_or_else(invalid)?,
        child: &child,
    };
    match decide_round(
        &rule,
        &context.context(),
        binding.bundle().compiled().graph().context_limits(),
        round,
    ) {
        Ok(RoundDecision::Done) => finished(binding, activation, provisional, choice("done")?),
        Ok(RoundDecision::Exhausted) => {
            finished(binding, activation, provisional, choice("exhausted")?)
        }
        Ok(RoundDecision::Next(input)) => Ok(Outcome::Next(input)),
        Err(InvalidRound) => Ok(Outcome::InvalidRound),
    }
}

fn finished(
    binding: &ConfiguredBinding,
    activation: &ActivatedExecution,
    output: Value,
    route: RouteSelection,
) -> AppResult<Outcome> {
    match child_completion(binding.bundle(), activation, output, route) {
        Ok(completion) => Ok(Outcome::Finished(Box::new(completion))),
        Err(AppError::BadRequest(_)) => Ok(Outcome::InvalidOutput),
        Err(error) => Err(error),
    }
}
