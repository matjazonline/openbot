//! Host-only action/fence knowledge stays here. Gateway DTOs contain none of it.
use super::action_dispatch::{
    LockedActionAuthority, PostgresActionDispatch, SqlActionAuthority, SqlLocalAction,
};
use super::*;
use crate::application::workflow::actions::*;
use crate::{
    adapters::{
        integrations::owner::{client_namespace, company_binding},
        persistence::{integration_bindings::binding, integrations::PostgresVault},
    },
    application::{
        agent::ToolCallOrigin,
        integrations::{execution::CredentialDispatchGate, *},
        use_cases::integration_binding::BindingKind,
    },
    domain::integrations::{self as integration, authority::*},
    entities::mcp::{CompanyMcpConnection, McpAuth},
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

pub(super) async fn http_snapshot(
    db: &mut PgConnection,
    connection: &crate::application::workflow::http::HttpConnection,
) -> AppResult<Option<AuthorizationSnapshot>> {
    let CredentialSource::Integration(reference) = &connection.credential_source else {
        return Ok(None);
    };
    snapshot(
        db,
        ConnectionIdentity {
            company: connection.company.as_uuid(),
            id: connection.id,
            kind: BindingKind::Http,
            reference,
            revision: connection.revision,
            catalog: connection.revision,
            destination: connection.origin.as_str(),
        },
    )
    .await
    .map(Some)
}
pub(super) async fn mcp_snapshot(
    db: &mut PgConnection,
    connection: &CompanyMcpConnection,
) -> AppResult<Option<AuthorizationSnapshot>> {
    let McpAuth::Integration(reference) = &connection.auth else {
        return Ok(None);
    };
    snapshot(
        db,
        ConnectionIdentity {
            company: connection.company_id,
            id: connection.id,
            kind: BindingKind::Mcp,
            reference,
            revision: connection.revision,
            catalog: connection.revision,
            destination: connection.endpoint.as_str(),
        },
    )
    .await
    .map(Some)
}
struct ConnectionIdentity<'a> {
    company: Uuid,
    id: Uuid,
    kind: BindingKind,
    reference: &'a IntegrationReference,
    revision: i64,
    catalog: i64,
    destination: &'a str,
}
async fn snapshot(
    db: &mut PgConnection,
    c: ConnectionIdentity<'_>,
) -> AppResult<AuthorizationSnapshot> {
    let grant = PostgresVault::grant_metadata_on(db, c.reference.grant).await?;
    let destination = ApprovedDestination::parse(c.destination)?;
    if !grant.enabled
        || grant.client != client_namespace()
        || grant.account != c.reference.account
        || grant.account_authorization != c.reference.authorization
        || grant.revision != c.reference.grant_revision
        || grant.binding != binding(c.company, c.id, c.kind)
        || grant.host_scope != company_binding(c.company)
        || grant.destination != destination
        || grant.purpose != c.kind.purpose()
    {
        return Err(missing());
    }
    Ok(AuthorizationSnapshot {
        account: grant.account,
        grant: grant.id,
        client: grant.client,
        binding: grant.binding,
        app: grant.app,
        app_policy: grant.app_policy,
        destination,
        account_authorization: grant.account_authorization,
        grant_revision: grant.revision,
        connection_revision: integration::ConnectionRevision::new(c.revision)?,
        catalog_revision: integration::CatalogRevision::new(c.catalog)?,
    })
}
pub(super) fn check_snapshot(
    request: &ActionRequest,
    current: Option<AuthorizationSnapshot>,
) -> AppResult<()> {
    if request.context.integration != current
        || current.is_some() && request.context.origin != ToolCallOrigin::Workflow
    {
        return Err(missing());
    }
    Ok(())
}
struct IntegrationResource(BindingKind);
#[async_trait]
impl SqlActionAuthority for IntegrationResource {
    async fn lock_current(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        authority: &ActionRunAuthority,
    ) -> AppResult<LockedActionAuthority> {
        match self.0 {
            BindingKind::Http => http::HttpActionAuthority.lock_current(tx, authority).await,
            BindingKind::Mcp => mcp::McpActionAuthority.lock_current(tx, authority).await,
        }
    }
}
struct NoLocal;
#[async_trait]
impl SqlLocalAction for NoLocal {
    async fn apply(
        &self,
        _: &mut Transaction<'_, Postgres>,
        _: &FrozenAction,
        _: &ApprovalSubject,
    ) -> AppResult<Value> {
        Err(missing())
    }
}
/// Runtime capability is bound to one exact request and rechecks persistence on
/// every use; neither the opaque UUID nor the captured observation grants access.
struct HostDispatchAuthority {
    persistence: PostgresPersistence,
    request: ActionDispatchRequest,
    expected: AuthorizedInvocation,
    owner: Uuid,
    kind: BindingKind,
}
impl HostDispatchAuthority {
    async fn current(&self, tx: &mut Transaction<'_, Postgres>) -> AppResult<FrozenAction> {
        let actor = action_authority::prelock_accounts_on(
            tx,
            self.request.scope(),
            Some(WorkflowActor::authenticated(self.owner)?),
        )
        .await?;
        let dispatch = PostgresActionDispatch::new(
            self.persistence.clone(),
            IntegrationResource(self.kind),
            NoLocal,
        );
        let authorized = dispatch
            .authorize_prelocked_on(tx, &self.request, Duration::from_secs(2), actor)
            .await?;
        if authorized.approval_required
            || authorized.action.request().context.origin != ToolCallOrigin::Workflow
        {
            return Err(missing());
        }
        let facts = http::runtime::facts(tx, self.request.scope()).await?;
        let step: String = sqlx::query_scalar(
            "SELECT step_id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3",
        )
        .bind(self.request.scope().company.as_uuid())
        .bind(self.request.scope().run.as_uuid())
        .bind(self.request.scope().execution.as_uuid())
        .fetch_one(&mut **tx)
        .await?;
        let step = StepId::parse(step).map_err(|_| invalid())?;
        let definition = facts
            .bundle
            .compiled()
            .graph()
            .definition()
            .steps
            .get(&step)
            .ok_or_else(missing)?;
        let permitted = match self.kind {
            BindingKind::Http => {
                matches!(definition.step_type.as_str(), "http.request" | "tool.call")
            }
            BindingKind::Mcp => definition.step_type.as_str() == "mcp.call",
        };
        if !permitted {
            return Err(missing());
        }
        let member:Option<Uuid>=sqlx::query_scalar("SELECT member.user_id FROM company_members AS member JOIN principals AS principal ON principal.company_id=member.company_id AND principal.user_id=member.user_id WHERE member.company_id=$1 AND member.user_id=$2 AND principal.kind='person' FOR SHARE OF member,principal")
            .bind(self.request.scope().company.as_uuid()).bind(self.owner).fetch_optional(&mut **tx).await?;
        if member.is_none() {
            return Err(missing());
        }
        Ok(authorized.action)
    }
}
#[async_trait]
impl DispatchAuthority for HostDispatchAuthority {
    async fn check(
        &self,
        invocation: &AuthorizedInvocation,
        module: &dyn CredentialDispatchGate,
    ) -> integration::IntegrationResult<()> {
        if serde_json::to_value(invocation).map_err(|_| integration::IntegrationError::Invalid)?
            != serde_json::to_value(&self.expected)
                .map_err(|_| integration::IntegrationError::Invalid)?
            || invocation.deadline <= Utc::now()
        {
            return Err(integration::IntegrationError::Denied);
        }
        let mut tx = http::begin_http(&self.persistence)
            .await
            .map_err(host_error)?;
        let current = self.current(&mut tx).await.map_err(host_error)?;
        if digest(current.operation_digest().as_str()).map_err(host_error)?
            != invocation.request_digest
            || current.request().arguments != *invocation.arguments.value()
            || current.request().operation_key.as_str() != invocation.operation.as_str()
            || current.request().context.integration.as_ref() != Some(&invocation.authorization)
        {
            return Err(integration::IntegrationError::Denied);
        }
        module
            .check(
                invocation,
                HostAuthorityFacts {
                    owner_is_member: true,
                    connection_enabled: true,
                    action_authorized: true,
                    server_created_provenance: true,
                    kind: self.kind.purpose(),
                    connection_revision: invocation.authorization.connection_revision,
                    catalog_revision: invocation.authorization.catalog_revision,
                },
            )
            .await?;
        // Row locks preserve the checked identities, but time still passes while
        // the module waits. Match ordinary remote entry's final live-window check.
        if invocation.deadline <= Utc::now()
            || super::lease::live_window(&mut tx, self.request.fence, tokio::time::Instant::now())
                .await
                .map_err(host_error)?
                .is_none()
        {
            return Err(integration::IntegrationError::Denied);
        }
        tx.rollback()
            .await
            .map_err(|_| integration::IntegrationError::Storage)?;
        Ok(())
    }
}
fn host_error(error: AppError) -> integration::IntegrationError {
    match error {
        AppError::NotFound(_)
        | AppError::Forbidden(_)
        | AppError::Conflict(_)
        | AppError::BadRequest(_) => integration::IntegrationError::Denied,
        _ => integration::IntegrationError::Storage,
    }
}
fn digest(value: &str) -> AppResult<[u8; 32]> {
    if value.len() != 64 {
        return Err(invalid());
    }
    let mut bytes = [0; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).map_err(|_| invalid())?;
    }
    Ok(bytes)
}
/// This produces authority metadata only. Phase06 must call it from the existing
/// remote-entry protocol; it does not authorize replay or perform a business call.
impl PostgresPersistence {
    pub(crate) async fn prepare_integration_invocation(
        &self,
        request: ActionDispatchRequest,
        dispatch: integration::DispatchId,
        deadline: DateTime<Utc>,
        cancel: CancellationToken,
    ) -> AppResult<(AuthorizedInvocation, ExecutionContext)> {
        let (operation,run_deadline,correlation):(Value,DateTime<Utc>,String)=sqlx::query_as("SELECT intent.operation,run.deadline,run.correlation_id::text FROM workflow_action_intents AS intent JOIN workflow_runs AS run ON run.company_id=intent.company_id AND run.id=intent.run_id WHERE intent.company_id=$1 AND intent.run_id=$2 AND intent.execution_id=$3 AND intent.id=$4 AND intent.argument_digest=$5")
            .bind(request.scope().company.as_uuid()).bind(request.scope().run.as_uuid()).bind(request.scope().execution.as_uuid()).bind(request.subject.invocation.as_uuid()).bind(request.subject.argument_digest.as_str()).fetch_optional(self.pool()).await?.ok_or_else(missing)?;
        let action = FrozenAction::restore(operation)?;
        let authorization = action
            .request()
            .context
            .integration
            .clone()
            .ok_or_else(missing)?;
        let kind = match &action.request().target {
            ActionTarget::Connection { resource_kind, .. }
            | ActionTarget::Local { resource_kind, .. } => match resource_kind.as_str() {
                "http" => BindingKind::Http,
                "mcp" => BindingKind::Mcp,
                _ => return Err(missing()),
            },
        };
        let grant = PostgresVault::new(self.clone())
            .grant_metadata(authorization.grant)
            .await?;
        if grant.client != client_namespace() {
            return Err(missing());
        }
        let owner = Uuid::parse_str(grant.owner.subject.as_str()).map_err(|_| missing())?;
        let invocation = AuthorizedInvocation {
            authorization,
            operation: integration::OperationId::parse(action.operation_key().as_str())?,
            arguments: OperationArguments::new(action.request().arguments.clone())?,
            request_digest: digest(action.operation_digest().as_str())?,
            correlation: integration::CorrelationId::parse(correlation)?,
            dispatch,
            deadline: deadline.min(run_deadline),
            cancellation: integration::CancellationId::parse(Uuid::new_v4().to_string())?,
            host_authority: integration::HostAuthorityReference::parse(Uuid::new_v4().to_string())?,
        };
        let authority = HostDispatchAuthority {
            persistence: self.clone(),
            request,
            expected: invocation.clone(),
            owner,
            kind,
        };
        let mut tx = http::begin_http(self).await?;
        authority.current(&mut tx).await?;
        tx.rollback().await?;
        Ok((
            invocation,
            ExecutionContext {
                authority: Arc::new(authority),
                cancellation: cancel,
            },
        ))
    }
}

/// Reclaim preserves the integration authority already frozen in the exact
/// deterministic action; a changed binding cannot create a replacement action.
pub(super) async fn frozen_snapshot_on(
    db: &mut PgConnection,
    request: &ActionRequest,
    current: Option<AuthorizationSnapshot>,
) -> AppResult<Option<AuthorizationSnapshot>> {
    let rows:Vec<Value>=sqlx::query_scalar("SELECT operation FROM workflow_action_intents WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND operation_key=$4 ORDER BY created_at,id LIMIT 2")
        .bind(request.scope.company.as_uuid()).bind(request.scope.run.as_uuid()).bind(request.scope.execution.as_uuid()).bind(request.operation_key.as_str()).fetch_all(db).await?;
    if rows.len() > 1 {
        return Err(missing());
    }
    match rows.into_iter().next() {
        None => Ok(current),
        Some(saved) => {
            let action = FrozenAction::restore(saved)?;
            if action.request().target != request.target
                || action.request().context.origin != request.context.origin
            {
                return Err(missing());
            }
            Ok(action.request().context.integration.clone())
        }
    }
}
