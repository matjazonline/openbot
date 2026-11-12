use super::*;
use async_trait::async_trait;

fn write_error(error: sqlx::Error) -> AppError {
    match error
        .as_database_error()
        .and_then(|error| error.code())
        .as_deref()
    {
        Some("23505" | "40001" | "40P01" | "55P03") => conflict(),
        _ => AppError::from(error),
    }
}

#[async_trait]
impl AgentPublications for PostgresPersistence {
    #[tracing::instrument(skip_all, fields(company=%scope.company.as_uuid(), agent=%scope.agent.as_uuid()))]
    async fn preview_agent(&self, scope: AgentPublicationScope) -> AppResult<AgentPreview> {
        let mut tx = self.pool().begin().await?;
        sqlx::query("SELECT set_config('lock_timeout','1000ms',true),set_config('statement_timeout','5000ms',true)").execute(&mut *tx).await?;
        let preview = LockedAgentPublication::lock(&mut tx, scope)
            .await?
            .preview()
            .await
            .inspect_err(|error| {
                tracing::warn!(failure = publication_failure(error), "Agent preview failed")
            })?;
        tx.commit().await.map_err(write_error)?;
        Ok(preview)
    }

    #[tracing::instrument(skip_all, fields(company=%request.scope.company.as_uuid(), agent=%request.scope.agent.as_uuid(), draft=request.expected_draft_revision.get()))]
    async fn publish_agent(&self, request: PublishAgent) -> AppResult<PublishedAgentRevision> {
        let mut tx = self.pool().begin().await?;
        sqlx::query("SELECT set_config('lock_timeout','1000ms',true),set_config('statement_timeout','5000ms',true)")
            .execute(&mut *tx).await?;
        let result = LockedAgentPublication::lock(&mut tx, request.scope)
            .await?
            .publish(&request)
            .await
            .inspect_err(|error| {
                tracing::warn!(
                    failure = publication_failure(error),
                    "Agent publication failed"
                )
            })?;
        tx.commit().await.map_err(write_error)?;
        tracing::info!(revision=%result.id.as_uuid(), "Agent publication request completed");
        Ok(result)
    }

    async fn active_agent_revision(
        &self,
        scope: AgentPublicationScope,
    ) -> AppResult<Option<PublishedAgentRevision>> {
        let mut tx = begin_authorized(self, scope).await?;
        let result = match storage::active_on(&mut tx, scope).await? {
            Some(id) => Some(
                storage::revision_on(&mut tx, scope, id)
                    .await?
                    .ok_or_else(invalid)?,
            ),
            None => None,
        };
        tx.commit().await.map_err(write_error)?;
        Ok(result)
    }

    async fn published_agent_revision(
        &self,
        scope: AgentPublicationScope,
        revision: AgentRevisionId,
    ) -> AppResult<Option<PublishedAgentRevision>> {
        let mut tx = begin_authorized(self, scope).await?;
        let result = storage::revision_on(&mut tx, scope, revision).await?;
        tx.commit().await.map_err(write_error)?;
        Ok(result)
    }

    async fn deactivate_agent(
        &self,
        scope: AgentPublicationScope,
        expected: Option<AgentRevisionId>,
    ) -> AppResult<()> {
        let mut tx = begin_authorized(self, scope).await?;
        if storage::active_on(&mut tx, scope).await? != expected {
            return Err(conflict());
        }
        sqlx::query("DELETE FROM agent_active_revisions WHERE company_id=$1 AND agent_id=$2")
            .bind(scope.company.as_uuid())
            .bind(scope.agent.as_uuid())
            .execute(&mut *tx)
            .await?;
        tx.commit().await.map_err(write_error)
    }

    async fn revoke_agent_grant(
        &self,
        scope: AgentPublicationScope,
        identity: AgentGrantIdentity,
    ) -> AppResult<()> {
        let mut tx = begin_authorized(self, scope).await?;
        grants::revoke_on(&mut tx, scope, identity).await?;
        tx.commit().await.map_err(write_error)
    }
}

fn publication_failure(error: &AppError) -> &'static str {
    match error {
        AppError::WorkflowRole(_) => "role_resolution_refused",
        AppError::BadRequest(_) => "invalid_definition",
        AppError::Conflict(_) | AppError::Superseded(_) => "conflict",
        AppError::Forbidden(_) | AppError::InvalidCredentials => "forbidden",
        AppError::NotFound(_) => "missing",
        AppError::ServiceUnavailable(_) => "unavailable",
        AppError::McpAuthentication => "mcp_authentication_rejected",
        AppError::Database(_) | AppError::DatabaseTimeout(_) => "database",
        AppError::Timeout(_) => "timeout",
        AppError::Execution(_)
        | AppError::WorkflowGuard(_)
        | AppError::KnowledgeTrigger { .. }
        | AppError::KnowledgeRelease { .. } => "execution_refused",
        AppError::Internal(_) => "internal",
    }
}

pub(super) async fn publish_on(
    tx: &mut Transaction<'_, Postgres>,
    request: &PublishAgent,
) -> AppResult<PublishedAgentRevision> {
    if let Some(result) = storage::replay_on(tx, request).await? {
        return Ok(result);
    }
    let draft: i64 =
        sqlx::query_scalar("SELECT draft_revision FROM agents WHERE company_id=$1 AND id=$2")
            .bind(request.scope.company.as_uuid())
            .bind(request.scope.agent.as_uuid())
            .fetch_one(&mut **tx)
            .await?;
    if draft != request.expected_draft_revision.get() as i64
        || storage::active_on(tx, request.scope).await? != request.expected_active_revision
    {
        return Err(conflict());
    }
    let (draft, definition) = capture::capture_on(tx, request.scope).await?;
    if draft != request.expected_draft_revision {
        return Err(conflict());
    }
    let result = storage::insert_revision_on(tx, request.scope, draft, definition).await?;
    storage::activate_on(tx, request, &result).await?;
    Ok(result)
}
