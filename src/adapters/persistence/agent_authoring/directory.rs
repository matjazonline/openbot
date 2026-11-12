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
            departments: Vec::new(),
            desk: None,
        })
    }
}

/// Each entry's departments and desk address (01DEP-P03-D10).
async fn placed(
    db: &mut PgConnection,
    scope: AgentCompanyScope,
    domain: Option<&str>,
    entries: &mut [AgentDirectoryEntry],
) -> AppResult<()> {
    use crate::adapters::persistence::department_roster::{
        DeskAddresses, agent_departments_on, agent_desks_on,
    };
    let ids: Vec<Uuid> = entries.iter().map(|entry| entry.id.as_uuid()).collect();
    let mut departments = agent_departments_on(db, scope.company, &ids).await?;
    let mut addresses = DeskAddresses::new(scope.company, domain);
    let mut desks = agent_desks_on(db, &mut addresses, &ids).await?;
    for entry in entries {
        let id = entry.id.as_uuid();
        entry.departments = departments
            .remove(&id)
            .map(|(listed, _)| listed)
            .unwrap_or_default()
            .into_iter()
            .map(|department| DepartmentRef {
                id: department.id,
                name: department.name,
                slug: department.slug,
                charter: department.charter,
                roles: department.roles,
            })
            .collect();
        entry.desk = desks.remove(&id).map(|desk| AgentDesk {
            id: desk.id,
            address: desk.address,
        });
    }
    Ok(())
}

pub(super) async fn entry(
    db: &mut PgConnection,
    scope: AgentCompanyScope,
    domain: Option<&str>,
    key: &AgentKey,
) -> AppResult<Option<AgentDirectoryEntry>> {
    let entry = sqlx::query_as::<_, DirectoryRow>(
        "SELECT agent.id,agent.slug::text AS key,agent.name,active.revision_id AS active_revision FROM agents AS agent \
         LEFT JOIN agent_active_revisions AS active ON active.company_id=agent.company_id AND active.agent_id=agent.id \
         WHERE agent.company_id=$1 AND agent.slug=$2",
    ).bind(scope.company.as_uuid()).bind(key.as_str()).fetch_optional(&mut *db).await?
        .map(DirectoryRow::entry).transpose()?;
    let mut entries: Vec<AgentDirectoryEntry> = entry.into_iter().collect();
    placed(db, scope, domain, &mut entries).await?;
    Ok(entries.pop())
}

pub(super) async fn page(
    db: &mut PgConnection,
    scope: AgentCompanyScope,
    domain: Option<&str>,
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
    let rows: Vec<DirectoryRow> = query.build_query_as().fetch_all(&mut *db).await?;
    let more = rows.len() > AGENT_CATALOG_PAGE;
    let mut items = rows
        .into_iter()
        .take(AGENT_CATALOG_PAGE)
        .map(DirectoryRow::entry)
        .collect::<AppResult<Vec<_>>>()?;
    placed(db, scope, domain, &mut items).await?;
    let next = if more {
        items.last().map(|item| item.key.clone())
    } else {
        None
    };
    Ok(AgentDirectoryPage { items, next })
}
