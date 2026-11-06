//! Company-authorized draft changes and scoped capability authoring projections.
use super::PostgresPersistence;
use crate::app_error::{AppError, AppResult};
use crate::application::agent::*;
use crate::entities::agent_capability::*;
use sqlx::PgConnection;
use uuid::Uuid;

mod catalog;
mod choices;
mod directory;
mod history;
mod reads;
mod state;
mod status;

fn invalid() -> AppError {
    AppError::Internal("Invalid stored agent authoring metadata".into())
}

async fn bounded<T>(operation: impl std::future::Future<Output = AppResult<T>>) -> AppResult<T> {
    tokio::time::timeout(std::time::Duration::from_secs(5),operation).await.map_err(|_|AppError::ServiceUnavailable(
        "Agent authoring timed out. Reload the draft to confirm its current state before retrying.".into()))?
}

#[async_trait::async_trait]
impl AgentDraftPersistence for PostgresPersistence {
    async fn save_company_agent(
        &self,
        mut request: SaveCompanyAgent,
    ) -> AppResult<crate::entities::agent::Agent> {
        bounded(async {
            let mut tx = super::agent_publication::begin_authorized(self, request.scope).await?;
            let revision: i64 = sqlx::query_scalar(
                "SELECT draft_revision FROM agents WHERE company_id=$1 AND id=$2",
            )
            .bind(request.scope.company.as_uuid())
            .bind(request.scope.agent.as_uuid())
            .fetch_one(&mut *tx)
            .await?;
            if u64::try_from(revision).ok() != Some(request.expected_revision.get()) {
                return Err(AppError::Conflict(
                    "This agent draft changed. Reload it before saving your changes.".into(),
                ));
            }
            if request.write.tool_bindings.is_some() {
                return Err(AppError::BadRequest(
                    "Company draft commands require explicit tool choices".into(),
                ));
            }
            if let Some(assignments) = &request.tools {
                let previous = super::agent_capability::bindings_on(
                    &mut tx,
                    Some(request.scope.company.as_uuid()),
                    request.scope.agent.as_uuid(),
                )
                .await?;
                request.write.tool_bindings = Some(
                    choices::resolve(
                        &mut tx,
                        request.scope.company.as_uuid(),
                        &previous,
                        assignments,
                    )
                    .await?,
                );
            }
            let db = super::agent::update_agent_on(
                &mut tx,
                Some(request.scope.company.as_uuid()),
                request.scope.agent.as_uuid(),
                &request.write,
            )
            .await?;
            tx.commit()
                .await
                .map_err(super::agent_capability::relationship_error)?;
            db.try_into()
        })
        .await
    }
}
