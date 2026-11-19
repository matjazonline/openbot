//! Root-only company authority on department management surfaces.
use super::*;

pub(super) async fn root_role_authority_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    actor: WorkflowActor,
    department: DepartmentId,
    role: DepartmentRole,
) -> AppResult<ManagementRole> {
    use crate::application::ports::administration::ManagementTarget;
    let current = lock_department_on(tx, company, department).await?;
    let acting = if current.is_root {
        if super::super::administration::management_role_on(
            tx,
            actor.user_id(),
            company,
            ManagementTarget::Root,
        )
        .await?
            != Some(ManagementRole::Owner)
        {
            return Err(department_not_found());
        }
        ManagementRole::Owner
    } else {
        require_department_management_on(tx, DepartmentScope { company, actor }, current.parent_id)
            .await?
    };
    match role {
        DepartmentRole::Owner => {
            return Err(AppError::BadRequest(
                "The owner role follows the company owner and cannot be edited here.".into(),
            ));
        }
        DepartmentRole::Admin if !current.is_root => {
            return Err(AppError::BadRequest(
                "The admin role belongs to the company root.".into(),
            ));
        }
        _ => {}
    }

    Ok(acting)
}

#[derive(sqlx::FromRow)]
struct RootRemovalTarget {
    user_id: Option<Uuid>,
    is_agent: bool,
}

pub(super) async fn remove_root_member(
    persistence: &PostgresPersistence,
    scope: DepartmentScope,
    department: DepartmentId,
    principal: PrincipalId,
) -> AppResult<()> {
    use super::super::company_invite::authority::{self, TeamScope};
    // Learn which account to lock before the company. This selects locks, never authority;
    // the target is re-read under the complete lock set and a changed incarnation is refused.
    let expected_user: Option<Uuid> =
        sqlx::query_scalar("SELECT user_id FROM principals WHERE company_id = $1 AND id = $2")
            .bind(scope.company.as_uuid())
            .bind(principal.as_uuid())
            .fetch_optional(&persistence.pool)
            .await?
            .ok_or_else(department_not_found)?;
    let team_scope = TeamScope {
        actor_id: scope.actor.user_id(),
        company_id: scope.company.as_uuid(),
    };
    let mut tx = authority::start(&persistence.pool, team_scope, expected_user).await?;
    let team = authority::lock_team(&mut tx, team_scope, expected_user).await?;
    if !team.actor.membership.administers_company() {
        return Err(company_not_found());
    }
    lock_department_on(&mut tx, scope.company, department).await?;
    let current = sqlx::query_as::<_, RootRemovalTarget>(
        "SELECT user_id, kind = 'agent' AS is_agent FROM principals \
         WHERE company_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(scope.company.as_uuid())
    .bind(principal.as_uuid())
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(department_not_found)?;
    if current.user_id != expected_user {
        return Err(AppError::Conflict(
            "Team membership changed; retry the request.".into(),
        ));
    }
    // The root's membership is company administration: the owner, or a root admin for agents.
    let acting = if team.actor.membership.is_owner() {
        ManagementRole::Owner
    } else {
        ManagementRole::Admin
    };
    let removed = AdministrationEvent::MemberRemoved(principal);
    if let Some(user) = current.user_id {
        team.require_owner()?;
        // Before retirement, which demotes the principal these rows name: leaving the company
        // removes the person from every department they were a member of, the root included.
        audit::record_member_departure_on(&mut tx, scope.company, scope.actor, acting, principal)
            .await?;
        super::super::company::retire_member_on(&mut tx, scope.company, user).await?;
    } else if current.is_agent {
        remove_department_member_on(&mut tx, scope.company, department, principal).await?;
        audit::record_on(
            &mut tx,
            scope.company,
            scope.actor,
            acting,
            department,
            removed,
        )
        .await?;
    } else {
        return Err(AppError::NotFound(
            "That principal is not a root member.".into(),
        ));
    }
    tx.commit().await?;
    Ok(())
}
