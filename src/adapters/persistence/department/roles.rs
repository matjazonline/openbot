//! Role writes retain the department transaction and its company serialization.
use super::*;

/// Give `principal` the role. A single-holder role replaces its current holder in this same
/// transaction, so the one-lead index is never the person's error. Assigning the current holder
/// again changes nothing. The caller holds the company write lock.
pub(crate) async fn assign_department_role_on(
    tx: &mut Transaction<'_, Postgres>,
    templates: &ProvisioningDeployment,
    company: CompanyId,
    actor: WorkflowActor,
    department: DepartmentId,
    assignment: DepartmentRoleAssignment,
) -> AppResult<()> {
    let DepartmentRoleAssignment {
        role, principal, ..
    } = assignment;
    let acting = root_role_authority_on(tx, company, actor, department, role).await?;
    let placed =
        write_placement_on(tx, company, department, principal, false, Some(assignment)).await?;
    audit::record_role_placement_on(
        tx, company, actor, acting, department, role, principal, &placed,
    )
    .await?;
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
    Box::pin(super::super::workflow::follow_reviewers::refresh_on(
        tx, company,
    ))
    .await
}

/// Take the role from `principal`. The caller holds the company write lock.
pub(crate) async fn clear_department_role_on(
    tx: &mut Transaction<'_, Postgres>,
    templates: &ProvisioningDeployment,
    company: CompanyId,
    actor: WorkflowActor,
    department: DepartmentId,
    removal: DepartmentRoleRemoval,
) -> AppResult<()> {
    let DepartmentRoleRemoval {
        role, principal, ..
    } = removal;
    let acting = root_role_authority_on(tx, company, actor, department, role).await?;
    lock_department_on(tx, company, department).await?;
    clear_sponsor_on(tx, company, department, removal).await?;
    lock_principals_on(tx, company, &[principal.as_uuid()]).await?;
    let cleared = sqlx::query(
        "DELETE FROM department_roles \
          WHERE company_id = $1 AND department_id = $2 AND role = $3 AND principal_id = $4",
    )
    .bind(company.as_uuid())
    .bind(department.as_uuid())
    .bind(role.as_str())
    .bind(principal.as_uuid())
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if cleared == 0 {
        return Err(AppError::NotFound(format!(
            "That principal does not hold the {role} role in this department."
        )));
    }
    audit::record_on(
        tx,
        company,
        actor,
        acting,
        department,
        AdministrationEvent::RoleCleared(principal, role),
    )
    .await?;
    super::super::workflow::role_binding::check_principals_on(tx, company, &[principal.as_uuid()])
        .await?;
    let reach = role_reach_on(tx, company, department).await?;
    sync_department_grants_on(tx, company, &reach).await?;
    follow_lead_on(tx, templates, company, actor, department, role, None).await?;
    Box::pin(super::super::workflow::follow_reviewers::refresh_on(
        tx, company,
    ))
    .await
}

#[derive(sqlx::FromRow)]
struct SponsorState {
    holders: i64,
    deciders: i64,
    existing: bool,
}

fn needs_decider() -> AppError {
    AppError::Conflict(
        "department.sponsor_needs_decider: choose a remaining sponsor to decide".into(),
    )
}

pub(super) async fn assign_sponsor_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    department: DepartmentId,
    assignment: DepartmentRoleAssignment,
) -> AppResult<bool> {
    let state: SponsorState = sqlx::query_as(
        "SELECT count(*) AS holders,count(*) FILTER (WHERE decider) AS deciders, \
         coalesce(bool_or(principal_id=$3),false) AS existing FROM department_roles \
         WHERE company_id=$1 AND department_id=$2 AND role='sponsor'",
    )
    .bind(company.as_uuid())
    .bind(department.as_uuid())
    .bind(assignment.principal.as_uuid())
    .fetch_one(&mut **tx)
    .await?;
    if !state.existing
        && state.holders >= crate::domain::workflow::TokenAllowance::MAX_REVIEWERS as i64
    {
        return Err(AppError::Conflict(
            "department.sponsor_limit: at most 16 sponsors".into(),
        ));
    }
    if state.holders > 0 && state.deciders == 0 && !assignment.decider {
        return Err(needs_decider());
    }
    // The data dependency runs demotion before the upsert, so the immediate partial index
    // never observes two deciding sponsors. Company/department ownership serializes writers.
    sqlx::query_scalar(
        "WITH demoted AS (UPDATE department_roles SET decider=false \
             WHERE company_id=$1 AND department_id=$2 AND role='sponsor' AND decider \
               AND principal_id<>$3 AND $4 RETURNING principal_id) \
         INSERT INTO department_roles(company_id,department_id,role,principal_id,decider) \
         SELECT $1,$2,'sponsor',$3,$4 FROM (SELECT count(*) FROM demoted) AS completed \
         ON CONFLICT (department_id,role,principal_id) DO UPDATE \
           SET decider=EXCLUDED.decider OR department_roles.decider \
         RETURNING xmax=0",
    )
    .bind(company.as_uuid())
    .bind(department.as_uuid())
    .bind(assignment.principal.as_uuid())
    .bind(assignment.decider || state.holders == 0)
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::from)
}

#[derive(sqlx::FromRow)]
struct Sponsor {
    principal_id: Uuid,
    decider: bool,
}

async fn clear_sponsor_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    department: DepartmentId,
    removal: DepartmentRoleRemoval,
) -> AppResult<()> {
    if removal.role != DepartmentRole::Sponsor {
        if removal.successor.is_some() {
            return Err(AppError::BadRequest(
                "Only sponsor removal takes a successor".into(),
            ));
        }
        return Ok(());
    }
    let holders: Vec<Sponsor> = sqlx::query_as(
        "SELECT principal_id,decider FROM department_roles WHERE company_id=$1 \
         AND department_id=$2 AND role='sponsor' ORDER BY principal_id LIMIT 17",
    )
    .bind(company.as_uuid())
    .bind(department.as_uuid())
    .fetch_all(&mut **tx)
    .await?;
    let principals: Vec<_> = holders.iter().map(|holder| holder.principal_id).collect();
    lock_principals_on(tx, company, &principals).await?;
    let Some(removed) = holders
        .iter()
        .find(|holder| holder.principal_id == removal.principal.as_uuid())
    else {
        return Ok(()); // The authoritative DELETE supplies the ordinary not-found outcome.
    };
    if let Some(successor) = removal.successor {
        if successor == removal.principal || !principals.contains(&successor.as_uuid()) {
            return Err(AppError::BadRequest(
                "The successor must be another existing sponsor".into(),
            ));
        }
        assign_sponsor_on(
            tx,
            company,
            department,
            DepartmentRoleAssignment {
                role: DepartmentRole::Sponsor,
                principal: successor,
                decider: true,
            },
        )
        .await?;
    } else if holders.len() > 1 && (removed.decider || !holders.iter().any(|holder| holder.decider))
    {
        return Err(needs_decider());
    }
    Ok(())
}

pub(super) async fn write_role_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    department: DepartmentId,
    assignment: DepartmentRoleAssignment,
    placed: &mut Placed,
) -> AppResult<()> {
    let principal = assignment.principal;
    let role = assignment.role;
    if role != DepartmentRole::Sponsor && assignment.decider {
        return Err(AppError::BadRequest(
            "Only a sponsor can be marked as decider".into(),
        ));
    }
    if role.is_single_holder() {
        let replaced: Vec<Uuid> = sqlx::query_scalar(
            "DELETE FROM department_roles \
              WHERE company_id = $1 AND department_id = $2 AND role = $3 AND principal_id <> $4 \
              RETURNING principal_id",
        )
        .bind(company.as_uuid())
        .bind(department.as_uuid())
        .bind(role.as_str())
        .bind(principal.as_uuid())
        .fetch_all(&mut **tx)
        .await?;
        placed.replaced = replaced.into_iter().map(PrincipalId::new).collect();
    }
    placed.role_assigned = if role == DepartmentRole::Sponsor {
        assign_sponsor_on(tx, company, department, assignment).await?
    } else {
        sqlx::query(
            "INSERT INTO department_roles (company_id, department_id, role, principal_id) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (department_id, role, principal_id) DO NOTHING",
        )
        .bind(company.as_uuid())
        .bind(department.as_uuid())
        .bind(role.as_str())
        .bind(principal.as_uuid())
        .execute(&mut **tx)
        .await?
        .rows_affected()
            == 1
    };
    Ok(())
}
