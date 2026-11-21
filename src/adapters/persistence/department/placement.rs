//! Agent placement construction and final grant/role synchronization.
use super::*;

/// Make `principal` a member of the department and, with `role`, its holder of that role, as
/// company-agent creation places a new agent (plan 01-departments 03C item 0). One write: the
/// rows, then a single grant sync over everything they reach, so the department's desk gates
/// are taken once, in order, with its neighbours' (01DEP-P03-B1-F1). The caller holds the company
/// write lock.
pub(crate) async fn place_in_department_on(
    tx: &mut Transaction<'_, Postgres>,
    templates: &ProvisioningDeployment,
    company: CompanyId,
    actor: WorkflowActor,
    department: DepartmentId,
    principal: PrincipalId,
    role: Option<DepartmentRole>,
) -> AppResult<()> {
    let role_authority = match role {
        Some(role) => Some(root_role_authority_on(tx, company, actor, department, role).await?),
        None => None,
    };
    let assignment = role.map(|role| DepartmentRoleAssignment {
        role,
        principal,
        decider: false,
    });
    let placed = write_placement_on(tx, company, department, principal, true, assignment).await?;
    if placed.member_added {
        // The caller authorized this placement; the history names the role the actor manages
        // the department with, read under the same company lock.
        let acting = super::super::administration::management_role_on(
            tx,
            actor.user_id(),
            company,
            crate::application::ports::administration::ManagementTarget::Unit(department),
        )
        .await?
        .ok_or_else(department_not_found)?;
        audit::record_on(
            tx,
            company,
            actor,
            acting,
            department,
            AdministrationEvent::MemberAdded(principal),
        )
        .await?;
    }
    if let (Some(role), Some(acting)) = (role, role_authority) {
        audit::record_role_placement_on(
            tx, company, actor, acting, department, role, principal, &placed,
        )
        .await?;
    }
    if let Some(role) = role {
        follow_lead_on(
            tx,
            templates,
            company,
            actor,
            department,
            role,
            Some(principal),
        )
        .await?;
    }
    Box::pin(crate::adapters::persistence::workflow::follow_reviewers::refresh_on(tx, company))
        .await
}

/// A new agent's actual owning membership must precede its first publication. Grant gates
/// and role/reviewer effects are finalized once construction is complete, in this transaction.
pub(crate) async fn prepare_new_agent_placement_on(
    tx: &mut Transaction<'_, Postgres>,
    scope: DepartmentScope,
    principal: PrincipalId,
    placement: crate::use_cases::agent::AgentPlacement,
) -> AppResult<()> {
    let acting = require_department_management_on(tx, scope, Some(placement.department)).await?;
    if let Some(role) = placement.role {
        root_role_authority_on(tx, scope.company, scope.actor, placement.department, role).await?;
    }
    lock_department_on(tx, scope.company, placement.department).await?;
    check_principal_on(tx, scope.company, principal).await?;
    let added = department_member_insert(scope.company, placement.department, principal)
        .execute(&mut **tx)
        .await?
        .rows_affected()
        == 1;
    if added {
        audit::record_on(
            tx,
            scope.company,
            scope.actor,
            acting,
            placement.department,
            AdministrationEvent::MemberAdded(principal),
        )
        .await?;
    }
    Ok(())
}
