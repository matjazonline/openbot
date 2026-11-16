//! Builder agent creation and explicit shared-desk staffing through real fenced actions.
use super::*;

async fn agent_commands(f: &AdmissionFixture) -> Vec<Value> {
    sqlx::query_scalar("SELECT result FROM workflow_provisioning_commands WHERE company_id=$1 AND command_key LIKE 'agent:%'")
        .bind(company(f).as_uuid()).fetch_all(f.persistence().pool()).await.unwrap()
}

async fn assert_direct(f: &AdmissionFixture, created: &Value) {
    assert!(created.get("channel").is_none(), "{created}");
    let principal = Uuid::parse_str(created["principal_id"].as_str().unwrap()).unwrap();
    let binding = Uuid::parse_str(created["binding_id"].as_str().unwrap()).unwrap();
    let actual: (Uuid, Option<Uuid>, bool, String) = sqlx::query_as(
        "SELECT binding.principal_id,binding.channel_id,binding.active,address.slug::text FROM workflow_bindings AS binding JOIN addresses AS address ON address.company_id=binding.company_id AND address.principal_id=binding.principal_id WHERE binding.id=$1",
    ).bind(binding).fetch_one(f.persistence().pool()).await.unwrap();
    assert_eq!(
        actual,
        (
            principal,
            None,
            true,
            created["agent"]["slug"].as_str().unwrap().to_owned()
        )
    );
    assert!(agent_commands(f).await.is_empty());
}

#[tokio::test]
async fn agent_address_create_builder_returns_direct_binding_without_a_desk() {
    let (f, claim) = claimed(false).await;
    let before = written(&f).await;
    let request = call(&f, &claim, ProvisioningKind::CreateAgent, spec("helper")).await;
    let created = committed(&dispatch(&f), &request).await;
    assert_direct(&f, &created).await;
    assert_eq!(
        grew(&before, &written(&f).await),
        serde_json::from_str::<Value>(ONE_ASSISTANT).unwrap()
    );
}

#[tokio::test]
async fn agent_address_create_builder_refuses_implicit_desk_configuration() {
    let (f, claim) = claimed(false).await;
    let cases = [
        json!({"reply_mode":"reviewed"}),
        json!({"steps":[]}),
        json!({"channel_slug":"another", "channel_name":"Another desk"}),
    ];
    for fields in cases {
        let mut arguments = spec("helper");
        arguments
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        let before = written(&f).await;
        let result = ActionService::new(f.persistence().clone())
            .prepare_tool(
                provisioning_action(&f, &claim, ProvisioningKind::CreateAgent, arguments),
                ModelToolCallId::parse(Uuid::new_v4().to_string()).unwrap(),
            )
            .await;
        assert!(matches!(result, Err(AppError::BadRequest(_))));
        assert_eq!(
            written(&f).await,
            before,
            "schema refusal writes no action or effects"
        );
    }
    assert!(agent_commands(&f).await.is_empty());
}

#[tokio::test]
async fn agent_address_create_builder_can_staff_a_separate_reviewed_desk() {
    let (f, claim) = claimed(false).await;
    let request = call(&f, &claim, ProvisioningKind::CreateAgent, spec("helper")).await;
    let agent = committed(&dispatch(&f), &request).await;
    assert_direct(&f, &agent).await;
    let before = written(&f).await;
    let request = call(&f, &claim, ProvisioningKind::CreateDeskForAgent,
        json!({"agent_slug":"helper","channel_slug":"helper-review","channel_name":"Review","reply_mode":"reviewed"})).await;
    let desk = committed(&dispatch(&f), &request).await;
    assert_eq!(desk["channel"]["slug"], "helper-review", "{desk}");
    assert_eq!(desk["reply_mode"], "reviewed");
    assert_eq!(
        grew(&before, &written(&f).await),
        json!({"markers":1,"receipts":1,"agents":0,"channels":1,"workflows":1,"bindings":1})
    );
    assert_eq!(committed(&dispatch(&f), &request).await, desk);
    assert_direct(&f, &agent).await;
}

/// D5, F2: refusals of the shared creation -- no keyed model, an address a desk already holds --
/// are worded tool errors with a committed receipt and no agent or desk.
#[tokio::test]
async fn agent_address_create_builder_refusals_are_tool_errors() {
    let (f, claim) = claimed(false).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    let desk = Uuid::new_v4();
    let write = ChannelWrite {
        name: "Front desk".into(),
        slug: "front".into(),
        enabled: true,
        ..ChannelWrite::default()
    };
    insert_channel_row_on(&mut tx, company(&f).as_uuid(), desk, &write)
        .await
        .unwrap();
    insert_desk_addresses(&mut tx, company(&f).as_uuid(), desk, "front", &[])
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let before = written(&f).await;
    let request = call(&f, &claim, ProvisioningKind::CreateAgent, spec("front")).await;
    let refused = committed(&dispatch(&f), &request).await;
    assert_eq!(refused["error"]["code"], PROVISIONING_CONFLICT, "{refused}");
    assert_eq!(
        grew(&before, &written(&f).await),
        json!({"markers": 1, "receipts": 1, "agents": 0, "channels": 0, "workflows": 0,
            "bindings": 0})
    );

    CompanyPersistence::replace_model_connections_for_user(
        f.persistence(),
        owner(&f).user_id(),
        company(&f).as_uuid(),
        vec![],
    )
    .await
    .unwrap();
    let before = written(&f).await;
    let request = call(&f, &claim, ProvisioningKind::CreateAgent, spec("helper")).await;
    let refused = committed(&dispatch(&f), &request).await;
    assert_eq!(refused["error"]["code"], PROVISIONING_INVALID, "{refused}");
    assert_eq!(
        grew(&before, &written(&f).await),
        json!({"markers": 1, "receipts": 1, "agents": 0, "channels": 0, "workflows": 0,
            "bindings": 0})
    );
    assert!(agent_commands(&f).await.is_empty());
}

/// 03C.0: the Builder places the agent in a named department (with a role), or by default in the
/// requester's one department; an unknown department or a role without one is refused.
#[tokio::test]
async fn agent_address_create_builder_places_the_agent_in_a_department() {
    let (f, claim) = claimed(false).await;
    let pool = f.persistence().pool();
    let company_id = company(&f).as_uuid();
    let department = |slug: &'static str| async move {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO departments (id, company_id, parent_id, name, slug, created_by) \
             VALUES ($1, $2, (SELECT id FROM departments WHERE company_id=$2 AND is_root), $3, $3, '{\"actor_type\":\"system\",\"actor_id\":null,\"actor_name\":\"Test\"}')",
        )
        .bind(id)
        .bind(company_id)
        .bind(slug)
        .execute(pool)
        .await
        .unwrap();
        id
    };
    let support = department("support").await;
    let sales = department("sales").await;
    let requester: Uuid = sqlx::query_scalar(
        "SELECT id FROM principals WHERE company_id = $1 AND user_id = $2 AND kind = 'person'",
    )
    .bind(company_id)
    .bind(owner(&f).user_id())
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO department_members (company_id, department_id, principal_id) VALUES ($1, $2, $3)",
    )
    .bind(company_id)
    .bind(support)
    .bind(requester)
    .execute(pool)
    .await
    .unwrap();
    let placed = |slug: &'static str| async move {
        sqlx::query_as::<_, (Uuid, Option<String>)>(
            "SELECT member.department_id, role.role FROM agents AS agent \
             JOIN principals AS principal ON principal.agent_id = agent.id \
             JOIN department_members AS member ON member.principal_id = principal.id \
             LEFT JOIN department_roles AS role ON role.principal_id = principal.id \
              AND role.department_id = member.department_id \
             WHERE agent.company_id = $1 AND agent.slug = $2",
        )
        .bind(company_id)
        .bind(slug)
        .fetch_all(pool)
        .await
        .unwrap()
    };

    let request = call(&f, &claim, ProvisioningKind::CreateAgent, spec("member")).await;
    committed(&dispatch(&f), &request).await;
    assert_eq!(placed("member").await, [(support, None)]);

    let mut lead = spec("lead");
    lead["department"] = json!("sales");
    lead["role"] = json!("lead");
    let request = call(&f, &claim, ProvisioningKind::CreateAgent, lead).await;
    committed(&dispatch(&f), &request).await;
    assert_eq!(placed("lead").await, [(sales, Some("lead".into()))]);

    let mut unknown = spec("stray");
    unknown["department"] = json!("nowhere");
    let mut roleless = spec("stray");
    roleless["role"] = json!("lead");
    for arguments in [unknown, roleless] {
        let request = call(&f, &claim, ProvisioningKind::CreateAgent, arguments).await;
        let refused = committed(&dispatch(&f), &request).await;
        assert_eq!(refused["error"]["code"], PROVISIONING_INVALID, "{refused}");
    }

    // In a second department, the requester has no single one, so the agent is placed nowhere.
    sqlx::query(
        "INSERT INTO department_members (company_id, department_id, principal_id) VALUES ($1, $2, $3)",
    )
    .bind(company_id)
    .bind(sales)
    .bind(requester)
    .execute(pool)
    .await
    .unwrap();
    let request = call(&f, &claim, ProvisioningKind::CreateAgent, spec("unplaced")).await;
    committed(&dispatch(&f), &request).await;
    assert!(placed("unplaced").await.is_empty());
}

#[tokio::test]
async fn agent_address_create_competing_builder_claimants_make_one_direct_binding() {
    let (f, claim) = claimed(false).await;
    let request = call(&f, &claim, ProvisioningKind::CreateAgent, spec("helper")).await;
    let before = written(&f).await;
    let (left, right) = (dispatch(&f), dispatch(&f));
    let barrier = Barrier::new(2);
    let (a, b) = tokio::join!(
        async {
            barrier.wait().await;
            committed(&left, &request).await
        },
        async {
            barrier.wait().await;
            committed(&right, &request).await
        }
    );
    assert_eq!(a, b);
    assert_direct(&f, &a).await;
    assert_eq!(
        grew(&before, &written(&f).await),
        serde_json::from_str::<Value>(ONE_ASSISTANT).unwrap()
    );
}

#[tokio::test]
async fn root_membership_builder_infers_no_working_department_but_accepts_an_explicit_root() {
    let (f, claim) = claimed(false).await;
    let pool = f.persistence().pool();
    let tenant = company(&f).as_uuid();
    let (root, slug): (Uuid, String) =
        sqlx::query_as("SELECT id,slug::text FROM departments WHERE company_id=$1 AND is_root")
            .bind(tenant)
            .fetch_one(pool)
            .await
            .unwrap();
    let implicit = call(
        &f,
        &claim,
        ProvisioningKind::CreateAgent,
        spec("implicit-root"),
    )
    .await;
    let receipt = committed(&dispatch(&f), &implicit).await;
    assert_eq!(receipt["agent"]["slug"], "implicit-root");
    let mut specification = spec("explicit-root");
    specification["department"] = json!(slug);
    let explicit = call(&f, &claim, ProvisioningKind::CreateAgent, specification).await;
    let receipt = committed(&dispatch(&f), &explicit).await;
    assert_eq!(receipt["agent"]["slug"], "explicit-root");
    let placed: Vec<(String, Uuid)> = sqlx::query_as(
        "SELECT agent.slug::text,member.department_id FROM agents AS agent \
         JOIN principals AS principal ON principal.company_id=agent.company_id AND principal.agent_id=agent.id \
         JOIN department_members AS member ON member.company_id=principal.company_id AND member.principal_id=principal.id \
         WHERE agent.company_id=$1 AND agent.slug IN ('implicit-root','explicit-root') ORDER BY agent.slug,member.department_id",
    ).bind(tenant).fetch_all(pool).await.unwrap();
    assert_eq!(placed, [("explicit-root".to_owned(), root)]);
}
