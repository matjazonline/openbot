//! Standalone agent publication over the existing company registries.
use super::PostgresPersistence;
use crate::app_error::{AppError, AppResult};
use crate::application::agent::*;
use crate::domain::workflow::{CompanyId, TypeName};
use crate::entities::agent_capability::*;
use sqlx::{PgConnection, Postgres, Transaction};
use uuid::Uuid;

mod capture;
mod commands;
mod grants;
mod storage;
mod tools;

#[cfg(test)]
mod tests;

fn missing() -> AppError {
    AppError::NotFound("Agent or current publication source not found".into())
}
fn invalid() -> AppError {
    AppError::Internal("Invalid stored agent publication data".into())
}
fn conflict() -> AppError {
    AppError::Conflict("Agent draft or active revision changed; refresh and try again".into())
}
fn unavailable() -> AppError {
    AppError::BadRequest("An agent source is unavailable or its reviewed contract changed".into())
}

async fn begin_authorized(
    persistence: &PostgresPersistence,
    scope: AgentPublicationScope,
) -> AppResult<Transaction<'_, Postgres>> {
    use super::company_invite::authority::{TeamScope, start};
    let mut tx = start(
        persistence.pool(),
        TeamScope {
            actor_id: scope.actor.user_id(),
            company_id: scope.company.as_uuid(),
        },
    )
    .await?;
    super::workflow::authority::authorize_company(&mut tx, scope.company, scope.actor).await?;
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM agents WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(scope.company.as_uuid())
        .bind(scope.agent.as_uuid())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(missing)?;
    Ok(tx)
}
