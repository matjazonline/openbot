//! Payload fetches precede a fresh, short native authority projection transaction.
use super::*;
use crate::application::workflow::{
    agent_history::*, agent_history_views::*, run_views::RunReadScope,
};
use crate::entities::access_level::AccessLevel;
use serde_json::Value;
use sqlx::{Postgres, Transaction};
mod projection;
mod rows;
#[cfg(test)]
pub(crate) mod testing;
use rows::*;

struct AuthorizedAnchor {
    anchor: Anchor,
    level: AccessLevel,
}

async fn anchor_on(
    db: &mut PgConnection,
    scope: RunReadScope,
    execution: Uuid,
) -> AppResult<Option<AuthorizedAnchor>> {
    // Pin the current manager role/level and association using the same native Inspect policy.
    // These bounded locks are acquired only after payload reads on the final projection pass.
    let actor = authority::authorize_company(db, scope.company, scope.viewer).await?;
    let row: Option<Anchor> = sqlx::query_as(
        "SELECT exchange.conversation_id,conversation.expired_count,conversation.expired_through_order,run.channel_id,run.thread_id
        FROM workflow_runs AS run JOIN agent_history_exchanges AS exchange
          ON exchange.company_id=run.company_id AND exchange.run_id=run.id
        JOIN agent_history_conversations AS conversation ON conversation.company_id=exchange.company_id AND conversation.id=exchange.conversation_id
        WHERE run.company_id=$1 AND run.id=$2 AND exchange.execution_id=$3",
    ).bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(execution).fetch_optional(&mut *db).await?;
    let Some(anchor) = row else {
        return Ok(None);
    };
    let association = binding_rows::association_from_columns(anchor.channel_id, anchor.thread_id)?;
    association::authorize_association(db, scope.company, scope.viewer, association).await?;
    Ok(Some(AuthorizedAnchor {
        anchor,
        level: actor.access_level,
    }))
}

async fn source_allowed(
    db: &mut PgConnection,
    scope: RunReadScope,
    row: &Metadata,
) -> AppResult<bool> {
    let association = binding_rows::association_from_columns(row.channel_id, row.thread_id)?;
    match association::authorize_association(db, scope.company, scope.viewer, association).await {
        Ok(()) => Ok(true),
        Err(AppError::NotFound(_)) => Ok(false),
        Err(error) => Err(error),
    }
}

fn validate_cursor(request: &HistoryPageRequest, anchor: &Anchor) -> AppResult<()> {
    if !(1..=MAX_HISTORY_PAGE).contains(&request.limit) {
        return Err(AppError::BadRequest(
            "Invalid agent history page size".into(),
        ));
    }
    if let Some(cursor) = &request.after
        && cursor.parts()?.0.as_uuid() != anchor.conversation_id
    {
        return Err(AppError::NotFound("Agent history cursor".into()));
    }
    Ok(())
}

pub(super) async fn page(
    p: &PostgresPersistence,
    scope: RunReadScope,
    execution: Uuid,
    request: HistoryPageRequest,
) -> AppResult<Option<HistoryPage>> {
    optional(page_inner(p, scope, execution, request).await)
}
fn optional<T>(result: AppResult<Option<T>>) -> AppResult<Option<T>> {
    match result {
        Err(AppError::NotFound(_)) => Ok(None),
        other => other,
    }
}

async fn page_inner(
    p: &PostgresPersistence,
    scope: RunReadScope,
    execution: Uuid,
    request: HistoryPageRequest,
) -> AppResult<Option<HistoryPage>> {
    let mut tx = super::storage::begin(p.pool()).await?;
    let Some(initial) = anchor_on(&mut tx, scope, execution).await? else {
        return Ok(None);
    };
    validate_cursor(&request, &initial.anchor)?;
    let mut ids = page_ids_on(
        &mut tx,
        scope.company,
        initial.anchor.conversation_id,
        &request,
    )
    .await?;
    let more = ids.len() > usize::from(request.limit);
    ids.truncate(usize::from(request.limit));
    tx.commit().await?;
    let mut tx = super::storage::begin(p.pool()).await?;
    let Some(current) = anchor_on(&mut tx, scope, execution).await? else {
        return Ok(None);
    };
    if current.anchor.conversation_id != initial.anchor.conversation_id {
        return Err(projection::changed());
    }
    let rows = metadata_on(&mut tx, scope.company, current.anchor.conversation_id, &ids).await?;
    if rows.len() != ids.len() {
        return Err(projection::changed());
    }
    let next = if more {
        rows.last()
            .map(|row| {
                HistoryCursor::after(
                    AgentConversationId::new(current.anchor.conversation_id),
                    row.position,
                )
            })
            .transpose()?
    } else {
        None
    };
    let mut entries = Vec::new();
    for row in rows {
        if source_allowed(&mut tx, scope, &row).await? {
            entries.push(projection::entry(&row, current.level)?);
        }
    }
    tx.commit().await?;
    Ok(Some(HistoryPage {
        entries,
        next,
        expired_count: current.anchor.expired_count,
        expired_through_order: current.anchor.expired_through_order,
    }))
}

pub(super) async fn detail(
    p: &PostgresPersistence,
    scope: RunReadScope,
    execution: Uuid,
    exchange: AgentExchangeId,
) -> AppResult<Option<HistoryDetail>> {
    optional(detail_inner(p, scope, execution, exchange).await)
}

async fn detail_inner(
    p: &PostgresPersistence,
    scope: RunReadScope,
    execution: Uuid,
    exchange: AgentExchangeId,
) -> AppResult<Option<HistoryDetail>> {
    let mut tx = super::storage::begin(p.pool()).await?;
    let Some(initial) = anchor_on(&mut tx, scope, execution).await? else {
        return Ok(None);
    };
    let initial_rows = metadata_on(
        &mut tx,
        scope.company,
        initial.anchor.conversation_id,
        &[exchange.as_uuid()],
    )
    .await?;
    let Some(initial_row) = initial_rows.first() else {
        return Ok(None);
    };
    if !source_allowed(&mut tx, scope, initial_row).await? {
        return Ok(None);
    }
    let initially_visible =
        projection::entry(initial_row, initial.level)?.payload == HistoryPayloadState::Retained;
    tx.commit().await?;
    let payload = if initially_visible {
        payload_on(p, scope.company, exchange.as_uuid()).await?
    } else {
        None
    };
    #[cfg(test)]
    testing::after_payload(scope.company).await;
    let mut tx = super::storage::begin(p.pool()).await?;
    let Some(current) = anchor_on(&mut tx, scope, execution).await? else {
        return Ok(None);
    };
    if current.anchor.conversation_id != initial.anchor.conversation_id {
        return Err(projection::changed());
    }
    let rows = metadata_on(
        &mut tx,
        scope.company,
        current.anchor.conversation_id,
        &[exchange.as_uuid()],
    )
    .await?;
    let Some(row) = rows.first() else {
        return Ok(None);
    };
    if !source_allowed(&mut tx, scope, row).await? {
        return Ok(None);
    }
    let mut detail = projection::detail(row, current.level, payload)?;
    if let Some(payload) = &mut detail.payload
        && let Some(frozen) = &payload.frozen
    {
        payload.selected_sources =
            selected_sources(&mut tx, scope, &current, &frozen.history).await?;
    }
    tx.commit().await?;
    Ok(Some(detail))
}

async fn selected_sources(
    tx: &mut Transaction<'_, Postgres>,
    scope: RunReadScope,
    anchor: &AuthorizedAnchor,
    history: &FrozenAgentHistory,
) -> AppResult<Vec<HistorySelectedSource>> {
    let ids: Vec<_> = history
        .selected
        .iter()
        .map(|selected| selected.id.as_uuid())
        .collect();
    let rows = metadata_on(tx, scope.company, anchor.anchor.conversation_id, &ids).await?;
    let mut sources = Vec::new();
    for selected in &history.selected {
        let Some(row) = rows.iter().find(|row| row.id == selected.id.as_uuid()) else {
            sources.push(HistorySelectedSource::Expired {
                exchange: selected.id,
            });
            continue;
        };
        let entry = projection::entry(row, anchor.level)?;
        let source = if source_allowed(tx, scope, row).await? {
            entry.source
        } else {
            None
        };
        sources.push(match source {
            None => HistorySelectedSource::Unavailable {
                exchange: selected.id,
            },
            Some(_) if !row.payload_retained => HistorySelectedSource::Expired {
                exchange: selected.id,
            },
            Some(source) => HistorySelectedSource::Available {
                exchange: selected.id,
                source,
            },
        });
    }
    Ok(sources)
}
