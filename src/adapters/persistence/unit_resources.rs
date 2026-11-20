//! Moving a unit-owned skill or connection (Plan07 01B.4), and what a department still owns.
use sqlx::PgConnection;
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    application::{
        ports::{
            administration::ManagementTarget,
            unit_resources::{
                MAX_LISTED_RESOURCE_USES, MovedResource, ResourceUse, ResourceUseKind,
                UnitResourceKind,
            },
        },
        workflow::WorkflowActor,
    },
    domain::workflow::CompanyId,
    entities::department::DepartmentId,
};

use super::administration::management_role_on;

fn not_found(kind: UnitResourceKind) -> AppError {
    AppError::NotFound(
        match kind {
            UnitResourceKind::Skill => "Skill not found in this company.",
            UnitResourceKind::McpConnection => "MCP connection not found",
            UnitResourceKind::HttpConnection => "HTTP connection not found",
        }
        .into(),
    )
}

fn live_rows(kind: UnitResourceKind) -> &'static str {
    match kind {
        UnitResourceKind::Skill => "skills WHERE company_id=$1 AND id=$2",
        UnitResourceKind::McpConnection => {
            "company_mcp_connections WHERE company_id=$1 AND id=$2 AND deleted_at IS NULL"
        }
        UnitResourceKind::HttpConnection => {
            "workflow_http_connections WHERE company_id=$1 AND id=$2"
        }
    }
}

/// Configuration authority over one live skill or connection (Plan07, user decision a): the
/// person locks of company authority (which a move also takes, so the unit cannot change
/// underneath), then management of the resource's unit. A refusal is the resource's not-found.
pub(crate) async fn authorize_resource_on(
    db: &mut PgConnection,
    actor: WorkflowActor,
    company: CompanyId,
    kind: UnitResourceKind,
    id: Uuid,
) -> AppResult<crate::entities::access_level::LevelActor> {
    let level = super::workflow::authority::authorize_person_with_locks(
        db,
        company,
        actor,
        super::workflow::row_locks::RowLocks::Wait,
    )
    .await?;
    let unit: Option<Uuid> =
        sqlx::query_scalar::<_, Option<Uuid>>(&format!("SELECT unit_id FROM {}", live_rows(kind)))
            .bind(company.as_uuid())
            .bind(id)
            .fetch_optional(&mut *db)
            .await?
            .flatten();
    let unit = DepartmentId::new(unit.ok_or_else(|| not_found(kind))?);
    management_role_on(db, actor.user_id(), company, ManagementTarget::Unit(unit))
        .await?
        .ok_or_else(|| not_found(kind))?;
    Ok(level)
}

/// Requires managing the resource's current unit and the target, under the company and person
/// locks and the resource's row lock. Changes only `unit_id`: revisions stay, so forms, pending
/// actions and published snapshots are untouched (G1 r2 F6).
pub(crate) async fn move_resource_on(
    db: &mut PgConnection,
    actor: WorkflowActor,
    company: CompanyId,
    kind: UnitResourceKind,
    id: Uuid,
    target: DepartmentId,
) -> AppResult<MovedResource> {
    super::workflow::authority::authorize_person_with_locks(
        db,
        company,
        actor,
        super::workflow::row_locks::RowLocks::Wait,
    )
    .await?;
    let current: Option<Uuid> = sqlx::query_scalar::<_, Option<Uuid>>(&format!(
        "SELECT unit_id FROM {} FOR UPDATE",
        live_rows(kind)
    ))
    .bind(company.as_uuid())
    .bind(id)
    .fetch_optional(&mut *db)
    .await?
    .flatten();
    let current = DepartmentId::new(current.ok_or_else(|| not_found(kind))?);
    for unit in [current, target] {
        management_role_on(db, actor.user_id(), company, ManagementTarget::Unit(unit))
            .await?
            .ok_or_else(|| not_found(kind))?;
    }
    if current != target {
        let table = match kind {
            UnitResourceKind::Skill => "skills",
            UnitResourceKind::McpConnection => "company_mcp_connections",
            UnitResourceKind::HttpConnection => "workflow_http_connections",
        };
        sqlx::query(&format!(
            "UPDATE {table} SET unit_id=$3 WHERE company_id=$1 AND id=$2"
        ))
        .bind(company.as_uuid())
        .bind(id)
        .bind(target.as_uuid())
        .execute(&mut *db)
        .await?;
    }
    let (uses, more_uses) = uses_on(db, actor, company, kind, id).await?;
    Ok(MovedResource {
        from: current,
        to: target,
        uses,
        more_uses,
    })
}

#[derive(sqlx::FromRow)]
struct UseRow {
    id: Uuid,
    name: String,
    managed: bool,
    total: i64,
}

/// Current uses: agents whose active revision uses the resource and, for a connection, active
/// bindings whose current configuration maps a slot to it. Only uses the mover manages are
/// named; the others are counted (B1-F5).
async fn uses_on(
    db: &mut PgConnection,
    actor: WorkflowActor,
    company: CompanyId,
    kind: UnitResourceKind,
    id: Uuid,
) -> AppResult<(Vec<ResourceUse>, u64)> {
    let agents = match kind {
        UnitResourceKind::Skill => {
            "SELECT DISTINCT agent.id, agent.name, agent_management_role($3,$1,agent.id) IS NOT NULL AS managed FROM agent_active_revisions AS active
             JOIN agent_revision_skills AS used ON used.company_id=active.company_id
                AND used.revision_id=active.revision_id
             JOIN agents AS agent ON agent.company_id=active.company_id AND agent.id=active.agent_id
             WHERE active.company_id=$1 AND used.skill_id=$2"
        }
        UnitResourceKind::McpConnection => {
            "SELECT DISTINCT agent.id, agent.name, agent_management_role($3,$1,agent.id) IS NOT NULL AS managed FROM agent_active_revisions AS active
             JOIN agent_revision_bindings AS used ON used.company_id=active.company_id
                AND used.revision_id=active.revision_id
             JOIN agent_capability_bindings AS binding ON binding.company_id=used.company_id
                AND binding.agent_id=used.agent_id AND binding.id=used.binding_id
             JOIN agents AS agent ON agent.company_id=active.company_id AND agent.id=active.agent_id
             WHERE active.company_id=$1 AND binding.mcp_connection_id=$2"
        }
        UnitResourceKind::HttpConnection => {
            "SELECT DISTINCT agent.id, agent.name, agent_management_role($3,$1,agent.id) IS NOT NULL AS managed FROM agent_active_revisions AS active
             JOIN agent_revision_bindings AS used ON used.company_id=active.company_id
                AND used.revision_id=active.revision_id
             JOIN agent_capability_bindings AS binding ON binding.company_id=used.company_id
                AND binding.agent_id=used.agent_id AND binding.id=used.binding_id
             JOIN agents AS agent ON agent.company_id=active.company_id AND agent.id=active.agent_id
             WHERE active.company_id=$1 AND binding.http_connection_id=$2"
        }
    };
    let mut uses = listed(db, actor, company, id, agents, ResourceUseKind::Agent).await?;
    if kind != UnitResourceKind::Skill {
        let bindings = listed(
            db,
            actor,
            company,
            id,
            "SELECT binding.id, definition.title AS name, \
                workflow_definition_management_role($3,$1,definition.id) IS NOT NULL AS managed \
             FROM workflow_bindings AS binding
             JOIN workflow_binding_revisions AS revision ON revision.company_id=binding.company_id
                AND revision.binding_id=binding.id AND revision.revision=binding.configuration_revision
             JOIN workflow_versions AS version ON version.company_id=revision.company_id
                AND version.id=revision.version_id
             JOIN workflow_definitions AS definition ON definition.company_id=version.company_id
                AND definition.id=version.workflow_id
             WHERE binding.company_id=$1 AND binding.active
                AND EXISTS (SELECT 1 FROM jsonb_each_text(revision.resources) AS slot
                            WHERE slot.value=$2::text)",
            ResourceUseKind::Binding,
        )
        .await?;
        uses.0.extend(bindings.0);
        uses.1 += bindings.1;
    }
    let (mut listed, total) = uses;
    listed.truncate(MAX_LISTED_RESOURCE_USES);
    let more = total.saturating_sub(listed.len() as u64);
    Ok((listed, more))
}

/// The managed uses (at most the listing bound) and the count of all uses.
async fn listed(
    db: &mut PgConnection,
    actor: WorkflowActor,
    company: CompanyId,
    id: Uuid,
    query: &str,
    kind: ResourceUseKind,
) -> AppResult<(Vec<ResourceUse>, u64)> {
    let rows: Vec<UseRow> = sqlx::query_as(&format!(
        "SELECT id, name, managed, count(*) OVER () AS total FROM ({query}) AS used \
         ORDER BY managed DESC, name, id LIMIT {}",
        MAX_LISTED_RESOURCE_USES
    ))
    .bind(company.as_uuid())
    .bind(id)
    .bind(actor.user_id())
    .fetch_all(&mut *db)
    .await?;
    let total = rows.first().map_or(0, |row| row.total.max(0) as u64);
    Ok((
        rows.into_iter()
            .filter(|row| row.managed)
            .map(|row| ResourceUse {
                kind,
                id: row.id,
                name: row.name,
            })
            .collect(),
        total,
    ))
}

/// What a department still owns, named for its delete refusal (Plan07 01A.3): at most `limit`
/// names and the total.
pub(crate) async fn owned_resources_on(
    db: &mut PgConnection,
    company: Uuid,
    unit: Uuid,
    limit: i64,
) -> AppResult<(Vec<String>, i64)> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT name, count(*) OVER () FROM (
            SELECT 'skill ' || slug::text AS name FROM skills WHERE company_id=$1 AND unit_id=$2
            UNION ALL SELECT 'MCP connection ' || slug FROM company_mcp_connections
                WHERE company_id=$1 AND unit_id=$2
            UNION ALL SELECT 'HTTP connection ' || origin FROM workflow_http_connections
                WHERE company_id=$1 AND unit_id=$2
         ) AS owned ORDER BY name LIMIT $3",
    )
    .bind(company)
    .bind(unit)
    .bind(limit)
    .fetch_all(db)
    .await?;
    let total = rows.first().map_or(0, |row| row.1);
    Ok((rows.into_iter().map(|row| row.0).collect(), total))
}
