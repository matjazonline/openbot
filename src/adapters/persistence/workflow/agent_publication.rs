//! Durable owner of agent and skill publication snapshots (06-D5). Capture derives them
//! from the agent/skill rows; publish re-derives them under `FOR SHARE` and requires exact
//! equality, so a caller can never persist a forged or widened agent capability.
use super::http::begin_http;
use super::*;
use crate::application::workflow::lifecycle::{PublicationDependencies, PublicationDirectory};
use crate::application::workflow::message::{MessageKind, builtin_message_snapshot};
use crate::application::workflow::templates::CompanyWorkflowDraft;
use crate::entities::skill::SkillInstruction;
use serde_json::Value;

#[derive(sqlx::FromRow)]
struct AgentRow {
    id: Uuid,
    slug: String,
    system_prompt: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    workflow_tools: Vec<String>,
}

#[derive(sqlx::FromRow)]
struct SkillRow {
    id: Uuid,
    slug: String,
    instructions: Value,
    required_tools: Vec<String>,
}

fn refused(message: String) -> AppError {
    AppError::BadRequest(message)
}

fn type_names(names: &[String], what: &str) -> AppResult<Vec<TypeName>> {
    names
        .iter()
        .map(|name| TypeName::parse(name).map_err(|_| refused(format!("Invalid {what} {name}"))))
        .collect()
}

/// How the derivation reads the agent, skill and model-connection rows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Rows {
    /// Capture: a plain read; publish re-derives and compares.
    Read,
    /// Publish: held `FOR SHARE` until the publication commits.
    Shared,
}

/// The one pure-from-rows mapping used by capture and by publish verification.
async fn derive_on(
    db: &mut PgConnection,
    company: CompanyId,
    key: &AgentKey,
    rows: Rows,
) -> AppResult<(AgentSnapshot, Vec<SkillSnapshot>)> {
    let share = if rows == Rows::Shared {
        " FOR SHARE"
    } else {
        ""
    };
    let key_name = key.as_str();
    let agent: AgentRow = sqlx::query_as(&format!(
        "SELECT id, slug::text AS slug, system_prompt, provider, model, workflow_tools FROM agents WHERE company_id=$1 AND slug=$2{share}"
    ))
    .bind(company.as_uuid())
    .bind(key.as_str())
    .fetch_optional(&mut *db)
    .await?
    .ok_or_else(|| refused(format!("Agent {key_name} is not an agent of this company")))?;
    let (Some(provider), Some(model)) = (agent.provider, agent.model) else {
        return Err(refused(format!(
            "Agent {key_name} needs an explicit provider and model for workflows"
        )));
    };
    let offered: Option<Vec<String>> = sqlx::query_scalar(&format!(
        "SELECT models FROM company_model_connections WHERE company_id=$1 AND provider=$2{share}"
    ))
    .bind(company.as_uuid())
    .bind(&provider)
    .fetch_optional(&mut *db)
    .await?;
    if !offered.is_some_and(|models| models.contains(&model)) {
        return Err(refused(format!(
            "Agent {key_name}: the company {provider} connection does not offer model {model}"
        )));
    }
    let skills: Vec<SkillRow> = sqlx::query_as(&format!(
        "SELECT skill.id, skill.slug::text AS slug, skill.instructions, skill.required_tools
         FROM agent_skills AS link JOIN skills AS skill ON skill.id=link.skill_id
         WHERE link.agent_id=$1 ORDER BY link.position, skill.id{}",
        if rows == Rows::Shared {
            " FOR SHARE OF skill"
        } else {
            ""
        }
    ))
    .bind(agent.id)
    .fetch_all(&mut *db)
    .await?;
    let mut snapshots = Vec::with_capacity(skills.len());
    for skill in skills {
        let name = TypeName::parse(&skill.slug).map_err(|_| {
            refused(format!(
                "Skill {} has a slug workflows cannot name",
                skill.slug
            ))
        })?;
        let steps: Vec<SkillInstruction> = serde_json::from_value(skill.instructions)
            .map_err(|_| AppError::Internal("Invalid stored skill instructions".into()))?;
        let texts: Vec<String> = steps
            .into_iter()
            .map(|SkillInstruction::Prompt { text }| text)
            .collect();
        snapshots.push(SkillSnapshot {
            company_id: company,
            skill_id: skill.id,
            name,
            instructions: texts.join("\n\n"),
            required_tools: type_names(&skill.required_tools, "required tool")?,
        });
    }
    let agent = AgentSnapshot {
        company_id: company,
        agent_id: agent.id,
        key: AgentKey::parse(&agent.slug).ok_or_else(|| {
            refused(format!(
                "Agent {} has a slug workflows cannot name",
                agent.slug
            ))
        })?,
        instructions: agent.system_prompt.unwrap_or_default(),
        model: ModelSettings {
            provider: TypeName::parse(&provider)
                .map_err(|_| refused(format!("Invalid provider {provider}")))?,
            model,
            max_output_tokens: u32::try_from(
                crate::services::harness::runs::RigExecutionPolicy::default().request_output_tokens,
            )
            .map_err(|_| invalid())?,
        },
        tools: type_names(&agent.workflow_tools, "workflow tool")?,
        skills: snapshots.iter().map(|skill| skill.name.clone()).collect(),
    };
    Ok((agent, snapshots))
}

/// Publish-only (06-D5 F008): every agent snapshot in the new root bundle, and every skill
/// snapshot, equals what its durable rows derive now, under `FOR SHARE`. Stored child
/// bundles were verified at their own publication and are not re-derived.
pub(super) async fn verify_on(
    tx: &mut Transaction<'_, Postgres>,
    bundle: &PublishedBundle,
) -> AppResult<()> {
    let snapshots = bundle.snapshots();
    let mut skills: Vec<SkillSnapshot> = Vec::new();
    for agent in &snapshots.agents {
        let (derived, derived_skills) =
            derive_on(tx, bundle.company_id(), &agent.key, Rows::Shared).await?;
        if serde_json::to_value(&derived).map_err(|_| invalid())?
            != serde_json::to_value(agent).map_err(|_| invalid())?
        {
            return Err(AppError::Conflict(format!(
                "Agent {} changed or does not match its saved definition",
                agent.key.as_str()
            )));
        }
        for skill in derived_skills {
            if !skills.iter().any(|known| known.skill_id == skill.skill_id) {
                skills.push(skill);
            }
        }
    }
    let canonical = |list: &[SkillSnapshot]| -> AppResult<Vec<Value>> {
        let mut values = list
            .iter()
            .map(|skill| serde_json::to_value(skill).map_err(|_| invalid()))
            .collect::<AppResult<Vec<_>>>()?;
        values.sort_by_key(|value| value["skill_id"].to_string());
        Ok(values)
    };
    if canonical(&skills)? != canonical(&snapshots.skills)? {
        return Err(AppError::Conflict(
            "Agent skills changed or do not match their saved definitions".into(),
        ));
    }
    Ok(())
}

/// Wraps the step-direct tool directories and adds the agents the draft can select, their
/// skills, and their granted tools, merged into the bundle's single tool universe.
pub struct AgentPublicationDirectory<D> {
    pub persistence: PostgresPersistence,
    pub inner: D,
}

#[async_trait]
impl<D: PublicationDirectory> PublicationDirectory for AgentPublicationDirectory<D> {
    async fn capture(
        &self,
        actor: WorkflowActor,
        draft: &CompanyWorkflowDraft,
    ) -> AppResult<PublicationDependencies> {
        let mut captured = self.inner.capture(actor, draft).await?;
        let decoded = crate::adapters::workflow_source::decode(draft.source())
            .map_err(|diagnostic| AppError::BadRequest(diagnostic.message.into()))?;
        let keys = agent_universe(&decoded).map_err(AppError::BadRequest)?;
        let parsed = crate::application::workflow::compiler::parse_workflow(
            &decoded,
            VersionId::new(Uuid::nil()),
        )
        .map_err(|diagnostic| AppError::BadRequest(diagnostic.message.into()))?;
        if keys.is_empty() {
            return Ok(captured);
        }
        let company = draft.company();
        let mut tx = begin_http(&self.persistence).await?;
        authority::authorize_company(&mut tx, company, actor).await?;
        for key in &keys {
            let (agent, skills) = derive_on(&mut tx, company, key, Rows::Read).await?;
            for name in &agent.tools {
                let tool = granted_tool(
                    &mut tx,
                    company,
                    name,
                    &captured.snapshots.tools,
                    &parsed.definition,
                )
                .await?;
                if let Some(tool) = tool {
                    captured.snapshots.tools.push(tool);
                }
            }
            for skill in skills {
                match captured
                    .snapshots
                    .skills
                    .iter()
                    .find(|known| known.name == skill.name)
                {
                    Some(known) if known.skill_id == skill.skill_id => {}
                    Some(_) => {
                        return Err(refused(format!(
                            "Two different skills are named {}",
                            skill.name
                        )));
                    }
                    None => captured.snapshots.skills.push(skill),
                }
            }
            captured.snapshots.agents.push(agent);
        }
        tx.commit().await?;
        Ok(captured)
    }
}

/// Resolve one agent grant with the SQL owner's rules: an already captured snapshot of that
/// name (step-direct HTTP, a selected MCP slot, or the same workflow tool) is reused when
/// unique; a built-in message tool is its trusted snapshot; then the company's workflow tool
/// registration of that name (07-D2 §D2.2); otherwise the company's single ready HTTP
/// registration. A name that two of these could answer is refused as ambiguous.
async fn granted_tool(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    name: &TypeName,
    captured: &[ToolSnapshot],
    definition: &crate::domain::workflow::WorkflowDefinition,
) -> AppResult<Option<ToolSnapshot>> {
    let ambiguous = || {
        refused(format!(
            "Agent tool {name} conflicts with another tool of the same name in this workflow"
        ))
    };
    let workflow = super::workflow_tools::workflow_tool_snapshot(tx, company, name).await?;
    let known: Vec<&ToolSnapshot> = captured
        .iter()
        .filter(|tool| &tool.contract.name == name)
        .collect();
    match (known.as_slice(), &workflow) {
        ([], _) => {}
        // Another agent of this workflow already captured the same workflow tool.
        ([tool], Some(registered)) => {
            return match (tool.kind(), registered.kind()) {
                (ToolKind::Workflow(frozen), ToolKind::Workflow(current)) if frozen == current => {
                    Ok(None)
                }
                _ => Err(ambiguous()),
            };
        }
        // A built-in message tool, a registration (no slot) or a selected MCP slot is a usable
        // agent tool as captured.
        ([tool], None)
            if match tool.kind() {
                ToolKind::Message | ToolKind::Http => true,
                ToolKind::Slot(slot) => {
                    super::resources::slot_kind(definition, slot) == Some("mcp")
                }
                ToolKind::Workflow(_) => false,
            } =>
        {
            return Ok(None);
        }
        // A slot-bound HTTP step selection is not an agent tool, and a second snapshot of
        // the same name would make the agent's selection ambiguous (06-R016 F017).
        _ => return Err(ambiguous()),
    }
    if let Some(kind) = MessageKind::parse(name.as_str()) {
        return Ok(Some(builtin_message_snapshot(company, kind)?));
    }
    let snapshots: Vec<Value> = sqlx::query_scalar("SELECT operation.snapshot FROM workflow_http_operations AS operation JOIN workflow_http_connections AS connection ON connection.company_id=operation.company_id AND connection.id=operation.connection_id WHERE operation.company_id=$1 AND operation.name=$2 AND connection.enabled AND (NOT connection.bearer OR EXISTS(SELECT 1 FROM workflow_http_credentials AS credential WHERE credential.company_id=connection.company_id AND credential.connection_id=connection.id)) LIMIT 2")
        .bind(company.as_uuid())
        .bind(name.as_str())
        .fetch_all(&mut **tx)
        .await?;
    if let Some(workflow) = workflow {
        if !snapshots.is_empty() {
            return Err(refused(format!(
                "Agent tool {name} is registered both as a workflow and as an HTTP operation"
            )));
        }
        return Ok(Some(workflow));
    }
    match snapshots.as_slice() {
        [snapshot] => Ok(Some(
            serde_json::from_value(snapshot.clone()).map_err(|_| invalid())?,
        )),
        [] => Err(refused(format!(
            "Agent tool {name} has no ready HTTP registration, workflow registration or selected MCP slot"
        ))),
        _ => Err(refused(format!(
            "Agent tool {name} is registered on several connections"
        ))),
    }
}
