//! Plan only the final-response accounts consumed by this actual wait continuation.
use super::*;
use crate::application::workflow::{actions::ActionScope, batch::PureCompletion};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Accounts {
    Company,
    Reply,
}

async fn purpose_on(db: &mut PgConnection, scope: ActivationRequest) -> AppResult<Accounts> {
    if !channel_reply_completion::pending_reply_on(db, scope).await? {
        return Ok(Accounts::Company);
    }
    let Some(run) = Box::pin(waits::read_run_on(db, scope, waits::WaitRead::Observe)).await? else {
        return Ok(Accounts::Company);
    };
    let Some(wait) = Box::pin(waits::read_row_on(
        db,
        scope,
        waits::WaitSelection::Execution,
        waits::WaitRead::Observe,
    ))
    .await?
    else {
        return Ok(Accounts::Company);
    };
    Box::pin(wait_purpose_on(db, scope, &run, &wait)).await
}

async fn wait_purpose_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    run: &waits::WaitRun,
    wait: &waits::WaitRow,
) -> AppResult<Accounts> {
    let now = waits::now(db).await?;
    if run.state != "waiting"
        || run.deadline <= now
        || wait.state != "waiting"
        || matches!(wait.reason.as_str(), "decision" | "agent_turn")
    {
        return Ok(Accounts::Company);
    }
    let wake = match wait.reason.as_str() {
        "timer" if wait.deadline > now => return Ok(Accounts::Company),
        "timer" => Wake::Timer,
        "event" => {
            let before = (wait.deadline <= now).then_some(wait.deadline);
            let Some(event) =
                Box::pin(event_on(db, scope, wait, before, waits::WaitRead::Observe)).await?
            else {
                return Ok(Accounts::Company);
            };
            Wake::Event(event)
        }
        "child_run" => {
            return Ok(
                match Box::pin(child_waits::reply_use_on(db, scope, run, wait.id)).await? {
                    child_waits::ReplyUse::None => Accounts::Company,
                    child_waits::ReplyUse::Required => Accounts::Reply,
                },
            );
        }
        "effect" => return Box::pin(effect_purpose_on(db, scope, run, wait.id)).await,
        // Unknown families retain authority until their actual owners are resolved.
        _ => return Ok(Accounts::Reply),
    };
    // Box actual preparation so the read/evaluation phase stays off the
    // observation owner's stack.
    Box::pin(wake_purpose_on(db, scope, run, wake)).await
}

async fn effect_purpose_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    run: &waits::WaitRun,
    wait: Uuid,
) -> AppResult<Accounts> {
    // Match the actual owner's family order and share its exact result readers.
    // A continuation reopens saved work; its next provider owner still authorizes
    // the effect, and only prepared step output can create a final reply here.
    let wake = if let Some(memory) =
        Box::pin(memory_steps::effect_resume_on(db, scope, wait)).await?
    {
        match memory {
            memory_steps::MemoryResume::Delivered(output) => Wake::Effect(output),
            _ => return Ok(Accounts::Company),
        }
    } else {
        use crate::application::workflow::message::MessageOrigin;
        if local_agents::due_on(db, scope, wait).await?.is_some()
            || message::receipts::wait_origin_on(db, scope, wait, MessageOrigin::ModelTool).await?
        {
            return Ok(Accounts::Company);
        }
        if !message::receipts::wait_origin_on(db, scope, wait, MessageOrigin::Step).await? {
            return Ok(Accounts::Reply);
        }
        match Box::pin(message::receipts::observe_effect_on(db, scope, wait)).await? {
            message::receipts::EffectResume::Accepted(result) => Wake::Effect(result),
            _ => return Ok(Accounts::Company),
        }
    };
    Box::pin(wake_purpose_on(db, scope, run, wake)).await
}

async fn wake_purpose_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    run: &waits::WaitRun,
    wake: Wake,
) -> AppResult<Accounts> {
    let binding = admission_binding::read_saved_binding(
        db,
        scope.company,
        WorkflowBindingId::new(run.binding_id),
        scope.run.as_uuid(),
    )
    .await?;
    let Some(activation) = Box::pin(activation::preview_on(db, scope)).await? else {
        return Ok(Accounts::Company);
    };
    if activation.ordinal > u64::try_from(run.max_steps).map_err(|_| invalid())? {
        return Ok(Accounts::Company);
    }
    let (prepared, _) = prepare_wake(binding.bundle(), &activation, wake);
    Ok(match prepared {
        Ok(completion)
            if needs_reply(&completion)
                && !activation_limited(activation.ordinal, &completion, run.max_steps)? =>
        {
            Accounts::Reply
        }
        // Invalid output is retired by the actual owner; it creates no final reply.
        _ => Accounts::Company,
    })
}

pub(super) async fn prelock_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<Accounts> {
    let observed = Box::pin(purpose_on(db, scope)).await?;
    let owned = if observed == Accounts::Reply {
        let reply = action_authority::prelock_reply_on(
            db,
            ActionScope {
                company: scope.company,
                run: scope.run,
                execution: scope.execution,
            },
            None,
        )
        .await;
        match reply {
            Ok(_) => Accounts::Reply,
            Err(error) => {
                if matches!(&error, AppError::NotFound(_))
                    && Box::pin(purpose_on(db, scope)).await? == Accounts::Company
                {
                    Accounts::Company
                } else {
                    return Err(error);
                }
            }
        }
    } else {
        Accounts::Company
    };
    lease::lock_lease_company_on(db, scope.company).await?;
    if owned == Accounts::Company && Box::pin(purpose_on(db, scope)).await? == Accounts::Reply {
        return Err(changed());
    }
    Ok(owned)
}

pub(super) async fn ensure_wait_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    run: &waits::WaitRun,
    wait: &waits::WaitRow,
    owned: Accounts,
) -> AppResult<()> {
    if owned == Accounts::Company
        && channel_reply_completion::pending_reply_on(db, scope).await?
        && Box::pin(wait_purpose_on(db, scope, run, wait)).await? == Accounts::Reply
    {
        return Err(changed());
    }
    Ok(())
}

pub(super) async fn ensure_completion_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    owned: Accounts,
    completion: &PureCompletion,
    activation: &crate::application::workflow::activation::ActivatedExecution,
    max_steps: i32,
) -> AppResult<()> {
    if owned == Accounts::Company
        && needs_reply(completion)
        && !activation_limited(activation.ordinal, completion, max_steps)?
        && channel_reply_completion::pending_reply_on(db, scope).await?
    {
        return Err(changed());
    }
    Ok(())
}

fn needs_reply(completion: &PureCompletion) -> bool {
    channel_reply_completion::final_response_disposition(completion)
        == Some(crate::domain::workflow::ResponseDisposition::Automatic)
}

fn changed() -> AppError {
    AppError::Conflict("Workflow wait purpose changed; retry".into())
}

pub(super) async fn event_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    wait: &waits::WaitRow,
    recorded_before: Option<chrono::DateTime<chrono::Utc>>,
    read: waits::WaitRead,
) -> AppResult<Option<ReceivedEvent>> {
    const SELECT: &str = "SELECT id,payload FROM workflow_wait_events WHERE company_id=$1 AND run_id=$2 AND event_name=$3 AND correlation=$4 AND consumed_execution_id IS NULL AND ($5::timestamptz IS NULL OR created_at<=$5) ORDER BY created_at,id LIMIT 1";
    let query = match read {
        waits::WaitRead::Observe => SELECT.to_owned(),
        waits::WaitRead::Lock => format!("{SELECT} FOR UPDATE"),
    };
    Ok(sqlx::query_as(&query)
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .bind(&wait.event_name)
        .bind(&wait.correlation)
        .bind(recorded_before)
        .fetch_optional(db)
        .await?)
}

pub(super) fn prepare_wake(
    bundle: &crate::application::workflow::publication::PublishedBundle,
    activation: &crate::application::workflow::activation::ActivatedExecution,
    wake: Wake,
) -> (AppResult<PureCompletion>, Option<Uuid>) {
    match wake {
        Wake::Timer => (
            prepare(
                bundle,
                activation,
                json!({"deadline":activation.inputs["deadline"]}),
            ),
            None,
        ),
        Wake::Event(event) => {
            let question = bundle.compiled().graph().definition().steps[&activation.step]
                .step_type
                .as_str()
                == "message.ask";
            let prepared = if question {
                prepare_io(bundle, activation, event.payload)
            } else {
                prepare(bundle, activation, event.payload)
            };
            (prepared, Some(event.id))
        }
        Wake::Effect(result) => (prepare_io(bundle, activation, result), None),
    }
}
