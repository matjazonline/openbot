//! Standalone agent publication over the existing company registries.
use super::PostgresPersistence;
use crate::app_error::{AppError, AppResult};
use crate::application::agent::*;
use crate::domain::workflow::{CompanyId, TypeName};
use crate::entities::agent_capability::*;
use sqlx::{PgConnection, Postgres, Transaction};
use uuid::Uuid;

mod capture;
pub(in crate::adapters::persistence) mod commands;
mod grants;
pub(in crate::adapters::persistence) mod storage;
pub(in crate::adapters::persistence) mod tools;

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
    let mut tx = persistence.pool().begin().await?;
    sqlx::query("SELECT set_config('lock_timeout','1000ms',true),set_config('statement_timeout','5000ms',true)")
        .execute(&mut *tx).await?;
    LockedAgentPublication::lock(&mut tx, scope).await?;
    Ok(tx)
}

/// The borrow keeps authorization/owner locks and writes on this exact transaction.
/// Existing callers acquire the actor account before any company lock they already hold.
pub(in crate::adapters::persistence) struct LockedAgentPublication<'a, 'db> {
    tx: &'a mut Transaction<'db, Postgres>,
    scope: AgentPublicationScope,
}
impl<'a, 'db> LockedAgentPublication<'a, 'db> {
    pub(in crate::adapters::persistence) async fn lock(
        tx: &'a mut Transaction<'db, Postgres>,
        scope: AgentPublicationScope,
    ) -> AppResult<Self> {
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE id=$1 FOR SHARE")
            .bind(scope.actor.user_id())
            .fetch_optional(&mut **tx)
            .await?
            .ok_or_else(missing)?;
        super::workflow::authority::authorize_company(tx, scope.company, scope.actor).await?;
        sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM agents WHERE company_id=$1 AND id=$2 FOR UPDATE",
        )
        .bind(scope.company.as_uuid())
        .bind(scope.agent.as_uuid())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(missing)?;
        Ok(Self { tx, scope })
    }
    pub(in crate::adapters::persistence) async fn preview(self) -> AppResult<AgentPreview> {
        let (draft_revision, definition) = capture::capture_on(self.tx, self.scope).await?;
        let active_revision = storage::active_on(self.tx, self.scope).await?;
        Ok(AgentPreview {
            draft_revision,
            active_revision,
            definition,
        })
    }
    pub(in crate::adapters::persistence) async fn publish(
        self,
        request: &PublishAgent,
    ) -> AppResult<PublishedAgentRevision> {
        if request.scope.company != self.scope.company
            || request.scope.agent != self.scope.agent
            || request.scope.actor != self.scope.actor
        {
            return Err(missing());
        }
        commands::publish_on(self.tx, request).await
    }
}

/// Trusted resource creators publish once, on their resource transaction. An ordinary draft
/// save never calls this, and a deactivated agent with history is deliberately left inactive.
pub(in crate::adapters::persistence) async fn publish_initial_on(
    tx: &mut Transaction<'_, Postgres>,
    scope: AgentPublicationScope,
) -> AppResult<()> {
    let locked = LockedAgentPublication::lock(tx, scope).await?;
    let published: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM agent_revisions WHERE company_id=$1 AND agent_id=$2)",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.agent.as_uuid())
    .fetch_one(&mut **locked.tx)
    .await?;
    if published {
        return Ok(());
    }
    let draft: i64 =
        sqlx::query_scalar("SELECT draft_revision FROM agents WHERE company_id=$1 AND id=$2")
            .bind(scope.company.as_uuid())
            .bind(scope.agent.as_uuid())
            .fetch_one(&mut **locked.tx)
            .await?;
    let request = PublishAgent {
        scope,
        expected_draft_revision: AgentDraftRevision::new(
            u64::try_from(draft).map_err(|_| invalid())?,
        )
        .map_err(|_| invalid())?,
        expected_active_revision: None,
        key: AgentPublicationKey::parse(format!("initial:{}", scope.agent.as_uuid()))
            .map_err(AppError::BadRequest)?,
    };
    locked.publish(&request).await?;
    Ok(())
}
