//! One `channel.call` admitted and its child written in the dispatch transaction
//! (agent-workflow-call gate AWC-P02-D1 r3, D3 and D4).
//!
//! Lock order, in one transaction:
//! 1. the caller's sorted account union (actor, requester, the destination `run_as` hint),
//!    `users FOR SHARE`, then company ownership (`prelock_channel_call_on`);
//! 2. the job fence (`lease::lock_fence`);
//! 3. the replay lookup by primary key; a hit takes no destination lock;
//! 4. `channels FOR SHARE`, the calling agent's principal and its desk grants `FOR SHARE`, and
//!    the desk's settings read under that share lock, which blocks `configure_reply_on`'s
//!    `channels FOR UPDATE` until commit;
//! 5. the settings check against step 1's hint: no account is ever locked after step 1;
//! 6. destination authority (`authorize_association_on`, which re-takes the company and
//!    channel locks already held and takes `department_members`/`principals FOR SHARE`);
//! 7. the child writer's savepoint: manifest capture (its own 1 s lock and 5 s statement
//!    bounds), unit scopes, the record and the unit spend cap.
//!
//! No step waits on a model or provider call; a dispatch runs after its model turn was saved.
use super::*;
use crate::app_error::ExecutionFailure;
use crate::application::agent::AgentToolOrigin;
use crate::application::workflow::channel_call::{
    ALLOWANCE_WITHOUT_REQUEST_RULES, ChannelCallError, ChannelCallOutcome, ChannelCallRequest,
    DeniedCode, DeskFacts, INVALID_INPUT, TargetDecision, arguments_digest, check_intake,
    child_ceiling, decide_target, declared_fields, release_request,
};
use crate::application::workflow::channel_reply::ChannelReplySettings;
use crate::application::workflow::tools::TOOL_LIMIT;
use crate::application::workflow::waits::ParkedWorkflow;
use crate::application::workflow::{
    actions::ActionScope, activation::ActivationRequest, binding::ConfiguredBinding,
    lease::WorkflowFence,
};
use crate::domain::workflow::ToolCallId;
use crate::entities::access_level::AccessLevel;
use crate::entities::address::AddressTarget;
use crate::entities::agent_capability::DependencyManifestId;
use crate::entities::channel::{ChannelAccessMode, participant_authorized};
use chrono::Utc;
use serde_json::Value;
use std::time::Duration;

/// The longest run chain the ancestry walk follows; a deeper chain is refused as a cycle
/// rather than truncated.
const MAX_ANCESTRY: i32 = 1000;

/// One saved call to dispatch. `origin` is what the agent runtime claims; the dispatch proves it
/// against the run's manifest and the harness checkpoint before trusting it (D1.1 step 2).
pub(super) struct CallDispatch<'a> {
    pub fence: WorkflowFence,
    pub call: ToolCallId,
    /// The saved call's arguments, which the record binds by digest.
    pub arguments: &'a Value,
    pub origin: AgentToolOrigin,
}

/// How a dispatch ended inside its transaction.
#[derive(Debug)]
pub(super) enum CallStart {
    /// This call was already recorded: the same invocation and child.
    Replayed { child: RunId },
    /// A tool result with no child: an error or a denied closure.
    Answered(Value),
    /// A child exists and the caller must park on its wait.
    Started {
        child: RunId,
        parked: ParkedWorkflow,
    },
    /// The desk's settings changed after step 1 locked their accounts: nothing was written, and
    /// the dispatch rolls back and reads them again (D1 "Conflict").
    Stale,
}

/// The immutable invocation, minus the child and its manifest, which the child writer adds.
pub(super) struct ChannelCallRecord<'a> {
    pub scope: ActivationRequest,
    pub call: ToolCallId,
    pub arguments_digest: String,
    pub origin: AgentToolOrigin,
    pub channel: Uuid,
    pub binding: WorkflowBindingId,
    pub binding_revision: BindingRevision,
    pub settings: &'a ChannelReplySettings,
    pub run_as: WorkflowActor,
}

pub(super) async fn record_on(
    db: &mut PgConnection,
    record: &ChannelCallRecord<'_>,
    child: RunId,
    manifest: DependencyManifestId,
    source: super::channel_call_source::CallSource,
) -> AppResult<()> {
    let responder = record.settings.responder.as_ref().ok_or_else(invalid)?;
    let contract = serde_json::to_value(&responder.contract).map_err(|_| invalid())?;
    let policy = record
        .settings
        .approval_policy
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(|_| invalid())?;
    sqlx::query(
        "INSERT INTO workflow_channel_calls(company_id,run_id,execution_id,call_id,arguments_digest, \
           agent_manifest_id,agent_id,agent_revision_id,agent_binding_id,channel_id,binding_id,binding_revision, \
           settings_revision,contract,approval_policy,run_as_user_id,child_run_id,scope_manifest_id, \
           thread_id,source_message_id) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20)",
    )
    .bind(record.scope.company.as_uuid())
    .bind(record.scope.run.as_uuid())
    .bind(record.scope.execution.as_uuid())
    .bind(record.call.as_uuid())
    .bind(&record.arguments_digest)
    .bind(record.origin.manifest.as_uuid())
    .bind(record.origin.agent.as_uuid())
    .bind(record.origin.revision.as_uuid())
    .bind(record.origin.binding.as_uuid())
    .bind(record.channel)
    .bind(record.binding.as_uuid())
    .bind(revision(record.binding_revision.get())?)
    .bind(revision(record.settings.revision.get())?)
    .bind(contract)
    .bind(policy)
    .bind(record.run_as.user_id())
    .bind(child.as_uuid())
    .bind(manifest.as_uuid())
    .bind(source.thread)
    .bind(source.message.as_uuid())
    .execute(db)
    .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct RecordedRow {
    arguments_digest: String,
    child_run_id: Uuid,
}

async fn replayed_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    call: ToolCallId,
) -> AppResult<Option<RecordedRow>> {
    Ok(sqlx::query_as(
        "SELECT arguments_digest,child_run_id FROM workflow_channel_calls \
         WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND call_id=$4",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(call.as_uuid())
    .fetch_optional(db)
    .await?)
}

/// One dispatch in gate AWC-P03-D1 r2 D1's order: the desk alone; accounts, company, fence
/// and replay (AWC-P02-D1 r3 D3 steps 1–3); then for a new call the full parse, the origin
/// proof and D1.1 steps 3–8; then the child under its savepoint. The caller commits; on
/// `Started` it parks on the wait.
pub(super) async fn start_on(
    tx: &mut Transaction<'_, Postgres>,
    dispatch: &CallDispatch<'_>,
) -> AppResult<CallStart> {
    let scope = dispatch.fence.scope;
    let action = ActionScope {
        company: scope.company,
        run: scope.run,
        execution: scope.execution,
    };
    // 0. The desk alone, before any lock. No record exists for arguments without one.
    let Some(desk) = ChannelCallRequest::desk(dispatch.arguments) else {
        return Ok(answered_error(
            INVALID_INPUT,
            "channel_id must be a desk id",
        ));
    };
    // 1. Accounts, then company ownership.
    let (actor, hint) = action_authority::prelock_channel_call_on(tx, action, desk)
        .await?
        .ok_or_else(missing)?;
    // 2. The fence.
    if !lease::lock_fence(tx, dispatch.fence, Duration::from_secs(1)).await? {
        return Err(AppError::Execution(ExecutionFailure::OwnershipLost));
    }
    // 3. Replay: the record answers before the request is judged again (AWC-C002).
    let digest = arguments_digest(dispatch.arguments)?;
    if let Some(saved) = replayed_on(tx, scope, dispatch.call).await? {
        if saved.arguments_digest != digest {
            return Ok(answered_error(
                INVALID_INPUT,
                "This call ID was already used with other arguments",
            ));
        }
        return Ok(CallStart::Replayed {
            child: RunId::new(saved.child_run_id),
        });
    }
    let run = workflow_tool_calls::parent_continuation_on(tx, scope).await?;
    // D1.1 step 1: the whole request, its deadline against the caller's.
    let request = match ChannelCallRequest::parse(dispatch.arguments, Utc::now(), run.deadline) {
        Ok(request) => request,
        Err(error) => return Ok(CallStart::Answered(error.to_value())),
    };
    // D1.1 step 2: the claimed origin is this run's agent and its saved `channel.call`.
    let caller = caller_binding_on(tx, scope, actor).await?;
    let selection = agent_origin::channel_call_on(
        tx,
        action,
        actor,
        caller.bundle(),
        dispatch.origin,
        dispatch.call,
        dispatch.arguments,
    )
    .await?;
    agent_authority::check_execution_on(tx, action, selection.agent(), AgentUseMode::Tools).await?;
    // 4. Desk facts under the desk's share lock.
    let target = target_on(tx, scope, dispatch.origin.agent, desk).await?;
    match decide_target(target.as_ref().map(|target| target.facts)) {
        TargetDecision::Proceed => {}
        TargetDecision::Error(code) => {
            return Ok(answered_error(code, "This desk is unavailable"));
        }
        TargetDecision::Denied(code) => return denied(code),
    }
    let target = target.ok_or_else(invalid)?;
    let (Some(settings), Some(state)) = (target.settings, target.responder) else {
        return Err(invalid());
    };
    let responder = settings.responder.as_ref().ok_or_else(invalid)?;
    let configuration = state.configuration;
    let run_as = configuration.run_as();
    // 5. The locked settings must be the ones whose account step 1 locked.
    if hint
        != Some(action_authority::CallAccountHint {
            settings_revision: revision(settings.revision.get())?,
            run_as: run_as.user_id(),
        })
    {
        return if account_live_on(tx, run_as).await? {
            Ok(CallStart::Stale)
        } else {
            denied(DeniedCode::DestinationUnavailable)
        };
    }
    // 6. Intake against the pinned revision.
    let definition = configuration.bundle().compiled().graph().definition();
    if let Err(error) = check_intake(
        declared_fields(definition.input_schema.as_ref()),
        request.fields.as_ref(),
    )? {
        return Ok(CallStart::Answered(error.to_value()));
    }
    // 7. Levels: the child reads no higher than the caller, the allowance and `run_as`.
    let levels = caller_levels_on(tx, scope).await?;
    let ceiling = child_ceiling(
        levels.ceiling,
        ALLOWANCE_WITHOUT_REQUEST_RULES,
        super::knowledge::actor_level_on(tx, scope.company, Some(run_as.user_id())).await?,
    );
    if let Err(error) = release_request(levels.content, ceiling) {
        return Ok(CallStart::Answered(error.to_value()));
    }
    // 8. Destination admission, as a message on the desk is admitted; then the tree limit. A
    //    reviewed desk answers under its captured policy (AWC-P04-D1 D4).
    let inbox = RelatedAssociation::inbox(AddressTarget::Desk(desk));
    if !destination_admits_on(tx, scope.company, &configuration, inbox, responder).await? {
        return denied(DeniedCode::DestinationUnavailable);
    }
    if run.continuation_count >= super::WORKFLOW_JOB_CONTINUATION_LIMIT {
        return Ok(answered_error(
            TOOL_LIMIT,
            "This agent step cannot start another call",
        ));
    }
    // 9. The request, then the child, under one savepoint.
    let author = crate::adapters::persistence::memory::agent_principal_on(
        tx,
        scope.company.as_uuid(),
        dispatch.origin.agent.as_uuid(),
    )
    .await?;
    let record = ChannelCallRecord {
        scope,
        call: dispatch.call,
        arguments_digest: digest,
        origin: dispatch.origin,
        channel: desk,
        binding: responder.binding,
        binding_revision: responder.configuration_revision,
        settings: &settings,
        run_as,
    };
    let call = children::CallChild {
        actor: run_as,
        desk,
        request: &request,
        author,
        ceiling,
        content: levels.content,
        content_exact: levels.content_exact,
        record,
    };
    let step = StepId::parse(run.step_id).map_err(|_| invalid())?;
    let start = children::ChildStart {
        step: &step,
        origin: children::ChildOrigin::Call {
            call: dispatch.call,
            attempt: dispatch.fence.attempt.0,
        },
        binding: &configuration,
        input: &Value::Null,
        deadline: request.child_deadline(run.deadline),
    };
    match children::create_call_on(tx, scope, &start, &call).await? {
        Ok((child, parked)) => Ok(CallStart::Started { child, parked }),
        Err(code) => denied(code),
    }
}

/// The calling run's actor, for a consumption that did not re-run the account prelock's result.
pub(super) async fn caller_actor_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<WorkflowActor> {
    let actor: Uuid =
        sqlx::query_scalar("SELECT actor_id FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(scope.company.as_uuid())
            .bind(scope.run.as_uuid())
            .fetch_one(db)
            .await?;
    WorkflowActor::authenticated(actor).map_err(|_| invalid())
}

/// The calling run's own binding, read for its actor as a workflow tool call reads it.
pub(super) async fn caller_binding_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    actor: WorkflowActor,
) -> AppResult<std::sync::Arc<ConfiguredBinding>> {
    let binding_id: Uuid = sqlx::query_scalar(
        "SELECT binding_id FROM workflow_runs WHERE company_id=$1 AND id=$2 AND actor_id=$3",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(actor.user_id())
    .fetch_optional(&mut *db)
    .await?
    .ok_or_else(missing)?;
    admission_binding::read_saved_binding(
        db,
        scope.company,
        WorkflowBindingId::new(binding_id),
        scope.run.as_uuid(),
    )
    .await
}

fn answered_error(code: &'static str, message: &str) -> CallStart {
    CallStart::Answered(ChannelCallError::new(code, message).to_value())
}

fn denied(code: DeniedCode) -> AppResult<CallStart> {
    Ok(CallStart::Answered(
        ChannelCallOutcome::refused(code).to_value()?,
    ))
}

/// The desk as step 4 reads it.
struct Target {
    facts: DeskFacts,
    settings: Option<ChannelReplySettings>,
    /// The selected responder's pinned revision, when it is a usable responder.
    responder: Option<crate::application::workflow::lifecycle::BindingState>,
}

/// The desk's own launch facts under its share lock: no caller and no ancestry. Live dispatch
/// and sample capture read a desk through this one reader.
pub(super) struct DeskState {
    pub access_mode: ChannelAccessMode,
    pub enabled: bool,
    pub unit_archived: bool,
    pub settings: Option<ChannelReplySettings>,
    /// The selected responder's pinned revision, when it is a usable responder.
    pub responder: Option<crate::application::workflow::lifecycle::BindingState>,
}

#[derive(sqlx::FromRow)]
struct DeskRow {
    access_mode: String,
    enabled: bool,
    archived: bool,
}

pub(super) async fn desk_state_on(
    db: &mut PgConnection,
    company: CompanyId,
    desk: Uuid,
) -> AppResult<Option<DeskState>> {
    let row: Option<DeskRow> = sqlx::query_as(
        "SELECT desk.access_mode, desk.enabled, unit.archived_at IS NOT NULL AS archived \
         FROM channels AS desk JOIN departments AS unit ON unit.company_id=desk.company_id AND unit.id=desk.department_id \
         WHERE desk.company_id=$1 AND desk.id=$2 FOR SHARE OF desk FOR KEY SHARE OF unit",
    )
    .bind(company.as_uuid())
    .bind(desk)
    .fetch_optional(&mut *db)
    .await?;
    let Some(row) = row else { return Ok(None) };
    let access_mode: ChannelAccessMode = row.access_mode.parse().map_err(|_| invalid())?;
    let settings =
        super::channel_reply::settings_on(db, company, AddressTarget::Desk(desk)).await?;
    let responder = match settings.as_ref().and_then(|value| value.responder.as_ref()) {
        Some(selected) => super::binding_rows::read_binding_at(
            db,
            company,
            selected.binding,
            Some(selected.configuration_revision),
        )
        .await?
        .filter(|state| {
            state.active
                && state.association == RelatedAssociation::inbox(AddressTarget::Desk(desk))
                && state.configuration.message_trigger().is_some()
        }),
        None => None,
    };
    Ok(Some(DeskState {
        access_mode,
        enabled: row.enabled,
        unit_archived: row.archived,
        settings,
        responder,
    }))
}

/// Whether `agent` passes the desk's internal-relay participant rule (U2). An agent principal
/// is a company member and never the owner: nobody is exempt.
pub(super) async fn agent_writes_on(
    db: &mut PgConnection,
    company: CompanyId,
    agent: crate::entities::agent_capability::AgentId,
    desk: Uuid,
    access_mode: ChannelAccessMode,
) -> AppResult<bool> {
    let listed = agent_participates_on(db, company, agent, desk).await?;
    Ok(participant_authorized(access_mode, listed, false))
}

async fn target_on(
    tx: &mut Transaction<'_, Postgres>,
    scope: ActivationRequest,
    agent: crate::entities::agent_capability::AgentId,
    desk: Uuid,
) -> AppResult<Option<Target>> {
    let Some(state) = desk_state_on(tx, scope.company, desk).await? else {
        return Ok(None);
    };
    Ok(Some(Target {
        facts: DeskFacts {
            writable: agent_writes_on(tx, scope.company, agent, desk, state.access_mode).await?,
            in_ancestry: in_ancestry_on(tx, scope, desk).await?,
            unit_archived: state.unit_archived,
            enabled: state.enabled,
            responder: state.responder.is_some(),
        },
        settings: state.settings,
        responder: state.responder,
    }))
}

/// The calling agent's `participate` grant on the desk, read as agent preparation reads it.
pub(super) async fn agent_participates_on(
    db: &mut PgConnection,
    company: CompanyId,
    agent: crate::entities::agent_capability::AgentId,
    desk: Uuid,
) -> AppResult<bool> {
    let principal: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM principals WHERE company_id=$1 AND agent_id=$2 AND kind='agent' FOR SHARE",
    )
    .bind(company.as_uuid())
    .bind(agent.as_uuid())
    .fetch_optional(&mut *db)
    .await?;
    let Some(principal) = principal else {
        return Ok(false);
    };
    let grants: Vec<Uuid> = sqlx::query_scalar(
        "SELECT principal_id FROM channel_principal_grants WHERE company_id=$1 AND channel_id=$2 \
         AND principal_id=$3 AND capability='participate' ORDER BY provenance FOR SHARE",
    )
    .bind(company.as_uuid())
    .bind(desk)
    .bind(principal)
    .fetch_all(db)
    .await?;
    Ok(!grants.is_empty())
}

/// Whether `desk` is the desk of a run in the caller's chain, or of a call recorded for one.
pub(super) async fn in_ancestry_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    desk: Uuid,
) -> AppResult<bool> {
    let (found, depth): (bool, i32) = sqlx::query_as(
        "WITH RECURSIVE chain(id, depth) AS ( \
           SELECT $2::uuid, 0 \
           UNION ALL \
           SELECT budget.parent_run_id, chain.depth + 1 FROM chain \
           JOIN workflow_run_budgets AS budget ON budget.company_id=$1 AND budget.run_id=chain.id \
           WHERE budget.parent_run_id IS NOT NULL AND chain.depth < $4) \
         SELECT EXISTS(SELECT 1 FROM chain JOIN workflow_runs AS run ON run.company_id=$1 AND run.id=chain.id \
             WHERE run.channel_id=$3) \
           OR EXISTS(SELECT 1 FROM chain JOIN workflow_channel_calls AS call \
             ON call.company_id=$1 AND call.child_run_id=chain.id WHERE call.channel_id=$3), \
           (SELECT max(depth) FROM chain)",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(desk)
    .bind(MAX_ANCESTRY)
    .fetch_one(db)
    .await?;
    Ok(found || depth >= MAX_ANCESTRY)
}

async fn account_live_on(db: &mut PgConnection, actor: WorkflowActor) -> AppResult<bool> {
    Ok(
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id=$1)")
            .bind(actor.user_id())
            .fetch_one(db)
            .await?,
    )
}

struct CallerLevels {
    ceiling: AccessLevel,
    content: AccessLevel,
    content_exact: bool,
}

/// The calling run's current levels, read under its fence.
async fn caller_levels_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<CallerLevels> {
    let (ceiling, content, content_exact): (i16, i16, bool) = sqlx::query_as(
        "SELECT knowledge_ceiling::smallint, content_level::smallint, content_exact FROM workflow_runs \
         WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_one(db)
    .await?;
    let level = |value: i16| AccessLevel::try_from(value).map_err(|e| AppError::Internal(e.into()));
    Ok(CallerLevels {
        ceiling: level(ceiling)?,
        content: level(content)?,
        content_exact,
    })
}

/// D1.1 step 8 before the child write: `run_as` may run on the desk, the selection still
/// verifies, its resources are ready for the desk, and its contract fits its output. Each
/// refusal is a fact about the destination; an infrastructure error propagates.
async fn destination_admits_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    configuration: &ConfiguredBinding,
    association: RelatedAssociation,
    responder: &crate::application::workflow::channel_reply::SelectedResponder,
) -> AppResult<bool> {
    let checked: AppResult<()> = async {
        management::authorize_association_on(tx, company, configuration.run_as(), association)
            .await?;
        bindings::verify_selection(tx, configuration).await?;
        let reach = resources::association_reach_on(tx, company, association).await?;
        resources::check_readiness(
            tx,
            resources::ReadinessSelection::of(
                configuration,
                reach,
                crate::app_error::UnitCheckStage::Admission,
            ),
        )
        .await?;
        responder.contract.validate(
            configuration
                .bundle()
                .compiled()
                .graph()
                .definition()
                .output_schema
                .as_ref()
                .unwrap_or(&Value::Bool(true)),
        )
    }
    .await;
    match checked {
        Ok(()) => Ok(true),
        Err(error) if destination_refusal(&error) => Ok(false),
        Err(error) => Err(error),
    }
}

/// Refusals that describe the destination's configuration or authority: its selection, run-as
/// authority, resources, contract, dependency graph (unavailable, cyclic or over its limits,
/// an ungrantable agent) or roles. Timeouts, database errors, ownership loss and defects are
/// not destination facts, and propagate.
pub(super) fn destination_refusal(error: &AppError) -> bool {
    matches!(
        error,
        AppError::BadRequest(_)
            | AppError::NotFound(_)
            | AppError::Forbidden(_)
            | AppError::ResourceOutsideUnit(_)
            | AppError::UnitArchived
            | AppError::WorkflowRole(_)
    )
}

/// The desks a calling agent's `channel.call` help lists (gate AWC-P05-D1 r2 D2; U6, U8): public
/// desks that launch would admit now, judged by the launch readers (`target_on`,
/// `decide_target`). Nothing is listed while the run answers an external correspondent.
pub(super) async fn desk_listing_on(
    tx: &mut Transaction<'_, Postgres>,
    scope: ActivationRequest,
    agent: crate::entities::agent_capability::AgentId,
) -> AppResult<crate::application::workflow::channel_call_listing::DeskListing> {
    use crate::application::workflow::channel_call_listing::{
        DeskListing, ListedDesk, MAX_DESK_CANDIDATES, intake_fields,
    };
    let in_call =
        super::channel_call_source::enclosing_call_source_on(tx, scope.company, scope.run)
            .await?
            .is_some();
    if !in_call && answers_external_on(tx, scope).await? {
        return Ok(DeskListing::External);
    }
    let public: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM channels WHERE company_id=$1 AND access_mode='public'",
    )
    .bind(scope.company.as_uuid())
    .fetch_one(&mut **tx)
    .await?;
    let candidates: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, name FROM channels WHERE company_id=$1 AND access_mode='public' ORDER BY name, id LIMIT $2",
    )
    .bind(scope.company.as_uuid())
    .bind(MAX_DESK_CANDIDATES)
    .fetch_all(&mut **tx)
    .await?;
    let mut desks = Vec::new();
    for (desk, name) in &candidates {
        let Some(target) = target_on(tx, scope, agent, *desk).await? else {
            continue;
        };
        if decide_target(Some(target.facts)) != TargetDecision::Proceed {
            continue;
        }
        let intake = target.responder.as_ref().and_then(|state| {
            declared_fields(
                state
                    .configuration
                    .bundle()
                    .compiled()
                    .graph()
                    .definition()
                    .input_schema
                    .as_ref(),
            )
            .cloned()
        });
        desks.push(ListedDesk::new(*desk, name, intake_fields(intake.as_ref())));
    }
    // Admitted desks, plus public desks past the candidate window (not judged here).
    let unjudged = (public as usize).saturating_sub(candidates.len());
    let callable = desks.len() + unjudged;
    Ok(DeskListing::listed(desks, callable))
}

/// Whether the run can answer an external correspondent (gate AWC-P05-D1 F9): its root's trigger
/// message, its own or its root's thread, or its input's source message is not internal-only.
async fn answers_external_on(db: &mut PgConnection, scope: ActivationRequest) -> AppResult<bool> {
    Ok(sqlx::query_scalar(
        "WITH tree AS ( \
           SELECT run.id, run.thread_id, run.input FROM workflow_runs AS run \
           WHERE run.company_id=$1 AND run.id IN ($2, \
             (SELECT budget.root_run_id FROM workflow_run_budgets AS budget WHERE budget.company_id=$1 AND budget.run_id=$2))) \
         SELECT EXISTS(SELECT 1 FROM tree \
             JOIN workflow_message_intent_bindings AS outcome ON outcome.company_id=$1 AND outcome.run_id=tree.id \
             JOIN workflow_message_intents AS intent ON intent.company_id=outcome.company_id AND intent.id=outcome.intent_id \
             JOIN messages AS message ON message.company_id=intent.company_id AND message.id=intent.message_id \
             WHERE message.audience <> 'internal_only') \
           OR EXISTS(SELECT 1 FROM tree \
             JOIN thread_messages AS filed ON filed.company_id=$1 AND filed.thread_id=tree.thread_id \
             JOIN messages AS message ON message.company_id=filed.company_id AND message.id=filed.message_id \
             WHERE message.audience <> 'internal_only') \
           OR EXISTS(SELECT 1 FROM tree JOIN messages AS message \
             ON message.company_id=$1 AND message.id::text = tree.input->>'source_message' \
             WHERE message.audience <> 'internal_only')",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_one(db)
    .await?)
}
