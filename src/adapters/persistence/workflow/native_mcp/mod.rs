//! Real workflow admission and immutable provenance for the native MCP ingress.
use super::*;
use crate::application::agent::ResolvedAgentTool;
use crate::application::local_agents::mcp::{NativeMcpInvocation, NativeMcpResponse};
use crate::application::workflow::native_mcp::{NativeBrokerIdentity, PreparedNativeMcp};
use crate::domain::local_agents::{
    InstallationId, MachineId, NativeHarnessSelection, NativeMcpCallId, OperationId, SessionId,
};
use chrono::{DateTime, Utc};
use serde_json::Value;

mod live;
mod maintenance;
pub(super) use live::authorize_on as live_on;
mod provenance;
pub(in crate::adapters::persistence) use maintenance::sweep;
mod reads;
pub(super) use reads::authorize_history_on;
mod retention;
pub(super) use provenance::restore_on as structural_on;
pub(super) use retention::retire_on;
mod registry;
pub(in crate::adapters::persistence) use reads::{invocation_id_on, recover_on, response_on};

pub(in crate::adapters::persistence) async fn source_facts_on(
    db: &mut PgConnection,
    company: CompanyId,
    actor: WorkflowActor,
    source: &crate::domain::local_agents::SessionAssociation,
) -> AppResult<(
    RelatedAssociation,
    crate::entities::access_level::AccessLevel,
)> {
    use crate::domain::local_agents::SessionAssociation;
    let actor_level = super::knowledge::actor_level_on(db, company, Some(actor.user_id())).await?;
    match source {
        SessionAssociation::Conversation { conversation_id } => {
            let channel: Uuid =
                sqlx::query_scalar("SELECT channel_id FROM threads WHERE company_id=$1 AND id=$2")
                    .bind(company.as_uuid())
                    .bind(conversation_id)
                    .fetch_optional(db)
                    .await?
                    .ok_or_else(missing)?;
            Ok((
                RelatedAssociation::Thread {
                    channel_id: RelatedChannelId::new(channel),
                    thread_id: RelatedThreadId::new(*conversation_id),
                },
                actor_level,
            ))
        }
        SessionAssociation::Workflow { run_id } => {
            let (channel,thread,ceiling):(Option<Uuid>,Option<Uuid>,i16) = sqlx::query_as("SELECT channel_id,thread_id,knowledge_ceiling::smallint FROM workflow_runs WHERE company_id=$1 AND id=$2")
                .bind(company.as_uuid()).bind(run_id).fetch_optional(db).await?.ok_or_else(missing)?;
            let level = crate::entities::access_level::AccessLevel::try_from(ceiling)
                .map_err(|_| invalid())?;
            Ok((
                binding_rows::association_from_columns(channel, thread)?,
                std::cmp::min(actor_level, level),
            ))
        }
    }
}

pub(in crate::adapters::persistence) struct NativeMcpOwner {
    pub company: CompanyId,
    pub actor: WorkflowActor,
    pub operation: OperationId,
    pub machine: MachineId,
    pub installation: InstallationId,
    pub session: SessionId,
    pub harness: NativeHarnessSelection,
    pub invocation: NativeMcpInvocation,
    pub association: RelatedAssociation,
    pub ceiling: crate::entities::access_level::AccessLevel,
    pub deadline: DateTime<Utc>,
}

/// Caller already owns normal company/source/grant -> machine -> operation authority.
/// No existing broker run is locked. The job and its provenance publish together.
pub(in crate::adapters::persistence) async fn admit_on(
    db: &mut PgConnection,
    owner: &NativeMcpOwner,
    tool: &ResolvedAgentTool,
    arguments: &Value,
) -> AppResult<NativeMcpResponse> {
    let (broker, prepared) = registry::prepare_on(db, owner, tool, arguments).await?;
    if let Some(saved) = reads::replay_on(db, owner, &broker, &prepared).await? {
        return Ok(saved);
    }
    reads::capacity_on(db, owner).await?;
    let call = NativeMcpCallId::new(Uuid::new_v4()).map_err(|_| invalid())?;
    let run = RunId::new(Uuid::new_v4());
    let execution = ExecutionId::new(Uuid::new_v4());
    let correlation = Uuid::new_v4();
    let trigger = TriggerRef::new(
        owner.company,
        TriggerId::new(Uuid::new_v4()),
        TriggerSource::Manual,
    )
    .map_err(|_| invalid())?;
    insert_call(db, owner, &broker, &prepared, call, run).await?;
    admission_write::insert_run(
        db,
        &admission_write::RunRow {
            knowledge: super::knowledge::stored_levels(
                owner.ceiling.as_i16(),
                owner.ceiling.as_i16(),
            )?,
            content_exact: false,
            company: owner.company,
            run,
            binding: prepared.binding(),
            association: owner.association,
            actor_id: owner.actor.user_id(),
            trigger: trigger.trigger_id(),
            correlation,
            input: prepared.input(),
            deadline: admission_write::RunDeadline::Inherited(owner.deadline),
            parent: None,
            rerun_of: None,
            delivery_policy: DeliveryPolicy::InAppOnly,
            native_mcp_call: Some(call),
        },
    )
    .await?;
    admission_write::insert_first_job(
        db,
        &admission_write::FirstJob {
            company: owner.company,
            run,
            execution,
            entry: &prepared
                .binding()
                .bundle()
                .compiled()
                .graph()
                .definition()
                .entry,
            association: owner.association,
            correlation,
        },
    )
    .await?;
    admission_write::insert_admission(
        db,
        owner.company,
        prepared.binding().id(),
        &super::admission_binding::SourceKey::from_trigger(&trigger),
        run,
    )
    .await?;
    admission_write::insert_admitted(db, owner.company, run, owner.actor.user_id()).await?;
    reads::response_on(db, owner.company, call, Some(owner)).await
}

async fn insert_call(
    db: &mut PgConnection,
    owner: &NativeMcpOwner,
    broker: &registry::Broker,
    prepared: &PreparedNativeMcp,
    call: NativeMcpCallId,
    run: RunId,
) -> AppResult<()> {
    let NativeHarnessSelection::Agent { digest, .. } = &owner.harness else {
        return Err(missing());
    };
    let (channel, thread) = binding_rows::association_columns(owner.association);
    sqlx::query("INSERT INTO native_mcp_calls(company_id,id,broker_id,operation_id,machine_id,installation_id,session_id,
        native_thread_id,native_turn_id,client_request_id,model_call_id,actor_id,harness_digest,argument_digest,
        original_run_id,run_id,channel_id,thread_id,input,deadline,source_operation_id,source_session_id)
        VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$15,$16,$17,$18,$19,$4,$7)")
        .bind(owner.company.as_uuid()).bind(call.as_uuid()).bind(broker.id)
        .bind(owner.operation.as_uuid()).bind(owner.machine.as_uuid()).bind(owner.installation.as_uuid())
        .bind(owner.session.as_uuid()).bind(owner.invocation.thread_id.as_str()).bind(owner.invocation.turn_id.as_str())
        .bind(owner.invocation.client_request_id.as_str()).bind(owner.invocation.model_call_id.as_str())
        .bind(owner.actor.user_id()).bind(digest.as_str()).bind(prepared.argument_digest())
        .bind(run.as_uuid()).bind(channel).bind(thread).bind(prepared.input()).bind(owner.deadline).execute(db).await?;
    Ok(())
}
