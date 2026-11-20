//! Current person authority over organization-owned configuration.
use async_trait::async_trait;
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    domain::workflow::{CompanyId, WorkflowId},
    entities::agent_capability::AgentId,
    entities::department::DepartmentId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagementTarget {
    /// Collection entry only; each returned object still requires its own target check.
    AnyUnit,
    Root,
    Unit(DepartmentId),
    Agent(AgentId),
    DefinitionCreation(WorkflowId),
    Definition(WorkflowId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagementRole {
    Owner,
    Admin,
    Lead(DepartmentId),
}

impl std::fmt::Display for ManagementRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Owner => f.write_str("owner"),
            Self::Admin => f.write_str("admin"),
            Self::Lead(unit) => write!(f, "lead:{unit}"),
        }
    }
}

/// A unit this person manages, named for a unit picker (Plan07).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedUnit {
    pub id: DepartmentId,
    pub name: String,
    pub is_root: bool,
}

/// Configuration authority is independent of desk and conversation visibility.
/// Every adapter must state its current-person lookup; errors are never refusals.
#[async_trait]
pub trait AdministrationPersistence: Send + Sync {
    async fn management_role(
        &self,
        actor: Uuid,
        company: CompanyId,
        target: ManagementTarget,
    ) -> AppResult<Option<ManagementRole>>;

    /// Every unit the person currently manages (the whole company for a root manager), to scope
    /// configuration lists. Listing is not authority: each write still checks its own target.
    async fn managed_units(&self, actor: Uuid, company: CompanyId) -> AppResult<Vec<DepartmentId>>;

    /// [`Self::managed_units`] with names, root first, for a unit picker.
    async fn managed_unit_choices(
        &self,
        actor: Uuid,
        company: CompanyId,
    ) -> AppResult<Vec<ManagedUnit>>;

    /// The unit a new skill or connection gets when its creator names none (Plan07): the root for
    /// a root manager, else the one department they lead, else none.
    async fn default_resource_unit(
        &self,
        actor: Uuid,
        company: CompanyId,
    ) -> AppResult<Option<DepartmentId>>;
}

pub async fn manages_unit(
    persistence: &dyn AdministrationPersistence,
    actor: Uuid,
    company: CompanyId,
    unit: DepartmentId,
) -> AppResult<bool> {
    Ok(persistence
        .management_role(actor, company, ManagementTarget::Unit(unit))
        .await?
        .is_some())
}

/// The unit a new skill or connection is created in (Plan07): the one named, else the creator's
/// default, and the creator must manage it. Someone who manages no unit gets the usual 404.
pub async fn resource_creation_unit(
    persistence: &dyn AdministrationPersistence,
    actor: Uuid,
    company: CompanyId,
    requested: Option<DepartmentId>,
) -> AppResult<DepartmentId> {
    require_management(persistence, actor, company, ManagementTarget::AnyUnit).await?;
    let unit = match requested {
        Some(unit) => unit,
        None => persistence
            .default_resource_unit(actor, company)
            .await?
            .ok_or_else(|| AppError::BadRequest("Choose a unit for this resource.".into()))?,
    };
    require_management(persistence, actor, company, ManagementTarget::Unit(unit)).await?;
    Ok(unit)
}

pub async fn require_management(
    persistence: &dyn AdministrationPersistence,
    actor: Uuid,
    company: CompanyId,
    target: ManagementTarget,
) -> AppResult<ManagementRole> {
    persistence
        .management_role(actor, company, target)
        .await?
        .ok_or_else(|| AppError::NotFound("Managed unit not found".into()))
}
