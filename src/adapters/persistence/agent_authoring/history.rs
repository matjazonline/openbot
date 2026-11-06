use super::*;
use chrono::{DateTime, Utc};

#[derive(sqlx::FromRow)]
struct HistoryRow {
    id: Uuid,
    draft_revision: i64,
    created_at: DateTime<Utc>,
    digest: Vec<u8>,
}

pub(super) async fn page(
    db: &mut PgConnection,
    scope: AgentPublicationScope,
    before: Option<AgentHistoryCursor>,
) -> AppResult<AgentHistoryPage> {
    // Separate shapes retain an index range condition for prepared cursor queries.
    let rows:Vec<HistoryRow>=match before {
        Some(cursor)=>sqlx::query_as("SELECT id,draft_revision,created_at,digest FROM agent_revisions WHERE company_id=$1 AND agent_id=$2 AND (created_at,id)<($3,$4) ORDER BY created_at DESC,id DESC LIMIT 21")
            .bind(scope.company.as_uuid()).bind(scope.agent.as_uuid()).bind(cursor.created_at).bind(cursor.id.as_uuid()).fetch_all(&mut *db).await?,
        None=>sqlx::query_as("SELECT id,draft_revision,created_at,digest FROM agent_revisions WHERE company_id=$1 AND agent_id=$2 ORDER BY created_at DESC,id DESC LIMIT 21")
            .bind(scope.company.as_uuid()).bind(scope.agent.as_uuid()).fetch_all(&mut *db).await?,
    };
    let more = rows.len() > AGENT_HISTORY_PAGE;
    let items = rows
        .into_iter()
        .take(AGENT_HISTORY_PAGE)
        .map(|row| {
            if row.digest.len() != 32 {
                return Err(invalid());
            }
            Ok(AgentHistoryEntry {
                id: AgentRevisionId::new(row.id),
                draft_revision: draft_revision(row.draft_revision)?,
                created_at: row.created_at,
                digest: row
                    .digest
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    let next = if more {
        items.last().map(|item| AgentHistoryCursor {
            created_at: item.created_at,
            id: item.id,
        })
    } else {
        None
    };
    Ok(AgentHistoryPage { items, next })
}

pub(super) fn draft_revision(value: i64) -> AppResult<AgentDraftRevision> {
    AgentDraftRevision::new(u64::try_from(value).map_err(|_| invalid())?).map_err(|_| invalid())
}
