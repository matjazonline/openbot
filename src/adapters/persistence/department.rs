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

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{Postgres, Transaction};
use std::str::FromStr;
use uuid::Uuid;

use crate::adapters::persistence::PostgresPersistence;
use crate::adapters::persistence::agent_publication::{begin_bounded, lock_company_on};
use crate::adapters::persistence::department_desks::sync_department_grants_on;
use crate::app_error::{AppError, AppResult};
use crate::application::agent::AgentCompanyScope;
use crate::application::ports::department::{
    DepartmentEdit, DepartmentPersistence, DepartmentScope, DepartmentWrite,
};
use crate::domain::workflow::CompanyId;
use crate::entities::creation::CreationProvenance;
use crate::entities::department::{
    Department, DepartmentId, DepartmentMember, DepartmentRole, DepartmentRoleHolder,
    HeldDepartmentRole, MAX_DEPARTMENT_DEPTH,
};
use crate::entities::participant::PrincipalKind;
use crate::entities::transport::PrincipalId;
use crate::entities::value_objects::DepartmentSlug;
use crate::use_cases::company::company_not_found;
use crate::use_cases::department::department_not_found;

const DEPARTMENT_COLUMNS: &str = "department.id, department.company_id, department.name, \
     department.slug::text AS slug, department.description, department.charter, \
     department.parent_id, department.created_by, department.created_at";

#[derive(sqlx::FromRow)]
struct DepartmentDb {
    id: Uuid,
    company_id: Uuid,
    name: String,
    slug: String,
    description: Option<String>,
    charter: Option<String>,
    parent_id: Option<Uuid>,
    created_by: serde_json::Value,
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
    let mut tx = begin_bounded(persistence).await?;
    let company = AgentCompanyScope {
        company: scope.company,
        actor: scope.actor,
    };
    lock_company_on(&mut tx, company)
        .await
        .map_err(|error| match error {
            AppError::NotFound(_) => company_not_found(),
            other => other,
        })?;
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

/// Create a department, with its lead when one is named. The caller holds the company write lock.
pub(crate) async fn create_department_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    write: &DepartmentWrite,
    lead: Option<PrincipalId>,
) -> AppResult<Department> {
    create_department_with_id_on(tx, company, DepartmentId::random(), write, lead).await
}

/// [`create_department_on`] under a caller-chosen id, for a creator that replays by id
/// (onboarding's first department, 01DEP-P03-D4).
pub(crate) async fn create_department_with_id_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    id: DepartmentId,
    write: &DepartmentWrite,
    lead: Option<PrincipalId>,
) -> AppResult<Department> {
    if let Some(parent) = write.parent_id {
        let parent = lock_parent_on(tx, company, parent).await?;
        check_shape_on(tx, company, id, &write.name, &parent).await?;
    }
    let created_by = serde_json::to_value(
        write
            .created_by
            .clone()
            .unwrap_or_else(CreationProvenance::system),
    )
    .map_err(|error| AppError::Internal(error.to_string()))?;
    let department = sqlx::query_as::<_, DepartmentDb>(&format!(
        "INSERT INTO departments AS department \
             (id, company_id, name, slug, description, charter, parent_id, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING {DEPARTMENT_COLUMNS}"
    ))
    .bind(id.as_uuid())
    .bind(company.as_uuid())
    .bind(&write.name)
    .bind(&write.slug)
    .bind(&write.description)
    .bind(&write.charter)
    .bind(write.parent_id.map(DepartmentId::as_uuid))
    .bind(created_by)
    .fetch_one(&mut **tx)
    .await
    .map_err(|error| slug_conflict(error, &write.slug))?;
    let department = Department::try_from(department)?;
    if let Some(lead) = lead {
        assign_department_role_on(tx, company, department.id, DepartmentRole::Lead, lead).await?;
    }
    Ok(department)
}

/// Apply an edit to a department under its row lock. The caller holds the company write lock.
pub(crate) async fn update_department_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    department: DepartmentId,
    edit: &DepartmentEdit,
) -> AppResult<Department> {
    let current = lock_department_on(tx, company, department).await?;
    let name = edit.name.clone().unwrap_or_else(|| current.name.clone());
    let slug = edit
        .slug
        .clone()
        .unwrap_or_else(|| current.slug.to_string());
    let description = edit
        .description
        .clone()
        .unwrap_or_else(|| current.description.clone());
    let charter = edit
        .charter
        .clone()
        .unwrap_or_else(|| current.charter.clone());
    let parent_id = edit.parent_id.unwrap_or(current.parent_id);
    if parent_id != current.parent_id
        && let Some(parent) = parent_id
    {
        let parent = lock_parent_on(tx, company, parent).await?;
        check_shape_on(tx, company, department, &name, &parent).await?;
    }
    let updated = sqlx::query_as::<_, DepartmentDb>(&format!(
        "UPDATE departments AS department \
            SET name = $3, slug = $4, description = $5, charter = $6, parent_id = $7 \
          WHERE department.company_id = $1 AND department.id = $2 \
         RETURNING {DEPARTMENT_COLUMNS}"
    ))
    .bind(company.as_uuid())
    .bind(department.as_uuid())
    .bind(&name)
    .bind(&slug)
    .bind(&description)
    .bind(&charter)
    .bind(parent_id.map(DepartmentId::as_uuid))
    .fetch_one(&mut **tx)
    .await
    .map_err(|error| slug_conflict(error, &slug))?;
    let updated = Department::try_from(updated)?;
    // The moved department's desks gain a new parent lead; the old and the new parent's desks
    // lose and gain it as a child lead (01DEP-C002).
    if updated.parent_id != current.parent_id {
        let mut reach = vec![department];
        reach.extend(current.parent_id);
        reach.extend(updated.parent_id);
        sync_department_grants_on(tx, company, &reach).await?;
    }
    Ok(updated)
}

/// Delete a department that has no child departments; its members and roles go with it. The
/// caller holds the company write lock.
pub(crate) async fn delete_department_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    department: DepartmentId,
) -> AppResult<()> {
    let current = lock_department_on(tx, company, department).await?;
    let has_children: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM departments AS child \
                         WHERE child.company_id = $1 AND child.parent_id = $2)",
    )
    .bind(company.as_uuid())
    .bind(department.as_uuid())
    .fetch_one(&mut **tx)
    .await?;
    if has_children {
        return Err(AppError::Conflict(format!(
            "Department '{}' still has child departments. Move or delete them first.",
            current.name
        )));
    }
    let owns_desks: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM channels AS desk \
                         WHERE desk.company_id = $1 AND desk.department_id = $2)",
    )
    .bind(company.as_uuid())
    .bind(department.as_uuid())
    .fetch_one(&mut **tx)
    .await?;
    if owns_desks {
        return Err(AppError::Conflict(format!(
            "Department '{}' still owns desks. Move them to another department or delete them first.",
            current.name
        )));
    }
    // Its member and role rows cascade with it: lock their principals first, as row deletes do.
    let placed: Vec<Uuid> = sqlx::query_scalar(
        "SELECT member.principal_id FROM department_members AS member \
          WHERE member.company_id = $1 AND member.department_id = $2 \
         UNION \
         SELECT holder.principal_id FROM department_roles AS holder \
          WHERE holder.company_id = $1 AND holder.department_id = $2 \
         ORDER BY 1",
    )
    .bind(company.as_uuid())
    .bind(department.as_uuid())
    .fetch_all(&mut **tx)
    .await?;
    lock_principals_on(tx, company, &placed).await?;
    sqlx::query("DELETE FROM departments WHERE company_id = $1 AND id = $2")
        .bind(company.as_uuid())
        .bind(department.as_uuid())
        .execute(&mut **tx)
        .await
        .map_err(|error| {
            if error
                .as_database_error()
                .is_some_and(|database| database.is_foreign_key_violation())
            {
                AppError::Conflict(format!(
                    "Department '{}' is still referenced and cannot be deleted.",
                    current.name
                ))
            } else {
                AppError::from(error)
            }
        })?;
    // Its lead was a child lead of the parent's desks.
    if let Some(parent) = current.parent_id {
        sync_department_grants_on(tx, company, &[parent]).await?;
    }
    Ok(())
}

/// Give `principal` the role. A single-holder role replaces its current holder in this same
/// transaction, so the one-lead index is never the person's error. Assigning the current holder
/// again changes nothing. The caller holds the company write lock.
pub(crate) async fn assign_department_role_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    department: DepartmentId,
    role: DepartmentRole,
    principal: PrincipalId,
) -> AppResult<()> {
    write_placement_on(tx, company, department, principal, false, Some(role)).await
}

/// Make `principal` a member of the department and, with `role`, its holder of that role, as
/// company-agent creation places a new agent (plan 01-departments 03C item 0). One write: the
/// rows, then a single grant sync over everything they reach, so the department's desk gates
/// are taken once, in order, with its neighbours' (01DEP-P03-B1-F1). The caller holds the company
/// write lock.
pub(crate) async fn place_in_department_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    department: DepartmentId,
    principal: PrincipalId,
    role: Option<DepartmentRole>,
) -> AppResult<()> {
    write_placement_on(tx, company, department, principal, true, role).await
}

/// Membership and role rows for one principal, under the department row and the principals the
/// write touches (sorted), then one sync of the derived grants: the department's for membership,
/// the role's reach for a role.
async fn write_placement_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    department: DepartmentId,
    principal: PrincipalId,
    member: bool,
    role: Option<DepartmentRole>,
) -> AppResult<()> {
    lock_department_on(tx, company, department).await?;
    // The holder a single-holder role replaces loses a row: lock them with the new one, first.
    let mut involved: Vec<Uuid> = match role.filter(|role| role.is_single_holder()) {
        Some(role) => {
            sqlx::query_scalar(
                "SELECT holder.principal_id FROM department_roles AS holder \
              WHERE holder.company_id = $1 AND holder.department_id = $2 AND holder.role = $3",
            )
            .bind(company.as_uuid())
            .bind(department.as_uuid())
            .bind(role.as_str())
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
    if member {
        sqlx::query(
            "INSERT INTO department_members (company_id, department_id, principal_id) \
             VALUES ($1, $2, $3) ON CONFLICT (department_id, principal_id) DO NOTHING",
        )
        .bind(company.as_uuid())
        .bind(department.as_uuid())
        .bind(principal.as_uuid())
        .execute(&mut **tx)
        .await?;
    }
    let Some(role) = role else {
        return sync_department_grants_on(tx, company, &[department]).await;
    };
    if role.is_single_holder() {
        sqlx::query(
            "DELETE FROM department_roles \
              WHERE company_id = $1 AND department_id = $2 AND role = $3 AND principal_id <> $4",
        )
        .bind(company.as_uuid())
        .bind(department.as_uuid())
        .bind(role.as_str())
        .bind(principal.as_uuid())
        .execute(&mut **tx)
        .await?;
    }
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
    .await?;
    // The reach contains the department itself, so one sync covers the membership too.
    let reach = role_reach_on(tx, company, department).await?;
    sync_department_grants_on(tx, company, &reach).await
}

/// Take the role from `principal`. The caller holds the company write lock.
pub(crate) async fn clear_department_role_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    department: DepartmentId,
    role: DepartmentRole,
    principal: PrincipalId,
) -> AppResult<()> {
    lock_department_on(tx, company, department).await?;
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
    let reach = role_reach_on(tx, company, department).await?;
    sync_department_grants_on(tx, company, &reach).await
}

/// Add `principal` to the department; adding a member again changes nothing. The caller holds
/// the company write lock.
pub(crate) async fn add_department_member_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    department: DepartmentId,
    principal: PrincipalId,
) -> AppResult<()> {
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
    sync_department_grants_on(tx, company, &[department]).await
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

#[async_trait]
impl DepartmentPersistence for PostgresPersistence {
    async fn create(
        &self,
        scope: DepartmentScope,
        write: DepartmentWrite,
        lead: Option<PrincipalId>,
    ) -> AppResult<Department> {
        let mut tx = begin_department_write(self, scope).await?;
        let department = create_department_on(&mut tx, scope.company, &write, lead).await?;
        tx.commit().await?;
        Ok(department)
    }

    async fn update(
        &self,
        scope: DepartmentScope,
        department: DepartmentId,
        edit: DepartmentEdit,
    ) -> AppResult<Department> {
        let mut tx = begin_department_write(self, scope).await?;
        let department = update_department_on(&mut tx, scope.company, department, &edit).await?;
        tx.commit().await?;
        Ok(department)
    }

    async fn delete(&self, scope: DepartmentScope, department: DepartmentId) -> AppResult<()> {
        let mut tx = begin_department_write(self, scope).await?;
        delete_department_on(&mut tx, scope.company, department).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn assign_role(
        &self,
        scope: DepartmentScope,
        department: DepartmentId,
        role: DepartmentRole,
        principal: PrincipalId,
    ) -> AppResult<()> {
        let mut tx = begin_department_write(self, scope).await?;
        assign_department_role_on(&mut tx, scope.company, department, role, principal).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn clear_role(
        &self,
        scope: DepartmentScope,
        department: DepartmentId,
        role: DepartmentRole,
        principal: PrincipalId,
    ) -> AppResult<()> {
        let mut tx = begin_department_write(self, scope).await?;
        clear_department_role_on(&mut tx, scope.company, department, role, principal).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn add_member(
        &self,
        scope: DepartmentScope,
        department: DepartmentId,
        principal: PrincipalId,
    ) -> AppResult<()> {
        let mut tx = begin_department_write(self, scope).await?;
        add_department_member_on(&mut tx, scope.company, department, principal).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn remove_member(
        &self,
        scope: DepartmentScope,
        department: DepartmentId,
        principal: PrincipalId,
    ) -> AppResult<()> {
        let mut tx = begin_department_write(self, scope).await?;
        remove_department_member_on(&mut tx, scope.company, department, principal).await?;
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
        .fetch_optional(&self.pool)
        .await?
        .map(Department::try_from)
        .transpose()
    }

    async fn list_by_company(&self, company: CompanyId) -> AppResult<Vec<Department>> {
        sqlx::query_as::<_, DepartmentDb>(&format!(
            "SELECT {DEPARTMENT_COLUMNS} FROM departments AS department \
             WHERE department.company_id = $1 ORDER BY department.slug, department.id"
        ))
        .bind(company.as_uuid())
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(Department::try_from)
        .collect()
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
        .fetch_all(&self.pool)
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
             ORDER BY member.created_at, member.principal_id",
        )
        .bind(company.as_uuid())
        .bind(department.as_uuid())
        .fetch_all(&self.pool)
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
        let rows: Vec<(String, Uuid)> = sqlx::query_as(
            "SELECT holder.role, holder.principal_id FROM department_roles AS holder \
             WHERE holder.company_id = $1 AND holder.department_id = $2 \
             ORDER BY holder.role, holder.created_at, holder.principal_id",
        )
        .bind(company.as_uuid())
        .bind(department.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|(role, principal_id)| {
                Ok(DepartmentRoleHolder {
                    role: stored_role(&role)?,
                    principal_id: PrincipalId::new(principal_id),
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
        .fetch_all(&self.pool)
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
        .fetch_all(&self.pool)
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
}

#[cfg(test)]
#[path = "department_tests.rs"]
mod tests;
