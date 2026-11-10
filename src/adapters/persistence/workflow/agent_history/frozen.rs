//! Exact immutable replay material, with live pre-provider audience checks.
use super::*;
use crate::application::workflow::agent::{DepartmentContext, PreparedAgent};
use crate::entities::access_level::AccessLevel;
use serde_json::Value;

fn action(fence: WorkflowFence) -> ActionScope {
    ActionScope {
        company: fence.scope.company,
        run: fence.scope.run,
        execution: fence.scope.execution,
    }
}

pub(super) async fn restore_on(
    db: &mut PgConnection,
    fence: WorkflowFence,
    prepared: PreparedAgent,
) -> AppResult<Option<PreparedAgent>> {
    let saved: Option<Value> = sqlx::query_scalar("SELECT frozen_preparation FROM agent_history_exchanges WHERE company_id=$1 AND run_id=$2 AND execution_id=$3")
        .bind(fence.scope.company.as_uuid()).bind(fence.scope.run.as_uuid()).bind(fence.scope.execution.as_uuid())
        .fetch_one(&mut *db).await?;
    let Some(saved) = saved else { return Ok(None) };
    let history: FrozenAgentHistory =
        serde_json::from_value(saved["history"].clone()).map_err(|_| invalid())?;
    // The saved department first: it rebuilds the preamble, whose length the frozen history's
    // budget counts, so a department edited since replays as it was frozen (01DEP-P03-D11).
    let department: Option<DepartmentContext> =
        serde_json::from_value(saved["department"].clone()).map_err(|_| invalid())?;
    let prepared = prepared
        .with_department(department)?
        .with_frozen_history(history)?;
    if serde_json::to_value(&prepared).map_err(|_| invalid())? != saved {
        return Err(AppError::Conflict(
            "Frozen agent preparation changed".into(),
        ));
    }
    audience_on(db, action(fence)).await?;
    tracing::info!(company_id=%fence.scope.company.as_uuid(),exchange_id=%fence.scope.execution.as_uuid(),
        durability="staged", event="agent_history.recovered", "Frozen agent history restored in current transaction");
    Ok(Some(prepared))
}

pub(super) async fn freeze_on(
    db: &mut PgConnection,
    fence: WorkflowFence,
    prepared: PreparedAgent,
) -> AppResult<PreparedAgent> {
    if let Some(saved) = restore_on(db, fence, prepared.clone()).await? {
        return Ok(saved);
    }
    let scope = action(fence);
    let history = Box::pin(super::selection::select_on(db, scope, &prepared)).await?;
    super::super::knowledge::raise_content_level_on(
        db,
        scope.company,
        scope.run,
        history.selected_level,
    )
    .await?;
    let prepared = prepared.with_frozen_history(history)?;
    let document = serde_json::to_value(&prepared).map_err(|_| invalid())?;
    let changed = sqlx::query("UPDATE agent_history_exchanges AS exchange SET frozen_preparation=$4,access_level=GREATEST(exchange.access_level,run.content_level) FROM workflow_runs AS run WHERE exchange.company_id=$1 AND exchange.run_id=$2 AND exchange.execution_id=$3 AND exchange.frozen_preparation IS NULL AND run.company_id=exchange.company_id AND run.id=exchange.run_id")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(document)
        .execute(&mut *db).await?;
    if changed.rows_affected() != 1 {
        return Err(missing());
    }
    audience_on(db, scope).await?;
    Ok(prepared)
}

#[derive(sqlx::FromRow)]
struct Audience {
    actor_id: Uuid,
    channel_id: Option<Uuid>,
    thread_id: Option<Uuid>,
    content_level: i16,
    frozen_level: Option<i16>,
    scope_kind: Option<String>,
}

/// Caller owns its run before this read; no new global lock is taken after it.
pub(in crate::adapters::persistence::workflow) async fn audience_on(
    db: &mut PgConnection,
    scope: ActionScope,
) -> AppResult<()> {
    let facts: Audience = sqlx::query_as("SELECT run.actor_id,run.channel_id,run.thread_id,run.content_level::smallint AS content_level,(exchange.frozen_preparation->'history'->>'content_level')::smallint AS frozen_level,conversation.scope_kind FROM workflow_runs AS run LEFT JOIN agent_history_exchanges AS exchange ON exchange.company_id=run.company_id AND exchange.run_id=run.id AND exchange.execution_id=$3 LEFT JOIN agent_history_conversations AS conversation ON conversation.company_id=exchange.company_id AND conversation.id=exchange.conversation_id WHERE run.company_id=$1 AND run.id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .fetch_optional(&mut *db).await?.ok_or_else(missing)?;
    let Some(frozen) = facts.frozen_level else {
        return Ok(());
    };
    let content = AccessLevel::try_from(frozen.max(facts.content_level)).map_err(|_| invalid())?;
    let association = binding_rows::association_from_columns(facts.channel_id, facts.thread_id)?;
    let destination = match (facts.scope_kind.as_deref(), association) {
        (Some("thread"), RelatedAssociation::Thread { .. }) => {
            super::super::knowledge::thread_audience_on(db, scope.company, association).await?
        }
        (Some("private_session" | "child_branch"), _) => {
            super::super::knowledge::actor_level_on(db, scope.company, Some(facts.actor_id)).await?
        }
        _ => return Err(invalid()),
    };
    if destination < content {
        return Err(AppError::KnowledgeRelease {
            content,
            destination,
        });
    }
    Ok(())
}
