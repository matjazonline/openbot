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
            ArchivedUnits, Department, DepartmentDesk, DepartmentId, DepartmentMember,
            DepartmentPrincipal, DepartmentRole, DepartmentRoleHolder, HeldDepartmentRole,
        },
        transport::PrincipalId,
    },
};

/// Maximum members or held roles in a department detail response. Readers fetch one extra
/// row so the application can report truncation; this never bounds an authorization lookup.
pub const MAX_DEPARTMENT_ROSTER_ENTRIES: usize = 500;

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
    pub escalation: Option<crate::entities::workflow_escalation::EscalationPolicy>,
    /// Set by the use case; persistence stores the system when it is absent.
    pub created_by: Option<CreationProvenance>,
    /// `Some` creates a goal unit (Plan08 03A.4); `None` a standing one.
    pub goal: Option<GoalWrite>,
}

/// What a new goal unit sets besides a department's fields: its target date and sponsors. The
/// sponsors are assigned in the creating transaction; the first one decides.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GoalWrite {
    pub target_date: Option<chrono::NaiveDate>,
    pub sponsors: Vec<PrincipalId>,
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
    pub escalation: Option<Option<crate::entities::workflow_escalation::EscalationPolicy>>,
    /// Plan08 03A.3: a kind change rewrites no recorded goal.
    pub kind: Option<crate::entities::department::DepartmentKind>,
    pub target_date: Option<Option<chrono::NaiveDate>>,
}

/// What archiving does with runs on the subtree's desks that have not finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenWork {
    Refuse,
    /// Cancel them and archive in one write; the archiving manager may end runs on desks they
    /// cannot read (decision U3), and each cancellation records who did it.
    Cancel,
}

/// The actor explicitly chooses whether this sponsor decides.
#[derive(Debug, Clone, Copy)]
pub struct DepartmentRoleAssignment {
    pub role: DepartmentRole,
    pub principal: PrincipalId,
    pub decider: bool,
}

/// Removing a sponsor decider can name an existing sponsor as successor.
#[derive(Debug, Clone, Copy)]
pub struct DepartmentRoleRemoval {
    pub role: DepartmentRole,
    pub principal: PrincipalId,
    pub successor: Option<PrincipalId>,
}

/// One authenticated write against one company. Persistence re-checks the actor under the
/// company lock, so a demotion between the use case's check and the write cannot let it through.
#[derive(Debug, Clone, Copy)]
pub struct DepartmentScope {
    pub actor: WorkflowActor,
    pub company: CompanyId,
}

#[async_trait]
pub trait DepartmentPersistence:
    crate::application::ports::administration::AdministrationPersistence
{
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
    /// Archive the unit and its subtree (Plan08 02A.3). Refuses with
    /// `AppError::UnitArchiveOpenWork` while runs on its desks are unfinished, unless `open`
    /// asks to cancel them in the same write.
    async fn archive(
        &self,
        scope: DepartmentScope,
        department: DepartmentId,
        open: OpenWork,
    ) -> AppResult<Department>;
    /// Restore the unit alone; its parent must be active (Plan08 02A.4).
    async fn restore(
        &self,
        scope: DepartmentScope,
        department: DepartmentId,
    ) -> AppResult<Department>;
    async fn assign_role(
        &self,
        scope: DepartmentScope,
        department: DepartmentId,
        assignment: DepartmentRoleAssignment,
    ) -> AppResult<()>;
    async fn clear_role(
        &self,
        scope: DepartmentScope,
        department: DepartmentId,
        removal: DepartmentRoleRemoval,
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
    async fn list_by_company(
        &self,
        company: CompanyId,
        archived: ArchivedUnits,
    ) -> AppResult<Vec<Department>>;
    /// The departments where `user`'s person principal is a member or holds a role.
    async fn list_for_user(
        &self,
        company: CompanyId,
        user: Uuid,
        archived: ArchivedUnits,
    ) -> AppResult<Vec<Department>>;
    /// At most MAX_DEPARTMENT_ROSTER_ENTRIES + 1 rows, ordered for the roster.
    async fn members(
        &self,
        company: CompanyId,
        department: DepartmentId,
    ) -> AppResult<Vec<DepartmentMember>>;
    /// At most MAX_DEPARTMENT_ROSTER_ENTRIES + 1 rows; owner and lead precede other roles.
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
    /// Each agent's departments (member or role holder) by slug, at most 16, and whether it has
    /// more; archived ones only when asked.
    async fn agent_departments(
        &self,
        company: CompanyId,
        agents: &[Uuid],
        archived: ArchivedUnits,
    ) -> AppResult<
        std::collections::HashMap<Uuid, (Vec<crate::entities::department::HeldDepartment>, bool)>,
    >;
    /// Every department-owned desk of the company with its owner.
    async fn desk_owners(&self, company: CompanyId) -> AppResult<Vec<(Uuid, DepartmentId)>>;
    /// The department's administration history, newest first, at most
    /// [`DEPARTMENT_HISTORY_ENTRIES`](crate::entities::department::DEPARTMENT_HISTORY_ENTRIES).
    async fn administration_history(
        &self,
        company: CompanyId,
        department: DepartmentId,
    ) -> AppResult<Vec<crate::entities::department::DepartmentAdministrationEntry>>;
    /// The desks this department owns, by address.
    async fn desks(
        &self,
        company: CompanyId,
        department: DepartmentId,
    ) -> AppResult<Vec<DepartmentDesk>>;
}
