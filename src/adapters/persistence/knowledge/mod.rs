//! Short, company-serialized knowledge operations. Model work never runs in these transactions.
mod bounds;
mod moves;
mod permissions;
mod rows;
mod search;
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
            let mut tx = authority::start(self.pool(), team_scope(scope), None).await?;
            let result = authorize(&mut tx, scope).await;
            tx.rollback().await.map_err(authority::database)?;
            result
        })
        .await
    }
    async fn preflight_search(
        &self,
        scope: KnowledgeScope,
        unit: Option<crate::entities::department::DepartmentId>,
    ) -> AppResult<()> {
        let sent = AtomicBool::new(false);
        bounded(&sent, async {
            let mut tx = authority::start(self.pool(), team_scope(scope), None).await?;
            let result = async {
                let permissions = permissions::load(&mut tx, scope).await?;
                if let Some(unit) = unit {
                    permissions.require_read(&mut tx, scope, unit).await?;
                }
                Ok(())
            }
            .await;
            tx.rollback().await.map_err(authority::database)?;
            result
        })
        .await
    }
    async fn unit_choices(
        &self,
        scope: KnowledgeScope,
        query: KnowledgeUnitQuery,
    ) -> AppResult<KnowledgeUnitPage> {
        let sent = AtomicBool::new(false);
        bounded(&sent, async {
            let mut tx = authority::start(self.pool(), team_scope(scope), None).await?;
            let result = async {
                let permissions = permissions::load(&mut tx, scope).await?;
                for unit in [query.selected, query.after].into_iter().flatten() {
                    permissions
                        .require_write(&mut tx, KnowledgeUploadTarget { scope, unit })
                        .await?;
                }
                permissions.choices(&mut tx, scope, query).await
            }
            .await;
            tx.rollback().await.map_err(authority::database)?;
            result
        })
        .await
    }
    async fn list(
        &self,
        scope: KnowledgeScope,
        unit: Option<crate::entities::department::DepartmentId>,
    ) -> AppResult<KnowledgeLibraryView> {
        let sent = AtomicBool::new(false);
        bounded(&sent, async {
            let mut tx = authority::start(self.pool(), team_scope(scope), None).await?;
            let result = async {
                let permissions = permissions::load(&mut tx, scope).await?;
                if let Some(unit) = unit {
                    permissions.require_read(&mut tx, scope, unit).await?;
                }
                let layers = rows::visible(&mut tx, scope, &permissions, unit).await?;
                let default_unit = permissions.default_for(unit);
                let choices = permissions
                    .choices(
                        &mut tx,
                        scope,
                        KnowledgeUnitQuery {
                            selected: default_unit,
                            after: None,
                        },
                    )
                    .await?;
                let writable_units = permissions.writable_visible_units(&layers);
                Ok(KnowledgeLibraryView {
                    actor: permissions.actor,
                    layers,
                    upload_units: choices.units,
                    writable_units,
                    default_unit,
                    units_truncated: choices.next.is_some(),
                })
            }
            .await;
            tx.rollback().await.map_err(authority::database)?;
            result
        })
        .await
    }
    async fn search_manual(
        &self,
        query: ManualKnowledgeQuery,
    ) -> AppResult<Vec<crate::application::knowledge::search::StoredKnowledgeChunk>> {
        search::manual(self, query).await
    }
    async fn preflight_upload(
        &self,
        target: KnowledgeUploadTarget,
        layers: &[KnowledgeLayerSize],
    ) -> AppResult<()> {
        let sent = AtomicBool::new(false);
        bounded(&sent, async {
            let scope = target.scope;
            let mut tx = authority::start(self.pool(), team_scope(scope), None).await?;
            let result = async {
                let permissions = permissions::load(&mut tx, scope).await?;
                permissions.require_write(&mut tx, target).await?;
                // The multipart boundary may authorize its target before reading any files.
                if !layers.is_empty() {
                    authorize_layers(permissions.actor, layers)?;
                }
                rows::project_quota(&mut tx, target, layers).await
            }
            .await;
            tx.rollback().await.map_err(authority::database)?;
            result
        })
        .await
    }
    async fn replace(
        &self,
        target: KnowledgeUploadTarget,
        documents: &[PreparedKnowledgeDocument],
    ) -> AppResult<Vec<KnowledgeDocumentLayer>> {
        let sent = AtomicBool::new(false);
        bounded(&sent, async {
            let scope = target.scope;
            let mut tx = authority::start(self.pool(), team_scope(scope), None).await?;
            let result = async {
                let permissions = permissions::load(&mut tx, scope).await?;
                permissions.require_write(&mut tx, target).await?;
                let layers: Vec<_> = documents
                    .iter()
                    .map(|document| document.file.size())
                    .collect();
                authorize_layers(permissions.actor, &layers)?;
                writes::validate(documents)?;
                rows::project_quota(&mut tx, target, &layers).await?;
                let mut ids = Vec::with_capacity(documents.len());
                for document in documents {
                    ids.push(writes::replace(&mut tx, target, document).await?);
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
            let mut tx = authority::start(self.pool(), team_scope(command.scope), None).await?;
            let result = async {
                let permissions = permissions::load(&mut tx, command.scope).await?;
                let layer = rows::one_visible(&mut tx, command, &permissions).await?;
                permissions
                    .require_write(
                        &mut tx,
                        KnowledgeUploadTarget {
                            scope: command.scope,
                            unit: layer.unit_id,
                        },
                    )
                    .await?;
                may_write_document(permissions.actor, layer.access_level)
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

    async fn move_document(&self, command: MoveKnowledgeDocument) -> AppResult<()> {
        let sent = AtomicBool::new(false);
        bounded(&sent, async {
            let mut tx = authority::start(self.pool(), team_scope(command.scope), None).await?;
            let result = moves::move_on(&mut tx, command).await;
            finish(tx, result, &sent).await
        })
        .await
    }
}
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) async fn root_target(
    p: &PostgresPersistence,
    scope: KnowledgeScope,
) -> KnowledgeUploadTarget {
    let unit: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM departments WHERE company_id=$1 AND is_root")
            .bind(scope.company.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    KnowledgeUploadTarget {
        scope,
        unit: crate::entities::department::DepartmentId::new(unit),
    }
}
