//! Departments, their members and their roles (01-departments).
//!
//! Every write is one bounded transaction, in this order, so the writers of one company serialize
//! and a refusal leaves nothing behind:
//!
//! 1. the actor's account row, then the company row `FOR NO KEY UPDATE` and the actor's current
//!    authority ([`lock_company_on`]): the company write lock, in the account-then-company order
//!    member removal and agent publication use;
//! 2. the department row `FOR UPDATE` within its tenant (a named parent `FOR SHARE`);
//! 3. the change, preflighted against the rules the schema enforces, so the person gets a worded
//!    refusal instead of a constraint error. The schema's guards remain the backstop.
//!
//! Step 1 is what makes the tree rules hold under concurrency: two re-parents that would together
//! form a cycle, or two creates of one slug, cannot both read the tree before either commits.
//!
//! The `_on` functions are steps 2–3 on a caller's transaction, for a caller that already holds
//! the company write lock and must change a department inside its own commit.

use crate::application::ports::department::{DepartmentRoleAssignment, DepartmentRoleRemoval};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{Postgres, Transaction};
use std::str::FromStr;
use uuid::Uuid;

use crate::adapters::persistence::PostgresPersistence;
use crate::adapters::persistence::department_desks::sync_department_grants_on;
use crate::adapters::persistence::workflow::provisioning::{
    ProvisioningDeployment, department_desk_key, follow_department_lead_on,
    provision_department_desk_on,
};
use crate::app_error::{AppError, AppResult};
use crate::application::ports::administration::ManagementRole;
use crate::application::ports::department::{
    DepartmentEdit, DepartmentPersistence, DepartmentScope, DepartmentWrite,
};
use crate::application::workflow::WorkflowActor;
use crate::domain::workflow::CompanyId;
use crate::entities::creation::CreationProvenance;
use crate::entities::department::{
    Department, DepartmentDesk, DepartmentId, DepartmentMember, DepartmentPrincipal,
    DepartmentRole, DepartmentRoleHolder, HeldDepartmentRole, MAX_DEPARTMENT_DEPTH,
};
use crate::entities::participant::PrincipalKind;
use crate::entities::transport::PrincipalId;
use crate::entities::value_objects::DepartmentSlug;
use crate::use_cases::company::company_not_found;
use crate::use_cases::department::department_not_found;

mod audit;
pub(crate) use audit::{
    AdministrationEvent, record_member_departure_on, record_on as record_administration_on,
};
mod placement;
pub(crate) use placement::{place_in_department_on, prepare_new_agent_placement_on};
mod lifecycle;
mod roles;
mod root;
pub(crate) use lifecycle::{
    create_department_on, create_department_with_id_on, delete_department_on, update_department_on,
};
pub(crate) use roles::{assign_department_role_on, clear_department_role_on};
use root::root_role_authority_on;

/// The role with which the actor manages `unit` (the root when `None`), under the caller's
/// company lock; not found otherwise. Writes record it in their administration history.
async fn require_department_management_on(
    tx: &mut Transaction<'_, Postgres>,
    scope: DepartmentScope,
    unit: Option<DepartmentId>,
) -> AppResult<ManagementRole> {
    use crate::application::ports::administration::ManagementTarget;
    super::administration::management_role_on(
        tx,
        scope.actor.user_id(),
        scope.company,
        unit.map_or(ManagementTarget::Root, ManagementTarget::Unit),
    )
    .await?
    .ok_or_else(department_not_found)
}

/// The role with which the actor manages `unit`'s parent (the root itself for the root), and the
/// department as locked.
async fn require_parent_management_on(
    tx: &mut Transaction<'_, Postgres>,
    scope: DepartmentScope,
    unit: DepartmentId,
) -> AppResult<(ManagementRole, Department)> {
    let current = lock_department_on(tx, scope.company, unit).await?;
    let role = require_department_management_on(tx, scope, current.parent_id).await?;
    Ok((role, current))
}

/// A principal named as the department pages show it: an agent by its current name and handle, a
/// person by their account's username and email (not `display_label`, which a rename leaves).
const PRINCIPAL_CHOICE: &str = "SELECT principal.id, principal.kind, \
     COALESCE(agent.name, account.username::text, principal.display_label) AS name, \
     COALESCE(agent.slug::text, account.email::text, '') AS detail \
     FROM principals AS principal \
     LEFT JOIN agents AS agent ON agent.company_id = principal.company_id \
      AND agent.id = principal.agent_id \
     LEFT JOIN users AS account ON account.id = principal.user_id \
     WHERE principal.company_id = $1";

fn principal_choices(
    rows: Vec<(Uuid, String, String, String)>,
) -> AppResult<Vec<DepartmentPrincipal>> {
    rows.into_iter()
        .map(|(id, kind, name, detail)| {
            Ok(DepartmentPrincipal {
                principal_id: PrincipalId::new(id),
                kind: PrincipalKind::from_str(&kind)
                    .map_err(|error| AppError::Internal(error.to_string()))?,
                name,
                detail,
            })
        })
        .collect()
}

const DEPARTMENT_COLUMNS: &str = "department.id, department.company_id, department.name, \
     department.slug::text AS slug, department.description, department.charter, \
     department.parent_id, department.is_root, department.created_by, department.created_at, \
     department.escalation";

#[derive(sqlx::FromRow)]
struct DepartmentDb {
    id: Uuid,
    company_id: Uuid,
    name: String,
    slug: String,
    description: Option<String>,
    charter: Option<String>,
    parent_id: Option<Uuid>,
    is_root: bool,
    created_by: serde_json::Value,
    escalation: Option<serde_json::Value>,
    created_at: DateTime<Utc>,
}

impl TryFrom<DepartmentDb> for Department {
    type Error = AppError;

    fn try_from(db: DepartmentDb) -> AppResult<Self> {
        Ok(Department {
            created_by: serde_json::from_value(db.created_by).map_err(|error| {
                AppError::Internal(format!(
                    "Invalid departments.created_by provenance for {}: {error}",
                    db.id
                ))
            })?,
            id: DepartmentId::new(db.id),
            company_id: db.company_id,
            name: db.name,
            slug: DepartmentSlug::from(db.slug),
            description: db.description,
            charter: db.charter,
            parent_id: db.parent_id.map(DepartmentId::new),
            escalation: db
                .escalation
                .map(serde_json::from_value)
                .transpose()
                .map_err(|error| {
                    AppError::Internal(format!(
                        "Invalid departments.escalation for {}: {error}",
                        db.id
                    ))
                })?,
            is_root: db.is_root,
            created_at: db.created_at,
        })
    }
}

fn stored_role(role: &str) -> AppResult<DepartmentRole> {
    DepartmentRole::from_str(role)
        .map_err(|error| AppError::Internal(format!("Invalid department_roles.role: {error}")))
}

/// A taken department address is routine input, not a database fault.
fn slug_conflict(error: sqlx::Error, slug: &str) -> AppError {
    let taken = error
        .as_database_error()
        .is_some_and(|database| database.constraint() == Some("departments_company_slug_key"));
    if taken {
        return AppError::Conflict(format!(
            "Department address '{slug}' is already in use in this company."
        ));
    }
    AppError::from(error)
}

/// Begin a department write: bounded statements, then the company write lock and the actor's
/// authority under it. Someone who may not manage the company sees it as missing.
async fn begin_department_write(
    persistence: &PostgresPersistence,
    scope: DepartmentScope,
) -> AppResult<Transaction<'_, Postgres>> {
    use super::company_invite::authority::{self as company_level_authority, TeamScope};
    let team = TeamScope {
        actor_id: scope.actor.user_id(),
        company_id: scope.company.as_uuid(),
    };
    let mut tx = company_level_authority::start(persistence.pool(), team, None).await?;
    company_level_authority::lock_team(&mut tx, team, None).await?;
    Ok(tx)
}

/// How strongly a write holds a department row it reads.
#[derive(Debug, Clone, Copy)]
enum RowLock {
    /// The department this write changes.
    Update,
    /// A parent it places a department under, which must not be deleted meanwhile.
    Share,
}

impl RowLock {
    const fn clause(self) -> &'static str {
        match self {
            Self::Update => "FOR UPDATE",
            Self::Share => "FOR SHARE",
        }
    }
}

async fn department_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    department: DepartmentId,
    lock: RowLock,
) -> AppResult<Option<Department>> {
    sqlx::query_as::<_, DepartmentDb>(&format!(
        "SELECT {DEPARTMENT_COLUMNS} FROM departments AS department \
         WHERE department.company_id = $1 AND department.id = $2 {}",
        lock.clause()
    ))
    .bind(company.as_uuid())
    .bind(department.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .map(Department::try_from)
    .transpose()
}

/// The department row, locked for this write, within its tenant.
async fn lock_department_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    department: DepartmentId,
) -> AppResult<Department> {
    department_on(tx, company, department, RowLock::Update)
        .await?
        .ok_or_else(department_not_found)
}

/// A named parent, held so it cannot be deleted under this write.
async fn lock_parent_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    parent: DepartmentId,
) -> AppResult<Department> {
    department_on(tx, company, parent, RowLock::Share)
        .await?
        .ok_or_else(|| AppError::NotFound("Parent department not found in this company.".into()))
}

/// Refuse a placement the shape guard would refuse, naming the departments involved. Uses the
/// schema's own `department_shape_problem`, so the two cannot disagree.
async fn check_shape_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    department: DepartmentId,
    name: &str,
    parent: &Department,
) -> AppResult<()> {
    let problem: Option<String> = sqlx::query_scalar("SELECT department_shape_problem($1, $2, $3)")
        .bind(company.as_uuid())
        .bind(department.as_uuid())
        .bind(parent.id.as_uuid())
        .fetch_one(&mut **tx)
        .await?;
    match problem.as_deref() {
        None => Ok(()),
        Some("cycle") if parent.id == department => Err(AppError::BadRequest(format!(
            "Department '{name}' cannot be its own parent."
        ))),
        Some("cycle") => Err(AppError::BadRequest(format!(
            "'{}' sits inside '{name}', so it cannot be its parent.",
            parent.name
        ))),
        Some("depth") => Err(AppError::BadRequest(format!(
            "Departments nest at most {MAX_DEPARTMENT_DEPTH} levels deep; placing '{name}' \
             under '{}' would go deeper.",
            parent.name
        ))),
        Some(other) => Err(AppError::Internal(format!(
            "Unknown department shape problem '{other}'"
        ))),
    }
}

/// A member or role holder must be one of this company's people or agents. `FOR KEY SHARE` waits
/// for a demotion in flight (removing a teammate or deleting an agent rewrites `kind`, a key
/// column) and then reads its outcome, as `guard_department_principal_kind` does.
async fn check_principal_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    principal: PrincipalId,
) -> AppResult<()> {
    let kind: Option<String> = sqlx::query_scalar(
        "SELECT kind FROM principals WHERE company_id = $1 AND id = $2 FOR KEY SHARE",
    )
    .bind(company.as_uuid())
    .bind(principal.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;
    let kind =
        kind.ok_or_else(|| AppError::NotFound("Principal not found in this company.".into()))?;
    let kind =
        PrincipalKind::from_str(&kind).map_err(|error| AppError::Internal(error.to_string()))?;
    match kind {
        PrincipalKind::Person | PrincipalKind::Agent => Ok(()),
        PrincipalKind::External | PrincipalKind::System => Err(AppError::BadRequest(
            "Only people and agents can belong to or hold a role in a department.".into(),
        )),
    }
}

/// Lock principals whose department rows this write is about to delete, `FOR KEY SHARE` and in
/// id order, before deleting them. A demotion holds its principal `FOR UPDATE` and then deletes
/// the same rows through `department_principal_demoted`; taking the principal first keeps the
/// two in one order (01DEP-P02-B1-F1).
async fn lock_principals_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    principals: &[Uuid],
) -> AppResult<()> {
    if principals.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "SELECT principal.id FROM principals AS principal \
          WHERE principal.company_id = $1 AND principal.id = ANY($2) \
          ORDER BY principal.id FOR KEY SHARE",
    )
    .bind(company.as_uuid())
    .bind(principals)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// After a `lead` write and its grant sync, the department desk follows the lead (01DEP-P03-D9):
/// last, so the gates the sync took stay the last gates taken.
async fn follow_lead_on(
    tx: &mut Transaction<'_, Postgres>,
    templates: &ProvisioningDeployment,
    company: CompanyId,
    actor: WorkflowActor,
    department: DepartmentId,
    role: DepartmentRole,
    lead: Option<PrincipalId>,
) -> AppResult<()> {
    if role != DepartmentRole::Lead {
        return Ok(());
    }
    // Boxed: re-pointing descends into template compilation and publication.
    Box::pin(follow_department_lead_on(
        tx, templates, company, actor, department, lead,
    ))
    .await
}

/// Membership and role rows for one principal, under the department row and the principals the
/// write touches (sorted), then one sync of the derived grants: the department's for membership,
/// the role's reach for a role.
fn department_member_insert(
    company: CompanyId,
    department: DepartmentId,
    principal: PrincipalId,
) -> sqlx::query::Query<'static, Postgres, sqlx::postgres::PgArguments> {
    sqlx::query(
            "INSERT INTO department_members (company_id, department_id, principal_id, access_level) \
             SELECT $1, $2, $3, CASE WHEN department.is_root THEN \
               CASE WHEN company.user_id = principal.user_id THEN 9 ELSE 0 END END \
             FROM departments AS department JOIN companies AS company ON company.id = department.company_id \
             JOIN principals AS principal ON principal.company_id = company.id AND principal.id = $3 \
             WHERE department.company_id = $1 AND department.id = $2 \
             ON CONFLICT (department_id, principal_id) DO NOTHING",
        )
        .bind(company.as_uuid())
        .bind(department.as_uuid())
        .bind(principal.as_uuid())
}

/// What a placement write actually changed, for the administration history.
#[derive(Debug, Clone, Default)]
pub(crate) struct Placed {
    member_added: bool,
    role_assigned: bool,
    /// The holders a single-holder role was taken from to make room.
    replaced: Vec<PrincipalId>,
}

async fn write_placement_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    department: DepartmentId,
    principal: PrincipalId,
    member: bool,
    role: Option<DepartmentRoleAssignment>,
) -> AppResult<Placed> {
    lock_department_on(tx, company, department).await?;
    // The holder a single-holder role replaces loses a row: lock them with the new one, first.
    let mut involved: Vec<Uuid> = match role
        .filter(|write| write.role.is_single_holder() || write.role == DepartmentRole::Sponsor)
    {
        Some(role) => {
            sqlx::query_scalar(
                "SELECT holder.principal_id FROM department_roles AS holder \
              WHERE holder.company_id = $1 AND holder.department_id = $2 AND holder.role = $3",
            )
            .bind(company.as_uuid())
            .bind(department.as_uuid())
            .bind(role.role.as_str())
            .fetch_all(&mut **tx)
            .await?
        }
        None => Vec::new(),
    };
    involved.push(principal.as_uuid());
    involved.sort_unstable();
    involved.dedup();
    lock_principals_on(tx, company, &involved).await?;
    check_principal_on(tx, company, principal).await?;
    let mut placed = Placed::default();
    if member {
        placed.member_added = department_member_insert(company, department, principal)
            .execute(&mut **tx)
            .await?
            .rows_affected()
            == 1;
    }
    let Some(assignment) = role else {
        super::workflow::role_binding::check_principals_on(tx, company, &involved).await?;
        sync_department_grants_on(tx, company, &[department]).await?;
        return Ok(placed);
    };
    roles::write_role_on(tx, company, department, assignment, &mut placed).await?;
    super::workflow::role_binding::check_principals_on(tx, company, &involved).await?;
    // The reach contains the department itself, so one sync covers the membership too.
    let reach = role_reach_on(tx, company, department).await?;
    sync_department_grants_on(tx, company, &reach).await?;
    Ok(placed)
}

/// Add `principal` to the department; adding a member again changes nothing. The caller holds
/// the company write lock.
pub(crate) async fn add_department_member_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    department: DepartmentId,
    principal: PrincipalId,
) -> AppResult<Placed> {
    write_placement_on(tx, company, department, principal, true, None).await
}

/// Remove `principal` from the department. The caller holds the company write lock.
pub(crate) async fn remove_department_member_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    department: DepartmentId,
    principal: PrincipalId,
) -> AppResult<()> {
    lock_department_on(tx, company, department).await?;
    lock_principals_on(tx, company, &[principal.as_uuid()]).await?;
    let removed = sqlx::query(
        "DELETE FROM department_members \
          WHERE company_id = $1 AND department_id = $2 AND principal_id = $3",
    )
    .bind(company.as_uuid())
    .bind(department.as_uuid())
    .bind(principal.as_uuid())
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if removed == 0 {
        return Err(AppError::NotFound(
            "That principal is not a member of this department.".into(),
        ));
    }
    super::workflow::role_binding::check_principals_on(tx, company, &[principal.as_uuid()]).await?;
    sync_department_grants_on(tx, company, &[department]).await?;
    Box::pin(super::workflow::follow_reviewers::refresh_on(tx, company)).await
}

/// The departments whose desks a role holder of `department` reaches: its own, its parent's
/// (as a child lead) and its children's (as their parent's lead), one step both ways
/// (01DEP-C002).
async fn role_reach_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    department: DepartmentId,
) -> AppResult<Vec<DepartmentId>> {
    let neighbours: Vec<Uuid> = sqlx::query_scalar(
        "SELECT department.parent_id FROM departments AS department \
          WHERE department.company_id = $1 AND department.id = $2 AND department.parent_id IS NOT NULL \
         UNION \
         SELECT child.id FROM departments AS child \
          WHERE child.company_id = $1 AND child.parent_id = $2",
    )
    .bind(company.as_uuid())
    .bind(department.as_uuid())
    .fetch_all(&mut **tx)
    .await?;
    Ok(std::iter::once(department)
        .chain(neighbours.into_iter().map(DepartmentId::new))
        .collect())
}

/// Every department of the company, by slug.
pub(crate) async fn departments_of_company(
    pool: &sqlx::PgPool,
    company: CompanyId,
) -> AppResult<Vec<Department>> {
    sqlx::query_as::<_, DepartmentDb>(&format!(
        "SELECT {DEPARTMENT_COLUMNS} FROM departments AS department \
         WHERE department.company_id = $1 ORDER BY department.slug, department.id"
    ))
    .bind(company.as_uuid())
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(Department::try_from)
    .collect()
}

/// Department persistence with the provisioning templates a department's desk is created and
/// re-pointed with (01DEP-P03-D7).
#[derive(Clone)]
pub struct PostgresDepartments {
    pub(super) persistence: PostgresPersistence,
    templates: ProvisioningDeployment,
}
impl PostgresDepartments {
    pub fn new(persistence: PostgresPersistence, templates: ProvisioningDeployment) -> Self {
        Self {
            persistence,
            templates,
        }
    }
}

#[async_trait]
impl DepartmentPersistence for PostgresDepartments {
    async fn create(
        &self,
        scope: DepartmentScope,
        write: DepartmentWrite,
        lead: Option<PrincipalId>,
    ) -> AppResult<Department> {
        let mut tx = begin_department_write(&self.persistence, scope).await?;
        let acting = require_department_management_on(&mut tx, scope, write.parent_id).await?;
        let department = create_department_on(
            &mut tx,
            &self.templates,
            scope.company,
            scope.actor,
            acting,
            &write,
            lead,
        )
        .await?;
        tx.commit().await?;
        Ok(department)
    }

    async fn update(
        &self,
        scope: DepartmentScope,
        department: DepartmentId,
        edit: DepartmentEdit,
    ) -> AppResult<Department> {
        let mut tx = begin_department_write(&self.persistence, scope).await?;
        let (mut acting, current) =
            require_parent_management_on(&mut tx, scope, department).await?;
        let destination = match edit.parent_id {
            Some(parent) => Some(require_department_management_on(&mut tx, scope, parent).await?),
            None => None,
        };
        let department = update_department_on(&mut tx, scope.company, department, &edit).await?;
        // A move is admitted by its destination's manager, so it records that role.
        let event = match destination {
            Some(role) if department.parent_id != current.parent_id => {
                acting = role;
                AdministrationEvent::Moved
            }
            _ => AdministrationEvent::Updated,
        };
        audit::record_on(
            &mut tx,
            scope.company,
            scope.actor,
            acting,
            department.id,
            event,
        )
        .await?;
        tx.commit().await?;
        Ok(department)
    }

    async fn delete(&self, scope: DepartmentScope, department: DepartmentId) -> AppResult<()> {
        let mut tx = begin_department_write(&self.persistence, scope).await?;
        let (acting, _) = require_parent_management_on(&mut tx, scope, department).await?;
        // Before the delete: the row snapshots the department's label while it still exists.
        audit::record_on(
            &mut tx,
            scope.company,
            scope.actor,
            acting,
            department,
            AdministrationEvent::Deleted,
        )
        .await?;
        delete_department_on(&mut tx, scope.company, department).await?;
        tx.commit().await.map_err(lifecycle::delete_error)?;
        Ok(())
    }

    async fn assign_role(
        &self,
        scope: DepartmentScope,
        department: DepartmentId,
        assignment: DepartmentRoleAssignment,
    ) -> AppResult<()> {
        let mut tx = begin_department_write(&self.persistence, scope).await?;
        assign_department_role_on(
            &mut tx,
            &self.templates,
            scope.company,
            scope.actor,
            department,
            assignment,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn clear_role(
        &self,
        scope: DepartmentScope,
        department: DepartmentId,
        removal: DepartmentRoleRemoval,
    ) -> AppResult<()> {
        let mut tx = begin_department_write(&self.persistence, scope).await?;
        clear_department_role_on(
            &mut tx,
            &self.templates,
            scope.company,
            scope.actor,
            department,
            removal,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn add_member(
        &self,
        scope: DepartmentScope,
        department: DepartmentId,
        principal: PrincipalId,
    ) -> AppResult<()> {
        let mut tx = begin_department_write(&self.persistence, scope).await?;
        let acting = require_department_management_on(&mut tx, scope, Some(department)).await?;
        let placed =
            add_department_member_on(&mut tx, scope.company, department, principal).await?;
        if placed.member_added {
            audit::record_on(
                &mut tx,
                scope.company,
                scope.actor,
                acting,
                department,
                AdministrationEvent::MemberAdded(principal),
            )
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn remove_member(
        &self,
        scope: DepartmentScope,
        department: DepartmentId,
        principal: PrincipalId,
    ) -> AppResult<()> {
        let root: Option<bool> =
            sqlx::query_scalar("SELECT is_root FROM departments WHERE company_id = $1 AND id = $2")
                .bind(scope.company.as_uuid())
                .bind(department.as_uuid())
                .fetch_optional(&self.persistence.pool)
                .await?;
        if root == Some(true) {
            return root::remove_root_member(&self.persistence, scope, department, principal).await;
        }
        let mut tx = begin_department_write(&self.persistence, scope).await?;
        let acting = require_department_management_on(&mut tx, scope, Some(department)).await?;
        remove_department_member_on(&mut tx, scope.company, department, principal).await?;
        audit::record_on(
            &mut tx,
            scope.company,
            scope.actor,
            acting,
            department,
            AdministrationEvent::MemberRemoved(principal),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn get(
        &self,
        company: CompanyId,
        department: DepartmentId,
    ) -> AppResult<Option<Department>> {
        sqlx::query_as::<_, DepartmentDb>(&format!(
            "SELECT {DEPARTMENT_COLUMNS} FROM departments AS department \
             WHERE department.company_id = $1 AND department.id = $2"
        ))
        .bind(company.as_uuid())
        .bind(department.as_uuid())
        .fetch_optional(&self.persistence.pool)
        .await?
        .map(Department::try_from)
        .transpose()
    }

    async fn list_by_company(&self, company: CompanyId) -> AppResult<Vec<Department>> {
        departments_of_company(&self.persistence.pool, company).await
    }

    async fn list_for_user(&self, company: CompanyId, user: Uuid) -> AppResult<Vec<Department>> {
        sqlx::query_as::<_, DepartmentDb>(&format!(
            "SELECT {DEPARTMENT_COLUMNS} FROM departments AS department \
             JOIN principals AS principal \
               ON principal.company_id = department.company_id \
              AND principal.user_id = $2 AND principal.kind = 'person' \
             WHERE department.company_id = $1 \
               AND (EXISTS (SELECT 1 FROM department_members AS member \
                             WHERE member.company_id = department.company_id \
                               AND member.department_id = department.id \
                               AND member.principal_id = principal.id) \
                 OR EXISTS (SELECT 1 FROM department_roles AS holder \
                             WHERE holder.company_id = department.company_id \
                               AND holder.department_id = department.id \
                               AND holder.principal_id = principal.id)) \
             ORDER BY department.slug, department.id"
        ))
        .bind(company.as_uuid())
        .bind(user)
        .fetch_all(&self.persistence.pool)
        .await?
        .into_iter()
        .map(Department::try_from)
        .collect()
    }

    async fn members(
        &self,
        company: CompanyId,
        department: DepartmentId,
    ) -> AppResult<Vec<DepartmentMember>> {
        let rows: Vec<(Uuid, DateTime<Utc>)> = sqlx::query_as(
            "SELECT member.principal_id, member.created_at FROM department_members AS member \
             WHERE member.company_id = $1 AND member.department_id = $2 \
             ORDER BY member.created_at, member.principal_id LIMIT $3",
        )
        .bind(company.as_uuid())
        .bind(department.as_uuid())
        .bind((crate::application::ports::department::MAX_DEPARTMENT_ROSTER_ENTRIES + 1) as i64)
        .fetch_all(&self.persistence.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(principal_id, created_at)| DepartmentMember {
                principal_id: PrincipalId::new(principal_id),
                created_at,
            })
            .collect())
    }

    async fn roles(
        &self,
        company: CompanyId,
        department: DepartmentId,
    ) -> AppResult<Vec<DepartmentRoleHolder>> {
        #[derive(sqlx::FromRow)]
        struct Holder {
            role: String,
            principal_id: Uuid,
            decider: bool,
        }
        let rows: Vec<Holder> = sqlx::query_as(
            "SELECT holder.role, holder.principal_id, holder.decider FROM department_roles AS holder \
             WHERE holder.company_id = $1 AND holder.department_id = $2 \
             ORDER BY CASE holder.role WHEN 'owner' THEN 0 WHEN 'lead' THEN 1 ELSE 2 END, \
                      holder.role, holder.created_at, holder.principal_id LIMIT $3",
        )
        .bind(company.as_uuid())
        .bind(department.as_uuid())
        .bind((crate::application::ports::department::MAX_DEPARTMENT_ROSTER_ENTRIES + 1) as i64)
        .fetch_all(&self.persistence.pool)
        .await?;
        rows.into_iter()
            .map(|holder| {
                Ok(DepartmentRoleHolder {
                    decider: holder.decider,
                    role: stored_role(&holder.role)?,
                    principal_id: PrincipalId::new(holder.principal_id),
                })
            })
            .collect()
    }

    async fn memberships(
        &self,
        company: CompanyId,
        principal: PrincipalId,
    ) -> AppResult<Vec<DepartmentId>> {
        let rows: Vec<Uuid> = sqlx::query_scalar(
            "SELECT member.department_id FROM department_members AS member \
             WHERE member.company_id = $1 AND member.principal_id = $2 \
             ORDER BY member.department_id",
        )
        .bind(company.as_uuid())
        .bind(principal.as_uuid())
        .fetch_all(&self.persistence.pool)
        .await?;
        Ok(rows.into_iter().map(DepartmentId::new).collect())
    }

    async fn roles_held(
        &self,
        company: CompanyId,
        principal: PrincipalId,
    ) -> AppResult<Vec<HeldDepartmentRole>> {
        let rows: Vec<(Uuid, String)> = sqlx::query_as(
            "SELECT holder.department_id, holder.role FROM department_roles AS holder \
             WHERE holder.company_id = $1 AND holder.principal_id = $2 \
             ORDER BY holder.department_id, holder.role",
        )
        .bind(company.as_uuid())
        .bind(principal.as_uuid())
        .fetch_all(&self.persistence.pool)
        .await?;
        rows.into_iter()
            .map(|(department_id, role)| {
                Ok(HeldDepartmentRole {
                    department_id: DepartmentId::new(department_id),
                    role: stored_role(&role)?,
                })
            })
            .collect()
    }

    async fn principals(
        &self,
        company: CompanyId,
        limit: usize,
    ) -> AppResult<Vec<DepartmentPrincipal>> {
        let rows = sqlx::query_as(&format!(
            "{PRINCIPAL_CHOICE} AND principal.kind IN ('agent', 'person') \
             ORDER BY principal.kind, name, principal.id LIMIT $2"
        ))
        .bind(company.as_uuid())
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .fetch_all(&self.persistence.pool)
        .await?;
        principal_choices(rows)
    }

    async fn principals_by_id(
        &self,
        company: CompanyId,
        ids: &[PrincipalId],
    ) -> AppResult<Vec<DepartmentPrincipal>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<Uuid> = ids.iter().map(|id| id.as_uuid()).collect();
        let rows = sqlx::query_as(&format!(
            "{PRINCIPAL_CHOICE} AND principal.id = ANY($2) ORDER BY name, principal.id"
        ))
        .bind(company.as_uuid())
        .bind(&ids)
        .fetch_all(&self.persistence.pool)
        .await?;
        principal_choices(rows)
    }

    async fn agent_departments(
        &self,
        company: CompanyId,
        agents: &[Uuid],
    ) -> AppResult<
        std::collections::HashMap<Uuid, (Vec<crate::entities::department::HeldDepartment>, bool)>,
    > {
        let mut connection = self.persistence.pool.acquire().await?;
        let placed = crate::adapters::persistence::department_roster::agent_departments_on(
            &mut connection,
            company,
            agents,
        )
        .await?;
        Ok(placed
            .into_iter()
            .map(|(agent, (listed, more))| {
                let listed = listed
                    .into_iter()
                    .map(|department| crate::entities::department::HeldDepartment {
                        id: DepartmentId::new(department.id),
                        name: department.name,
                        slug: department.slug,
                        roles: department.roles,
                    })
                    .collect();
                (agent, (listed, more))
            })
            .collect())
    }

    async fn desk_owners(&self, company: CompanyId) -> AppResult<Vec<(Uuid, DepartmentId)>> {
        let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
            "SELECT desk.id, desk.department_id FROM channels AS desk \
             WHERE desk.company_id = $1 AND desk.department_id IS NOT NULL",
        )
        .bind(company.as_uuid())
        .fetch_all(&self.persistence.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(desk, department)| (desk, DepartmentId::new(department)))
            .collect())
    }

    async fn administration_history(
        &self,
        company: CompanyId,
        department: DepartmentId,
    ) -> AppResult<Vec<crate::entities::department::DepartmentAdministrationEntry>> {
        use crate::entities::department::{
            DEPARTMENT_HISTORY_ENTRIES, DepartmentAdministrationAction,
            DepartmentAdministrationEntry,
        };
        type Row = (
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            DateTime<Utc>,
        );
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT transition_kind, actor_label, acting_role, principal_label, role, created_at \
             FROM department_administration_events \
             WHERE company_id = $1 AND department_id = $2 \
             ORDER BY created_at DESC, id DESC LIMIT $3",
        )
        .bind(company.as_uuid())
        .bind(department.as_uuid())
        .bind(DEPARTMENT_HISTORY_ENTRIES as i64)
        .fetch_all(&self.persistence.pool)
        .await?;
        rows.into_iter()
            .map(|(kind, actor, acting_role, subject, role, created_at)| {
                Ok(DepartmentAdministrationEntry {
                    action: DepartmentAdministrationAction::from_str(&kind)
                        .map_err(AppError::Internal)?,
                    actor,
                    acting_role,
                    subject,
                    role: role
                        .map(|role| DepartmentRole::from_str(&role))
                        .transpose()
                        .map_err(|error| AppError::Internal(error.to_string()))?,
                    created_at,
                })
            })
            .collect()
    }

    async fn desks(
        &self,
        company: CompanyId,
        department: DepartmentId,
    ) -> AppResult<Vec<DepartmentDesk>> {
        let rows: Vec<(Uuid, String, String, bool)> = sqlx::query_as(
            "SELECT desk.id, desk.name, desk_slug.slug::text, \
                    desk_record.command_key IS NOT NULL AS own \
             FROM channels AS desk \
             JOIN addresses AS desk_slug ON desk_slug.company_id = desk.company_id \
              AND desk_slug.channel_id = desk.id AND desk_slug.is_primary \
             LEFT JOIN workflow_provisioning_commands AS desk_record \
               ON desk_record.company_id = desk.company_id AND desk_record.command_key = $3 \
              AND (desk_record.result->'channel'->>'id')::uuid = desk.id \
             WHERE desk.company_id = $1 AND desk.department_id = $2 \
             ORDER BY own DESC, desk_slug.slug, desk.id",
        )
        .bind(company.as_uuid())
        .bind(department.as_uuid())
        .bind(department_desk_key(department))
        .fetch_all(&self.persistence.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, name, slug, own)| DepartmentDesk {
                id,
                name,
                slug,
                own,
            })
            .collect())
    }
}

#[cfg(test)]
#[path = "department_tests.rs"]
mod tests;
