use super::*;
use sqlx::{Postgres, QueryBuilder};

#[derive(sqlx::FromRow)]
struct DirectoryRow {
    id: Uuid,
    key: String,
    name: String,
    active_revision: Option<Uuid>,
}

impl DirectoryRow {
    fn entry(self) -> AppResult<AgentDirectoryEntry> {
        Ok(AgentDirectoryEntry {
            id: AgentId::new(self.id),
            key: AgentKey::parse(self.key).ok_or_else(invalid)?,
            name: self.name,
            active_revision: self.active_revision.map(AgentRevisionId::new),
        })
    }
}

pub(super) async fn entry(
    db: &mut PgConnection,
    scope: AgentCompanyScope,
    key: &AgentKey,
) -> AppResult<Option<AgentDirectoryEntry>> {
    sqlx::query_as::<_, DirectoryRow>(
        "SELECT agent.id,agent.slug::text AS key,agent.name,active.revision_id AS active_revision FROM agents AS agent \
         LEFT JOIN agent_active_revisions AS active ON active.company_id=agent.company_id AND active.agent_id=agent.id \
         WHERE agent.company_id=$1 AND agent.slug=$2",
    ).bind(scope.company.as_uuid()).bind(key.as_str()).fetch_optional(db).await?
        .map(DirectoryRow::entry).transpose()
}

pub(super) async fn page(
    db: &mut PgConnection,
    scope: AgentCompanyScope,
    after: Option<AgentKey>,
) -> AppResult<AgentDirectoryPage> {
    let mut query = QueryBuilder::<Postgres>::new(
        "SELECT agent.id,agent.slug::text AS key,agent.name,active.revision_id AS active_revision FROM agents AS agent LEFT JOIN agent_active_revisions AS active ON active.company_id=agent.company_id AND active.agent_id=agent.id WHERE agent.company_id=",
    );
    query.push_bind(scope.company.as_uuid());
    if let Some(after) = after {
        query
            .push(" AND agent.slug>")
            .push_bind(after.as_str().to_owned());
    }
    query
        .push(" ORDER BY agent.slug LIMIT ")
        .push_bind((AGENT_CATALOG_PAGE + 1) as i64);
    let rows: Vec<DirectoryRow> = query.build_query_as().fetch_all(db).await?;
    let more = rows.len() > AGENT_CATALOG_PAGE;
    let items = rows
        .into_iter()
        .take(AGENT_CATALOG_PAGE)
        .map(DirectoryRow::entry)
        .collect::<AppResult<Vec<_>>>()?;
    let next = if more {
        items.last().map(|item| item.key.clone())
    } else {
        None
    };
    Ok(AgentDirectoryPage { items, next })
}
