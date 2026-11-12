use super::*;

#[async_trait::async_trait]
impl AgentAuthoringReads for PostgresPersistence {
    async fn authoring_state(
        &self,
        scope: AgentPublicationScope,
    ) -> AppResult<AgentAuthoringState> {
        bounded(async {
            let mut tx = super::super::agent_publication::begin_authorized(self, scope).await?;
            let result = super::state::read(&mut tx, scope).await?;
            tx.commit().await?;
            Ok(result)
        })
        .await
    }
    async fn revision_history(
        &self,
        scope: AgentPublicationScope,
        before: Option<AgentHistoryCursor>,
    ) -> AppResult<AgentHistoryPage> {
        bounded(async {
            let mut tx = super::super::agent_publication::begin_authorized(self, scope).await?;
            let result = super::history::page(&mut tx, scope, before).await?;
            tx.commit().await?;
            Ok(result)
        })
        .await
    }
    async fn tool_connections(
        &self,
        scope: AgentPublicationScope,
        kind: AgentToolKind,
        after: Option<Uuid>,
    ) -> AppResult<AgentConnectionPage> {
        bounded(async {
            let mut tx = super::super::agent_publication::begin_authorized(self, scope).await?;
            let result = super::catalog::connections(&mut tx, scope, kind, after).await?;
            tx.commit().await?;
            Ok(result)
        })
        .await
    }
    async fn tool_options(&self, request: AgentToolPageRequest) -> AppResult<AgentToolPage> {
        bounded(async {
            let mut tx =
                super::super::agent_publication::begin_authorized(self, request.scope).await?;
            let result = match request.kind {
                AgentToolKind::Builtin => {
                    let memory = crate::adapters::persistence::memory::memory_enabled_on(
                        &mut tx,
                        request.scope.company,
                    )
                    .await?;
                    super::catalog::builtins(memory)?
                }
                AgentToolKind::Http => super::catalog::http(&mut tx, &request).await?,
                AgentToolKind::Mcp => super::catalog::mcp(&mut tx, &request).await?,
                AgentToolKind::Workflow => super::catalog::workflows(&mut tx, &request).await?,
            };
            tx.commit().await?;
            Ok(result)
        })
        .await
    }
    async fn agent_directory(
        &self,
        scope: AgentCompanyScope,
        after: Option<AgentKey>,
    ) -> AppResult<AgentDirectoryPage> {
        bounded(async {
            let mut tx = super::super::agent_publication::begin_bounded(self).await?;
            super::super::agent_publication::lock_company_on(&mut tx, scope).await?;
            let result = super::directory::page(&mut tx, scope, self.app_domain(), after).await?;
            tx.commit().await?;
            Ok(result)
        })
        .await
    }
    async fn authorize_authoring(&self, scope: AgentPublicationScope) -> AppResult<()> {
        bounded(async {
            super::super::agent_publication::begin_authorized(self, scope)
                .await?
                .commit()
                .await
                .map_err(AppError::from)
        })
        .await
    }

    async fn agent_directory_entry(
        &self,
        scope: AgentCompanyScope,
        key: AgentKey,
    ) -> AppResult<Option<AgentDirectoryEntry>> {
        bounded(async {
            let mut tx = super::super::agent_publication::begin_bounded(self).await?;
            super::super::agent_publication::lock_company_on(&mut tx, scope).await?;
            let entry = super::directory::entry(&mut tx, scope, self.app_domain(), &key).await?;
            tx.commit().await?;
            Ok(entry)
        })
        .await
    }

    async fn authorize_company_authoring(&self, scope: AgentCompanyScope) -> AppResult<()> {
        bounded(async {
            let mut tx = super::super::agent_publication::begin_bounded(self).await?;
            super::super::agent_publication::lock_company_on(&mut tx, scope).await?;
            tx.commit().await?;
            Ok(())
        })
        .await
    }
}
