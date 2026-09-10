//! No network calls occur while a database transaction is held. Intent is conservatively
//! indeterminate until a fenced receipt commits; remote cancellation cannot undo an accepted call.
use super::harness_runs::{load_on, lock_task_execution_on, persist_checkpoint_on};
use crate::{
    app_error::{AppError, AppResult, ExecutionFailure},
    entities::task::TaskLeaseRef,
    services::{
        harness::{mcp::McpToolDeclaration, runs::*},
        mcp_runtime::{McpClaim, McpInvocationJournal, McpRunScope},
    },
};
use async_trait::async_trait;
use serde_json::Value;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

pub(super) struct PostgresMcpJournal {
    pool: PgPool,
    lease: TaskLeaseRef,
}
impl PostgresMcpJournal {
    pub(super) fn new(pool: PgPool, lease: TaskLeaseRef) -> Self {
        Self { pool, lease }
    }
    async fn load(&self, tx: &mut PgConnection, scope: &McpRunScope) -> AppResult<RunCheckpoint> {
        if scope.task_id != self.lease.task_id
            || !lock_task_execution_on(tx, scope.company_id, self.lease).await?
        {
            return Err(AppError::Execution(ExecutionFailure::OwnershipLost));
        }
        let run = load_on(tx, scope.company_id, scope.task_id, RunId(scope.run_id))
            .await?
            .ok_or_else(|| AppError::NotFound("MCP continuation".into()))?;
        if run.identity.agent_id != scope.agent_id || run.state != RunState::Active {
            return Err(AppError::Execution(ExecutionFailure::OwnershipLost));
        }
        Ok(run)
    }
}
fn id(value: &str) -> AppResult<InvocationId> {
    Uuid::parse_str(value)
        .map(InvocationId)
        .map_err(|_| AppError::BadRequest("Invalid durable MCP invocation".into()))
}
fn identity(declaration: &McpToolDeclaration) -> SavedMcpInvocation {
    SavedMcpInvocation {
        connection_id: declaration.identity.connection_id,
        tool_name: declaration.identity.name.clone(),
        definition_revision: declaration.definition_revision,
        selection_revision: declaration.selection_revision,
        credential_revision: declaration.credential_revision,
        schema_fingerprint: crate::services::mcp_runtime::fingerprint(&declaration.input_schema),
    }
}
fn invocation<'a>(
    run: &'a RunCheckpoint,
    id: InvocationId,
    declaration: &McpToolDeclaration,
    arguments: &Value,
) -> AppResult<&'a SavedInvocation> {
    let saved = run
        .invocations
        .iter()
        .find(|inv| inv.call.invocation_id == id)
        .ok_or_else(|| AppError::BadRequest("MCP invocation was not prepared".into()))?;
    if saved.call.arguments != *arguments
        || saved.call.tool_id != crate::services::harness::mcp::mcp_model_id(&declaration.identity)
        || saved
            .mcp
            .as_ref()
            .is_some_and(|value| value != &identity(declaration))
    {
        return Err(AppError::BadRequest(
            "MCP invocation identity changed".into(),
        ));
    }
    Ok(saved)
}
async fn authorize(
    tx: &mut PgConnection,
    scope: &McpRunScope,
    declaration: &McpToolDeclaration,
) -> AppResult<()> {
    let found = sqlx::query_scalar::<_, Uuid>(
        r#"SELECT connection.id FROM company_mcp_connections AS connection
           JOIN company_mcp_tool_grants AS grant_row ON grant_row.company_id = connection.company_id AND grant_row.connection_id = connection.id
           JOIN agent_mcp_selections AS selection ON selection.company_id = connection.company_id AND selection.connection_id = connection.id
           JOIN agent_mcp_selection_revisions AS revision ON revision.company_id = selection.company_id AND revision.agent_id = selection.agent_id
           WHERE connection.company_id = $1 AND connection.id = $2 AND connection.enabled AND connection.deleted_at IS NULL
             AND selection.agent_id = $3 AND grant_row.tool_name = $4 AND connection.revision = $5
             AND revision.revision = $6 AND connection.credential_revision = $7
             AND EXISTS (SELECT 1 FROM jsonb_array_elements(connection.discovery_json) AS discovered(value)
                 WHERE discovered.value->>'name' = $4 AND discovered.value->'input_schema' = $8)
           FOR SHARE OF connection, grant_row, selection, revision"#,
    ).bind(scope.company_id).bind(declaration.identity.connection_id).bind(scope.agent_id)
    .bind(declaration.identity.name.as_str()).bind(declaration.definition_revision).bind(declaration.selection_revision)
    .bind(declaration.credential_revision).bind(&declaration.input_schema).fetch_optional(&mut *tx).await?;
    if found.is_none() {
        return Err(AppError::BadRequest(
            "MCP capability or credential revision changed".into(),
        ));
    }
    Ok(())
}

#[async_trait]
impl McpInvocationJournal for PostgresMcpJournal {
    async fn completed(
        &self,
        scope: &McpRunScope,
        declaration: &McpToolDeclaration,
        invocation_id: &str,
        arguments: &Value,
    ) -> AppResult<Option<Value>> {
        let mut tx = self.pool.begin().await?;
        let run = self.load(&mut tx, scope).await?;
        authorize(&mut tx, scope, declaration).await?;
        let saved = invocation(&run, id(invocation_id)?, declaration, arguments)?;
        if saved.state == InvocationState::Indeterminate {
            return Err(AppError::Execution(ExecutionFailure::IndeterminateEffect));
        }
        Ok(saved.result.clone())
    }
    async fn claim(
        &self,
        scope: &McpRunScope,
        declaration: &McpToolDeclaration,
        invocation_id: &str,
        arguments: &Value,
    ) -> AppResult<McpClaim> {
        let mut tx = self.pool.begin().await?;
        let run = self.load(&mut tx, scope).await?;
        authorize(&mut tx, scope, declaration).await?;
        let id = id(invocation_id)?;
        let saved = invocation(&run, id, declaration, arguments)?;
        if let Some(value) = &saved.result {
            return Ok(McpClaim::Completed(value.clone()));
        }
        let (next, _) = run.apply(Mutation::BeginRemote(id, identity(declaration)))?;
        persist_checkpoint_on(&mut tx, scope.company_id, scope.task_id, &run, &next).await?;
        tx.commit().await?;
        Ok(McpClaim::Execute)
    }
    async fn complete(
        &self,
        scope: &McpRunScope,
        invocation_id: &str,
        result: &Value,
    ) -> AppResult<()> {
        let mut tx = self.pool.begin().await?;
        let run = self.load(&mut tx, scope).await?;
        let id = id(invocation_id)?;
        if !run
            .invocations
            .iter()
            .any(|inv| inv.call.invocation_id == id && inv.mcp.is_some())
        {
            return Err(AppError::BadRequest("Remote effect was not claimed".into()));
        }
        let (next, _) = run.apply(Mutation::Result(id, result.clone()))?;
        persist_checkpoint_on(&mut tx, scope.company_id, scope.task_id, &run, &next).await?;
        tx.commit().await?;
        Ok(())
    }
    async fn indeterminate(&self, scope: &McpRunScope, invocation_id: &str) -> AppResult<()> {
        let mut tx = self.pool.begin().await?;
        let run = self.load(&mut tx, scope).await?;
        let invocation_id = id(invocation_id)?;
        if !run.invocations.iter().any(|inv| {
            inv.call.invocation_id == invocation_id && inv.state == InvocationState::Indeterminate
        }) {
            return Err(AppError::BadRequest("Missing remote effect intent".into()));
        }
        // Intent was charged and marked indeterminate before sending; never reset it for retry.
        Ok(())
    }
}
