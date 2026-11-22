//! Plan06 02B: each person's administration of a department, as one append-only row written in
//! the write's own transaction with the role its locked recheck returned. A refused or rolled
//! back write leaves no row; a committed one cannot lose it.
use super::*;
use crate::application::ports::administration::ManagementRole;

#[derive(Debug, Clone, Copy)]
pub(crate) enum AdministrationEvent {
    Created,
    Updated,
    Moved,
    Deleted,
    Archived,
    Restored,
    MemberAdded(PrincipalId),
    MemberRemoved(PrincipalId),
    RoleAssigned(PrincipalId, DepartmentRole),
    RoleCleared(PrincipalId, DepartmentRole),
}

impl AdministrationEvent {
    fn kind(self) -> &'static str {
        match self {
            Self::Created => "department_created",
            Self::Updated => "department_updated",
            Self::Moved => "department_moved",
            Self::Deleted => "department_deleted",
            Self::Archived => "department_archived",
            Self::Restored => "department_restored",
            Self::MemberAdded(_) => "member_added",
            Self::MemberRemoved(_) => "member_removed",
            Self::RoleAssigned(..) => "role_assigned",
            Self::RoleCleared(..) => "role_cleared",
        }
    }

    fn subject(self) -> Option<PrincipalId> {
        match self {
            Self::MemberAdded(principal)
            | Self::MemberRemoved(principal)
            | Self::RoleAssigned(principal, _)
            | Self::RoleCleared(principal, _) => Some(principal),
            Self::Created
            | Self::Updated
            | Self::Moved
            | Self::Deleted
            | Self::Archived
            | Self::Restored => None,
        }
    }

    fn role(self) -> Option<DepartmentRole> {
        match self {
            Self::RoleAssigned(_, role) | Self::RoleCleared(_, role) => Some(role),
            _ => None,
        }
    }
}

/// Record `event` on `department` as `actor` acting with `role`. Every row one transaction
/// writes shares its operation id, so a create with its lead reads as one operation.
pub(crate) async fn record_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    actor: WorkflowActor,
    role: ManagementRole,
    department: DepartmentId,
    event: AdministrationEvent,
) -> AppResult<()> {
    let inserted = sqlx::query(
        "INSERT INTO department_administration_events (company_id, operation_id, principal_id, \
           principal_kind, principal_label, department_id, department_label, role, \
           transition_kind, actor_kind, actor_principal_id, actor_label, acting_role) \
         SELECT $1, COALESCE(NULLIF(current_setting('openbot.administration_operation', true), ''), \
             set_config('openbot.administration_operation', gen_random_uuid()::text, true))::uuid, \
           subject.id, subject.kind, subject.display_label, department.id, \
           department_audit_label(department.name), $4, $5, 'person', actor.id, \
           actor.display_label, $6 \
         FROM departments AS department \
         JOIN principals AS actor \
           ON actor.company_id = $1 AND actor.user_id = $7 AND actor.kind = 'person' \
         LEFT JOIN principals AS subject ON subject.company_id = $1 AND subject.id = $3 \
         WHERE department.company_id = $1 AND department.id = $2 \
           AND ($3::uuid IS NULL OR subject.id IS NOT NULL)",
    )
    .bind(company.as_uuid())
    .bind(department.as_uuid())
    .bind(event.subject().map(PrincipalId::as_uuid))
    .bind(event.role().map(DepartmentRole::as_str))
    .bind(event.kind())
    .bind(role.to_string())
    .bind(actor.user_id())
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if inserted != 1 {
        return Err(AppError::Internal(format!(
            "The {} audit of department {department} found no current actor or subject",
            event.kind()
        )));
    }
    Ok(())
}

/// The role rows one placement changed: each holder a single-holder role was taken from, then the
/// new holder. A placement that changed no role records nothing.
#[allow(clippy::too_many_arguments)]
pub(super) async fn record_role_placement_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    actor: WorkflowActor,
    acting: ManagementRole,
    department: DepartmentId,
    role: DepartmentRole,
    principal: PrincipalId,
    placed: &super::Placed,
) -> AppResult<()> {
    for replaced in &placed.replaced {
        let cleared = AdministrationEvent::RoleCleared(*replaced, role);
        record_on(tx, company, actor, acting, department, cleared).await?;
    }
    if placed.role_assigned {
        let assigned = AdministrationEvent::RoleAssigned(principal, role);
        record_on(tx, company, actor, acting, department, assigned).await?;
    }
    Ok(())
}

/// Before a person leaves the company: one `member_removed` row in every department they were a
/// member of, written while their principal still carries the label the rows snapshot.
pub(crate) async fn record_member_departure_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    actor: WorkflowActor,
    acting: ManagementRole,
    principal: PrincipalId,
) -> AppResult<()> {
    let departments: Vec<Uuid> = sqlx::query_scalar(
        "SELECT department_id FROM department_members \
         WHERE company_id = $1 AND principal_id = $2 ORDER BY department_id",
    )
    .bind(company.as_uuid())
    .bind(principal.as_uuid())
    .fetch_all(&mut **tx)
    .await?;
    for department in departments {
        let removed = AdministrationEvent::MemberRemoved(principal);
        record_on(
            tx,
            company,
            actor,
            acting,
            DepartmentId::new(department),
            removed,
        )
        .await?;
    }
    Ok(())
}
