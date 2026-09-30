use super::*;
use crate::application::app_error::{AppError, AppResult};
use crate::application::workflow::authorization::WorkflowActor;
use crate::application::workflow::binding::{ResourceDirectory, ResourceReadiness};
use crate::application::workflow::publication::{PublishedBundle, ToolSnapshot};
use crate::domain::workflow::{ResourceName, RuntimeResourceId, TypeName};
use async_trait::async_trait;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// Loaded from the authoritative saved run, never reconstructed from the action caller.
pub struct ActionRunAuthority {
    pub action: FrozenAction,
    pub actor: WorkflowActor,
    pub bundle: Arc<PublishedBundle>,
    pub resources: BTreeMap<ResourceName, RuntimeResourceId>,
}

#[async_trait]
pub trait ActionAuthorities: Send + Sync {
    /// Load the scoped immutable subject and run snapshot, rechecking current company access.
    /// This read is not dispatch permission. The dispatch writer must repeat authorization
    /// while holding its ownership/revocation locks, immediately before recording dispatch.
    async fn action_authority(
        &self,
        scope: ActionScope,
        subject: &ApprovalSubject,
    ) -> AppResult<ActionRunAuthority>;
}

pub struct CurrentActionPolicy {
    pub tool: ToolSnapshot,
    pub approval_required: bool,
}

/// Trusted operator policy, not provider discovery metadata. No credentials or fallback grant.
#[async_trait]
pub trait ActionPolicyDirectory: Send + Sync {
    async fn action_policy(
        &self,
        company: crate::domain::workflow::CompanyId,
        actor: WorkflowActor,
        target: &ActionTarget,
        tool: &TypeName,
    ) -> AppResult<Option<CurrentActionPolicy>>;
}

/// Current observation only: it cannot authorize a later provider call or settle approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionAccessDecision {
    CurrentAccessConfirmed,
    ApprovalRequired,
}

impl<P: ActionAuthorities> ActionService<P> {
    pub async fn authorize(
        &self,
        scope: ActionScope,
        subject: &ApprovalSubject,
        resources: &impl ResourceDirectory,
        policies: &impl ActionPolicyDirectory,
    ) -> AppResult<ActionAccessDecision> {
        let authority = self.persistence.action_authority(scope, subject).await?;
        validate_run_authority(scope, subject, &authority)?;
        let request = &authority.action.request;
        let (resource, _) = target(&request.target);
        let current = tokio::time::timeout(
            Duration::from_secs(5),
            resources.inspect(
                scope.company,
                authority.actor,
                RuntimeResourceId::new(resource),
            ),
        )
        .await
        .map_err(|_| AppError::Timeout("Workflow action resource lookup".into()))??
        .ok_or_else(denied)?;
        validate_resource(request, &current)?;
        let current = tokio::time::timeout(
            Duration::from_secs(5),
            policies.action_policy(
                scope.company,
                authority.actor,
                &request.target,
                &request.contract.contract.name,
            ),
        )
        .await
        .map_err(|_| AppError::Timeout("Workflow action policy lookup".into()))??
        .ok_or_else(denied)?;
        validate_current_authority(request, &current)
    }
}

pub(crate) fn validate_resource(
    request: &ActionRequest,
    current: &crate::application::workflow::binding::ResourceStatus,
) -> AppResult<()> {
    let (resource, kind) = target(&request.target);
    if current.id.as_uuid() != resource
        || current.company_id != request.scope.company
        || current.kind != *kind
        || !current.authorized
        || current.readiness != ResourceReadiness::Ready
        || current.supported_contracts.len() > 256
        || !current
            .supported_contracts
            .contains(&request.contract.contract.name)
    {
        return Err(denied());
    }
    Ok(())
}

pub(crate) fn validate_current_authority(
    request: &ActionRequest,
    current: &CurrentActionPolicy,
) -> AppResult<ActionAccessDecision> {
    validate_current_policy(request, current)?;
    Ok(
        if request.context.approval_required || current.approval_required {
            ActionAccessDecision::ApprovalRequired
        } else {
            ActionAccessDecision::CurrentAccessConfirmed
        },
    )
}

pub(crate) fn validate_run_authority(
    scope: ActionScope,
    subject: &ApprovalSubject,
    authority: &ActionRunAuthority,
) -> AppResult<()> {
    let request = &authority.action.request;
    if request.scope != scope
        || subject.invocation.as_uuid().is_nil()
        || authority.action.argument_digest() != &subject.argument_digest
        || authority.bundle.company_id() != scope.company
        || request.context.actor != authority.actor.user_id()
        || authority.resources.len() > 256
    {
        return Err(denied());
    }
    let tools = &authority.bundle.snapshots().tools;
    let ceiling: BTreeSet<_> = tools.iter().map(|tool| &tool.policy.capability).collect();
    if !request
        .context
        .capability_ceiling
        .contains(&request.contract.policy.capability)
        || request
            .context
            .capability_ceiling
            .iter()
            .any(|capability| !ceiling.contains(capability))
    {
        return Err(denied());
    }
    let mut selected = false;
    for tool in tools {
        selected |= same_tool(tool, &request.contract)?;
    }
    if !selected {
        return Err(denied());
    }
    match (&request.target, &request.contract.contract.connection) {
        (ActionTarget::Connection { resource, .. }, Some(slot))
            if authority
                .resources
                .get(slot)
                .is_some_and(|id| id.as_uuid() == *resource) => {}
        (ActionTarget::Local { .. }, None) => {}
        _ => return Err(denied()),
    }
    Ok(())
}

fn validate_current_policy(
    request: &ActionRequest,
    current: &CurrentActionPolicy,
) -> AppResult<()> {
    // Reuse the bounded canonical/schema boundary for untrusted persisted policy facts.
    let mut checked = request.clone();
    checked.contract = current.tool.clone();
    let checked = super::freeze::freeze(checked)?;
    if !same_tool(&request.contract, &checked.request.contract)? {
        return Err(denied());
    }
    Ok(())
}

fn same_tool(left: &ToolSnapshot, right: &ToolSnapshot) -> AppResult<bool> {
    let left = serde_json::to_value(left).map_err(|_| contracts::invalid())?;
    let right = serde_json::to_value(right).map_err(|_| contracts::invalid())?;
    Ok(left == right)
}

fn target(target: &ActionTarget) -> (Uuid, &TypeName) {
    match target {
        ActionTarget::Connection {
            resource,
            resource_kind,
        }
        | ActionTarget::Local {
            resource,
            resource_kind,
        } => (*resource, resource_kind),
    }
}

fn denied() -> AppError {
    AppError::NotFound("Workflow action authority".into())
}
