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
    async fn preview_agent(&self, scope: AgentPublicationScope) -> AppResult<AgentPreview> {
        let mut tx = begin_authorized(self, scope).await?;
        let (draft_revision, definition) = capture::capture_on(&mut tx, scope).await?;
        let active_revision = storage::active_on(&mut tx, scope).await?;
        tx.commit().await.map_err(write_error)?;
        Ok(AgentPreview {
            draft_revision,
            active_revision,
            definition,
        })
    }

    async fn publish_agent(&self, request: PublishAgent) -> AppResult<PublishedAgentRevision> {
        let mut tx = self.pool().begin().await?;
        sqlx::query("SELECT set_config('lock_timeout','1000ms',true),set_config('statement_timeout','5000ms',true)")
            .execute(&mut *tx).await?;
        let result = LockedAgentPublication::lock(&mut tx, request.scope)
            .await?
            .publish(&request)
            .await?;
        tx.commit().await.map_err(write_error)?;
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
