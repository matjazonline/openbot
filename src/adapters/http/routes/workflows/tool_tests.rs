//! Workflow tool registrations and agent grants (WF-C003).
use super::*;

/// WF-C003: registration with the revision the caller saw, revocation, and agent grants against
/// the list the caller saw; a foreign version or agent is not found.
#[tokio::test]
async fn workflow_tool_routes_register_and_revoke() {
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

/// The former global grant race now exercises the actual revision-checked JSON draft command.
#[tokio::test]
async fn autonomous_agent_authoring_competing_json_draft_writes_apply_once() {
    use crate::use_cases::{agent::AgentUseCases, skill::SkillUseCases};
    let Some(f) = Fixture::new().await else {
        return;
    };
    let shared = Arc::new(f.persistence.clone());
    let agents = Arc::new(AgentUseCases::new(
        shared.clone(),
        shared.clone(),
        shared.clone(),
    ));
    let skills = Arc::new(SkillUseCases::new(
        shared.clone(),
        shared.clone(),
        shared.clone(),
    ));
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
    .unwrap();
    let departments =
        crate::adapters::persistence::test_support::department_use_cases(&f.persistence);
    let save = |user, tool| {
        let agents = agents.clone();
        let departments = departments.clone();
        let skills = skills.clone();
        let company = f.company;
        let agent = agent.clone();
        async move {
            crate::adapters::http::routes::agent::update_agent_json(State(agents), State(skills), State(departments), by(user), Path((company, agent.id)), body(json!({
                "expected_revision":agent.draft_revision,
                "agent":{"name":"Raced","slug":"raced","native_permissions":"full"},
                "tool_assignments":[{"binding":null,"alias":if tool == "message_reply" {"message.reply"} else {"message.send"},"choice":{"kind":"builtin","tool":tool}}]
            }))).await.map(IntoResponse::into_response).unwrap_or_else(IntoResponse::into_response).status()
        }
    };
    let (left, right) = tokio::join!(
        save(f.owner, "message_reply"),
        save(f.admin, "message_send")
    );
    let mut statuses = [left, right];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::OK, StatusCode::CONFLICT]);
    let (revision, bindings): (i64, i64) = sqlx::query_as("SELECT draft_revision, (SELECT count(*) FROM agent_draft_bindings WHERE agent_id=agents.id) FROM agents WHERE id=$1")
        .bind(agent.id).fetch_one(f.persistence.pool()).await.unwrap();
    assert_eq!(
        revision,
        i64::try_from(agent.draft_revision.get() + 1).unwrap()
    );
    assert_eq!(bindings, 1);
}
