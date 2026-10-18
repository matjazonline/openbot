//! Workflow use of company MCP connections. The company MCP catalog (`persistence::mcp`)
//! owns connections, credentials, reviewed discovery and grants; this module owns trusted
//! workflow approvals (`workflow_mcp_operations`) and every current-authority read of them.
//! Lock order: company (authorize_company) → run/attempt where applicable → connection
//! FOR SHARE/UPDATE → operation. Catalog writers lock the company first and never a run.
use super::http::{begin_http, live_run_on};
use super::invalid;
use super::*;
use crate::adapters::persistence::credentials::envelope::CredentialContext;
use crate::application::workflow::{
    actions::*,
    binding::{ResourceReadiness, ResourceStatus},
    mcp::*,
};
use crate::entities::mcp::{CompanyMcpConnection, McpAuth};
use serde_json::Value;
use std::collections::BTreeSet;
#[path = "mcp_publication.rs"]
mod publication_owner;
pub use publication_owner::{McpPublicationDirectory, McpPublicationSelection};
#[path = "mcp_runtime.rs"]
mod runtime;

/// A current (not deleted) company MCP connection, row-locked FOR SHARE so catalog writers
/// cannot change it before this transaction ends.
pub(super) async fn connection_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    id: Uuid,
) -> AppResult<Option<CompanyMcpConnection>> {
    let locked: Option<Uuid> = sqlx::query_scalar("SELECT id FROM company_mcp_connections WHERE company_id=$1 AND id=$2 AND deleted_at IS NULL FOR SHARE")
        .bind(company.as_uuid()).bind(id).fetch_optional(&mut **tx).await?;
    if locked.is_none() {
        return Ok(None);
    }
    crate::adapters::persistence::mcp::connection_on(tx, company.as_uuid(), id).await
}

pub(super) async fn status_on(
    tx: &mut Transaction<'_, Postgres>,
    connection: &CompanyMcpConnection,
) -> AppResult<ResourceStatus> {
    let names: Vec<String> = sqlx::query_scalar("SELECT name FROM workflow_mcp_operations WHERE company_id=$1 AND connection_id=$2 ORDER BY name LIMIT 257")
        .bind(connection.company_id).bind(connection.id).fetch_all(&mut **tx).await?;
    if names.len() > MAX_MCP_OPERATIONS {
        return Err(invalid());
    }
    let supported_contracts = names
        .into_iter()
        .map(|name| TypeName::parse(name).map_err(|_| invalid()))
        .collect::<AppResult<BTreeSet<_>>>()?;
    Ok(ResourceStatus {
        id: RuntimeResourceId::new(connection.id),
        company_id: CompanyId::new(connection.company_id),
        kind: TypeName::parse("mcp").map_err(|_| invalid())?,
        authorized: true,
        readiness: if !connection.enabled || connection.tool_grants.is_empty() {
            ResourceReadiness::Revoked
        } else if connection.auth == McpAuth::Bearer && !connection.secret_set {
            ResourceReadiness::Unavailable
        } else {
            ResourceReadiness::Ready
        },
        supported_contracts,
    })
}

/// The stored approval re-validated against the connection's current grant and reviewed
/// discovery. Drift or a removed grant makes the approval unusable (missing).
fn approved(
    connection: &CompanyMcpConnection,
    tool: &publication::ToolSnapshot,
    approval_required: bool,
) -> AppResult<()> {
    let name = tool.contract.name.as_str();
    McpOperation {
        tool: tool.clone(),
        approval_required,
    }
    .validate(
        CompanyId::new(connection.company_id),
        connection
            .discovered_tools
            .iter()
            .find(|reviewed| reviewed.name.as_str() == name),
        connection
            .tool_grants
            .iter()
            .any(|grant| grant.as_str() == name),
    )
    .map_err(|_| missing())
}

/// Current approval for `wanted` on exactly this connection; only the slot may differ.
pub(super) async fn policy_on(
    tx: &mut Transaction<'_, Postgres>,
    connection: &CompanyMcpConnection,
    wanted: &publication::ToolSnapshot,
) -> AppResult<CurrentActionPolicy> {
    let (snapshot, approval_required): (Value, bool) = sqlx::query_as("SELECT snapshot,approval_required FROM workflow_mcp_operations WHERE company_id=$1 AND connection_id=$2 AND name=$3 FOR SHARE")
        .bind(connection.company_id).bind(connection.id).bind(wanted.contract.name.as_str()).fetch_optional(&mut **tx).await?.ok_or_else(missing)?;
    let mut tool: publication::ToolSnapshot =
        serde_json::from_value(snapshot).map_err(|_| invalid())?;
    approved(connection, &tool, approval_required)?;
    tool.contract.connection = wanted.contract.connection.clone();
    if serde_json::to_value(&tool).map_err(|_| invalid())?
        != serde_json::to_value(wanted).map_err(|_| invalid())?
    {
        return Err(missing());
    }
    Ok(CurrentActionPolicy {
        tool,
        approval_required,
    })
}

/// Publication: some ready company connection currently approves exactly this tool.
/// Binding and admission later check the exact bound connection with `policy_on`.
pub(super) async fn registered_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    wanted: &publication::ToolSnapshot,
) -> AppResult<Uuid> {
    let mut normalized = wanted.clone();
    normalized.contract.connection = None;
    let snapshot = serde_json::to_value(&normalized).map_err(|_| invalid())?;
    let ids: Vec<Uuid> = sqlx::query_scalar("SELECT operation.connection_id FROM workflow_mcp_operations AS operation JOIN company_mcp_connections AS connection ON connection.company_id=operation.company_id AND connection.id=operation.connection_id WHERE operation.company_id=$1 AND operation.name=$2 AND operation.snapshot=$3 AND connection.deleted_at IS NULL AND connection.enabled ORDER BY operation.connection_id LIMIT 65")
        .bind(company.as_uuid()).bind(wanted.contract.name.as_str()).bind(snapshot).fetch_all(&mut **tx).await?;
    for id in ids {
        let Some(connection) = connection_on(tx, company, id).await? else {
            continue;
        };
        if status_on(tx, &connection).await?.readiness == ResourceReadiness::Ready {
            policy_on(tx, &connection, &normalized).await?;
            return Ok(id);
        }
    }
    Err(missing())
}

pub(super) fn mcp_target(target: &ActionTarget) -> AppResult<Uuid> {
    match target {
        ActionTarget::Connection {
            resource,
            resource_kind,
        } if resource_kind.as_str() == "mcp" => Ok(*resource),
        _ => Err(missing()),
    }
}

/// The same readiness and approval checks reservation enforces, for preparation and use.
pub(super) async fn current_on(
    tx: &mut Transaction<'_, Postgres>,
    request: &ActionRequest,
) -> AppResult<(CompanyMcpConnection, ActionAccessDecision)> {
    let connection = connection_on(tx, request.scope.company, mcp_target(&request.target)?)
        .await?
        .ok_or_else(missing)?;
    validate_resource(request, &status_on(tx, &connection).await?)?;
    let decision = validate_current_authority(
        request,
        &policy_on(tx, &connection, &request.contract).await?,
    )?;
    Ok((connection, decision))
}

pub struct McpActionAuthority;
#[async_trait]
impl action_dispatch::SqlActionAuthority for McpActionAuthority {
    async fn lock_current(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        authority: &ActionRunAuthority,
    ) -> AppResult<action_dispatch::LockedActionAuthority> {
        let request = authority.action.request();
        let connection = connection_on(tx, request.scope.company, mcp_target(&request.target)?)
            .await?
            .ok_or_else(missing)?;
        Ok(action_dispatch::LockedActionAuthority {
            resource: status_on(tx, &connection).await?,
            policy: policy_on(tx, &connection, &request.contract).await?,
        })
    }
}

/// Registration and revocation lock the company (authorize_company), then the connection.
async fn edit_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    actor: WorkflowActor,
    id: Uuid,
    catalog_revision: i64,
) -> AppResult<CompanyMcpConnection> {
    authority::authorize_company(tx, company, actor).await?;
    sqlx::query("SELECT id FROM company_mcp_connections WHERE company_id=$1 AND id=$2 AND deleted_at IS NULL FOR UPDATE")
        .bind(company.as_uuid()).bind(id).fetch_optional(&mut **tx).await?.ok_or_else(missing)?;
    let connection = crate::adapters::persistence::mcp::connection_on(tx, company.as_uuid(), id)
        .await?
        .ok_or_else(missing)?;
    if connection.revision != catalog_revision {
        return Err(conflict());
    }
    Ok(connection)
}
async fn current_version(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    connection: Uuid,
    name: &str,
) -> AppResult<Option<Uuid>> {
    Ok(sqlx::query_scalar("SELECT version FROM workflow_mcp_operations WHERE company_id=$1 AND connection_id=$2 AND name=$3 FOR UPDATE")
        .bind(company.as_uuid()).bind(connection).bind(name).fetch_optional(&mut **tx).await?)
}

#[async_trait]
impl McpOperations for PostgresPersistence {
    async fn register_mcp_operation(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        write: McpOperationWrite,
    ) -> AppResult<Uuid> {
        let tool = &write.operation.tool;
        let name = tool.contract.name.as_str();
        let snapshot =
            serde_json::to_value(tool).map_err(|_| crate::application::workflow::mcp::invalid())?;
        let mut tx = begin_http(self).await?;
        let connection = edit_on(
            &mut tx,
            company,
            actor,
            write.connection,
            write.catalog_revision,
        )
        .await?;
        write.operation.validate(
            company,
            connection
                .discovered_tools
                .iter()
                .find(|reviewed| reviewed.name.as_str() == name),
            connection
                .tool_grants
                .iter()
                .any(|grant| grant.as_str() == name),
        )?;
        let others: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_mcp_operations WHERE company_id=$1 AND connection_id=$2 AND name<>$3")
            .bind(company.as_uuid()).bind(connection.id).bind(name).fetch_one(&mut *tx).await?;
        if others >= MAX_MCP_OPERATIONS as i64 {
            return Err(crate::application::workflow::mcp::invalid());
        }
        let version = Uuid::new_v4();
        let current = current_version(&mut tx, company, connection.id, name).await?;
        let written = match (current, write.expected_version) {
            (None, None) => sqlx::query("INSERT INTO workflow_mcp_operations(company_id,connection_id,name,snapshot,approval_required,version) VALUES($1,$2,$3,$4,$5,$6)")
                .bind(company.as_uuid()).bind(connection.id).bind(name).bind(snapshot).bind(write.operation.approval_required).bind(version).execute(&mut *tx).await?,
            (Some(current), Some(expected)) if current == expected => sqlx::query("UPDATE workflow_mcp_operations SET snapshot=$4,approval_required=$5,version=$6 WHERE company_id=$1 AND connection_id=$2 AND name=$3 AND version=$7")
                .bind(company.as_uuid()).bind(connection.id).bind(name).bind(snapshot).bind(write.operation.approval_required).bind(version).bind(expected).execute(&mut *tx).await?,
            _ => return Err(conflict()),
        };
        if written.rows_affected() != 1 {
            return Err(conflict());
        }
        tx.commit().await?;
        Ok(version)
    }
    async fn revoke_mcp_operation(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        revoke: McpOperationRevoke,
    ) -> AppResult<()> {
        let mut tx = begin_http(self).await?;
        edit_on(
            &mut tx,
            company,
            actor,
            revoke.connection,
            revoke.catalog_revision,
        )
        .await?;
        let deleted = sqlx::query("DELETE FROM workflow_mcp_operations WHERE company_id=$1 AND connection_id=$2 AND name=$3 AND version=$4")
            .bind(company.as_uuid()).bind(revoke.connection).bind(revoke.name.as_str()).bind(revoke.version).execute(&mut *tx).await?;
        if deleted.rows_affected() != 1 {
            return Err(conflict());
        }
        tx.commit().await?;
        Ok(())
    }
}

#[async_trait]
impl McpActionUse for PostgresPersistence {
    /// Its own short transaction: current company authority of the frozen actor, live run,
    /// connection readiness and approval, then the tenant-and-connection-bound credential.
    async fn mcp_use(&self, request: &ActionRequest) -> AppResult<McpUse> {
        let company = request.scope.company;
        let mut tx = begin_http(self).await?;
        authority::authorize_company(
            &mut tx,
            company,
            WorkflowActor::authenticated(request.context.actor)?,
        )
        .await?;
        live_run_on(&mut tx, request).await?;
        let (connection, decision) = current_on(&mut tx, request).await?;
        let envelope: Option<String> = sqlx::query_scalar(
            "SELECT envelope FROM company_mcp_credentials WHERE company_id=$1 AND connection_id=$2",
        )
        .bind(company.as_uuid())
        .bind(connection.id)
        .fetch_optional(&mut *tx)
        .await?;
        let token = envelope
            .map(|envelope| {
                self.credential_cipher()?.open_envelope(
                    &CredentialContext::company_mcp_credential(company.as_uuid(), connection.id),
                    &envelope,
                )
            })
            .transpose()?;
        if (connection.auth == McpAuth::Bearer) != token.is_some() {
            return Err(missing());
        }
        let approved = decisions::approvals::approved_request(&mut tx, request).await?;
        tx.commit().await?;
        Ok(McpUse {
            endpoint: connection.endpoint,
            token,
            credential_revision: connection.credential_revision,
            approval_required: decision == ActionAccessDecision::ApprovalRequired && !approved,
        })
    }
}

#[async_trait]
impl ActionEffectStates for PostgresPersistence {
    async fn effect_state(
        &self,
        scope: ActionScope,
        subject: &ApprovalSubject,
    ) -> AppResult<ActionEffectState> {
        let state: Option<String> = sqlx::query_scalar("SELECT effect_state FROM workflow_action_effect_states WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND invocation_id=$4 AND argument_digest=$5")
            .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
            .bind(subject.invocation.as_uuid()).bind(subject.argument_digest.as_str()).fetch_optional(self.pool()).await?;
        match state.as_deref() {
            Some("prepared") => Ok(ActionEffectState::Prepared),
            Some("possible_dispatch") => Ok(ActionEffectState::PossibleDispatch),
            Some("needs_reconciliation") => Ok(ActionEffectState::NeedsReconciliation),
            Some("not_applied") => Ok(ActionEffectState::NotApplied),
            Some("applied_without_result") => Ok(ActionEffectState::AppliedWithoutResult),
            Some("committed") => Ok(ActionEffectState::Committed),
            Some(_) => Err(invalid()),
            None => Err(missing()),
        }
    }
}
