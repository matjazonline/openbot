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
