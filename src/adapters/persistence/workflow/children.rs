//! Runtime child creation (07-D1 §D1.2). A parent step execution parks on a `child_run` wait
//! written in the same transaction as its child run; only the parent is locked (run →
//! execution → job), never company authority and never the child.
use super::*;
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
    let Some(run) = waits::lock(db, scope).await? else {
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
pub(super) struct ChildStart<'a> {
    pub step: &'a StepId,
    pub origin: ChildOrigin,
    pub binding: &'a ConfiguredBinding,
    pub input: &'a Value,
    /// The parent's absolute run deadline; a child can never outlive its parent.
    pub deadline: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct ParentRun {
    actor_id: Uuid,
    channel_id: Option<Uuid>,
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
    let parent = sqlx::query_as::<_, ParentRun>(
        "SELECT actor_id, channel_id, thread_id, correlation_id, delivery_policy FROM workflow_runs \
         WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_one(&mut *db)
    .await?;
    let association = binding_rows::association_from_columns(parent.channel_id, parent.thread_id)?;
    let child = RunId::new(Uuid::new_v4());
    let execution = ExecutionRef::new(
        scope.company,
        scope.run,
        scope.execution,
        start.step.clone(),
    );
    let (parent_cause, ordinal, call, attempt) = match start.origin {
        ChildOrigin::Round(ordinal) => (
            ChildCause::Step(ChildStepRef::new(execution, ordinal)),
            Some(i32::try_from(ordinal.get()).map_err(|_| invalid())?),
            None,
            None,
        ),
        ChildOrigin::Call { call, attempt } => (
            ChildCause::ToolCall(ChildToolCallRef::new(execution, call)),
            None,
            Some(call.as_uuid()),
            Some(attempt),
        ),
    };
    let trigger = TriggerRef::new(
        scope.company,
        TriggerId::new(Uuid::new_v4()),
        TriggerSource::Child {
            parent: parent_cause,
        },
    )
    .map_err(|_| invalid())?;
    admission_write::insert_run(
        db,
        &RunRow {
            company: scope.company,
            run: child,
            binding: start.binding,
            association,
            actor_id: parent.actor_id,
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
    admission_write::insert_admitted(db, scope.company, child, parent.actor_id).await?;
    let wait = Uuid::new_v4();
    // The stored deadline answers every replay, as for the other waits.
    let deadline: DateTime<Utc> = sqlx::query_scalar("INSERT INTO workflow_waits(company_id,run_id,execution_id,id,reason,deadline,created_at,child_run_id,child_ordinal,child_tool_call_id,parked_attempt_number) VALUES($1,$2,$3,$4,'child_run',$5,clock_timestamp(),$6,$7,$8,$9) RETURNING deadline")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(wait)
        .bind(start.deadline).bind(child.as_uuid()).bind(ordinal).bind(call).bind(attempt)
        .fetch_one(&mut *db).await?;
    Ok(ParkedWorkflow {
        wait: WaitId::new(wait),
        deadline,
    })
}
