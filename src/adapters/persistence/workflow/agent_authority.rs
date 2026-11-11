//! Current authority for an immutable agent revision, independent of mutable drafts.
use super::*;
use crate::application::agent::{ResolvedAgentDefinition, ResolvedAgentTool};
use crate::application::workflow::actions::ActionScope;
use crate::application::workflow::agent_contract::agent_action_snapshot;
use crate::application::workflow::publication::AgentUseMode;
use crate::entities::agent_capability::AgentBindingSource;

fn revoked() -> AppError {
    AppError::BadRequest("Selected agent authority is no longer available".into())
}

/// Current refusal cannot erase an already possible remote effect on this execution.
/// The native effect projection is a failure classification, never permission to retry.
pub(super) async fn authority_error_on(
    db: &mut PgConnection,
    scope: ActionScope,
    error: AppError,
) -> AppResult<AppError> {
    if matches!(
        &error,
        AppError::BadRequest(_) | AppError::NotFound(_) | AppError::InvalidCredentials
    ) {
        let unknown: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM workflow_action_effect_states WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND effect_state IN ('possible_dispatch','needs_reconciliation','applied_without_result'))",
        ).bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
            .fetch_one(db).await?;
        if unknown {
            return Ok(AppError::Execution(
                crate::app_error::ExecutionFailure::IndeterminateEffect,
            ));
        }
    }
    Ok(error)
}

pub(super) async fn check_execution_on(
    db: &mut PgConnection,
    scope: ActionScope,
    definition: &ResolvedAgentDefinition,
    mode: AgentUseMode,
) -> AppResult<()> {
    match check_definition_on(db, definition, mode).await {
        Ok(()) => Ok(()),
        Err(error) => Err(authority_error_on(db, scope, error).await?),
    }
}

pub(super) async fn check_definition_on(
    tx: &mut PgConnection,
    definition: &ResolvedAgentDefinition,
    mode: AgentUseMode,
) -> AppResult<()> {
    let company = definition.company_id;
    let agent = definition.agent_id.as_uuid();
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM agents WHERE company_id=$1 AND id=$2 FOR SHARE")
        .bind(company.as_uuid())
        .bind(agent)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(revoked)?;
    let model = definition.hosted_model()?;
    let available: bool = sqlx::query_scalar(
        "SELECT CASE WHEN cardinality(models) BETWEEN 1 AND $4 THEN $3=ANY(models) ELSE FALSE END \
         FROM company_model_connections WHERE company_id=$1 AND provider=$2 FOR SHARE",
    )
    .bind(company.as_uuid())
    .bind(model.provider.as_str())
    .bind(&model.model)
    .bind(crate::use_cases::company::MAX_MODELS_PER_CONNECTION as i32)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(revoked)?;
    if !available {
        return Err(revoked());
    }
    if mode == AgentUseMode::ModelOnly {
        return Ok(());
    }
    let ordered = super::super::agent_publication::tools::ordered_bindings(
        definition.tools.iter().map(|tool| &tool.binding),
    );
    for binding in ordered {
        let tool = definition
            .tools
            .iter()
            .find(|tool| tool.binding.id == binding.id)
            .ok_or_else(invalid)?;
        check_source_on(tx, company, tool).await?;
    }
    check_grants_on(tx, definition).await
}

async fn check_grants_on(
    tx: &mut PgConnection,
    definition: &ResolvedAgentDefinition,
) -> AppResult<()> {
    let company = definition.company_id.as_uuid();
    let agent = definition.agent_id.as_uuid();
    let mut tools: Vec<_> = definition
        .tools
        .iter()
        .map(|tool| tool.binding.id.as_uuid())
        .collect();
    tools.sort();
    for binding in tools {
        sqlx::query_scalar::<_, Uuid>("SELECT binding_id FROM agent_binding_grants WHERE company_id=$1 AND agent_id=$2 AND binding_id=$3 AND revoked_at IS NULL FOR SHARE")
            .bind(company).bind(agent).bind(binding).fetch_optional(&mut *tx).await?.ok_or_else(revoked)?;
    }
    let mut skills: Vec<_> = definition.skills.iter().collect();
    skills.sort_by_key(|skill| skill.assignment_id);
    for skill in skills {
        // Do not acquire mutable Skill after Assignment: source deletion uses Skill -> Assignment.
        let live: Option<Uuid> = sqlx::query_scalar("SELECT live_skill_id FROM agent_skill_assignments WHERE company_id=$1 AND agent_id=$2 AND id=$3 AND skill_id=$4 FOR SHARE")
            .bind(company).bind(agent).bind(skill.assignment_id.as_uuid()).bind(skill.skill.skill_id)
            .fetch_optional(&mut *tx).await?.flatten();
        if live != Some(skill.skill.skill_id) {
            return Err(revoked());
        }
        sqlx::query_scalar::<_, Uuid>("SELECT assignment_id FROM agent_skill_grants WHERE company_id=$1 AND agent_id=$2 AND assignment_id=$3 AND skill_id=$4 AND revoked_at IS NULL FOR SHARE")
            .bind(company).bind(agent).bind(skill.assignment_id.as_uuid()).bind(skill.skill.skill_id)
            .fetch_optional(&mut *tx).await?.ok_or_else(revoked)?;
    }
    Ok(())
}

pub(super) async fn check_source_on(
    tx: &mut PgConnection,
    company: CompanyId,
    frozen: &ResolvedAgentTool,
) -> AppResult<()> {
    let wanted = agent_action_snapshot(company, frozen)?;
    if matches!(frozen.binding.source, AgentBindingSource::Workflow { .. }) {
        // Run semantics retain old version/params after a same-workflow repin or archive.
        return workflow_tools::registered_on(tx, company, &wanted, workflow_tools::ToolCheck::Run)
            .await;
    }
    let current = super::super::agent_publication::tools::direct_registration_on(
        tx,
        company,
        &frozen.binding.source,
    )
    .await?;
    if matches!(frozen.binding.source, AgentBindingSource::Builtin { .. })
        && frozen.approval_required
    {
        return Err(revoked());
    }
    if serde_json::to_value(wanted).map_err(|_| invalid())?
        != serde_json::to_value(current.tool).map_err(|_| invalid())?
    {
        return Err(revoked());
    }
    Ok(())
}
