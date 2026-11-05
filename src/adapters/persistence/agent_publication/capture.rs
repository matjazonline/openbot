use super::*;
use crate::entities::skill::Skill;

#[derive(sqlx::FromRow)]
struct DraftRow {
    draft_revision: i64,
    slug: String,
    system_prompt: Option<String>,
    provider: Option<String>,
    model: Option<String>,
}
#[derive(sqlx::FromRow, PartialEq, Eq)]
struct SkillLink {
    skill_id: Uuid,
    assignment_id: Uuid,
    position: i32,
}

#[derive(sqlx::FromRow)]
struct SkillSourceRow {
    #[sqlx(flatten)]
    skill: super::super::skill::SkillDb,
    required_tools: Vec<String>,
}
struct SkillSource {
    skill: Skill,
    required_tools: Vec<TypeName>,
}
impl TryFrom<SkillSourceRow> for SkillSource {
    type Error = AppError;
    fn try_from(row: SkillSourceRow) -> AppResult<Self> {
        Ok(Self {
            skill: row.skill.try_into()?,
            required_tools: row
                .required_tools
                .into_iter()
                .map(|name| TypeName::parse(name).map_err(|_| invalid()))
                .collect::<AppResult<Vec<_>>>()?,
        })
    }
}

pub(super) async fn capture_on(
    tx: &mut Transaction<'_, Postgres>,
    scope: AgentPublicationScope,
) -> AppResult<(AgentDraftRevision, ResolvedAgentDefinition)> {
    let row: DraftRow = sqlx::query_as(
        "SELECT draft_revision,slug::text AS slug,system_prompt,provider,model \
         FROM agents WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.agent.as_uuid())
    .fetch_one(&mut **tx)
    .await?;
    let revision =
        AgentDraftRevision::new(u64::try_from(row.draft_revision).map_err(|_| invalid())?)
            .map_err(|_| invalid())?;
    let model = explicit_model(row.provider, row.model)?;
    let offered: bool = sqlx::query_scalar(
        "SELECT CASE WHEN cardinality(models) BETWEEN 1 AND $4 THEN $3=ANY(models) ELSE FALSE END          FROM company_model_connections WHERE company_id=$1 AND provider=$2 FOR SHARE"
    ).bind(scope.company.as_uuid()).bind(model.provider.as_str()).bind(&model.model)
        .bind(crate::use_cases::company::MAX_MODELS_PER_CONNECTION as i32)
        .fetch_optional(&mut **tx).await?.ok_or_else(unavailable)?;
    if !offered {
        return Err(unavailable());
    }
    let bindings = super::super::agent_capability::bindings_on(
        tx,
        Some(scope.company.as_uuid()),
        scope.agent.as_uuid(),
    )
    .await?;
    let tools = super::tools::resolve_tools(tx, scope.company, &bindings).await?;
    let skills = capture_skills(tx, scope).await?;
    let draft = AgentDraft {
        company_id: scope.company,
        agent_id: scope.agent,
        revision,
        key: AgentKey::parse(row.slug).ok_or_else(invalid)?,
        instructions: row.system_prompt.unwrap_or_default(),
        model: Some(model),
        bindings,
        skills: skills
            .iter()
            .map(|assigned| AgentSkillAssignment {
                id: assigned.assignment_id,
                skill_id: assigned.skill.skill_id,
            })
            .collect(),
    };
    Ok((
        revision,
        resolve_agent(AgentResolutionFacts {
            draft,
            tools,
            skills,
            model_available: true,
        })?,
    ))
}

fn explicit_model(provider: Option<String>, model: Option<String>) -> AppResult<ModelSettings> {
    let (Some(provider), Some(model)) = (provider, model) else {
        return Err(unavailable());
    };
    if model.is_empty() || model.len() > 128 {
        return Err(unavailable());
    }
    Ok(ModelSettings {
        provider: TypeName::parse(provider).map_err(|_| invalid())?,
        model,
        max_output_tokens: u32::try_from(
            crate::services::harness::runs::RigExecutionPolicy::default().request_output_tokens,
        )
        .map_err(|_| invalid())?,
    })
}

async fn skill_links(
    db: &mut PgConnection,
    scope: AgentPublicationScope,
    lock: &str,
) -> AppResult<Vec<SkillLink>> {
    let rows = sqlx::query_as::<_, SkillLink>(&format!(
        "SELECT skill_id,assignment_id,position FROM agent_skills \
         WHERE company_id=$1 AND agent_id=$2 ORDER BY skill_id LIMIT 17{lock}"
    ))
    .bind(scope.company.as_uuid())
    .bind(scope.agent.as_uuid())
    .fetch_all(db)
    .await?;
    if rows.len() > crate::entities::agent::MAX_AGENT_SKILLS {
        return Err(invalid());
    }
    Ok(rows)
}

async fn capture_skills(
    tx: &mut Transaction<'_, Postgres>,
    scope: AgentPublicationScope,
) -> AppResult<Vec<ResolvedAgentSkill>> {
    let mut links = skill_links(tx, scope, "").await?;
    let ids: Vec<_> = links.iter().map(|link| link.skill_id).collect();
    // Native deletion takes Skill -> draft link -> Assignment -> grant. Never reverse it.
    let rows = sqlx::query_as::<_, SkillSourceRow>(&format!(
        "SELECT {},skill.required_tools FROM skills AS skill WHERE skill.company_id=$1 AND skill.id=ANY($2) \
         ORDER BY skill.id FOR SHARE",
        super::super::skill::SKILL_COLUMNS
    ))
    .bind(scope.company.as_uuid())
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await?;
    if rows.len() != links.len() {
        return Err(unavailable());
    }
    let skills = rows
        .into_iter()
        .map(SkillSource::try_from)
        .collect::<AppResult<Vec<_>>>()?;
    if skill_links(tx, scope, " FOR SHARE").await? != links {
        return Err(conflict());
    }
    links.sort_by_key(|link| link.assignment_id);
    for link in &links {
        let live: Option<Uuid> = sqlx::query_scalar(
            "SELECT live_skill_id FROM agent_skill_assignments WHERE company_id=$1 \
             AND agent_id=$2 AND id=$3 AND skill_id=$4 FOR SHARE",
        )
        .bind(scope.company.as_uuid())
        .bind(scope.agent.as_uuid())
        .bind(link.assignment_id)
        .bind(link.skill_id)
        .fetch_optional(&mut **tx)
        .await?
        .flatten();
        if live != Some(link.skill_id) {
            return Err(unavailable());
        }
    }
    links.sort_by_key(|link| link.position);
    links
        .iter()
        .map(|link| {
            let skill = skills
                .iter()
                .find(|source| source.skill.id == link.skill_id)
                .ok_or_else(invalid)?;
            resolved_skill(scope.company, link.assignment_id, skill)
        })
        .collect()
}

fn resolved_skill(
    company: CompanyId,
    assignment: Uuid,
    source: &SkillSource,
) -> AppResult<ResolvedAgentSkill> {
    let skill = &source.skill;
    Ok(ResolvedAgentSkill {
        assignment_id: AgentSkillAssignmentId::new(assignment),
        skill: SkillSnapshot {
            company_id: company,
            skill_id: skill.id,
            name: TypeName::parse(skill.slug.as_str()).map_err(|_| invalid())?,
            instructions: skill.instructions.clone(),
            required_tools: source.required_tools.clone(),
        },
    })
}
