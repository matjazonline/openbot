//! Exact immutable replay material, with live pre-provider audience checks.
use super::*;
use crate::application::workflow::agent::PreparedAgent;
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
    // The frozen capture flag (E8, U7) and recall pack are bound before history, as when they
    // were frozen (E7).
    let prepared = prepared.with_capture(saved.get("capture") == Some(&Value::Bool(true)));
    let prepared = match saved.get("memory") {
        Some(memory) => {
            prepared.with_memory(serde_json::from_value(memory.clone()).map_err(|_| invalid())?)?
        }
        None => prepared,
    };
    let history: FrozenAgentHistory =
        serde_json::from_value(saved["history"].clone()).map_err(|_| invalid())?;
    let prepared = prepared.with_frozen_history(history)?;
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
    recall: Option<crate::application::workflow::memory_recall::FetchedRecall>,
    capture: bool,
) -> AppResult<PreparedAgent> {
    if let Some(saved) = restore_on(db, fence, prepared.clone()).await? {
        // An existing freeze wins; a pack fetched for this attempt is discarded unseen (E7), and
        // its capture flag stays what the first freeze decided (U7).
        return Ok(saved);
    }
    let prepared = prepared.with_capture(capture);
    let prepared = match recall {
        Some(fetched) if prepared.binds_memory() => {
            prepared.with_memory(super::recall::freeze_memory_on(db, fence, fetched).await?)?
        }
        _ => prepared,
    };
    let scope = action(fence);
    let selected = Box::pin(super::selection::select_on(db, scope, &prepared)).await?;
    let history = selected.history;
    // An inexact earlier exchange lowers the reading run (E3).
    super::super::knowledge::absorb_content_on(
        db,
        scope.company,
        scope.run,
        history.selected_level,
        selected.exact,
    )
    .await?;
    let prepared = prepared.with_frozen_history(history)?;
    let document = serde_json::to_value(&prepared).map_err(|_| invalid())?;
    let changed = sqlx::query("UPDATE agent_history_exchanges AS exchange SET frozen_preparation=$4,access_level=GREATEST(exchange.access_level,run.content_level),content_exact=exchange.content_exact AND run.content_exact FROM workflow_runs AS run WHERE exchange.company_id=$1 AND exchange.run_id=$2 AND exchange.execution_id=$3 AND exchange.frozen_preparation IS NULL AND run.company_id=exchange.company_id AND run.id=exchange.run_id")
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
