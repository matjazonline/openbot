//! Bindings and runs: a binding configured with the revision the caller saw and switched on or
//! off; runs started by key, listed, inspected, cancelled and retried.
use super::*;
use crate::application::workflow::lifecycle::{
    BindingState, BindingTarget, ConfigureBinding, SetBindingActivity,
};
use crate::application::workflow::manual::{StartRun, start};
use crate::application::workflow::reads::{
    BindingSummary, PageRequest, RunFilter, RunLineage, RunSummary,
};
use crate::application::workflow::{
    CancelResult, CancelWorkflowRequest, IdempotencyKey, RetryResult, RetryWorkflowRequest,
    RunRevision,
};
use crate::domain::workflow::{
    BindingStateRevision, MessageTrigger, ResourceName, RunId, RuntimeResourceId, VersionId,
    WorkflowBindingId,
};
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};

fn binding_target(company_id: Uuid, actor: WorkflowActor, binding: Uuid) -> BindingTarget {
    BindingTarget {
        company: company(company_id),
        actor,
        binding: WorkflowBindingId::new(binding),
    }
}

fn state_json(state: &BindingState) -> Value {
    json!({
        "binding_id": state.configuration.id().as_uuid(),
        "state_revision": state.revision.get(),
        "configuration_revision": state.configuration.revision().get(),
        "active": state.active,
        "association": association_json(state.association),
    })
}

fn summary_json(binding: &BindingSummary) -> Value {
    json!({
        "binding_id": binding.binding.as_uuid(),
        "state_revision": binding.state_revision,
        "configuration_revision": binding.configuration_revision,
        "active": binding.active,
        "association": association_json(binding.association),
        "workflow_id": binding.workflow.as_uuid(),
        "version_id": binding.version.as_uuid(),
        "params": binding.params,
        "run_as": binding.run_as,
        "message_trigger": binding.message_trigger.map(MessageTrigger::as_str),
    })
}

/// The bindings the caller may see, a page at a time: the company's, less those on channels they
/// cannot see.
pub async fn list(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Query(query): Query<ListQuery>,
) -> WorkflowResult<Json<Value>> {
    let (company, actor) = (company(company_id), actor(&user)?);
    authorize(&engine, company, actor, RelatedAssociation::Company).await?;
    let page = engine.reads().list_bindings(company, query.page()?).await?;
    let next = page
        .more
        .then(|| page.items.last().map(|last| last.binding.as_uuid()))
        .flatten();
    let mut visibility = Visibility::new(&engine, company, actor);
    let mut visible = Vec::new();
    for binding in &page.items {
        if visibility.allows(binding.association).await? {
            visible.push(summary_json(binding));
        }
    }
    visibility.report("workflow-bindings");
    Ok(page_json(visible, next))
}

/// Whether `actor` may see a record of `association`; refusal is `false`, anything else an error.
async fn visible_to(
    engine: &WorkflowEngine,
    company: CompanyId,
    actor: WorkflowActor,
    association: RelatedAssociation,
) -> AppResult<bool> {
    match authorize(engine, company, actor, association).await {
        Ok(()) => Ok(true),
        Err(AppError::NotFound(_)) => Ok(false),
        Err(error) => Err(error),
    }
}

/// Distinct channels one page checks past which the listing logs its working set: the evidence
/// for replacing the per-channel checks with one batch read.
const VISIBILITY_CHECKS_LOGGED: usize = 32;

/// Which stored associations the caller may see, each channel authorized once per request. A
/// stored thread association is its channel's: the `(company_id, channel_id, thread_id)` foreign
/// keys of `workflow_runs` and `workflow_bindings` already hold the thread in that channel, which
/// is all the thread check adds. So a page of mail-triggered runs, one thread each, checks each
/// channel once.
pub(super) struct Visibility<'a> {
    engine: &'a WorkflowEngine,
    company: CompanyId,
    actor: WorkflowActor,
    seen: HashMap<RelatedAssociation, bool>,
}

impl<'a> Visibility<'a> {
    pub(super) fn new(
        engine: &'a WorkflowEngine,
        company: CompanyId,
        actor: WorkflowActor,
    ) -> Self {
        Self {
            engine,
            company,
            actor,
            seen: HashMap::new(),
        }
    }

    pub(super) async fn allows(&mut self, association: RelatedAssociation) -> AppResult<bool> {
        let association = match association {
            RelatedAssociation::Thread { channel_id, .. } => {
                RelatedAssociation::Channel(channel_id)
            }
            other => other,
        };
        if let Some(&allowed) = self.seen.get(&association) {
            return Ok(allowed);
        }
        let allowed = visible_to(self.engine, self.company, self.actor, association).await?;
        self.seen.insert(association, allowed);
        Ok(allowed)
    }

    pub(super) fn report(&self, listing: &'static str) {
        if self.seen.len() > VISIBILITY_CHECKS_LOGGED {
            tracing::info!(
                listing,
                company_id = %self.company.as_uuid(),
                checks = self.seen.len(),
                "workflow listing visibility checks"
            );
        }
    }
}

pub(super) async fn visible_binding(
    engine: &WorkflowEngine,
    company: CompanyId,
    actor: WorkflowActor,
    binding: Uuid,
) -> AppResult<BindingSummary> {
    let not_found = || AppError::NotFound("Workflow binding not found".into());
    authorize(engine, company, actor, RelatedAssociation::Company).await?;
    let binding = engine
        .reads()
        .binding_summary(company, WorkflowBindingId::new(binding))
        .await?
        .ok_or_else(not_found)?;
    if !visible_to(engine, company, actor, binding.association).await? {
        return Err(not_found());
    }
    Ok(binding)
}

pub async fn read(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, binding)): Path<(Uuid, Uuid)>,
) -> WorkflowResult<Json<Value>> {
    let binding = visible_binding(&engine, company(company_id), actor(&user)?, binding).await?;
    Ok(ok(summary_json(&binding)))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigureBody {
    /// Absent creates the binding (with the id in the path); present must be its current state.
    pub expected_state_revision: Option<u64>,
    pub association: AssociationBody,
    pub version_id: Uuid,
    #[serde(default = "empty_object")]
    pub params: Value,
    #[serde(default)]
    pub resources: BTreeMap<String, Uuid>,
    /// Absent: the caller on create, the stored run-as on reconfigure. Only the owner may name
    /// somebody else (p08 D1.1).
    pub run_as: Option<Uuid>,
    pub message_trigger: Option<String>,
}

fn empty_object() -> Value {
    json!({})
}

pub async fn configure(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, binding)): Path<(Uuid, Uuid)>,
    Json(body): Json<ConfigureBody>,
) -> WorkflowResult<Json<Value>> {
    let request = BindingRequest {
        expected_state_revision: body.expected_state_revision,
        association: body.association.into(),
        version: body.version_id,
        params: body.params,
        resources: body.resources,
        run_as: body.run_as,
        message_trigger: body.message_trigger,
    };
    let state = configure_binding(&engine, company_id, actor(&user)?, binding, request).await?;
    Ok(ok(state_json(&state)))
}

/// One binding configuration as the JSON route and the bindings tab send it.
pub(super) struct BindingRequest {
    /// Absent creates the binding; present must be its current state revision.
    pub expected_state_revision: Option<u64>,
    pub association: RelatedAssociation,
    pub version: Uuid,
    pub params: Value,
    pub resources: BTreeMap<String, Uuid>,
    pub run_as: Option<Uuid>,
    pub message_trigger: Option<String>,
}

/// Configure (or create) `binding` through the binding command, which authorizes, checks the
/// revision, validates the params and checks readiness.
pub(super) async fn configure_binding(
    engine: &WorkflowEngine,
    company_id: Uuid,
    actor: WorkflowActor,
    binding: Uuid,
    request: BindingRequest,
) -> WorkflowResult<BindingState> {
    let resources = request
        .resources
        .iter()
        .map(|(slot, resource)| {
            Ok((
                ResourceName::parse(slot)
                    .map_err(|_| AppError::BadRequest(format!("'{slot}' is not a slot name")))?,
                RuntimeResourceId::new(*resource),
            ))
        })
        .collect::<AppResult<BTreeMap<_, _>>>()?;
    Ok(engine
        .bindings()
        .configure(ConfigureBinding {
            target: binding_target(company_id, actor, binding),
            expected: request
                .expected_state_revision
                .map(|value| revision(value, BindingStateRevision::new))
                .transpose()?,
            association: request.association,
            version: VersionId::new(request.version),
            params: request.params,
            resources,
            run_as: request
                .run_as
                .map(WorkflowActor::authenticated)
                .transpose()?,
            message_trigger: request
                .message_trigger
                .map(|trigger| {
                    trigger.parse::<MessageTrigger>().map_err(|_| {
                        AppError::BadRequest(format!("'{trigger}' is not a message trigger"))
                    })
                })
                .transpose()?,
        })
        .await?)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivityBody {
    pub expected_state_revision: u64,
}

async fn set_activity(
    engine: &WorkflowEngine,
    user: &AuthenticatedUser,
    company_id: Uuid,
    binding: Uuid,
    body: ActivityBody,
    active: bool,
) -> WorkflowResult<Json<Value>> {
    let state = set_binding_activity(
        engine,
        company_id,
        actor(user)?,
        binding,
        body.expected_state_revision,
        active,
    )
    .await?;
    Ok(ok(state_json(&state)))
}

/// Activate (readiness checked again) or deactivate `binding` at the state revision seen.
pub(super) async fn set_binding_activity(
    engine: &WorkflowEngine,
    company_id: Uuid,
    actor: WorkflowActor,
    binding: Uuid,
    expected_state_revision: u64,
    active: bool,
) -> WorkflowResult<BindingState> {
    let request = SetBindingActivity {
        target: binding_target(company_id, actor, binding),
        expected: revision(expected_state_revision, BindingStateRevision::new)?,
    };
    let bindings = engine.bindings();
    Ok(if active {
        bindings.activate(request).await?
    } else {
        bindings.deactivate(request).await?
    })
}

pub async fn activate(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, binding)): Path<(Uuid, Uuid)>,
    Json(body): Json<ActivityBody>,
) -> WorkflowResult<Json<Value>> {
    set_activity(&engine, &user, company_id, binding, body, true).await
}

pub async fn deactivate(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, binding)): Path<(Uuid, Uuid)>,
    Json(body): Json<ActivityBody>,
) -> WorkflowResult<Json<Value>> {
    set_activity(&engine, &user, company_id, binding, body, false).await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartBody {
    pub input: Value,
    /// A retry with the key returns its run; the key with a different start is a conflict.
    pub command_key: String,
    /// Within the binding's association; absent is the binding's own.
    pub association: Option<AssociationBody>,
    /// The top-level run this start reruns.
    pub rerun_of: Option<Uuid>,
}

/// 201 with the new run, or 200 with the run a retry of the key already started.
pub async fn start_run(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, binding)): Path<(Uuid, Uuid)>,
    Json(body): Json<StartBody>,
) -> WorkflowResult<(StatusCode, Json<Value>)> {
    let (run, replayed) = start_command(
        &engine,
        company_id,
        actor(&user)?,
        binding,
        body.association.map(Into::into),
        body.input,
        body.command_key,
        body.rerun_of,
    )
    .await?;
    let status = if replayed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok((
        status,
        ok(json!({"run_id": run.as_uuid(), "replayed": replayed})),
    ))
}

/// Start a run of `binding` (in its own association unless one is named) under `key`; a
/// `rerun_of` names the top-level run this one reruns. The run and whether the key replayed.
#[allow(clippy::too_many_arguments)]
pub(super) async fn start_command(
    engine: &WorkflowEngine,
    company_id: Uuid,
    actor: WorkflowActor,
    binding: Uuid,
    association: Option<RelatedAssociation>,
    input: Value,
    key: String,
    rerun_of: Option<Uuid>,
) -> WorkflowResult<(RunId, bool)> {
    let company = company(company_id);
    let association = match association {
        Some(association) => association,
        None => {
            visible_binding(engine, company, actor, binding)
                .await?
                .association
        }
    };
    let started = start(
        &engine.service(),
        StartRun {
            company_id: company,
            actor,
            binding_id: WorkflowBindingId::new(binding),
            association,
            input,
            idempotency_key: key,
            rerun_of: rerun_of.map(RunId::new),
        },
    )
    .await?;
    engine.wake_workers();
    Ok((started.run, started.replayed))
}

pub(super) fn run_json(run: &RunSummary) -> Value {
    json!({
        "run_id": run.run.as_uuid(),
        "binding_id": run.binding.as_uuid(),
        "workflow_id": run.workflow.as_uuid(),
        "version_id": run.version.as_uuid(),
        "association": association_json(run.association),
        "state": run.state,
        "waiting_reason": run.waiting_reason,
        "revision": run.revision,
        "actor": run.actor,
        // WF-C002: a runtime child carries its parent's binding; the list labels it.
        "parent_run_id": run.parent.map(|parent| parent.as_uuid()),
        "rerun_of": run.rerun_of.map(|run| run.as_uuid()),
        "created_at": run.created_at,
        "deadline": run.deadline,
        "finished_at": run.finished_at,
    })
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunsQuery {
    pub binding_id: Option<Uuid>,
    pub state: Option<String>,
    /// Runs of versions that use this agent of the company — including branches a run never
    /// took — not the agents a run actually called (see [`RunFilter::agent`]). Another
    /// company's agent, or one deleted, is not found.
    pub agent_id: Option<Uuid>,
    /// `top_level` leaves out child runs (the board's view); absent or `any` lists every run.
    pub lineage: Option<RunLineage>,
    /// The id of the last run of the previous page.
    pub after: Option<Uuid>,
    pub limit: Option<i64>,
}

/// `agent` when it is an agent of `company`; another company's, or a deleted one, is not found
/// rather than an empty listing.
pub(super) async fn company_agent(
    engine: &WorkflowEngine,
    company: CompanyId,
    agent: Uuid,
) -> AppResult<Uuid> {
    engine
        .authoring_reads()
        .agent_tool_grants(company, agent)
        .await?
        .map(|_| agent)
        .ok_or_else(|| AppError::NotFound("Agent not found".into()))
}

/// The newest runs the caller may see, a page at a time, children labelled with their parent;
/// `agent_id` keeps those of versions that use that agent of the company ("uses", not "ran").
pub async fn runs(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Query(query): Query<RunsQuery>,
) -> WorkflowResult<Json<Value>> {
    let (company, actor) = (company(company_id), actor(&user)?);
    authorize(&engine, company, actor, RelatedAssociation::Company).await?;
    let agent = match query.agent_id {
        Some(agent) => Some(company_agent(&engine, company, agent).await?),
        None => None,
    };
    let filter = RunFilter {
        binding: query.binding_id.map(WorkflowBindingId::new),
        state: query.state,
        agent,
        lineage: query.lineage.unwrap_or_default(),
    };
    let page = engine
        .reads()
        .list_runs(
            company,
            &filter,
            PageRequest::new(query.after, query.limit)?,
        )
        .await?;
    let next = page
        .more
        .then(|| page.items.last().map(|last| last.run.as_uuid()))
        .flatten();
    let mut visibility = Visibility::new(&engine, company, actor);
    let mut visible = Vec::new();
    for run in &page.items {
        if visibility.allows(run.association).await? {
            visible.push(run_json(run));
        }
    }
    visibility.report("workflow-runs");
    Ok(page_json(visible, next))
}

pub async fn run(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, run)): Path<(Uuid, Uuid)>,
) -> WorkflowResult<Json<Value>> {
    let (company, actor) = (company(company_id), actor(&user)?);
    let not_found = || AppError::NotFound("Workflow run not found".into());
    authorize(&engine, company, actor, RelatedAssociation::Company).await?;
    let detail = engine
        .reads()
        .run_detail(company, RunId::new(run))
        .await?
        .ok_or_else(not_found)?;
    if !visible_to(&engine, company, actor, detail.summary.association).await? {
        return Err(not_found().into());
    }
    let mut body = run_json(&detail.summary);
    body["executions"] = json!(
        detail
            .executions
            .iter()
            .map(|execution| json!({
                "execution_id": execution.execution,
                "step": execution.step,
                "activation": execution.activation,
                "activated_at": execution.activated_at,
                "completed_at": execution.completed_at,
                "route": execution.route,
            }))
            .collect::<Vec<_>>()
    );
    body["actions"] = json!(
        detail
            .actions
            .iter()
            .map(|action| json!({
                "marker_id": action.marker,
                "execution_id": action.execution,
                "invocation_id": action.invocation,
                "argument_digest": action.argument_digest,
                "effect_kind": action.effect_kind,
                "attempt_number": action.attempt,
                "created_at": action.created_at,
                "has_receipt": action.has_receipt,
                "evidence_conflict": action.evidence_conflict,
                "reconcilable": action.reconcilable(&detail.summary),
                "destinations": action.destinations.iter().map(|destination| json!({
                    "ordinal": destination.ordinal,
                    "delivery_id": destination.delivery,
                    "status": destination.status,
                    "parts": destination.parts,
                    "reconcilable": destination.reconcilable(),
                })).collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>()
    );
    body["actions_truncated"] = json!(detail.actions_truncated);
    body["executions_truncated"] = json!(detail.executions_truncated);
    body["waits_truncated"] = json!(detail.waits_truncated);
    body["waits"] = json!(
        detail
            .waits
            .iter()
            .map(|wait| json!({
                "wait_id": wait.wait,
                "execution_id": wait.execution,
                "reason": wait.reason,
                "state": wait.state,
                "deadline": wait.deadline,
                "decision_id": wait.decision,
                "child_run_id": wait.child_run.map(|run| run.as_uuid()),
            }))
            .collect::<Vec<_>>()
    );
    Ok(ok(body))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlBody {
    pub expected_revision: u64,
    pub command_key: String,
}

fn run_conflict(current: RunRevision) -> WorkflowError {
    AppError::Conflict(format!(
        "The run changed; its current revision is {}",
        current.0
    ))
    .into()
}

/// A top-level run the caller may see. A runtime child is controlled through its root (WF-C002):
/// cancelling the root cancels its children, and a child is never retried on its own. The run is
/// authorized before its parent is told, so the refusal discloses nothing to a caller who cannot
/// see the run. The engine's own child controls (a parent's cancellation cascading to its
/// children, `cancel_tree_on`) are persistence transitions that never come through this route.
/// The rule lives here because these routes are the only production caller of the service's
/// cancel and retry; a later ingress that calls the service must apply it too.
pub(super) async fn controllable_run(
    engine: &WorkflowEngine,
    company: CompanyId,
    actor: WorkflowActor,
    run: Uuid,
) -> AppResult<()> {
    let not_found = || AppError::NotFound("Workflow run not found".into());
    authorize(engine, company, actor, RelatedAssociation::Company).await?;
    let summary = engine
        .reads()
        .run_summary(company, RunId::new(run))
        .await?
        .ok_or_else(not_found)?;
    if !visible_to(engine, company, actor, summary.association).await? {
        return Err(not_found());
    }
    match summary.parent {
        Some(parent) => Err(AppError::Conflict(format!(
            "A child run is controlled through its parent run {}",
            parent.as_uuid()
        ))),
        None => Ok(()),
    }
}

pub async fn cancel(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, run)): Path<(Uuid, Uuid)>,
    Json(body): Json<ControlBody>,
) -> WorkflowResult<Json<Value>> {
    let revision = cancel_command(
        &engine,
        company_id,
        actor(&user)?,
        run,
        body.expected_revision,
        &body.command_key,
    )
    .await?;
    Ok(ok(json!({"run_id": run, "revision": revision})))
}

/// Cancel a top-level run at the revision seen; the run's revision after it.
pub(super) async fn cancel_command(
    engine: &WorkflowEngine,
    company_id: Uuid,
    actor: WorkflowActor,
    run: Uuid,
    expected_revision: u64,
    key: &str,
) -> WorkflowResult<u64> {
    controllable_run(engine, company(company_id), actor, run).await?;
    let result = engine
        .service()
        .cancel(CancelWorkflowRequest {
            expected_revision: RunRevision(expected_revision),
            command_key: IdempotencyKey::parse(key)?,
            company_id: company(company_id),
            actor,
            run_id: RunId::new(run),
        })
        .await?;
    match result {
        CancelResult::Applied { revision }
        | CancelResult::AlreadyTerminalOrApplied { revision } => Ok(revision.0),
        CancelResult::RevisionConflict { current_revision } => Err(run_conflict(current_revision)),
        CancelResult::NotFound => Err(AppError::NotFound("Workflow run not found".into()).into()),
    }
}

pub async fn retry(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, run)): Path<(Uuid, Uuid)>,
    Json(body): Json<ControlBody>,
) -> WorkflowResult<Json<Value>> {
    let revision = retry_command(
        &engine,
        company_id,
        actor(&user)?,
        run,
        body.expected_revision,
        &body.command_key,
    )
    .await?;
    Ok(ok(json!({"run_id": run, "revision": revision})))
}

/// Retry a failed top-level run whose failed step is safe to repeat; the run's revision after.
pub(super) async fn retry_command(
    engine: &WorkflowEngine,
    company_id: Uuid,
    actor: WorkflowActor,
    run: Uuid,
    expected_revision: u64,
    key: &str,
) -> WorkflowResult<u64> {
    controllable_run(engine, company(company_id), actor, run).await?;
    let result = engine
        .service()
        .retry(RetryWorkflowRequest {
            company_id: company(company_id),
            actor,
            run_id: RunId::new(run),
            expected_revision: RunRevision(expected_revision),
            command_key: IdempotencyKey::parse(key)?,
        })
        .await?;
    match result {
        RetryResult::Applied { revision } => {
            engine.wake_workers();
            Ok(revision.0)
        }
        // Unsafe covers every run retry may not resume: one that has not failed, a parent failed
        // by its child, and a failed step whose repeat is not known to be safe.
        RetryResult::Unsafe { revision } => Err(AppError::Conflict(format!(
            "This run cannot be retried: only a failed run whose failed step is safe to repeat \
             can be (revision {})",
            revision.0
        ))
        .into()),
        RetryResult::RevisionConflict { current_revision } => Err(run_conflict(current_revision)),
        RetryResult::NotFound => Err(AppError::NotFound("Workflow run not found".into()).into()),
    }
}
