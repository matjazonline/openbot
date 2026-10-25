//! Workflow tool registrations and agent grants (WF-C003).
use super::*;

/// WF-C003: registration with the revision the caller saw, revocation, and agent grants against
/// the list the caller saw; a foreign version or agent is not found.
#[tokio::test]
async fn workflow_tool_routes_register_revoke_and_grant() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let ours = f.resources(f.owner, f.company).await;
    let theirs = f.resources(f.stranger, f.other).await;
    let register = |name: &str, version: Uuid, expected: Option<i64>| {
        tools::register(
            f.state(),
            by(f.owner),
            Path((f.company, name.to_owned())),
            body(json!({"workflow_id": ours.workflow, "version_id": version,
                "expected_revision": expected})),
        )
    };
    let (status, created) = answer(register("report.make", ours.version, None).await).await;
    assert_eq!(
        (status, created["revision"].clone()),
        (StatusCode::CREATED, json!(1)),
        "{created}"
    );
    assert_eq!(
        answer(register("report.make", ours.version, None).await)
            .await
            .0,
        StatusCode::CONFLICT
    );
    let (status, updated) = answer(register("report.make", ours.version, Some(1)).await).await;
    assert_eq!(
        (status, updated["revision"].clone()),
        (StatusCode::OK, json!(2)),
        "{updated}"
    );
    assert_eq!(
        answer(register("report.make", ours.version, Some(1)).await)
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        answer(register("message.reply", ours.version, None).await)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        answer(register("bad name", ours.version, None).await)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        answer(register("theirs.tool", theirs.version, None).await)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (_, listed) = answer(tools::list(f.state(), by(f.owner), Path(f.company)).await).await;
    assert_eq!(listed.as_array().unwrap().len(), 1, "{listed}");
    assert_eq!(listed[0]["name"], "report.make");

    let agent = AgentPersistence::create(
        &f.persistence,
        f.company,
        AgentWrite {
            name: "Reporter".into(),
            slug: "reporter".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let foreign = AgentPersistence::create(
        &f.persistence,
        f.other,
        AgentWrite {
            name: "Theirs".into(),
            slug: "theirs".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let grant = |user: Uuid, agent: Uuid, expected: Value, granted: Value| {
        tools::grant(
            f.state(),
            by(user),
            Path((f.company, agent)),
            body(json!({"expected": expected, "tools": granted})),
        )
    };
    let (status, granted) =
        answer(grant(f.owner, agent, json!([]), json!(["report.make"])).await).await;
    assert_eq!(status, StatusCode::OK, "{granted}");
    assert_eq!(granted["tools"], json!(["report.make"]));
    assert_eq!(
        answer(grant(f.owner, agent, json!([]), json!(["message.reply"])).await)
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        answer(grant(f.owner, agent, json!(["report.make"]), json!(["a", "a"])).await)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let many = (0..33)
        .map(|index| format!("tool_{index}"))
        .collect::<Vec<_>>();
    assert_eq!(
        answer(grant(f.owner, agent, json!(["report.make"]), json!(many)).await)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    // F061: a retry of the grant that applied finds its own list.
    assert_eq!(
        answer(grant(f.owner, agent, json!([]), json!(["report.make"])).await)
            .await
            .0,
        StatusCode::OK
    );
    // F057: provisioning is the Agent Builder's; a grant cannot add it to another agent...
    let (status, refused) = answer(
        grant(
            f.owner,
            agent,
            json!(["report.make"]),
            json!(["report.make", "create_agent_channel"]),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    // ...while the Builder, which holds it, keeps or drops it with its other grants.
    let builder = AgentPersistence::create(
        &f.persistence,
        f.company,
        AgentWrite {
            name: "Builder".into(),
            slug: "builder".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let provisioning = json!(["list_company_agents", "create_agent_channel"]);
    sqlx::query("UPDATE agents SET workflow_tools = $2 WHERE id = $1")
        .bind(builder)
        .bind(vec!["list_company_agents", "create_agent_channel"])
        .execute(f.persistence.pool())
        .await
        .unwrap();
    let kept = json!(["list_company_agents", "create_agent_channel", "report.make"]);
    assert_eq!(
        answer(grant(f.owner, builder, provisioning, kept.clone()).await)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        answer(grant(f.owner, builder, kept, json!(["report.make"])).await)
            .await
            .0,
        StatusCode::OK
    );
    for (user, target) in [(f.owner, foreign), (f.member, agent), (f.stranger, agent)] {
        assert_eq!(
            answer(grant(user, target, json!([]), json!([])).await)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
    }
    let stored: Vec<String> = sqlx::query_scalar("SELECT workflow_tools FROM agents WHERE id = $1")
        .bind(agent)
        .fetch_one(f.persistence.pool())
        .await
        .unwrap();
    assert_eq!(stored, ["report.make"]);

    let revoke = |expected: i64| {
        tools::revoke(
            f.state(),
            by(f.owner),
            Path((f.company, "report.make".to_owned())),
            body(json!({"expected_revision": expected})),
        )
    };
    assert_eq!(answer(revoke(1).await).await.0, StatusCode::CONFLICT);
    assert_eq!(answer(revoke(2).await).await.0, StatusCode::NO_CONTENT);
    assert_eq!(answer(revoke(2).await).await.0, StatusCode::NOT_FOUND);
    let (_, listed) = answer(tools::list(f.state(), by(f.owner), Path(f.company)).await).await;
    assert_eq!(listed, json!([]));
}

/// Two grants racing from the same list the callers saw: the company lock serializes them, one
/// applies and the other finds the list changed.
#[tokio::test]
async fn competing_agent_grants_apply_once() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let agent = AgentPersistence::create(
        &f.persistence,
        f.company,
        AgentWrite {
            name: "Raced".into(),
            slug: "raced".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    let grant = |user: Uuid, tool: &'static str| {
        let state = f.state();
        let company = f.company;
        async move {
            answer(
                tools::grant(
                    state,
                    by(user),
                    Path((company, agent)),
                    body(json!({"expected": [], "tools": [tool]})),
                )
                .await,
            )
            .await
            .0
        }
    };
    let (first, second) = tokio::join!(
        tokio::spawn(grant(f.owner, "first.tool")),
        tokio::spawn(grant(f.admin, "second.tool"))
    );
    let mut statuses = [first.unwrap(), second.unwrap()];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::OK, StatusCode::CONFLICT]);
    let stored: Vec<String> = sqlx::query_scalar("SELECT workflow_tools FROM agents WHERE id = $1")
        .bind(agent)
        .fetch_one(f.persistence.pool())
        .await
        .unwrap();
    assert_eq!(stored.len(), 1, "{stored:?}");
}
