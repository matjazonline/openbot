//! Short, company-serialized knowledge operations. Model work never runs in these transactions.
mod bounds;
mod rows;
mod writes;
use super::{PostgresPersistence, company_invite::authority};
use crate::{
    app_error::{AppError, AppResult},
    application::knowledge::*,
    entities::{access_level::LevelActor, knowledge::*},
};
use async_trait::async_trait;
use bounds::{bounded, finish};
use sqlx::{Postgres, Transaction};
use std::sync::atomic::AtomicBool;

fn team_scope(scope: KnowledgeScope) -> authority::TeamScope {
    authority::TeamScope {
        actor_id: scope.actor_id,
        company_id: scope.company.as_uuid(),
    }
}
async fn authorize(
    tx: &mut Transaction<'_, Postgres>,
    scope: KnowledgeScope,
) -> AppResult<LevelActor> {
    let current = authority::lock_team(tx, team_scope(scope), None).await?;
    if !current.actor.membership.is_team() {
        return Err(AppError::NotFound("Company not found".into()));
    }
    if current.actor.membership.is_owner() != (current.owner_id == scope.actor_id) {
        return Err(AppError::Internal(
            "Stored company ownership is inconsistent".into(),
        ));
    }
    Ok(current.actor)
}
fn authorize_layers(actor: LevelActor, layers: &[KnowledgeLayerSize]) -> AppResult<()> {
    if layers.is_empty() || layers.len() > MAX_KNOWLEDGE_FILES {
        return Err(AppError::BadRequest(
            "Pick between one and ten files".into(),
        ));
    }
    for layer in layers {
        may_write_document(actor, layer.access_level)
            .map_err(|error| AppError::Forbidden(error.to_string()))?;
    }
    Ok(())
}
#[async_trait]
impl KnowledgeDocuments for PostgresPersistence {
    async fn access(&self, scope: KnowledgeScope) -> AppResult<LevelActor> {
        let sent = AtomicBool::new(false);
        bounded(&sent, async {
            let mut tx = authority::start(self.pool(), team_scope(scope)).await?;
            let result = authorize(&mut tx, scope).await;
            tx.rollback().await.map_err(authority::database)?;
            result
        })
        .await
    }
    async fn list(&self, scope: KnowledgeScope) -> AppResult<KnowledgeLibraryView> {
        let sent = AtomicBool::new(false);
        bounded(&sent, async {
            let mut tx = authority::start(self.pool(), team_scope(scope)).await?;
            let result = async {
                let actor = authorize(&mut tx, scope).await?;
                let layers = rows::visible(&mut tx, scope, actor.access_level).await?;
                Ok(KnowledgeLibraryView { actor, layers })
            }
            .await;
            tx.rollback().await.map_err(authority::database)?;
            result
        })
        .await
    }
    async fn preflight_upload(
        &self,
        scope: KnowledgeScope,
        layers: &[KnowledgeLayerSize],
    ) -> AppResult<()> {
        let sent = AtomicBool::new(false);
        bounded(&sent, async {
            let mut tx = authority::start(self.pool(), team_scope(scope)).await?;
            let result = async {
                let actor = authorize(&mut tx, scope).await?;
                authorize_layers(actor, layers)?;
                rows::project_quota(&mut tx, scope, layers).await
            }
            .await;
            tx.rollback().await.map_err(authority::database)?;
            result
        })
        .await
    }
    async fn replace(
        &self,
        scope: KnowledgeScope,
        documents: &[PreparedKnowledgeDocument],
    ) -> AppResult<Vec<KnowledgeDocumentLayer>> {
        let sent = AtomicBool::new(false);
        bounded(&sent, async {
            let mut tx = authority::start(self.pool(), team_scope(scope)).await?;
            let result = async {
                let actor = authorize(&mut tx, scope).await?;
                let layers: Vec<_> = documents
                    .iter()
                    .map(|document| document.file.size())
                    .collect();
                authorize_layers(actor, &layers)?;
                writes::validate(documents)?;
                rows::project_quota(&mut tx, scope, &layers).await?;
                let mut ids = Vec::with_capacity(documents.len());
                for document in documents {
                    ids.push(writes::replace(&mut tx, scope, document).await?);
                }
                rows::check_quota(&mut tx, scope).await?;
                rows::by_ids(&mut tx, scope, &ids).await
            }
            .await;
            finish(tx, result, &sent).await
        })
        .await
    }
    async fn delete(&self, command: DeleteKnowledgeLayer) -> AppResult<()> {
        let sent = AtomicBool::new(false);
        bounded(&sent, async {
            let mut tx = authority::start(self.pool(), team_scope(command.scope)).await?;
            let result = async {
                let actor = authorize(&mut tx, command.scope).await?;
                let layer = rows::one_visible(&mut tx, command, actor.access_level).await?;
                may_write_document(actor, layer.access_level)
                    .map_err(|error| AppError::Forbidden(error.to_string()))?;
                if layer.revision != command.expected_revision {
                    return Err(AppError::Conflict(
                        "The file changed; refresh before deleting it".into(),
                    ));
                }
                sqlx::query("DELETE FROM knowledge_documents WHERE company_id=$1 AND id=$2")
                    .bind(command.scope.company.as_uuid())
                    .bind(command.document_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(authority::database)?;
                Ok(())
            }
            .await;
            finish(tx, result, &sent).await
        })
        .await
    }
}
#[cfg(test)]
mod tests;
