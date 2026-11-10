//! Department persistence contract (01-departments). Every write is one transaction that takes
//! the company write lock and re-authorizes the actor before it touches a department.
use async_trait::async_trait;
use uuid::Uuid;

use crate::{
    app_error::AppResult,
    application::workflow::WorkflowActor,
    domain::workflow::CompanyId,
    entities::{
        creation::CreationProvenance,
        department::{
            Department, DepartmentDesk, DepartmentId, DepartmentMember, DepartmentPrincipal,
            DepartmentRole, DepartmentRoleHolder, HeldDepartmentRole,
        },
        transport::PrincipalId,
    },
};

/// Everything one department create or edit sets. Values reach persistence already normalized —
/// see `DepartmentWrite::normalize`.
#[derive(Debug, Clone, Default)]
pub struct DepartmentWrite {
    pub name: String,
    pub slug: String,
    /// `None` and a blank string mean the same thing; normalization stores `None`.
    pub description: Option<String>,
    pub charter: Option<String>,
    pub parent_id: Option<DepartmentId>,
    /// Set by the use case; persistence stores the system when it is absent.
    pub created_by: Option<CreationProvenance>,
}

/// One department edit: only the fields it sets change. A tri-state field is `None` to leave it,
/// `Some(None)` to clear it (for `parent_id`, to make the department a root) and `Some(Some(_))`
/// to set it. Persistence merges the edit onto the row under its lock, so two edits of different
/// fields cannot lose either. Values arrive normalized — see `DepartmentEdit::normalize`.
#[derive(Debug, Clone, Default)]
pub struct DepartmentEdit {
    pub name: Option<String>,
    pub slug: Option<String>,
    pub description: Option<Option<String>>,
    pub charter: Option<Option<String>>,
    pub parent_id: Option<Option<DepartmentId>>,
}

/// One authenticated write against one company. Persistence re-checks the actor under the
/// company lock, so a demotion between the use case's check and the write cannot let it through.
#[derive(Debug, Clone, Copy)]
pub struct DepartmentScope {
    pub actor: WorkflowActor,
    pub company: CompanyId,
}

#[async_trait]
pub trait DepartmentPersistence: Send + Sync {
    async fn create(
        &self,
        scope: DepartmentScope,
        write: DepartmentWrite,
        lead: Option<PrincipalId>,
    ) -> AppResult<Department>;
    async fn update(
        &self,
        scope: DepartmentScope,
        department: DepartmentId,
        edit: DepartmentEdit,
    ) -> AppResult<Department>;
    async fn delete(&self, scope: DepartmentScope, department: DepartmentId) -> AppResult<()>;
    async fn assign_role(
        &self,
        scope: DepartmentScope,
        department: DepartmentId,
        role: DepartmentRole,
        principal: PrincipalId,
    ) -> AppResult<()>;
    async fn clear_role(
        &self,
        scope: DepartmentScope,
        department: DepartmentId,
        role: DepartmentRole,
        principal: PrincipalId,
    ) -> AppResult<()>;
    async fn add_member(
        &self,
        scope: DepartmentScope,
        department: DepartmentId,
        principal: PrincipalId,
    ) -> AppResult<()>;
    async fn remove_member(
        &self,
        scope: DepartmentScope,
        department: DepartmentId,
        principal: PrincipalId,
    ) -> AppResult<()>;

    async fn get(
        &self,
        company: CompanyId,
        department: DepartmentId,
    ) -> AppResult<Option<Department>>;
    async fn list_by_company(&self, company: CompanyId) -> AppResult<Vec<Department>>;
    /// The departments where `user`'s person principal is a member or holds a role.
    async fn list_for_user(&self, company: CompanyId, user: Uuid) -> AppResult<Vec<Department>>;
    async fn members(
        &self,
        company: CompanyId,
        department: DepartmentId,
    ) -> AppResult<Vec<DepartmentMember>>;
    async fn roles(
        &self,
        company: CompanyId,
        department: DepartmentId,
    ) -> AppResult<Vec<DepartmentRoleHolder>>;
    async fn memberships(
        &self,
        company: CompanyId,
        principal: PrincipalId,
    ) -> AppResult<Vec<DepartmentId>>;
    async fn roles_held(
        &self,
        company: CompanyId,
        principal: PrincipalId,
    ) -> AppResult<Vec<HeldDepartmentRole>>;
    /// The company's people and agents, agents first, each by name; at most `limit`.
    async fn principals(
        &self,
        company: CompanyId,
        limit: usize,
    ) -> AppResult<Vec<DepartmentPrincipal>>;
    /// These principals of the company, by name; an id not in the company is left out.
    async fn principals_by_id(
        &self,
        company: CompanyId,
        ids: &[PrincipalId],
    ) -> AppResult<Vec<DepartmentPrincipal>>;
    /// The desks this department owns, by address.
    async fn desks(
        &self,
        company: CompanyId,
        department: DepartmentId,
    ) -> AppResult<Vec<DepartmentDesk>>;
}
