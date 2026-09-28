use super::*;
use crate::domain::workflow::{ResourceRequirement, TypeName};
use async_trait::async_trait;
use std::collections::BTreeSet;

/// Provider readiness derived from current state, separate from authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceReadiness {
    Ready,
    Unavailable,
    Revoked,
}

/// Bounded non-secret facts returned by a trusted adapter. Supported contracts
/// represent provider capabilities, not arbitrary user labels. Adapters must verify
/// them against current resource configuration and provider support.
#[derive(Debug, Clone)]
pub struct ResourceStatus {
    pub id: RuntimeResourceId,
    pub company_id: CompanyId,
    pub kind: TypeName,
    pub supported_contracts: BTreeSet<TypeName>,
    pub authorized: bool,
    pub readiness: ResourceReadiness,
}

/// Required current lookup: enforce bounded I/O and return no credentials. Missing
/// resources return None, directory outages return Err, never a readiness default.
/// Activation observations must not be cached as execution grants. At every effect
/// dispatch the action/provider adapter rechecks current access and revocation for
/// the frozen resource ID and resolves its current secrets internally. Secrets must
/// never enter this port, run snapshots, bundles, prompts or traces. Credential
/// rotation changes the next use, not the admitted resource identity or contract.
#[async_trait]
pub trait ResourceDirectory: Send + Sync {
    async fn inspect(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        id: RuntimeResourceId,
    ) -> AppResult<Option<ResourceStatus>>;
}

pub(crate) fn validate(
    company: CompanyId,
    id: RuntimeResourceId,
    requirement: &ResourceRequirement,
    status: &ResourceStatus,
) -> AppResult<()> {
    if status.id != id
        || status.company_id != company
        || !status.authorized
        || status.readiness == ResourceReadiness::Revoked
    {
        return Err(AppError::NotFound("Workflow resource".into()));
    }
    if status.supported_contracts.len() > 256 {
        return Err(AppError::BadRequest(format!(
            "Resource slot {} exceeds 256 supported contracts",
            requirement.slot
        )));
    }
    if status.kind != requirement.kind
        || requirement
            .contract
            .as_ref()
            .is_some_and(|contract| !status.supported_contracts.contains(contract))
    {
        return Err(AppError::BadRequest(format!(
            "Resource slot {} requires kind {} and its declared provider contract",
            requirement.slot, requirement.kind
        )));
    }
    if status.readiness != ResourceReadiness::Ready {
        return Err(AppError::Conflict(format!(
            "Resource slot {} is not ready",
            requirement.slot
        )));
    }
    Ok(())
}
