//! Native `agent.run` preparation and its current-authority check. The check narrows a
//! frozen selection to what the agent and company still allow; it never widens it.
use super::*;
use crate::application::workflow::{
    agent::{AgentLaunch, AgentPreparation, AgentPreparationResult, AgentToolTarget},
    lease::{ClaimedWorkflow, LeasePolicy},
};
use crate::entities::value_objects::ModelProvider;
use secrecy::SecretString;
use serde_json::Value;

#[derive(sqlx::FromRow)]
struct AuthorityFacts {
    actor_id: Uuid,
    bundle: Vec<u8>,
    channel_id: Option<Uuid>,
    principal_id: Option<Uuid>,
    thread_id: Option<Uuid>,
}

/// Reload the immutable definition named by the checkpoint; mutable draft edits grant nothing.
pub(in crate::adapters::persistence::workflow) async fn current_on(
    db: &mut PgConnection,
    scope: crate::application::workflow::actions::ActionScope,
    prepared: &Value,
) -> AppResult<()> {
    let identity: crate::application::workflow::agent_contract::AgentSelectionIdentity =
        serde_json::from_value(prepared["selection"].clone()).map_err(|_| invalid())?;
    let facts = sqlx::query_as::<_,AuthorityFacts>(
        "SELECT run.actor_id,stored.bundle,run.channel_id,run.principal_id,run.thread_id FROM workflow_runs AS run JOIN workflow_bundles AS stored ON stored.company_id=run.company_id AND stored.digest=run.bundle_digest WHERE run.company_id=$1 AND run.id=$2 AND octet_length(stored.bundle)<=$3"
    ).bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(MAX_BUNDLE_BYTES as i32)
        .fetch_optional(&mut *db).await?.ok_or_else(missing)?;
    let actor = WorkflowActor::authenticated(facts.actor_id).map_err(|_| invalid())?;
    let bundle = restore_bundle(
        &facts.bundle,
        &crate::adapters::workflow_source::WorkflowSourceDecoder,
    )?;
    let selection =
        super::agent_manifest::execution_selection_on(db, scope, actor, &bundle).await?;
    if selection.manifest_id() != identity.manifest
        || selection.agent().agent_id != identity.agent
        || selection.revision().id != identity.revision
        || selection.revision().digest != identity.digest
        || selection.mode() != identity.mode
    {
        return Err(invalid());
    }
    super::agent_authority::check_execution_on(db, scope, selection.agent(), identity.mode).await?;
    authorize_model_source_on(
        db,
        scope,
        actor,
        association::association_from_columns(association::AssociationColumns {
            channel_id: facts.channel_id,
            principal_id: facts.principal_id,
            thread_id: facts.thread_id,
        })?,
        selection.agent().agent_id,
    )
    .await?;
    agent_history::check_prepared_on(db, scope, selection.mode(), prepared).await
}

/// Human sessions keep their View requirement. An owned automatic reply may instead
/// execute as its exact selected agent with that agent's current desk grants.
async fn owned_agent_desk_on(
    db: &mut PgConnection,
    scope: crate::application::workflow::actions::ActionScope,
    actor: WorkflowActor,
    association: RelatedAssociation,
    agent: crate::entities::agent_capability::AgentId,
) -> AppResult<()> {
    let RelatedAssociation::Thread {
        target: AddressTarget::Desk(desk),
        thread_id,
    } = association
    else {
        return Err(missing());
    };
    let owned: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM channel_reply_runs AS owned \
         JOIN channel_reply_contexts AS source ON source.company_id=owned.company_id \
           AND source.intent_id=owned.intent_id AND source.binding_id=owned.binding_id \
           AND source.binding_revision=owned.binding_revision \
         JOIN workflow_runs AS run ON run.company_id=owned.company_id AND run.id=owned.run_id \
           AND run.binding_id=owned.binding_id AND run.binding_revision=owned.binding_revision \
         WHERE owned.company_id=$1 AND owned.run_id=$2 AND source.actor_id=$3 \
           AND run.actor_id=source.actor_id AND run.channel_id=$4 AND source.channel_id=run.channel_id \
           AND source.principal_id IS NULL AND run.principal_id IS NULL \
           AND source.thread_id=run.thread_id AND run.thread_id=$5 AND run.parent_run_id IS NULL)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(actor.user_id())
        .bind(desk).bind(thread_id.as_uuid()).fetch_one(&mut *db).await?;
    if !owned {
        return Err(missing());
    }
    let principal: Uuid = sqlx::query_scalar(
        "SELECT id FROM principals WHERE company_id=$1 AND agent_id=$2 AND kind='agent' FOR SHARE",
    )
    .bind(scope.company.as_uuid())
    .bind(agent.as_uuid())
    .fetch_optional(&mut *db)
    .await?
    .ok_or_else(missing)?;
    let capabilities: Vec<String> = sqlx::query_scalar(
        "SELECT capability FROM channel_principal_grants WHERE company_id=$1 AND channel_id=$2 \
         AND principal_id=$3 AND capability IN ('view','participate') ORDER BY capability,provenance FOR SHARE")
        .bind(scope.company.as_uuid()).bind(desk).bind(principal).fetch_all(db).await?;
    if !capabilities.iter().any(|capability| capability == "view")
        || !capabilities
            .iter()
            .any(|capability| capability == "participate")
    {
        return Err(missing());
    }
    Ok(())
}

pub(super) async fn authorize_model_source_on(
    db: &mut PgConnection,
    scope: crate::application::workflow::actions::ActionScope,
    actor: WorkflowActor,
    target: RelatedAssociation,
    agent: crate::entities::agent_capability::AgentId,
) -> AppResult<()> {
    management::authorize_association_on(db, scope.company, actor, target).await?;
    match association::authorize_association(db, scope.company, actor, target).await {
        Ok(()) => Ok(()),
        Err(AppError::NotFound(_)) => owned_agent_desk_on(db, scope, actor, target, agent).await,
        Err(error) => Err(error),
    }
}

/// Terminal: retrying cannot restore a revoked grant or connection.
fn revoked() -> AppError {
    AppError::BadRequest("Selected agent capability was revoked".into())
}

/// Concrete dispatch edges for the frozen selection; unsupported bindings fail here.
pub(super) fn targets(
    company: CompanyId,
    tools: &[crate::application::agent::ResolvedAgentTool],
) -> AppResult<Vec<AgentToolTarget>> {
    use crate::entities::agent_capability::AgentBindingSource;
    tools
        .iter()
        .map(|tool| {
            Ok(match &tool.binding.source {
                AgentBindingSource::Http { connection, .. } => AgentToolTarget::Http {
                    connection: *connection,
                },
                AgentBindingSource::Mcp { connection, .. } => AgentToolTarget::Mcp {
                    connection: *connection,
                },
                AgentBindingSource::Workflow { .. } => AgentToolTarget::Workflow {
                    pin: crate::application::workflow::agent_contract::agent_action_snapshot(
                        company, tool,
                    )?
                    .workflow
                    .ok_or_else(invalid)?,
                },
                AgentBindingSource::Builtin { tool } => AgentToolTarget::builtin(*tool)?,
            })
        })
        .collect()
}

#[async_trait]
impl AgentPreparation for PostgresPersistence {
    async fn prepare_agent(
        &self,
        claim: &ClaimedWorkflow,
        policy: LeasePolicy,
    ) -> AppResult<AgentPreparationResult> {
        self.prepare_agent_with_recall(claim, policy, Default::default())
            .await
    }

    async fn prepare_agent_with_recall(
        &self,
        claim: &ClaimedWorkflow,
        policy: LeasePolicy,
        memory: crate::application::workflow::memory_recall::MemoryPreparation,
    ) -> AppResult<AgentPreparationResult> {
        let scope = claim.fence.scope;
        let action = crate::application::workflow::actions::ActionScope {
            company: scope.company,
            run: scope.run,
            execution: scope.execution,
        };
        let prepared =
            match Box::pin(super::agent_history::prepare(self, claim, policy, memory)).await? {
                super::agent_history::Preparation::Ready(prepared) => *prepared,
                super::agent_history::Preparation::Parked(wait) => {
                    return Ok(AgentPreparationResult::Parked(wait));
                }
                super::agent_history::Preparation::AtCapacity => {
                    return Ok(AgentPreparationResult::AtCapacity);
                }
            };
        let provider = ModelProvider::from(
            prepared
                .selection()
                .agent()
                .hosted_model()?
                .provider
                .as_str(),
        );
        let secret = crate::application::use_cases::company::CompanyPersistence::model_api_key(
            self,
            scope.company.as_uuid(),
            &provider,
        )
        .await?;
        let Some(secret) = secret else {
            return Err(super::agent_authority::authority_error_on(
                &mut *self.pool().acquire().await?,
                action,
                revoked(),
            )
            .await?);
        };
        Ok(AgentPreparationResult::Ready(Box::new(AgentLaunch {
            prepared,
            secret: SecretString::from(secret),
        })))
    }
}
