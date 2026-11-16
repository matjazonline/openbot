//! Departments as an agent sees them (01-departments p03 B3): the departments an agent belongs to
//! and its direct address, for the directories (DEP-P03-D10), and the department context of a run on a
//! department-owned desk (DEP-P03-D11). Plain reads on the caller's transaction: they take no
//! lock and decide nothing.
//!
//! Agent addresses come from live principal addresses; department desks retain their explicit
//! department provisioning record. Both render their current primary slug.
use std::collections::HashMap;

use sqlx::PgConnection;
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    application::workflow::agent::{
        DepartmentContext, DepartmentRoster, MAX_ROSTER_AGENTS, MAX_ROSTER_CHILDREN, RosterAgent,
        RosterDepartment, RosterDesk,
    },
    domain::workflow::CompanyId,
    entities::{
        channel::Channel,
        department::DepartmentRole,
        value_objects::{ChannelSlug, CompanySlug},
    },
};

/// The most departments one agent lists, by slug.
pub(crate) const MAX_AGENT_DEPARTMENTS: usize = 16;

const AGENT_ADDRESS: &str = "LEFT JOIN principals AS agent_principal \
     ON agent_principal.company_id=agent.company_id AND agent_principal.agent_id=agent.id AND agent_principal.kind='agent' \
     LEFT JOIN addresses AS agent_address ON agent_address.company_id=agent_principal.company_id \
     AND agent_address.principal_id=agent_principal.id AND agent_address.is_primary";

const DEPARTMENT_DESK: &str = "LEFT JOIN workflow_provisioning_commands AS desk_record \
     ON desk_record.company_id = department.company_id \
     AND desk_record.command_key = 'department:' || department.id::text \
     LEFT JOIN channels AS desk ON desk.company_id = department.company_id \
     AND desk.id = (desk_record.result->'channel'->>'id')::uuid AND desk.department_id = department.id \
     LEFT JOIN addresses AS desk_slug ON desk_slug.company_id = desk.company_id \
     AND desk_slug.channel_id = desk.id AND desk_slug.is_primary";

/// Forms desk addresses under the company's slug and the app domain, read and required only when
/// a desk is listed.
pub(crate) struct CompanyAddresses<'a> {
    company: CompanyId,
    domain: Option<&'a str>,
    company_slug: Option<CompanySlug>,
}

impl<'a> CompanyAddresses<'a> {
    pub(crate) fn new(company: CompanyId, domain: Option<&'a str>) -> Self {
        Self {
            company,
            domain,
            company_slug: None,
        }
    }

    async fn address(&mut self, db: &mut PgConnection, slug: String) -> AppResult<String> {
        let domain = self
            .domain
            .ok_or_else(|| AppError::Internal("The app domain is not configured".into()))?;
        if self.company_slug.is_none() {
            let slug: String = sqlx::query_scalar("SELECT slug::text FROM companies WHERE id=$1")
                .bind(self.company.as_uuid())
                .fetch_one(&mut *db)
                .await?;
            self.company_slug = Some(CompanySlug::from(slug));
        }
        let company = self.company_slug.as_ref().expect("loaded above");
        Ok(Channel::address_for(&ChannelSlug::from(slug), company, domain).to_string())
    }

    async fn desk(
        &mut self,
        db: &mut PgConnection,
        id: Option<Uuid>,
        slug: Option<String>,
    ) -> AppResult<Option<RosterDesk>> {
        let (Some(id), Some(slug)) = (id, slug) else {
            return Ok(None);
        };
        Ok(Some(RosterDesk {
            id,
            address: self.address(db, slug).await?,
        }))
    }

    async fn agent(
        &mut self,
        db: &mut PgConnection,
        principal: Option<Uuid>,
        slug: Option<String>,
    ) -> AppResult<Option<crate::application::agent::AgentAddress>> {
        let (Some(principal), Some(slug)) = (principal, slug) else {
            return Ok(None);
        };
        Ok(Some(crate::application::agent::AgentAddress {
            principal_id: crate::entities::transport::PrincipalId::new(principal),
            address: self.address(db, slug).await?,
        }))
    }
}

/// The canonical principal addresses of the selected company agents.
pub(crate) async fn agent_addresses_on(
    db: &mut PgConnection,
    addresses: &mut CompanyAddresses<'_>,
    agents: &[Uuid],
) -> AppResult<HashMap<Uuid, crate::application::agent::AgentAddress>> {
    let rows: Vec<(Uuid, Uuid, String)> = sqlx::query_as(&format!(
        "SELECT agent.id, agent_principal.id, agent_address.slug::text FROM agents AS agent {AGENT_ADDRESS} \
         WHERE agent.company_id = $1 AND agent.id = ANY($2) AND agent_address.principal_id IS NOT NULL"
    ))
    .bind(addresses.company.as_uuid())
    .bind(agents)
    .fetch_all(&mut *db)
    .await?;
    let mut desks = HashMap::with_capacity(rows.len());
    for (agent, principal, slug) in rows {
        if let Some(address) = addresses.agent(db, Some(principal), Some(slug)).await? {
            desks.insert(agent, address);
        }
    }
    Ok(desks)
}

/// Agent, department id, slug, name, charter and the agent's roles there.
type PlacedRow = (Uuid, Uuid, String, String, Option<String>, Vec<String>);

/// A department an agent is a member of or holds a role in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentDepartment {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub charter: Option<String>,
    pub roles: Vec<DepartmentRole>,
}

/// Each agent's departments by slug, at most [`MAX_AGENT_DEPARTMENTS`], and whether it has more.
pub(crate) async fn agent_departments_on(
    db: &mut PgConnection,
    company: CompanyId,
    agents: &[Uuid],
) -> AppResult<HashMap<Uuid, (Vec<AgentDepartment>, bool)>> {
    let rows: Vec<PlacedRow> = sqlx::query_as(
        "WITH placed AS ( \
           SELECT principal.agent_id, member.department_id FROM principals AS principal \
           JOIN department_members AS member ON member.company_id = principal.company_id \
             AND member.principal_id = principal.id \
           WHERE principal.company_id = $1 AND principal.agent_id = ANY($2) \
           UNION \
           SELECT principal.agent_id, role.department_id FROM principals AS principal \
           JOIN department_roles AS role ON role.company_id = principal.company_id \
             AND role.principal_id = principal.id \
           WHERE principal.company_id = $1 AND principal.agent_id = ANY($2)) \
         SELECT placed.agent_id, department.id, department.slug::text, department.name, \
           department.charter, ARRAY(SELECT role.role FROM department_roles AS role \
             JOIN principals AS principal ON principal.company_id = role.company_id \
               AND principal.id = role.principal_id \
             WHERE role.company_id = $1 AND role.department_id = department.id \
               AND principal.agent_id = placed.agent_id ORDER BY role.role) \
         FROM placed JOIN departments AS department ON department.company_id = $1 \
           AND department.id = placed.department_id \
         ORDER BY placed.agent_id, department.slug",
    )
    .bind(company.as_uuid())
    .bind(agents)
    .fetch_all(&mut *db)
    .await?;
    let mut departments: HashMap<Uuid, (Vec<AgentDepartment>, bool)> = HashMap::new();
    for (agent, id, slug, name, charter, roles) in rows {
        let (listed, more) = departments.entry(agent).or_default();
        if listed.len() == MAX_AGENT_DEPARTMENTS {
            *more = true;
            continue;
        }
        listed.push(AgentDepartment {
            id,
            slug,
            name,
            charter,
            roles: roles
                .iter()
                .map(|role| role.parse().map_err(|_| stored_role(role)))
                .collect::<AppResult<_>>()?,
        });
    }
    Ok(departments)
}

fn stored_role(role: &str) -> AppError {
    AppError::Internal(format!("Stored department role '{role}' is unknown"))
}

#[derive(sqlx::FromRow)]
struct DepartmentRow {
    id: Uuid,
    name: String,
    slug: String,
    desk_id: Option<Uuid>,
    desk_slug: Option<String>,
}

async fn departments_on(
    db: &mut PgConnection,
    addresses: &mut CompanyAddresses<'_>,
    filter: &str,
    key: Uuid,
    limit: usize,
) -> AppResult<Vec<(Uuid, RosterDepartment)>> {
    let rows: Vec<DepartmentRow> = sqlx::query_as(&format!(
        "SELECT department.id, department.name, department.slug::text AS slug, \
           desk.id AS desk_id, desk_slug.slug::text AS desk_slug \
         FROM departments AS department {DEPARTMENT_DESK} \
         WHERE department.company_id = $1 AND department.{filter} = $2 \
         ORDER BY department.slug LIMIT $3"
    ))
    .bind(addresses.company.as_uuid())
    .bind(key)
    .bind(limit as i64)
    .fetch_all(&mut *db)
    .await?;
    let mut departments = Vec::with_capacity(rows.len());
    for row in rows {
        let desk = addresses.desk(db, row.desk_id, row.desk_slug).await?;
        departments.push((
            row.id,
            RosterDepartment {
                name: row.name,
                slug: row.slug,
                desk,
            },
        ));
    }
    Ok(departments)
}

#[derive(sqlx::FromRow)]
struct OwnerRow {
    id: Uuid,
    name: String,
    slug: String,
    charter: Option<String>,
    parent_id: Option<Uuid>,
    access_mode: String,
    lead: bool,
}

/// The context of a run on `channel` by `agent`, when a department owns the channel.
pub(crate) async fn department_context_on(
    db: &mut PgConnection,
    company: CompanyId,
    domain: Option<&str>,
    channel: Uuid,
    agent: Uuid,
) -> AppResult<Option<DepartmentContext>> {
    let owner: Option<OwnerRow> = sqlx::query_as(
        "SELECT department.id, department.name, department.slug::text AS slug, department.charter, \
           department.parent_id, channel.access_mode, \
           EXISTS (SELECT 1 FROM department_roles AS role JOIN principals AS principal \
             ON principal.company_id = role.company_id AND principal.id = role.principal_id \
             WHERE role.company_id = $1 AND role.department_id = department.id \
               AND role.role = 'lead' AND principal.agent_id = $3) AS lead \
         FROM channels AS channel JOIN departments AS department \
           ON department.company_id = channel.company_id AND department.id = channel.department_id \
         WHERE channel.company_id = $1 AND channel.id = $2",
    )
    .bind(company.as_uuid())
    .bind(channel)
    .bind(agent)
    .fetch_optional(&mut *db)
    .await?;
    let Some(owner) = owner else {
        return Ok(None);
    };
    let roster = if owner.access_mode == "public" {
        None
    } else {
        let mut addresses = CompanyAddresses::new(company, domain);
        Some(roster_on(db, &mut addresses, &owner).await?)
    };
    Ok(Some(DepartmentContext {
        name: owner.name,
        slug: owner.slug,
        charter: owner.charter,
        lead: owner.lead,
        roster,
    }))
}

/// An agent's name, slug, whether it leads, and its desk's id and primary slug.
type RosterRow = (String, String, bool, Option<Uuid>, Option<String>);

async fn roster_on(
    db: &mut PgConnection,
    addresses: &mut CompanyAddresses<'_>,
    owner: &OwnerRow,
) -> AppResult<DepartmentRoster> {
    let desk = departments_on(db, addresses, "id", owner.id, 1)
        .await?
        .pop()
        .and_then(|(_, own)| own.desk);
    let parent = match owner.parent_id {
        Some(parent) => departments_on(db, addresses, "id", parent, 1)
            .await?
            .pop()
            .map(|(_, parent)| parent),
        None => None,
    };
    let mut children: Vec<RosterDepartment> = departments_on(
        db,
        addresses,
        "parent_id",
        owner.id,
        MAX_ROSTER_CHILDREN + 1,
    )
    .await?
    .into_iter()
    .map(|(_, child)| child)
    .collect();
    let more_children = children.len() > MAX_ROSTER_CHILDREN;
    children.truncate(MAX_ROSTER_CHILDREN);
    let rows: Vec<RosterRow> = sqlx::query_as(&format!(
        "WITH placed AS ( \
               SELECT member.principal_id FROM department_members AS member \
               WHERE member.company_id = $1 AND member.department_id = $2 \
               UNION \
               SELECT role.principal_id FROM department_roles AS role \
               WHERE role.company_id = $1 AND role.department_id = $2) \
             SELECT agent.name, agent.slug::text, \
               EXISTS (SELECT 1 FROM department_roles AS role WHERE role.company_id = $1 \
                 AND role.department_id = $2 AND role.role = 'lead' \
                 AND role.principal_id = principal.id), \
               agent_principal.id, agent_address.slug::text \
             FROM placed JOIN principals AS principal ON principal.company_id = $1 \
               AND principal.id = placed.principal_id \
             JOIN agents AS agent ON agent.company_id = $1 AND agent.id = principal.agent_id \
             {AGENT_ADDRESS} \
             ORDER BY agent.slug LIMIT $3"
    ))
    .bind(addresses.company.as_uuid())
    .bind(owner.id)
    .bind((MAX_ROSTER_AGENTS + 1) as i64)
    .fetch_all(&mut *db)
    .await?;
    let more_agents = rows.len() > MAX_ROSTER_AGENTS;
    let mut agents = Vec::with_capacity(rows.len().min(MAX_ROSTER_AGENTS));
    for (name, slug, lead, principal_id, address_slug) in rows.into_iter().take(MAX_ROSTER_AGENTS) {
        agents.push(RosterAgent {
            name,
            slug,
            lead,
            address: addresses.agent(db, principal_id, address_slug).await?,
        });
    }
    Ok(DepartmentRoster {
        desk,
        parent,
        children,
        more_children,
        agents,
        more_agents,
    })
}
