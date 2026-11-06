//! Native `agent.run` preparation and its current-authority check. The check narrows a
//! frozen selection to what the agent and company still allow; it never widens it.
use super::http::runtime;
use super::*;
use crate::application::workflow::{
    agent::{AgentLaunch, AgentPreparation, AgentToolTarget, PreparedAgent, PreparedStep},
    lease::ClaimedWorkflow,
    polling::WorkflowStepKind,
    registry::ModelStep,
};
use crate::entities::value_objects::ModelProvider;
use secrecy::SecretString;
use serde_json::Value;

/// Reload the immutable definition named by the checkpoint; mutable draft edits grant nothing.
pub(in crate::adapters::persistence::workflow) async fn current_on(
    db: &mut PgConnection,
    scope: crate::application::workflow::actions::ActionScope,
    prepared: &Value,
) -> AppResult<()> {
    let identity: crate::application::workflow::agent_contract::AgentSelectionIdentity =
        serde_json::from_value(prepared["selection"].clone()).map_err(|_| invalid())?;
    let (actor, bundle): (Uuid, Vec<u8>) = sqlx::query_as(
        "SELECT run.actor_id,stored.bundle FROM workflow_runs AS run JOIN workflow_bundles AS stored ON stored.company_id=run.company_id AND stored.digest=run.bundle_digest WHERE run.company_id=$1 AND run.id=$2 AND octet_length(stored.bundle)<=$3"
    ).bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(MAX_BUNDLE_BYTES as i32)
        .fetch_optional(&mut *db).await?.ok_or_else(missing)?;
    let actor = WorkflowActor::authenticated(actor).map_err(|_| invalid())?;
    let bundle = restore_bundle(
        &bundle,
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
    super::agent_authority::check_execution_on(db, scope, selection.agent(), identity.mode).await
}

/// Terminal: retrying cannot restore a revoked grant or connection.
fn revoked() -> AppError {
    AppError::BadRequest("Selected agent capability was revoked".into())
}

/// Concrete dispatch edges for the frozen selection; unsupported bindings fail here.
fn targets(
    company: CompanyId,
    tools: &[crate::application::agent::ResolvedAgentTool],
) -> AppResult<Vec<AgentToolTarget>> {
    use crate::entities::agent_capability::{AgentBindingSource, AgentBuiltinTool};
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
                AgentBindingSource::Builtin { tool } => match tool {
                    AgentBuiltinTool::MessageSend | AgentBuiltinTool::MessageReply => {
                        AgentToolTarget::Message {
                            name: TypeName::parse(tool.name()).map_err(|_| invalid())?,
                        }
                    }
                    AgentBuiltinTool::ListCompanyAgents | AgentBuiltinTool::CreateAgentChannel => {
                        AgentToolTarget::Provisioning {
                            name: TypeName::parse(tool.name()).map_err(|_| invalid())?,
                        }
                    }
                    AgentBuiltinTool::SearchKnowledge => AgentToolTarget::Knowledge,
                },
            })
        })
        .collect()
}

#[async_trait]
impl AgentPreparation for PostgresPersistence {
    async fn prepare_agent(&self, claim: &ClaimedWorkflow) -> AppResult<AgentLaunch> {
        let scope = claim.fence.scope;
        let action = crate::application::workflow::actions::ActionScope {
            company: scope.company,
            run: scope.run,
            execution: scope.execution,
        };
        let mut tx = super::http::begin_http(self).await?;
        let facts = runtime::facts(&mut tx, action).await?;
        let definition = facts
            .bundle
            .compiled()
            .graph()
            .definition()
            .steps
            .get(&claim.activation.step)
            .ok_or_else(invalid)?;
        let step = ModelStep::of(definition.step_type.as_str()).ok_or_else(invalid)?;
        let kind = WorkflowStepKind(step.kind().into());
        let inputs = runtime::frozen_inputs(&mut tx, &facts, claim, &kind).await?;
        let selection = super::agent_manifest::execution_selection_on(
            &mut tx,
            action,
            facts.actor,
            &facts.bundle,
        )
        .await?;
        let prepared = match step {
            ModelStep::AgentRun => {
                let targets = targets(scope.company, selection.tools())?;
                PreparedAgent::new(selection, &inputs, PreparedStep::AgentRun { targets })?
            }
            ModelStep::Classify => {
                PreparedAgent::new(selection.model_only(), &inputs, PreparedStep::Classify)?
            }
            ModelStep::Decide => {
                let contract = facts
                    .bundle
                    .compiled()
                    .step_output_schema(&claim.activation.step)
                    .cloned()
                    .ok_or_else(invalid)?;
                PreparedAgent::new(
                    selection.model_only(),
                    &inputs,
                    PreparedStep::Decide { contract },
                )?
            }
        };
        // Current authority is checked by the store before local locks (06-D5), not here
        // after the run lock.
        tx.commit().await?;
        let provider = ModelProvider::from(prepared.selection().agent().model.provider.as_str());
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
        Ok(AgentLaunch {
            prepared,
            secret: SecretString::from(secret),
        })
    }
}
