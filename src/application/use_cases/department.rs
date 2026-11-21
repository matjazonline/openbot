//! Managing departments, their members and their roles (01-departments).
//!
//! Configuration follows the unit hierarchy. A refusal is `NotFound`, so department ids
//! cannot be probed and management never confers conversation visibility.
//! Persistence re-checks that authority under the company write lock.
use crate::application::ports::department::{DepartmentRoleAssignment, DepartmentRoleRemoval};
use std::sync::Arc;

use tracing::{info, instrument};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    application::{
        ports::department::{
            DepartmentEdit, DepartmentPersistence, DepartmentScope, DepartmentWrite,
        },
        workflow::WorkflowActor,
    },
    domain::workflow::CompanyId,
    entities::{
        creation::CreationProvenance,
        department::{
            Department, DepartmentDesk, DepartmentId, DepartmentMember, DepartmentPrincipal,
            DepartmentRoleHolder, DeskDepartments, MAX_DEPARTMENT_CHARTER_BYTES,
            MAX_DEPARTMENT_PRINCIPAL_CHOICES,
        },
        transport::PrincipalId,
    },
    use_cases::{
        channel::{SlugKind, validate_slug},
        company::{CompanyPersistence, readable_company},
    },
};

/// A department of another company is reported exactly like a missing one.
pub fn department_not_found() -> AppError {
    AppError::NotFound("Department not found in this company.".into())
}

/// Trimmed name, canonical slug and blank optional text as `None`.
fn normalized_name(name: &str) -> AppResult<String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::BadRequest(
            "The department name cannot be empty.".into(),
        ));
    }
    Ok(name)
}

/// The department's slug is also its desk's address, so it obeys the channel address rules: the
/// reserved suffixes, and the `departments_slug_format` shape, checked here so a malformed slug is
/// a worded refusal rather than a constraint error.
fn normalized_slug(slug: &str) -> AppResult<String> {
    let slug = slug.trim().to_lowercase().replace(' ', "-");
    validate_slug(&slug, SlugKind::DepartmentAddress)?;
    let well_formed = slug.chars().all(|character| {
        character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
    }) && !slug.starts_with('-')
        && !slug.ends_with('-');
    if !well_formed {
        return Err(AppError::BadRequest(format!(
            "Invalid department address '{slug}': use lowercase letters, digits and inner hyphens."
        )));
    }
    Ok(slug)
}

fn normalized_text(text: Option<String>) -> Option<String> {
    text.map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn normalized_charter(charter: Option<String>) -> AppResult<Option<String>> {
    let charter = normalized_text(charter);
    if charter
        .as_ref()
        .is_some_and(|charter| charter.len() > MAX_DEPARTMENT_CHARTER_BYTES)
    {
        return Err(AppError::BadRequest(format!(
            "A department charter can be at most {MAX_DEPARTMENT_CHARTER_BYTES} bytes."
        )));
    }
    Ok(charter)
}

impl DepartmentWrite {
    /// Canonical forms, once, in the use case, so every entry point stores the same shape.
    pub(crate) fn normalize(&mut self) -> AppResult<()> {
        self.name = normalized_name(&self.name)?;
        self.slug = normalized_slug(&self.slug)?;
        self.description = normalized_text(self.description.take());
        self.charter = normalized_charter(self.charter.take())?;
        Ok(())
    }
}

impl DepartmentEdit {
    /// The same canonical forms as [`DepartmentWrite::normalize`], for the fields this edit sets.
    pub(crate) fn normalize(&mut self) -> AppResult<()> {
        if let Some(name) = self.name.as_mut() {
            *name = normalized_name(name)?;
        }
        if let Some(slug) = self.slug.as_mut() {
            *slug = normalized_slug(slug)?;
        }
        if let Some(description) = self.description.as_mut() {
            *description = normalized_text(description.take());
        }
        if let Some(charter) = self.charter.as_mut() {
            *charter = normalized_charter(charter.take())?;
        }
        Ok(())
    }
}

/// One department with who belongs to it and who holds its roles.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DepartmentDetail {
    #[serde(flatten)]
    pub department: Department,
    pub members: Vec<DepartmentMember>,
    pub members_truncated: bool,
    pub roles: Vec<DepartmentRoleHolder>,
    pub roles_truncated: bool,
}

/// One department as the Departments tab shows it: its detail, the people and agents that detail
/// names, and the desks it owns.
#[derive(Debug, Clone)]
pub struct DepartmentView {
    pub detail: DepartmentDetail,
    pub principals: Vec<DepartmentPrincipal>,
    pub desks: Vec<DepartmentDesk>,
}

impl DepartmentView {
    /// The named principal, when it is still one of the company's.
    pub fn principal(&self, id: PrincipalId) -> Option<&DepartmentPrincipal> {
        self.principals
            .iter()
            .find(|principal| principal.principal_id == id)
    }
}

#[derive(Clone)]
pub struct DepartmentUseCases {
    companies: Arc<dyn CompanyPersistence>,
    departments: Arc<dyn DepartmentPersistence>,
}

impl DepartmentUseCases {
    pub fn new(
        companies: Arc<dyn CompanyPersistence>,
        departments: Arc<dyn DepartmentPersistence>,
    ) -> Self {
        Self {
            companies,
            departments,
        }
    }

    async fn managed(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        unit: Option<DepartmentId>,
    ) -> AppResult<DepartmentScope> {
        use crate::application::ports::administration::{ManagementTarget, require_management};
        require_management(
            self.departments.as_ref(),
            user_id,
            CompanyId::new(company_id),
            unit.map_or(ManagementTarget::Root, ManagementTarget::Unit),
        )
        .await?;
        Ok(DepartmentScope {
            actor: WorkflowActor::authenticated(user_id)?,
            company: CompanyId::new(company_id),
        })
    }

    /// Collection entry for unit workspaces: the person manages at least one unit.
    async fn administering(&self, user_id: Uuid, company_id: Uuid) -> AppResult<DepartmentScope> {
        use crate::application::ports::administration::{ManagementTarget, require_management};
        require_management(
            self.departments.as_ref(),
            user_id,
            CompanyId::new(company_id),
            ManagementTarget::AnyUnit,
        )
        .await?;
        Ok(DepartmentScope {
            actor: WorkflowActor::authenticated(user_id)?,
            company: CompanyId::new(company_id),
        })
    }

    /// The departments a non-administrator may see: those they belong to or hold a role in, and
    /// every department they manage (Plan06 02A). `None` means all of them, for a company
    /// administrator.
    async fn visible_set(
        &self,
        access: &crate::entities::company::CompanyAccess,
        user_id: Uuid,
    ) -> AppResult<Option<std::collections::HashSet<DepartmentId>>> {
        if access.membership.administers_company() {
            return Ok(None);
        }
        let company = CompanyId::new(access.company.id);
        let mut seen: std::collections::HashSet<DepartmentId> = self
            .departments
            .list_for_user(company, user_id)
            .await?
            .into_iter()
            .map(|department| department.id)
            .collect();
        seen.extend(self.departments.managed_units(user_id, company).await?);
        Ok(Some(seen))
    }

    async fn managed_parent(
        &self,
        user: Uuid,
        company: Uuid,
        unit: DepartmentId,
    ) -> AppResult<DepartmentScope> {
        let department = self
            .departments
            .get(CompanyId::new(company), unit)
            .await?
            .ok_or_else(department_not_found)?;
        self.managed(user, company, department.parent_id).await
    }

    async fn managed_roles(
        &self,
        user: Uuid,
        company: Uuid,
        unit: DepartmentId,
    ) -> AppResult<DepartmentScope> {
        use crate::application::ports::administration::{ManagementRole, ManagementTarget};
        let department = self
            .departments
            .get(CompanyId::new(company), unit)
            .await?
            .ok_or_else(department_not_found)?;
        if department.is_root
            && self
                .departments
                .management_role(user, CompanyId::new(company), ManagementTarget::Root)
                .await?
                != Some(ManagementRole::Owner)
        {
            return Err(department_not_found());
        }
        self.managed(user, company, department.parent_id).await
    }

    #[instrument(skip_all)]
    pub async fn create_department(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        mut write: DepartmentWrite,
        lead: Option<PrincipalId>,
    ) -> AppResult<Department> {
        let scope = self.managed(user_id, company_id, write.parent_id).await?;
        write.created_by = Some(CreationProvenance::user(user_id));
        write.normalize()?;
        info!(%company_id, "Creating department");
        self.departments.create(scope, write, lead).await
    }

    #[instrument(skip_all)]
    pub async fn update_department(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        department: DepartmentId,
        mut edit: DepartmentEdit,
    ) -> AppResult<Department> {
        let scope = self.managed_parent(user_id, company_id, department).await?;
        if let Some(parent) = edit.parent_id {
            self.managed(user_id, company_id, parent).await?;
        }
        edit.normalize()?;
        info!(%company_id, %department, "Updating department");
        self.departments.update(scope, department, edit).await
    }

    #[instrument(skip_all)]
    pub async fn delete_department(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        department: DepartmentId,
    ) -> AppResult<()> {
        let scope = self.managed_parent(user_id, company_id, department).await?;
        info!(%company_id, %department, "Deleting department");
        self.departments.delete(scope, department).await
    }

    #[instrument(skip_all)]
    pub async fn assign_role(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        department: DepartmentId,
        assignment: DepartmentRoleAssignment,
    ) -> AppResult<()> {
        let scope = self.managed_roles(user_id, company_id, department).await?;
        info!(%company_id, %department, role=%assignment.role, principal=%assignment.principal, "Assigning department role");
        self.departments
            .assign_role(scope, department, assignment)
            .await
    }

    #[instrument(skip_all)]
    pub async fn clear_role(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        department: DepartmentId,
        removal: DepartmentRoleRemoval,
    ) -> AppResult<()> {
        let scope = self.managed_roles(user_id, company_id, department).await?;
        info!(%company_id, %department, role=%removal.role, principal=%removal.principal, "Clearing department role");
        self.departments
            .clear_role(scope, department, removal)
            .await
    }

    #[instrument(skip_all)]
    pub async fn add_member(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        department: DepartmentId,
        principal: PrincipalId,
    ) -> AppResult<()> {
        let scope = self.managed(user_id, company_id, Some(department)).await?;
        info!(%company_id, %department, %principal, "Adding department member");
        self.departments
            .add_member(scope, department, principal)
            .await
    }

    #[instrument(skip_all)]
    pub async fn remove_member(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        department: DepartmentId,
        principal: PrincipalId,
    ) -> AppResult<()> {
        let scope = self.managed(user_id, company_id, Some(department)).await?;
        info!(%company_id, %department, %principal, "Removing department member");
        self.departments
            .remove_member(scope, department, principal)
            .await
    }

    /// The ids of the departments the caller manages (none for a plain member), to decide which
    /// controls a page offers. Each write still checks its own target.
    #[instrument(skip_all)]
    pub async fn managed_units(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<std::collections::HashSet<DepartmentId>> {
        readable_company(self.companies.as_ref(), user_id, company_id).await?;
        Ok(self
            .departments
            .managed_units(user_id, CompanyId::new(company_id))
            .await?
            .into_iter()
            .collect())
    }

    /// The departments the caller manages, for pickers and destinations: every one for a root
    /// manager. Each write still checks its own target.
    #[instrument(skip_all)]
    pub async fn list_departments(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Vec<Department>> {
        let scope = self.administering(user_id, company_id).await?;
        let managed: std::collections::HashSet<DepartmentId> = self
            .departments
            .managed_units(user_id, scope.company)
            .await?
            .into_iter()
            .collect();
        Ok(self
            .departments
            .list_by_company(scope.company)
            .await?
            .into_iter()
            .filter(|department| managed.contains(&department.id))
            .collect())
    }

    /// The departments this member may see: all of them for an owner or admin, otherwise the ones
    /// they belong to or hold a role in and every one they manage.
    #[instrument(skip_all)]
    pub async fn visible_departments(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Vec<Department>> {
        let access = readable_company(self.companies.as_ref(), user_id, company_id).await?;
        let all = self
            .departments
            .list_by_company(CompanyId::new(company_id))
            .await?;
        Ok(match self.visible_set(&access, user_id).await? {
            None => all,
            Some(seen) => all
                .into_iter()
                .filter(|department| seen.contains(&department.id))
                .collect(),
        })
    }

    /// One visible department with its members and role holders.
    #[instrument(skip_all)]
    pub async fn visible_department(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        department: DepartmentId,
    ) -> AppResult<DepartmentDetail> {
        let access = readable_company(self.companies.as_ref(), user_id, company_id).await?;
        let company = CompanyId::new(company_id);
        let visible = self
            .visible_set(&access, user_id)
            .await?
            .is_none_or(|seen| seen.contains(&department));
        let found = match visible {
            true => self.departments.get(company, department).await?,
            false => None,
        };
        let department = found.ok_or_else(department_not_found)?;
        use crate::application::ports::department::MAX_DEPARTMENT_ROSTER_ENTRIES;
        let mut members = self.departments.members(company, department.id).await?;
        let mut roles = self.departments.roles(company, department.id).await?;
        let members_truncated = members.len() > MAX_DEPARTMENT_ROSTER_ENTRIES;
        let roles_truncated = roles.len() > MAX_DEPARTMENT_ROSTER_ENTRIES;
        members.truncate(MAX_DEPARTMENT_ROSTER_ENTRIES);
        roles.truncate(MAX_DEPARTMENT_ROSTER_ENTRIES);
        Ok(DepartmentDetail {
            department,
            members,
            members_truncated,
            roles,
            roles_truncated,
        })
    }

    /// Each agent's departments and the roles it holds there, for anyone who may read the company.
    #[instrument(skip_all)]
    pub async fn agent_departments(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        agents: &[Uuid],
    ) -> AppResult<
        std::collections::HashMap<Uuid, (Vec<crate::entities::department::HeldDepartment>, bool)>,
    > {
        readable_company(self.companies.as_ref(), user_id, company_id).await?;
        self.departments
            .agent_departments(CompanyId::new(company_id), agents)
            .await
    }

    /// Which department owns each desk, as this viewer may see it: every department for an owner
    /// or admin; for anyone else the departments they may see ([`Self::visible_departments`]) and
    /// the owners of the desks they can read (`readable_desks`), so the department of a desk in
    /// front of them can be named.
    #[instrument(skip_all)]
    pub async fn desk_departments(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        readable_desks: &[Uuid],
    ) -> AppResult<DeskDepartments> {
        let access = readable_company(self.companies.as_ref(), user_id, company_id).await?;
        let company = CompanyId::new(company_id);
        let departments = self.departments.list_by_company(company).await?;
        let owners: std::collections::HashMap<Uuid, DepartmentId> = self
            .departments
            .desk_owners(company)
            .await?
            .into_iter()
            .collect();
        let Some(mut seen) = self.visible_set(&access, user_id).await? else {
            return Ok(DeskDepartments {
                departments,
                owners,
            });
        };
        let owners: std::collections::HashMap<Uuid, DepartmentId> = owners
            .into_iter()
            .filter(|(desk, _)| readable_desks.contains(desk))
            .collect();
        seen.extend(owners.values().copied());
        Ok(DeskDepartments {
            departments: departments
                .into_iter()
                .filter(|department| seen.contains(&department.id))
                .collect(),
            owners,
        })
    }

    /// [`Self::visible_department`] with the names of its members and role holders and the desks
    /// it owns.
    #[instrument(skip_all)]
    pub async fn department_view(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        department: DepartmentId,
    ) -> AppResult<DepartmentView> {
        let detail = self
            .visible_department(user_id, company_id, department)
            .await?;
        let company = CompanyId::new(company_id);
        let named: Vec<PrincipalId> = detail
            .members
            .iter()
            .map(|member| member.principal_id)
            .chain(detail.roles.iter().map(|holder| holder.principal_id))
            .collect();
        Ok(DepartmentView {
            principals: self.departments.principals_by_id(company, &named).await?,
            desks: self
                .departments
                .desks(company, detail.department.id)
                .await?,
            detail,
        })
    }

    /// The department's recent administration history (Plan06 02B.2), for its managers only:
    /// anyone else is told the department is not found.
    #[instrument(skip_all)]
    pub async fn administration_history(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        department: DepartmentId,
    ) -> AppResult<Vec<crate::entities::department::DepartmentAdministrationEntry>> {
        let scope = self.managed(user_id, company_id, Some(department)).await?;
        self.departments
            .administration_history(scope.company, department)
            .await
    }

    /// The people and agents a manager may add as members or make lead, agents first; at most
    /// [`MAX_DEPARTMENT_PRINCIPAL_CHOICES`], and whether the company has more.
    #[instrument(skip_all)]
    pub async fn principal_choices(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<(Vec<DepartmentPrincipal>, bool)> {
        let scope = self.administering(user_id, company_id).await?;
        let mut choices = self
            .departments
            .principals(scope.company, MAX_DEPARTMENT_PRINCIPAL_CHOICES + 1)
            .await?;
        let cut = choices.len() > MAX_DEPARTMENT_PRINCIPAL_CHOICES;
        choices.truncate(MAX_DEPARTMENT_PRINCIPAL_CHOICES);
        Ok((choices, cut))
    }
}

#[cfg(test)]
#[path = "department_tests.rs"]
mod tests;
