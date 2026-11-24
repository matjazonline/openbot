//! Runtime child creation (07-D1 §D1.2). A parent step execution parks on a `child_run` wait
//! written in the same transaction as its child run; only the parent is locked (run →
//! execution → job), never company authority and never the child.
use super::*;
use crate::application::workflow::channel_call::DeniedCode;
use crate::application::workflow::{
    activation::*,
    binding::{BindingConfiguration, ConfiguredBinding},
    budget::BudgetDisposition,
    children::*,
    waits::ParkedWorkflow,
};
use admission_write::{FirstJob, ParentLink, RunDeadline, RunRow};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::time::Duration;

#[async_trait]
impl WorkflowChildren for PostgresPersistence {
    async fn park_child(&self, scope: ActivationRequest) -> AppResult<Option<ParkedWorkflow>> {
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut tx = self.pool.begin().await?;
            // Box the transition: it activates, debits and writes a whole child run.
            let result = Box::pin(park_on(&mut tx, scope)).await?;
            tx.commit().await?;
            Ok(result)
        })
        .await
        .map_err(|_| lease::timed_out())?
    }
}

async fn park_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<Option<ParkedWorkflow>> {
    let Some(run) = waits::lock(db, scope, waits::LockPurpose::Transition).await? else {
        return Ok(None);
    };
    if let Some(wait) = waits::row(db, scope, waits::WaitSelection::Execution).await? {
        return Ok(Some(ParkedWorkflow {
            wait: WaitId::new(wait.id),
            deadline: wait.deadline,
        }));
    }
    let now = waits::now(db).await?;
    if !matches!(run.state.as_str(), "queued" | "running") || run.deadline <= now {
        return Ok(None);
    }
    let binding = admission_binding::read_saved_binding(
        db,
        scope.company,
        WorkflowBindingId::new(run.binding_id),
        scope.run.as_uuid(),
    )
    .await?;
    let Some(step) = child_step(db, scope, &binding).await? else {
        return Ok(None);
    };
    let pending_recovery::Pending::Ready(activation) =
        pending_recovery::activate(db, scope).await?
    else {
        return Ok(None);
    };
    let control = binding
        .bundle()
        .compiled()
        .child_control(&step)
        .ok_or_else(invalid)?;
    // Configuration and input are checked before any repetition is charged (07-D1 §D1.2).
    let params = activation
        .inputs
        .get("params")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let input = activation
        .inputs
        .get("input")
        .cloned()
        .ok_or_else(invalid)?;
    let Some(child) = child_binding(&binding, control.child_version_id, params)?
        .filter(|child| child.bundle().compiled().validate_input(&input).is_ok())
    else {
        pending_recovery::settle(db, scope, pending_recovery::PendingFailure::InvalidInput).await?;
        return Ok(None);
    };
    if control.repeat.is_some()
        && budget::repetition(db, scope, ChildOrdinal::FIRST).await? == BudgetDisposition::Exhausted
    {
        pending_recovery::settle(
            db,
            scope,
            pending_recovery::PendingFailure::RootBudgetExhausted,
        )
        .await?;
        return Ok(None);
    }
    let parked = create_on(
        db,
        scope,
        &ChildStart {
            step: &step,
            origin: ChildOrigin::Round(ChildOrdinal::FIRST),
            binding: &child,
            input: &input,
            deadline: run.deadline,
        },
    )
    .await?;
    completion_job::finish(db, scope, completion_job::CompletionOwner::Pure).await?;
    sqlx::query("UPDATE workflow_runs SET state='waiting',waiting_reason='child_run' WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut *db).await?;
    waits::audit_wait(db, scope, parked.wait.as_uuid(), "wait_parked").await?;
    Ok(Some(parked))
}

/// The step of this execution, when it runs a child (`workflow.call` or `flow.repeat`).
async fn child_step(
    db: &mut PgConnection,
    scope: ActivationRequest,
    binding: &ConfiguredBinding,
) -> AppResult<Option<StepId>> {
    let step: String = sqlx::query_scalar(
        "SELECT step_id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .fetch_one(&mut *db)
    .await?;
    let step = StepId::parse(step).map_err(|_| invalid())?;
    Ok(binding
        .bundle()
        .compiled()
        .child_control(&step)
        .is_some()
        .then_some(step))
}

/// The child's own configuration (07-D1 §D1.2, 07-D2 §D2.3): the pinned bundle from the
/// parent's frozen closure, the step's or tool's static parameters and the parent's resources
/// for exactly the child's slots, under the parent's binding identity (provenance only).
/// `None`: the child rejects the parameters or resources. A pinned bundle missing from the
/// closure breaks the codec invariant and is an error, not a settled failure.
pub(super) fn child_binding(
    parent: &ConfiguredBinding,
    version: VersionId,
    params: Value,
) -> AppResult<Option<ConfiguredBinding>> {
    let bundle = parent
        .bundle()
        .children()
        .get(&version)
        .cloned()
        .ok_or_else(|| AppError::Internal("Pinned child workflow bundle is missing".into()))?;
    configured_child(parent, bundle, params)
}

pub(super) fn configured_child(
    parent: &ConfiguredBinding,
    bundle: Arc<PublishedBundle>,
    params: Value,
) -> AppResult<Option<ConfiguredBinding>> {
    let slots: BTreeSet<_> = bundle
        .compiled()
        .graph()
        .definition()
        .resources
        .iter()
        .map(|requirement| requirement.slot.clone())
        .collect();
    let resources = parent
        .resources()
        .iter()
        .filter(|(slot, _)| slots.contains(*slot))
        .map(|(slot, resource)| (slot.clone(), *resource))
        .collect();
    Ok(ConfiguredBinding::new(
        BindingConfiguration {
            escalation: parent.escalation(),
            company_id: parent.company_id(),
            id: parent.id(),
            revision: parent.revision(),
            params,
            resources,
            // Provenance only, like the binding identity: a child admits no messages and runs
            // with the parent run's actor.
            run_as: parent.run_as(),
            message_trigger: None,
        },
        bundle,
    )
    .ok())
}

/// What starts a child of one parent execution.
#[derive(Clone, Copy)]
pub(super) enum ChildOrigin {
    /// A `workflow.call` or `flow.repeat` round (07-D1 §D1.2).
    Round(ChildOrdinal),
    /// A saved workflow tool call, and the agent attempt its park closes (07-D2 §D2.3).
    Call { call: ToolCallId, attempt: i32 },
}

/// One child of one parent execution.
#[derive(Clone, Copy)]
pub(super) struct ChildStart<'a> {
    pub step: &'a StepId,
    pub origin: ChildOrigin,
    pub binding: &'a ConfiguredBinding,
    pub input: &'a Value,
    /// The parent's absolute run deadline; a child can never outlive its parent.
    pub deadline: DateTime<Utc>,
}

/// Whose child it is. An ordinary child runs as its parent, in the parent's association and
/// dependency scope. A channel-call child runs the destination as configured (agent-workflow-call
/// AWC-P02-D1 D4): its own actor, desk, levels, dependency scope and unit scopes.
#[derive(Clone, Copy)]
enum ChildIdentity<'a> {
    Inherited,
    ChannelCall {
        call: &'a CallChild<'a>,
        /// The call's fresh thread on the destination desk.
        association: RelatedAssociation,
    },
}

/// The admitted facts of one channel-call child, decided before any child row exists.
pub(super) struct CallChild<'a> {
    pub actor: WorkflowActor,
    /// The destination desk; the child runs on the call's fresh thread there.
    pub desk: Uuid,
    pub request: &'a crate::application::workflow::channel_call::ChannelCallRequest,
    /// The calling agent's principal, the source message's author.
    pub author: crate::entities::transport::PrincipalId,
    pub ceiling: crate::entities::access_level::AccessLevel,
    /// The request's level and exactness: everything of the caller this child may see.
    pub content: crate::entities::access_level::AccessLevel,
    pub content_exact: bool,
    pub record: super::channel_call_target::ChannelCallRecord<'a>,
}

#[derive(sqlx::FromRow)]
struct ParentRun {
    knowledge_ceiling: i16,
    content_level: i16,
    content_exact: bool,
    actor_id: Uuid,
    channel_id: Option<Uuid>,
    principal_id: Option<Uuid>,
    thread_id: Option<Uuid>,
    correlation_id: Uuid,
    delivery_policy: String,
}

/// The single runtime child writer (07-D1 §D1.2a): the child run, its entry job, its admission
/// identity and the parent's `child_run` wait. It captures no thread history and checks no
/// authority. The caller holds the parent run, execution and job and settles the rest of the
/// parent's transition.
pub(super) async fn create_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    start: &ChildStart<'_>,
) -> AppResult<ParkedWorkflow> {
    Ok(write_on(db, scope, start, ChildIdentity::Inherited)
        .await?
        .1)
}

/// A channel-call child under its own savepoint (AWC-P02-D1 r3 D3 step 9). A destination
/// refusal found while writing it (its dependency graph, a role, or the unit's spend cap) rolls
/// the child back and answers the call; the caller's fence and locks stay held. Any other error
/// propagates.
pub(super) async fn create_call_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    start: &ChildStart<'_>,
    call: &CallChild<'_>,
) -> AppResult<Result<(RunId, ParkedWorkflow), DeniedCode>> {
    let mut savepoint = sqlx::Connection::begin(&mut *db).await?;
    let written = Box::pin(write_call_on(&mut savepoint, scope, start, call)).await;
    match written {
        Ok(Ok(started)) => {
            savepoint.commit().await?;
            Ok(Ok(started))
        }
        Ok(Err(code)) => {
            savepoint.rollback().await?;
            Ok(Err(code))
        }
        Err(error) => Err(error),
    }
}

async fn write_call_on(
    tx: &mut Transaction<'_, Postgres>,
    scope: ActivationRequest,
    start: &ChildStart<'_>,
    call: &CallChild<'_>,
) -> AppResult<Result<(RunId, ParkedWorkflow), DeniedCode>> {
    // The request first: the child runs on its thread and consumes its stored message.
    let caller = crate::application::workflow::actions::ActionScope {
        company: scope.company,
        run: scope.run,
        execution: scope.execution,
    };
    let (source, input) =
        super::channel_call_source::store_on(tx, caller, call.desk, call.request, call.author)
            .await?;
    let association = RelatedAssociation::Thread {
        target: crate::entities::address::AddressTarget::Desk(call.desk),
        thread_id: RelatedThreadId::new(source.thread),
    };
    let input = input.to_value();
    let start = ChildStart {
        input: &input,
        ..*start
    };
    let (child, parked) = write_on(
        tx,
        scope,
        &start,
        ChildIdentity::ChannelCall { call, association },
    )
    .await?;
    let capture = super::agent_manifest::CaptureScope {
        company: scope.company,
        actor: call.actor,
        binding: start.binding,
        association,
        scope_root: child,
    };
    let manifest = match super::agent_manifest::capture_on(tx, capture).await {
        Ok(manifest) => manifest,
        Err(error) if super::channel_call_target::destination_refusal(&error) => {
            return Ok(Err(DeniedCode::DestinationUnavailable));
        }
        Err(error) => return Err(error),
    };
    let manifest_id = super::agent_manifest::persist_on(tx, &manifest).await?;
    // The record first: the unit-scope seal admits a fresh desk scope only for a recorded call.
    super::channel_call_target::record_on(tx, &call.record, child, manifest_id, source).await?;
    // The call owns its child's final response (AWC-P04-D1 r2 D1): the existing projection,
    // review and publication run for it, and the guard proves record, child and owner agree.
    sqlx::query(
        "INSERT INTO channel_reply_runs(company_id,owner_kind,call_source_message_id,run_id,binding_id,binding_revision) \
         SELECT company_id,'call',source_message_id,child_run_id,binding_id,binding_revision \
         FROM workflow_channel_calls WHERE company_id=$1 AND child_run_id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(child.as_uuid())
    .execute(&mut **tx)
    .await?;
    super::run_scopes::capture_on(tx, capture, &manifest).await?;
    // The child's history is exactly its fresh thread, at its ceiling.
    super::admission_history::capture_for(tx, scope.company, child, association, start.binding)
        .await?;
    // The destination unit pays (D1.2): its cap is read from the scope just captured.
    match crate::adapters::persistence::unit_budgets::check_admission_on(tx, scope.company, child)
        .await
    {
        Ok(()) => Ok(Ok((child, parked))),
        Err(AppError::BudgetExhausted(refusal)) if refusal.is_goal() => {
            Ok(Err(DeniedCode::GoalBudgetExhausted))
        }
        Err(AppError::BudgetExhausted(_)) => Ok(Err(DeniedCode::UnitBudgetExhausted)),
        Err(error) => Err(error),
    }
}

async fn write_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    start: &ChildStart<'_>,
    identity: ChildIdentity<'_>,
) -> AppResult<(RunId, ParkedWorkflow)> {
    let parent = sqlx::query_as::<_, ParentRun>(
        "SELECT actor_id, channel_id, principal_id, thread_id, correlation_id, delivery_policy, knowledge_ceiling::smallint AS knowledge_ceiling, content_level::smallint AS content_level, content_exact FROM workflow_runs \
         WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_one(&mut *db)
    .await?;
    let (association, actor) = match identity {
        ChildIdentity::Inherited => (
            association::association_from_columns(association::AssociationColumns {
                channel_id: parent.channel_id,
                principal_id: parent.principal_id,
                thread_id: parent.thread_id,
            })?,
            parent.actor_id,
        ),
        ChildIdentity::ChannelCall { call, association } => (association, call.actor.user_id()),
    };
    let child = RunId::new(Uuid::new_v4());
    let coordinates = child_coordinates(scope, start)?;
    let trigger = TriggerRef::new(
        scope.company,
        TriggerId::new(Uuid::new_v4()),
        TriggerSource::Child {
            parent: coordinates.cause,
        },
    )
    .map_err(|_| invalid())?;
    insert_child_on(
        db,
        scope,
        start,
        &parent,
        NewChild {
            run: child,
            trigger: &trigger,
            association,
            identity,
        },
    )
    .await?;
    admission_write::insert_first_job(
        db,
        &FirstJob {
            company: scope.company,
            run: child,
            execution: ExecutionId::new(Uuid::new_v4()),
            entry: &start.binding.bundle().compiled().graph().definition().entry,
            association,
            correlation: parent.correlation_id,
        },
    )
    .await?;
    admission_write::insert_admission(
        db,
        scope.company,
        start.binding.id(),
        &admission_binding::SourceKey::from_trigger(&trigger),
        child,
    )
    .await?;
    admission_write::insert_admitted(db, scope.company, child, actor).await?;
    let wait = Uuid::new_v4();
    // The stored deadline answers every replay, as for the other waits.
    let deadline: DateTime<Utc> = sqlx::query_scalar("INSERT INTO workflow_waits(company_id,run_id,execution_id,id,reason,deadline,created_at,child_run_id,child_ordinal,child_tool_call_id,parked_attempt_number) VALUES($1,$2,$3,$4,'child_run',$5,clock_timestamp(),$6,$7,$8,$9) RETURNING deadline")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(wait)
        .bind(start.deadline).bind(child.as_uuid()).bind(coordinates.ordinal).bind(coordinates.call).bind(coordinates.attempt)
        .fetch_one(&mut *db).await?;
    Ok((
        child,
        ParkedWorkflow {
            wait: WaitId::new(wait),
            deadline,
        },
    ))
}

struct ChildCoordinates {
    cause: ChildCause,
    ordinal: Option<i32>,
    call: Option<Uuid>,
    attempt: Option<i32>,
}

fn child_coordinates(
    scope: ActivationRequest,
    start: &ChildStart<'_>,
) -> AppResult<ChildCoordinates> {
    let execution = ExecutionRef::new(
        scope.company,
        scope.run,
        scope.execution,
        start.step.clone(),
    );
    Ok(match start.origin {
        ChildOrigin::Round(ordinal) => ChildCoordinates {
            cause: ChildCause::Step(ChildStepRef::new(execution, ordinal)),
            ordinal: Some(i32::try_from(ordinal.get()).map_err(|_| invalid())?),
            call: None,
            attempt: None,
        },
        ChildOrigin::Call { call, attempt } => ChildCoordinates {
            cause: ChildCause::ToolCall(ChildToolCallRef::new(execution, call)),
            ordinal: None,
            call: Some(call.as_uuid()),
            attempt: Some(attempt),
        },
    })
}

/// The child run's own identity, decided by `write_on`.
struct NewChild<'a> {
    run: RunId,
    trigger: &'a TriggerRef,
    association: RelatedAssociation,
    identity: ChildIdentity<'a>,
}

// Insert the child and prove exact root-manifest membership on the caller's transaction.
async fn insert_child_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    start: &ChildStart<'_>,
    parent: &ParentRun,
    new: NewChild<'_>,
) -> AppResult<()> {
    let NewChild {
        run: child,
        trigger,
        association,
        identity,
    } = new;
    let (knowledge, content_exact, actor_id) = match identity {
        // A child starts from everything its parent consumed, exact or not (E3).
        ChildIdentity::Inherited => (
            super::knowledge::stored_levels(parent.knowledge_ceiling, parent.content_level)?,
            parent.content_exact,
            parent.actor_id,
        ),
        // A call child starts from its request, already released below its ceiling (D1.4).
        ChildIdentity::ChannelCall { call, .. } => (
            crate::entities::access_level::RunKnowledge::new(call.ceiling, call.content)
                .map_err(|e| AppError::Internal(e.into()))?,
            call.content_exact,
            call.actor.user_id(),
        ),
    };
    // The parent's goal first, then the child's own thread and desk (Plan08 03B.2).
    let goal = super::goals::resolve_on(
        db,
        scope.company,
        super::goals::GoalOrigin {
            parent: Some(scope.run),
            ..super::goals::GoalOrigin::of(association)
        },
    )
    .await?;
    admission_write::insert_run(
        db,
        &RunRow {
            goal,
            company: scope.company,
            knowledge,
            content_exact,
            run: child,
            binding: start.binding,
            association,
            actor_id,
            trigger: trigger.trigger_id(),
            correlation: parent.correlation_id,
            input: start.input,
            deadline: RunDeadline::Inherited(start.deadline),
            parent: Some(ParentLink {
                run: scope.run,
                execution: scope.execution,
            }),
            rerun_of: None,
            // A simulation's child is a simulation too: it cannot send what its parent may not.
            delivery_policy: DeliveryPolicy::parse(&parent.delivery_policy)?,
            native_mcp_call: None,
        },
    )
    .await?;
    if let ChildIdentity::ChannelCall { .. } = identity {
        // Its own scope and unit scopes are captured by `write_call_on` under the savepoint.
        return Ok(());
    }
    let definition = start.binding.bundle().compiled().graph().definition();
    super::agent_manifest::attach_child_on(
        db,
        scope.company,
        scope.run,
        child,
        definition.workflow_id,
        definition.version_id,
    )
    .await?;
    super::run_scopes::copy_child_on(db, scope.company, scope.run, child).await?;
    Ok(())
}
