//! The agent directories name departments and desks (01-departments p03 B3, 01DEP-P03-D10):
//! `list_company_agents` and the authoring picker's entries, `department_directory_`.
use super::*;
use crate::application::agent::{AgentAuthoringReads, AgentCompanyScope, AgentKey, DepartmentRef};
use crate::application::ports::channel::ChannelPersistence;
use crate::application::use_cases::agent::AgentPlacement;
use crate::entities::department::{DepartmentId, DepartmentRole};

const CHARTER: &str = "Win and keep customers.";

async fn department(f: &AdmissionFixture, slug: &str) -> DepartmentId {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO departments (id, company_id, name, slug, charter, created_by) \
         VALUES ($1, $2, initcap($3), $3, $4, \
           '{\"actor_type\":\"system\",\"actor_id\":null,\"actor_name\":\"Test\"}')",
    )
    .bind(id)
    .bind(company(f).as_uuid())
    .bind(slug)
    .bind(CHARTER)
    .execute(f.persistence().pool())
    .await
    .unwrap();
    DepartmentId::new(id)
}

/// A company agent with its default desk, placed as `placement` says.
async fn agent(f: &AdmissionFixture, slug: &str, placement: Option<AgentPlacement>) -> Uuid {
    crate::adapters::persistence::test_support::company_agents(f.persistence())
        .create(
            owner(f),
            company(f),
            AgentWrite {
                name: slug.into(),
                slug: slug.into(),
                ..Default::default()
            },
            placement,
        )
        .await
        .unwrap()
        .agent
        .id
}

async fn desk_of(f: &AdmissionFixture, agent: Uuid) -> Uuid {
    sqlx::query_scalar(
        "SELECT (result->'channel'->>'id')::uuid FROM workflow_provisioning_commands \
         WHERE company_id = $1 AND command_key = $2",
    )
    .bind(company(f).as_uuid())
    .bind(format!("agent:{agent}"))
    .fetch_one(f.persistence().pool())
    .await
    .unwrap()
}

async fn address(f: &AdmissionFixture, slug: &str) -> String {
    let company: String = sqlx::query_scalar("SELECT slug::text FROM companies WHERE id = $1")
        .bind(company(f).as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    format!("{slug}@{company}.agents.test")
}

async fn listed(f: &AdmissionFixture, claim: &ClaimedWorkflow, arguments: Value) -> Value {
    let request = call(f, claim, ProvisioningKind::ListAgents, arguments).await;
    committed(&dispatch(f), &request).await
}

/// 03C.0: each listed agent names the departments it is in, with the roles it holds there, and
/// its own desk; an agent in none lists none, and a deleted desk is null and is not recreated.
#[tokio::test]
async fn department_directory_lists_departments_roles_and_desks() {
    let (f, claim) = claimed(false).await;
    let sales = department(&f, "sales").await;
    let closer = agent(
        &f,
        "closer",
        Some(AgentPlacement {
            department: sales,
            role: Some(DepartmentRole::Lead),
        }),
    )
    .await;
    let scout = agent(
        &f,
        "scout",
        Some(AgentPlacement {
            department: sales,
            role: None,
        }),
    )
    .await;
    let loner = agent(&f, "loner", None).await;
    // A desk that answers through a workflow cannot be deleted, so this agent's recorded desk is
    // a plain one: once it is deleted the agent lists no desk, and listing makes none.
    let drifter = AgentPersistence::create(
        f.persistence(),
        company(&f).as_uuid(),
        AgentWrite {
            name: "Drifter".into(),
            slug: "drifter".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let plain = ChannelPersistence::create(
        f.persistence(),
        company(&f).as_uuid(),
        ChannelWrite {
            name: "Drifting".into(),
            slug: "drifting".into(),
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap()
    .id;
    sqlx::query(
        "INSERT INTO workflow_provisioning_commands \
         (company_id, command_key, request_digest, actor_id, result) \
         VALUES ($1, $2, repeat('0', 64), $3, $4)",
    )
    .bind(company(&f).as_uuid())
    .bind(format!("agent:{drifter}"))
    .bind(owner(&f).user_id())
    .bind(json!({"channel": {"id": plain.to_string(), "slug": "drifting"}, "binding_id": null}))
    .execute(f.persistence().pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO department_members (company_id, department_id, principal_id) \
         SELECT company_id, $2, id FROM principals WHERE company_id = $1 AND agent_id = $3",
    )
    .bind(company(&f).as_uuid())
    .bind(sales.as_uuid())
    .bind(drifter)
    .execute(f.persistence().pool())
    .await
    .unwrap();
    let listing = listed(&f, &claim, json!({})).await;
    let entry = |slug: &str| {
        listing["agents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|agent| agent["slug"] == slug)
            .cloned()
            .unwrap()
    };
    let desk = |id: Uuid, address: String| json!({"id": id.to_string(), "address": address});
    assert_eq!(
        entry("closer")["departments"],
        json!([{"slug": "sales", "name": "Sales", "roles": ["lead"]}])
    );
    assert_eq!(
        entry("closer")["desk"],
        desk(desk_of(&f, closer).await, address(&f, "closer").await)
    );
    assert_eq!(
        entry("scout")["departments"],
        json!([{"slug": "sales", "name": "Sales", "roles": []}])
    );
    assert_eq!(
        entry("loner")["departments"],
        json!([]),
        "an agent in no department lists none"
    );
    assert_eq!(
        entry("loner")["desk"],
        desk(desk_of(&f, loner).await, address(&f, "loner").await)
    );
    assert_eq!(
        listed_desk(&listing, "drifter"),
        desk(plain, address(&f, "drifting").await)
    );
    assert_eq!(listing["truncated"], false);

    let _ = scout;

    ChannelPersistence::delete(f.persistence(), plain)
        .await
        .unwrap();
    let desks = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM channels WHERE company_id = $1")
            .bind(company(&f).as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap()
    };
    let before = desks().await;
    // Another request: a repeated call replays its first receipt.
    let after = listed(&f, &claim, json!({"department": "sales"})).await;
    assert_eq!(listed_desk(&after, "drifter"), Value::Null);
    assert_eq!(
        listed_desk(&after, "scout"),
        desk(desk_of(&f, scout).await, address(&f, "scout").await)
    );
    assert_eq!(desks().await, before, "a listing recreates no desk");
}

fn listed_desk(listing: &Value, slug: &str) -> Value {
    listing["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|agent| agent["slug"] == slug)
        .unwrap_or_else(|| panic!("{slug} is listed: {listing}"))["desk"]
        .clone()
}

/// 03C.0: `department` lists only that department's members and role holders; an unknown slug
/// is an invalid-input refusal.
#[tokio::test]
async fn department_directory_filters_by_one_department() {
    let (f, claim) = claimed(false).await;
    let sales = department(&f, "sales").await;
    let support = department(&f, "support").await;
    let placed = |department, role| {
        Some(AgentPlacement {
            department,
            role: Some(role),
        })
    };
    agent(&f, "closer", placed(sales, DepartmentRole::Lead)).await;
    agent(
        &f,
        "scout",
        Some(AgentPlacement {
            department: sales,
            role: None,
        }),
    )
    .await;
    agent(&f, "helper", placed(support, DepartmentRole::Lead)).await;
    agent(&f, "loner", None).await;
    let slugs = |listing: &Value| -> Vec<String> {
        listing["agents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|agent| agent["slug"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(
        slugs(&listed(&f, &claim, json!({"department": "sales"})).await),
        ["closer", "scout"]
    );
    assert_eq!(
        slugs(&listed(&f, &claim, json!({"department": "support"})).await),
        ["helper"]
    );
    assert_eq!(
        slugs(&listed(&f, &claim, json!({})).await),
        ["closer", "helper", "loner", "scout"]
    );
    let refused = listed(&f, &claim, json!({"department": "nowhere"})).await;
    assert_eq!(refused["error"]["code"], PROVISIONING_INVALID);
    assert_eq!(
        refused["error"]["message"],
        "No department has the slug 'nowhere'"
    );
}

/// 03C.0: the authoring picker's entries name the agent's departments, with charter and roles,
/// and its desk's address; its paging key stays the slug.
#[tokio::test]
async fn department_directory_picker_entries_name_departments_and_the_desk() {
    let (f, _claim) = claimed(false).await;
    let sales = department(&f, "sales").await;
    agent(
        &f,
        "closer",
        Some(AgentPlacement {
            department: sales,
            role: Some(DepartmentRole::Lead),
        }),
    )
    .await;
    let reads = f.persistence().clone().with_app_domain("agents.test");
    let scope = AgentCompanyScope {
        company: company(&f),
        actor: owner(&f),
    };
    let page = reads.agent_directory(scope, None).await.unwrap();
    let closer = page
        .items
        .iter()
        .find(|entry| entry.key.as_str() == "closer")
        .unwrap();
    let expected = vec![DepartmentRef {
        id: sales.as_uuid(),
        name: "Sales".into(),
        slug: "sales".into(),
        charter: Some(CHARTER.into()),
        roles: vec![DepartmentRole::Lead],
    }];
    assert_eq!(closer.departments, expected);
    assert_eq!(closer.desk, Some(address(&f, "closer").await));
    let entry = reads
        .agent_directory_entry(scope, AgentKey::parse("closer").unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(entry.departments, expected);
    assert_eq!(entry.desk, closer.desk);
    assert_eq!(page.next, None);
}
