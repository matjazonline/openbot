//! Native `agent.run` preparation and its current-authority check. The check narrows a
//! frozen selection to what the agent and company still allow; it never widens it.
use super::http::{registered_on, runtime};
use super::*;
use crate::application::workflow::{
    agent::{AgentLaunch, AgentPreparation, AgentToolTarget, PreparedAgent, PreparedStep},
    lease::ClaimedWorkflow,
    message::{MessageKind, is_trusted_message_snapshot},
    polling::WorkflowStepKind,
    registry::ModelStep,
};
use crate::entities::value_objects::ModelProvider;
use secrecy::SecretString;
use serde_json::Value;

/// The selection fields of a prepared document this check relies on.
struct Selected {
    agent: Uuid,
    provider: String,
    model: String,
    tools: Vec<String>,
    skills: Vec<Uuid>,
}
impl Selected {
    fn read(prepared: &Value) -> AppResult<Self> {
        let selection = &prepared["selection"];
        let agent = &selection["agent"];
        let tools = agent["tools"]
            .as_array()
            .ok_or_else(invalid)?
            .iter()
            .map(|tool| tool.as_str().map(str::to_owned).ok_or_else(invalid))
            .collect::<AppResult<_>>()?;
        let skills = selection["skills"]
            .as_array()
            .ok_or_else(invalid)?
            .iter()
            .map(|skill| {
                skill["skill_id"]
                    .as_str()
                    .and_then(|id| Uuid::parse_str(id).ok())
                    .ok_or_else(invalid)
            })
            .collect::<AppResult<_>>()?;
        Ok(Self {
            agent: agent["agent_id"]
                .as_str()
                .and_then(|id| Uuid::parse_str(id).ok())
                .ok_or_else(invalid)?,
            provider: agent["model"]["provider"]
                .as_str()
                .ok_or_else(invalid)?
                .into(),
            model: agent["model"]["model"].as_str().ok_or_else(invalid)?.into(),
            tools,
            skills,
        })
    }
}

/// Authority-class locks (agent row, skill links, model connection) taken before local
/// workflow locks (06-D5): the frozen agent must still belong to the company, still grant
/// every frozen tool and still link every frozen skill, and its company model connection
/// must still offer the frozen model. A removal committed first is observed; one committed
/// later waits for this transaction. Additions after publication grant nothing until
/// republish. ActionService still rechecks each tool's registration and policy.
pub(in crate::adapters::persistence::workflow) async fn current_on(
    db: &mut PgConnection,
    company: CompanyId,
    prepared: &Value,
) -> AppResult<()> {
    let selected = Selected::read(prepared)?;
    let grants: Option<Vec<String>> = sqlx::query_scalar(
        "SELECT workflow_tools FROM agents WHERE id=$1 AND company_id=$2 FOR SHARE",
    )
    .bind(selected.agent)
    .bind(company.as_uuid())
    .fetch_optional(&mut *db)
    .await?;
    let grants = grants.ok_or_else(revoked)?;
    if selected.tools.iter().any(|tool| !grants.contains(tool)) {
        return Err(revoked());
    }
    let linked: Vec<Uuid> = sqlx::query_scalar(
        "SELECT skill_id FROM agent_skills WHERE agent_id=$1 AND skill_id=ANY($2)",
    )
    .bind(selected.agent)
    .bind(&selected.skills)
    .fetch_all(&mut *db)
    .await?;
    if selected.skills.iter().any(|skill| !linked.contains(skill)) {
        return Err(revoked());
    }
    let models: Option<Vec<String>> = sqlx::query_scalar(
        "SELECT models FROM company_model_connections WHERE company_id=$1 AND provider=$2 FOR SHARE",
    )
    .bind(company.as_uuid())
    .bind(&selected.provider)
    .fetch_optional(&mut *db)
    .await?;
    if !models.is_some_and(|models| models.contains(&selected.model)) {
        return Err(revoked());
    }
    Ok(())
}

/// Terminal: retrying cannot restore a revoked grant or connection.
fn revoked() -> AppError {
    AppError::BadRequest("Selected agent capability was revoked".into())
}

/// Concrete dispatch edges for the frozen selection; unsupported bindings fail here.
async fn targets(
    tx: &mut Transaction<'_, Postgres>,
    facts: &runtime::RunFacts,
    company: CompanyId,
    tools: &[ToolSnapshot],
) -> AppResult<Vec<AgentToolTarget>> {
    let mut targets = Vec::with_capacity(tools.len());
    for tool in tools {
        let target = match tool.kind() {
            ToolKind::Message => {
                if MessageKind::parse(tool.contract.name.as_str()).is_none()
                    || !is_trusted_message_snapshot(tool)?
                {
                    return Err(unsupported());
                }
                AgentToolTarget::Message {
                    name: tool.contract.name.clone(),
                }
            }
            // Only an MCP slot is a direct agent tool; a slot-bound HTTP selection belongs to
            // its `http.request` step and is unsupported here (06-R016 F017).
            ToolKind::Slot(slot) => {
                let definition = facts.bundle.compiled().graph().definition();
                if super::resources::slot_kind(definition, slot) != Some("mcp")
                    || !facts.resources.contains_key(slot)
                {
                    return Err(unsupported());
                }
                AgentToolTarget::Mcp { slot: slot.clone() }
            }
            ToolKind::Http => AgentToolTarget::Http {
                connection: registered_on(tx, company, tool).await?,
            },
            // Its registration is checked at each call, so a revocation refuses the call rather
            // than the whole agent step (07-D2 §D2.3).
            ToolKind::Workflow(pin) => AgentToolTarget::Workflow { pin: pin.clone() },
        };
        targets.push(target);
    }
    Ok(targets)
}

fn unsupported() -> AppError {
    AppError::BadRequest("Agent tool has no supported dispatch binding".into())
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
        let key = inputs
            .get("agent")
            .and_then(Value::as_str)
            .and_then(AgentKey::parse)
            .ok_or_else(invalid)?;
        let selection = facts.bundle.resolve_agent(scope.company, &key)?;
        let prepared = match step {
            ModelStep::AgentRun => {
                let targets = targets(&mut tx, &facts, scope.company, selection.tools()).await?;
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
        .await?
        .ok_or_else(revoked)?;
        Ok(AgentLaunch {
            prepared,
            secret: SecretString::from(secret),
        })
    }
}
