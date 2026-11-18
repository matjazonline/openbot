//! The department context a run on a department-owned desk reads, against PostgreSQL
//! (01-departments p03 B3, 01DEP-P03-D11): `department_context_`.
use super::*;
use crate::adapters::persistence::department_roster::department_context_on;
use crate::application::workflow::agent::{
    DepartmentContext, MAX_ROSTER_AGENTS, MAX_ROSTER_CHILDREN, RosterDesk,
};

const DOMAIN: &str = "agents.test";

async fn agent_of(f: &Departments, principal: PrincipalId) -> Uuid {
    sqlx::query_scalar("SELECT agent_id FROM principals WHERE company_id = $1 AND id = $2")
        .bind(f.company)
        .bind(principal.as_uuid())
        .fetch_one(f.pool())
        .await
        .unwrap()
}

async fn principal_for_agent(f: &Departments, agent: Uuid) -> PrincipalId {
    PrincipalId::new(
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND agent_id=$2")
            .bind(f.company)
            .bind(agent)
            .fetch_one(f.pool())
            .await
            .unwrap(),
    )
}

async fn address(f: &Departments, slug: &str) -> String {
    let company: String = sqlx::query_scalar("SELECT slug::text FROM companies WHERE id = $1")
        .bind(f.company)
        .fetch_one(f.pool())
        .await
        .unwrap();
    format!("{slug}@{company}.{DOMAIN}")
}

async fn context(f: &Departments, channel: Uuid, agent: Uuid) -> Option<DepartmentContext> {
    let mut connection = f.pool().acquire().await.unwrap();
    department_context_on(
        &mut connection,
        CompanyId::new(f.company),
        Some(DOMAIN),
        channel,
        agent,
    )
    .await
    .unwrap()
}

/// 03C.1 (C003): the lead's run on its department's desk reads the department's name and
/// charter, that it is the lead, and the desks around it: its own, its parent's, its children's
/// and its agents' own desks. A member's run reads the same without the lead flag; a run on an
/// agent's own unowned desk reads none, and a public desk lists no desks.
#[tokio::test]
async fn department_context_roster_lists_the_desks_around_the_department() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let revenue = f.create("Revenue", None).await;
    let sales = f.create("Sales", Some(revenue.id)).await;
    let inside = f.create("Inside sales", Some(sales.id)).await;
    sqlx::query("UPDATE departments SET charter = 'Win and keep customers.' WHERE id = $1")
        .bind(sales.id.as_uuid())
        .execute(f.pool())
        .await
        .unwrap();
    let closer = company_agent(&f.persistence, f.company, f.owner, "closer").await;
    f.use_cases
        .assign_role(f.owner, f.company, sales.id, DepartmentRole::Lead, closer)
        .await
        .unwrap();
    f.use_cases
        .add_member(f.owner, f.company, sales.id, f.agent_principal)
        .await
        .unwrap();
    // People have no desk of their own and are not listed.
    f.use_cases
        .add_member(f.owner, f.company, sales.id, f.member_principal)
        .await
        .unwrap();
    let desk = f.desk_of(sales.id).await;
    let closer_agent = agent_of(&f, closer).await;
    let worker_agent = agent_of(&f, f.agent_principal).await;
    let lead = context(&f, desk, closer_agent)
        .await
        .expect("an owned desk");
    assert_eq!(lead.name, "Sales");
    assert_eq!(lead.slug, "sales");
    assert_eq!(lead.charter.as_deref(), Some("Win and keep customers."));
    assert!(lead.lead);
    let roster = lead.roster.clone().expect("a team desk lists its desks");
    let at = |id: Uuid, address: String| Some(RosterDesk { id, address });
    assert_eq!(roster.desk, at(desk, address(&f, "sales").await));
    let parent = roster.parent.expect("its parent");
    assert_eq!(
        (parent.name.as_str(), parent.slug.as_str()),
        ("Revenue", "revenue")
    );
    assert_eq!(
        parent.desk,
        at(f.desk_of(revenue.id).await, address(&f, "revenue").await)
    );
    assert_eq!(roster.children.len(), 1);
    assert_eq!(roster.children[0].slug, "inside-sales");
    assert_eq!(
        roster.children[0].desk,
        at(
            f.desk_of(inside.id).await,
            address(&f, "inside-sales").await
        )
    );
    assert!(!roster.more_children);
    let agents: Vec<(&str, bool, Option<crate::application::agent::AgentAddress>)> = roster
        .agents
        .iter()
        .map(|agent| (agent.slug.as_str(), agent.lead, agent.address.clone()))
        .collect();
    assert_eq!(
        agents,
        [
            (
                "closer",
                true,
                Some(crate::application::agent::AgentAddress {
                    principal_id: principal_for_agent(&f, closer_agent).await,
                    address: address(&f, "closer").await
                })
            ),
            (
                "worker",
                false,
                Some(crate::application::agent::AgentAddress {
                    principal_id: principal_for_agent(&f, worker_agent).await,
                    address: address(&f, "worker").await
                })
            ),
        ]
    );
    assert!(!roster.more_agents);

    let member = context(&f, desk, worker_agent).await.unwrap();
    assert!(!member.lead);
    assert_eq!(member.roster, lead.roster);
    // Default-created shared desks receive their root department context.
    let unowned = crate::adapters::persistence::test_support::create_channel(
        &f.persistence,
        f.company,
        crate::application::ports::channel::ChannelWrite {
            name: "Unowned".into(),
            slug: "unowned".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let root_context = context(&f, unowned.id, worker_agent)
        .await
        .expect("root owns every default desk");
    let root_name: String =
        sqlx::query_scalar("SELECT name FROM departments WHERE company_id=$1 AND is_root")
            .bind(f.company)
            .fetch_one(f.pool())
            .await
            .unwrap();
    assert_eq!(root_context.name, root_name);
    assert!(!root_context.lead);

    sqlx::query("UPDATE channels SET access_mode = 'public' WHERE id = $1")
        .bind(desk)
        .execute(f.pool())
        .await
        .unwrap();
    let public = context(&f, desk, closer_agent).await.unwrap();
    assert!(public.lead);
    assert_eq!(public.charter, lead.charter);
    assert_eq!(public.roster, None);
}

/// The roster lists at most its bound of children and agents, by slug, and says there are more.
#[tokio::test]
async fn department_context_roster_is_bounded() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let sales = f.create("Sales", None).await;
    let provenance = json!({"actor_type": "system", "actor_id": null, "actor_name": "Test"});
    for index in 0..=MAX_ROSTER_CHILDREN {
        sqlx::query(
            "INSERT INTO departments (id, company_id, name, slug, parent_id, created_by) \
             VALUES ($1, $2, $3, $3, $4, $5)",
        )
        .bind(Uuid::new_v4())
        .bind(f.company)
        .bind(format!("child-{index:02}"))
        .bind(sales.id.as_uuid())
        .bind(&provenance)
        .execute(f.pool())
        .await
        .unwrap();
    }
    for index in 0..=MAX_ROSTER_AGENTS {
        let principal = agent_principal(f.pool(), f.company, &format!("agent-{index:02}")).await;
        sqlx::query(
            "INSERT INTO department_members (company_id, department_id, principal_id) \
             VALUES ($1, $2, $3)",
        )
        .bind(f.company)
        .bind(sales.id.as_uuid())
        .bind(principal.as_uuid())
        .execute(f.pool())
        .await
        .unwrap();
    }
    let any_agent = agent_of(&f, f.agent_principal).await;
    let roster = context(&f, f.desk_of(sales.id).await, any_agent)
        .await
        .unwrap()
        .roster
        .unwrap();
    assert_eq!(roster.children.len(), MAX_ROSTER_CHILDREN);
    assert_eq!(roster.children[0].slug, "child-00");
    assert!(roster.more_children);
    assert_eq!(roster.agents.len(), MAX_ROSTER_AGENTS);
    assert_eq!(roster.agents[0].slug, "agent-00");
    // Bare agents have no default desk record.
    assert!(roster.agents.iter().all(|agent| agent.address.is_none()));
    assert!(roster.more_agents);
}
