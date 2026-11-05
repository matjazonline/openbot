use super::*;

fn retired() -> AppError {
    AppError::Conflict(
        "A selected grant was revoked; select a fresh draft binding or skill assignment".into(),
    )
}

pub(super) async fn activate_on(
    db: &mut PgConnection,
    scope: AgentPublicationScope,
    definition: &ResolvedAgentDefinition,
) -> AppResult<()> {
    let mut bindings: Vec<_> = definition
        .tools
        .iter()
        .map(|tool| tool.binding.id.as_uuid())
        .collect();
    let mut assignments: Vec<_> = definition
        .skills
        .iter()
        .map(|skill| skill.assignment_id.as_uuid())
        .collect();
    bindings.sort();
    assignments.sort();
    sqlx::query(
        "UPDATE agent_binding_grants SET revoked_at=clock_timestamp() WHERE company_id=$1 AND agent_id=$2 \
         AND revoked_at IS NULL AND NOT(binding_id=ANY($3))"
    ).bind(scope.company.as_uuid()).bind(scope.agent.as_uuid()).bind(&bindings).execute(&mut *db).await?;
    sqlx::query(
        "UPDATE agent_skill_grants SET revoked_at=clock_timestamp() WHERE company_id=$1 AND agent_id=$2 \
         AND revoked_at IS NULL AND NOT(assignment_id=ANY($3))"
    ).bind(scope.company.as_uuid()).bind(scope.agent.as_uuid()).bind(&assignments).execute(&mut *db).await?;
    for binding in bindings {
        grant_binding(db, scope, binding).await?;
    }
    for assignment in assignments {
        let skill = definition
            .skills
            .iter()
            .find(|skill| skill.assignment_id.as_uuid() == assignment)
            .ok_or_else(invalid)?;
        grant_skill(db, scope, skill).await?;
    }
    Ok(())
}

async fn grant_binding(
    db: &mut PgConnection,
    scope: AgentPublicationScope,
    binding: Uuid,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO agent_binding_grants(company_id,agent_id,binding_id) VALUES($1,$2,$3) \
         ON CONFLICT(company_id,agent_id,binding_id) DO NOTHING",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.agent.as_uuid())
    .bind(binding)
    .execute(&mut *db)
    .await?;
    let revoked: bool = sqlx::query_scalar(
        "SELECT revoked_at IS NOT NULL FROM agent_binding_grants WHERE company_id=$1 AND agent_id=$2 AND binding_id=$3 FOR UPDATE"
    ).bind(scope.company.as_uuid()).bind(scope.agent.as_uuid()).bind(binding).fetch_one(db).await?;
    if revoked {
        return Err(retired());
    }
    Ok(())
}

async fn grant_skill(
    db: &mut PgConnection,
    scope: AgentPublicationScope,
    skill: &ResolvedAgentSkill,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO agent_skill_grants(company_id,agent_id,assignment_id,skill_id) VALUES($1,$2,$3,$4) \
         ON CONFLICT(company_id,agent_id,assignment_id) DO NOTHING"
    ).bind(scope.company.as_uuid()).bind(scope.agent.as_uuid()).bind(skill.assignment_id.as_uuid())
        .bind(skill.skill.skill_id).execute(&mut *db).await?;
    let revoked: bool = sqlx::query_scalar(
        "SELECT revoked_at IS NOT NULL FROM agent_skill_grants WHERE company_id=$1 AND agent_id=$2 AND assignment_id=$3 FOR UPDATE"
    ).bind(scope.company.as_uuid()).bind(scope.agent.as_uuid()).bind(skill.assignment_id.as_uuid())
        .fetch_one(db).await?;
    if revoked {
        return Err(retired());
    }
    Ok(())
}

struct GrantMutation {
    read: &'static str,
    write: &'static str,
    id: Uuid,
}

pub(super) async fn revoke_on(
    db: &mut PgConnection,
    scope: AgentPublicationScope,
    identity: AgentGrantIdentity,
) -> AppResult<()> {
    let mutation = match identity {
        AgentGrantIdentity::Binding(id) => GrantMutation {
            read: "SELECT revoked_at IS NOT NULL FROM agent_binding_grants WHERE company_id=$1 AND agent_id=$2 AND binding_id=$3 FOR UPDATE",
            write: "UPDATE agent_binding_grants SET revoked_at=clock_timestamp() WHERE company_id=$1 AND agent_id=$2 AND binding_id=$3 AND revoked_at IS NULL",
            id: id.as_uuid(),
        },
        AgentGrantIdentity::Skill(id) => GrantMutation {
            read: "SELECT revoked_at IS NOT NULL FROM agent_skill_grants WHERE company_id=$1 AND agent_id=$2 AND assignment_id=$3 FOR UPDATE",
            write: "UPDATE agent_skill_grants SET revoked_at=clock_timestamp() WHERE company_id=$1 AND agent_id=$2 AND assignment_id=$3 AND revoked_at IS NULL",
            id: id.as_uuid(),
        },
    };
    let revoked: bool = sqlx::query_scalar(mutation.read)
        .bind(scope.company.as_uuid())
        .bind(scope.agent.as_uuid())
        .bind(mutation.id)
        .fetch_optional(&mut *db)
        .await?
        .ok_or_else(missing)?;
    if !revoked {
        sqlx::query(mutation.write)
            .bind(scope.company.as_uuid())
            .bind(scope.agent.as_uuid())
            .bind(mutation.id)
            .execute(db)
            .await?;
    }
    Ok(())
}
