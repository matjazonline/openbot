use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(sqlx::FromRow)]
struct SkillRow {
    id: Uuid,
    assignment_id: Uuid,
    name: String,
    description: String,
    required_tools: Vec<String>,
}

async fn skills(
    db: &mut PgConnection,
    scope: AgentPublicationScope,
) -> AppResult<Vec<AgentAuthoringSkill>> {
    let rows:Vec<SkillRow>=sqlx::query_as("SELECT skill.id,selection.assignment_id,skill.name,skill.description,skill.required_tools FROM agent_skills AS selection JOIN skills AS skill ON skill.company_id=selection.company_id AND skill.id=selection.skill_id WHERE selection.company_id=$1 AND selection.agent_id=$2 ORDER BY selection.position LIMIT 17")
        .bind(scope.company.as_uuid()).bind(scope.agent.as_uuid()).fetch_all(db).await?;
    if rows.len() > crate::entities::agent::MAX_AGENT_SKILLS {
        return Err(invalid());
    }
    rows.into_iter()
        .map(|row| {
            Ok(AgentAuthoringSkill {
                id: row.id,
                assignment: AgentSkillAssignmentId::new(row.assignment_id),
                name: row.name,
                description: row.description,
                required_tools: row
                    .required_tools
                    .into_iter()
                    .map(|name| {
                        crate::domain::workflow::TypeName::parse(name).map_err(|_| invalid())
                    })
                    .collect::<AppResult<Vec<_>>>()?,
            })
        })
        .collect()
}

pub(super) async fn read(
    db: &mut PgConnection,
    scope: AgentPublicationScope,
) -> AppResult<AgentAuthoringState> {
    let row: super::super::agent::AgentDb = sqlx::query_as(&format!(
        "SELECT {} FROM agents AS agent WHERE agent.company_id=$1 AND agent.id=$2",
        super::super::agent::AGENT_COLUMNS
    ))
    .bind(scope.company.as_uuid())
    .bind(scope.agent.as_uuid())
    .fetch_one(&mut *db)
    .await?;
    let agent = row.try_into()?;
    let draft = super::super::agent_capability::bindings_on(
        db,
        Some(scope.company.as_uuid()),
        scope.agent.as_uuid(),
    )
    .await?;
    let selected_skills = skills(db, scope).await?;
    let selected = match super::super::agent_publication::storage::active_on(db, scope).await? {
        Some(id) => Some(
            super::super::agent_publication::storage::revision_on(db, scope, id)
                .await?
                .ok_or_else(invalid)?,
        ),
        None => None,
    };
    let ids: Vec<_> = draft
        .iter()
        .map(|binding| binding.id.as_uuid())
        .chain(selected.iter().flat_map(|revision| {
            revision
                .definition
                .tools
                .iter()
                .map(|tool| tool.binding.id.as_uuid())
        }))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let states = super::status::tools(db, scope, &ids).await?;
    let tools = draft
        .iter()
        .map(|binding| super::status::project(binding, &states))
        .collect::<AppResult<Vec<_>>>()?;
    let active = match selected {
        Some(revision) => Some(active(db, scope, revision, &states).await?),
        None => None,
    };
    Ok(AgentAuthoringState {
        agent,
        skills: selected_skills,
        tools,
        active,
        history: super::history::page(db, scope, None).await?,
    })
}

async fn active(
    db: &mut PgConnection,
    scope: AgentPublicationScope,
    revision: PublishedAgentRevision,
    states: &BTreeMap<Uuid, super::status::ToolStatus>,
) -> AppResult<AgentActiveState> {
    #[derive(sqlx::FromRow)]
    struct GrantRow {
        assignment_id: Uuid,
        access: String,
    }
    let tools = revision
        .definition
        .tools
        .iter()
        .map(|tool| super::status::project(&tool.binding, states))
        .collect::<AppResult<Vec<_>>>()?;
    if tools
        .iter()
        .any(|tool| tool.access == AgentGrantState::Unpublished)
    {
        return Err(invalid());
    }
    let ids: Vec<_> = revision
        .definition
        .skills
        .iter()
        .map(|skill| skill.assignment_id.as_uuid())
        .collect();
    let grants:Vec<GrantRow>=sqlx::query_as("SELECT assignment_id,CASE WHEN revoked_at IS NULL THEN 'live' ELSE 'revoked' END AS access FROM agent_skill_grants WHERE company_id=$1 AND agent_id=$2 AND assignment_id=ANY($3) LIMIT 17")
        .bind(scope.company.as_uuid()).bind(scope.agent.as_uuid()).bind(&ids).fetch_all(db).await?;
    let grants: BTreeMap<_, _> = grants
        .into_iter()
        .map(|row| Ok((row.assignment_id, super::status::access(&row.access)?)))
        .collect::<AppResult<_>>()?;
    let skills = revision
        .definition
        .skills
        .into_iter()
        .map(|skill| {
            Ok(AgentActiveSkill {
                assignment: skill.assignment_id,
                skill: skill.skill.skill_id,
                name: skill.skill.name,
                access: *grants
                    .get(&skill.assignment_id.as_uuid())
                    .ok_or_else(invalid)?,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok(AgentActiveState {
        id: revision.id,
        draft_revision: revision.draft_revision,
        key: revision.definition.key,
        model: revision.definition.model,
        tools,
        skills,
    })
}
