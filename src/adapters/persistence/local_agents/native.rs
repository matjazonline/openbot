//! Publication locks precede machine/profile locks for every native selection.
use crate::adapters::persistence::agent_publication::{LockedAgentPublication, storage};
use crate::app_error::{AppError, AppResult};
use crate::application::agent::{AgentPublicationScope, PublishedAgentRevision};
use crate::application::local_agents::harness::NativeBundle;
use crate::application::workflow::WorkflowActor;
use crate::domain::local_agents::{LocalAgentProfile, NativeHarnessSelection};
use crate::domain::workflow::CompanyId;
use crate::entities::agent_capability::AgentId;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

pub(in crate::adapters::persistence) async fn capture_on(
    tx: &mut Transaction<'_, Postgres>,
    actor: Uuid,
    profile: &mut LocalAgentProfile,
) -> AppResult<NativeHarnessSelection> {
    let Some(agent) = profile.agent_id else {
        return Ok(NativeHarnessSelection::Unconfigured);
    };
    let revision = active_on(tx, profile.company_id, actor, agent).await?;
    super::super::workflow::agent_authority::check_native_definition_on(tx, &revision.definition)
        .await?;
    let bundle = NativeBundle::from_definition(&revision.definition)?;
    if !bundle.tools.is_empty()
        && profile.selection.provider != crate::domain::local_agents::LocalProvider::ClaudeCode
    {
        return Err(AppError::BadRequest(
            "Native company MCP tools currently require the Claude Code provider".into(),
        ));
    }
    profile.selection.policy.permissions = bundle.permissions;
    Ok(NativeHarnessSelection::Agent {
        agent_id: agent,
        revision_id: revision.id,
        digest: bundle.digest()?,
    })
}

pub(in crate::adapters::persistence) async fn selected_on(
    tx: &mut sqlx::PgConnection,
    company: Uuid,
    actor: Uuid,
    selection: &NativeHarnessSelection,
) -> AppResult<Option<NativeBundle>> {
    selected_with_locks_on(
        tx,
        company,
        actor,
        selection,
        super::super::workflow::row_locks::RowLocks::Wait,
    )
    .await
}

pub(in crate::adapters::persistence) async fn selected_with_locks_on(
    tx: &mut sqlx::PgConnection,
    company: Uuid,
    actor: Uuid,
    selection: &NativeHarnessSelection,
    locks: super::super::workflow::row_locks::RowLocks,
) -> AppResult<Option<NativeBundle>> {
    let NativeHarnessSelection::Agent {
        agent_id,
        revision_id,
        digest,
    } = selection
    else {
        return Ok(None);
    };
    let scope = AgentPublicationScope {
        company: CompanyId::new(company),
        actor: WorkflowActor::authenticated(actor)?,
        agent: *agent_id,
    };
    // The owner lock precedes revision/source/grant locks, matching publication and capture.
    sqlx::query_scalar::<_, Uuid>(
        &locks.sql("SELECT id FROM agents WHERE company_id=$1 AND id=$2 FOR SHARE"),
    )
    .bind(company)
    .bind(agent_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound("Selected native agent is unavailable".into()))?;
    let revision = storage::revision_with_locks_on(tx, scope, *revision_id, locks)
        .await?
        .ok_or_else(|| AppError::NotFound("Selected native revision is unavailable".into()))?;
    super::super::workflow::agent_authority::check_native_definition_with_locks_on(
        tx,
        &revision.definition,
        locks,
    )
    .await
    .map_err(|error| match error {
        AppError::BadRequest(message) => AppError::Forbidden(message),
        other => other,
    })?;
    let bundle = NativeBundle::from_definition(&revision.definition)?;
    if bundle.digest()? != *digest {
        return Err(AppError::Internal("Native bundle digest mismatch".into()));
    }
    Ok(Some(bundle))
}

pub(in crate::adapters::persistence) async fn active_on(
    tx: &mut Transaction<'_, Postgres>,
    company: Uuid,
    actor: Uuid,
    agent: AgentId,
) -> AppResult<PublishedAgentRevision> {
    let scope = AgentPublicationScope {
        company: CompanyId::new(company),
        actor: WorkflowActor::authenticated(actor)?,
        agent,
    };
    LockedAgentPublication::lock(tx, scope).await?;
    let revision = storage::active_on(tx, scope).await?.ok_or_else(|| {
        AppError::BadRequest(
            "Publish the selected company agent before using it in a local profile".into(),
        )
    })?;
    storage::revision_on(tx, scope, revision)
        .await?
        .ok_or_else(|| AppError::Internal("Selected agent revision is missing".into()))
}
