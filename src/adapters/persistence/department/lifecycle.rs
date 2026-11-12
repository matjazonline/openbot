//! Department creation, editing and retained-history deletion guards.
use super::*;

/// Create a department with its desk, and its lead when one is named (01DEP-P03-D8). The caller
/// holds the company write lock and `actor`'s account row.
pub(crate) async fn create_department_on(
    tx: &mut Transaction<'_, Postgres>,
    templates: &ProvisioningDeployment,
    company: CompanyId,
    actor: WorkflowActor,
    write: &DepartmentWrite,
    lead: Option<PrincipalId>,
) -> AppResult<Department> {
    create_department_with_id_on(
        tx,
        templates,
        company,
        actor,
        DepartmentId::random(),
        write,
        lead,
    )
    .await
}

/// [`create_department_on`] under a caller-chosen id, for a creator that replays by id
/// (onboarding's first department, 01DEP-P03-D4).
pub(crate) async fn create_department_with_id_on(
    tx: &mut Transaction<'_, Postgres>,
    templates: &ProvisioningDeployment,
    company: CompanyId,
    actor: WorkflowActor,
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
    // Boxed: an agent lead's desk descends into template compilation and publication.
    Box::pin(provision_department_desk_on(
        tx,
        templates,
        company,
        actor,
        &department,
        lead,
    ))
    .await?;
    if let Some(lead) = lead {
        assign_department_role_on(
            tx,
            templates,
            company,
            actor,
            department.id,
            DepartmentRole::Lead,
            lead,
        )
        .await?;
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
        Box::pin(super::super::workflow::follow_reviewers::refresh_on(
            tx, company,
        ))
        .await?;
    }
    Ok(updated)
}

/// Delete a department that has no child departments; its members and roles go with it. The
/// caller holds the company write lock.
async fn guard_department_delete_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    current: &Department,
) -> AppResult<()> {
    let department = current.id;
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
    let captured: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM workflow_run_role_bindings WHERE company_id=$1 AND department_id=$2)",
    ).bind(company.as_uuid()).bind(department.as_uuid()).fetch_one(&mut **tx).await?;
    if captured {
        return Err(AppError::Conflict(format!(
            "Department '{}' is retained by workflow role history and cannot be deleted.",
            current.name
        )));
    }
    Ok(())
}

pub(crate) async fn delete_department_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    department: DepartmentId,
) -> AppResult<()> {
    let current = lock_department_on(tx, company, department).await?;
    guard_department_delete_on(tx, company, &current).await?;
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
    // Its desk is gone or detached by now (it owned none); so goes the record of it.
    sqlx::query(
        "DELETE FROM workflow_provisioning_commands WHERE company_id = $1 AND command_key = $2",
    )
    .bind(company.as_uuid())
    .bind(department_desk_key(department))
    .execute(&mut **tx)
    .await?;
    Ok(())
}
