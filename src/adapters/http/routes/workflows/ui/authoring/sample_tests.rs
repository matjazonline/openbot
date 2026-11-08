use super::*;

fn sample_form(revision: &str, scripts: &str) -> Bytes {
    form(&[
        ("expected_revision", revision),
        ("input", "{}"),
        ("params", "{}"),
        ("scripts", scripts),
    ])
}
async fn counts(f: &Fixture) -> BTreeMap<&'static str, i64> {
    let mut counts = BTreeMap::new();
    for table in [
        "workflow_runs",
        "workflow_executions",
        "workflow_decisions",
        "workflow_action_intents",
        "workflow_action_receipts",
        "background_tasks",
        "messages",
        "thread_messages",
        "message_deliveries",
        "local_agent_sessions",
        "local_agent_operations",
        "workflow_local_agent_calls",
    ] {
        let count = sqlx::query_scalar(&format!("SELECT count(*) AS count FROM {table}"))
            .fetch_one(f.persistence.pool())
            .await
            .unwrap();
        counts.insert(table, count);
    }
    counts
}

#[tokio::test]
async fn cloudroom_samples_local_run_uses_scripted_output_without_machine_or_production_effects() {
    let f = Fixture::new().await.unwrap();
    let workflow = Uuid::new_v4();
    f.draft(
        workflow,
        "Local sample",
        &source_of("local_agent.run", workflow),
    )
    .await;
    let before = counts(&f).await;
    let result = json!({"status":"succeeded","text":"Sample <img src=x onerror=alert(1)>","operation_id":Uuid::new_v4(),"session_id":Uuid::new_v4(),"thread_id":null,"usage":null,"truncated":false});
    let scripts = json!([{"workflow_id":workflow,"step":"start","responses":[{"output":result}]}])
        .to_string();
    let html = sample(
        f.ui(f.owner),
        Path(workflow),
        f.query(),
        sample_form("1", &scripts),
    )
    .await
    .unwrap()
    .0;
    assert!(html.contains("Sample completed"), "{html}");
    assert!(html.contains("local_agent.run") && html.contains("intercepted_write"));
    assert!(!html.contains("<img src=x"));
    assert_eq!(before, counts(&f).await);
}
#[tokio::test]
async fn samples_authorize_tenant_revision_and_preserve_invalid_hostile_text() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    f.draft(workflow, "Sample", &source_of("data.map", workflow))
        .await;
    let before = counts(&f).await;
    for user in [f.member, f.stranger] {
        assert!(matches!(
            f.pane(user, workflow, "sample").await,
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            sample(
                f.ui(user),
                Path(workflow),
                f.query(),
                sample_form("1", "[]")
            )
            .await,
            Err(WorkflowError::App(AppError::NotFound(_)))
        ));
    }
    assert!(matches!(
        sample(
            f.ui(f.stranger),
            Path(workflow),
            Query(CompanyQuery {
                company_id: f.other
            }),
            sample_form("1", "[]")
        )
        .await,
        Err(WorkflowError::App(AppError::NotFound(_)))
    ));
    let html = sample(
        f.ui(f.admin),
        Path(workflow),
        f.query(),
        sample_form("1", "[]"),
    )
    .await
    .unwrap()
    .0;
    assert!(html.contains("Sample completed"), "{html}");
    assert!(html.contains("Exact frozen content"));
    let stale = sample(
        f.ui(f.owner),
        Path(workflow),
        f.query(),
        sample_form("0", "[]"),
    )
    .await
    .unwrap()
    .0;
    assert!(!stale.contains("Sample completed"));
    assert!(stale.contains("revision 0"));
    let hostile = "</textarea><img src=x onerror=alert(1)>";
    let html = sample(
        f.ui(f.owner),
        Path(workflow),
        f.query(),
        form(&[
            ("expected_revision", "1"),
            ("input", hostile),
            ("params", "{}"),
            ("scripts", "[]"),
        ]),
    )
    .await
    .unwrap()
    .0;
    assert!(html.contains("Invalid sample JSON"));
    assert!(html.contains("&lt;/textarea&gt;"));
    assert!(!html.contains("<img src=x"));
    assert_eq!(before, counts(&f).await);
}

#[tokio::test]
async fn sample_direct_workflow_tool_child_and_message_have_zero_production_effects() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let agent = f.agent("Assistant", "assistant").await;
    let child = Uuid::new_v4();
    let mut child_source = example("message.send", child);
    child_source["input_schema"] = json!({"type":"object"});
    let version = f
        .published(child, "Captured message", &child_source.to_string())
        .await;
    let registered = register_tool(
        f.ui(f.owner),
        Path(child),
        f.query(),
        form(&[
            ("name", "sample.send"),
            ("version_id", &version.to_string()),
            ("params_json", "{}"),
        ]),
    )
    .await
    .unwrap();
    assert!(
        registered.0.contains("Registered sample.send"),
        "{}",
        registered.0
    );
    f.publish_with_workflow_tool(agent, "sample.send", child, version)
        .await;
    let workflow = Uuid::new_v4();
    f.draft(workflow, "Agent sample", &source_of("agent.run", workflow))
        .await;
    let result = json!({"status":"accepted","messages":[{"destination":{"kind":"channel","id":Uuid::from_u128(4)},"message_id":Uuid::new_v4(),"status":"accepted"}]});
    let scripts=json!([
        {"workflow_id":workflow,"step":"start","responses":[{"output":"</textarea><img src=x onerror=alert(1)>","tools":[{"name":"sample.send","input":{}}]}]},
        {"workflow_id":child,"step":"start","responses":[{"output":result}]}
    ]).to_string();
    let before = counts(&f).await;
    let html = sample(
        f.ui(f.owner),
        Path(workflow),
        f.query(),
        sample_form("1", &scripts),
    )
    .await
    .unwrap()
    .0;
    assert!(html.contains("Sample completed"), "{html}");
    assert!(
        html.contains("workflow_child") && html.contains("intercepted_write"),
        "{html}"
    );
    assert!(html.contains("message.send"));
    assert!(!html.contains("<img src=x"));
    assert_eq!(before, counts(&f).await);
}

#[tokio::test]
async fn sample_mcp_fixture_never_contacts_selected_endpoint_and_rejects_foreign_selection() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/mcp", listener.local_addr().unwrap());
    let connection = reviewed_connection_at(&f, &endpoint).await;
    let workflow = Uuid::new_v4();
    f.draft(workflow, "MCP fixture", &source_of("mcp.call", workflow))
        .await;
    let scripts=json!([{"workflow_id":workflow,"step":"start","responses":[{"output":{"content":[],"isError":false,"structuredContent":{"result":"fixture"}}}]}]).to_string();
    let send = |selected: String| {
        let ui = f.ui(f.owner);
        let scripts = scripts.clone();
        let query = f.query();
        async move {
            sample(
                ui,
                Path(workflow),
                query,
                form(&[
                    ("expected_revision", "1"),
                    ("input", "{}"),
                    ("params", "{}"),
                    ("scripts", &scripts),
                    ("mcp:service:lookup", &selected),
                ]),
            )
            .await
            .unwrap()
            .0
        }
    };
    let before = counts(&f).await;
    let html = send(connection.to_string()).await;
    assert!(html.contains("Sample completed"), "{html}");
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    );
    let html = send(Uuid::new_v4().to_string()).await;
    assert!(!html.contains("Sample completed"));
    assert!(html.contains("unavailable"));
    assert_eq!(before, counts(&f).await);
}

fn capture_target(
    f: &Fixture,
    user: Uuid,
    workflow: Uuid,
) -> crate::application::workflow::lifecycle::DefinitionTarget {
    crate::application::workflow::lifecycle::DefinitionTarget {
        company: crate::domain::workflow::CompanyId::new(f.company),
        actor: crate::application::workflow::WorkflowActor::authenticated(user).unwrap(),
        workflow: crate::domain::workflow::WorkflowId::new(workflow),
    }
}
fn capture_request(workflow: Uuid) -> crate::application::workflow::sample::SampleRequest {
    crate::application::workflow::sample::SampleRequest {
        agents: vec![],
        input: json!({}),
        params: json!({}),
        scripts: serde_json::from_value(
            json!([{"workflow_id":workflow,"step":"start","responses":[{"output":"answer"}]}]),
        )
        .unwrap(),
    }
}
#[tokio::test]
async fn autonomous_agent_samples_keep_old_capture_and_hash_after_draft_changes() {
    use crate::application::workflow::sample_capture::SampleDefinitionCapture;
    let Some(f) = Fixture::new().await else {
        return;
    };
    let agent = f.agent("Assistant", "assistant").await;
    let workflow = Uuid::new_v4();
    f.draft(workflow, "Sample hash", &source_of("agent.run", workflow))
        .await;
    let target = capture_target(&f, f.owner, workflow);
    let capture = f.engine.sample_capture(vec![], vec![]);
    let before = counts(&f).await;
    let old = capture
        .capture(
            target,
            crate::domain::workflow::DraftRevision::new(1).unwrap(),
            capture_request(workflow),
        )
        .await
        .unwrap();
    AgentPersistence::update(
        &f.persistence,
        agent,
        AgentWrite {
            name: "Assistant".into(),
            slug: "assistant".into(),
            provider: Some("openai".into()),
            model: Some("gpt-test".into()),
            system_prompt: Some("Changed instructions".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let new = capture
        .capture(
            target,
            crate::domain::workflow::DraftRevision::new(1).unwrap(),
            capture_request(workflow),
        )
        .await
        .unwrap();
    let old = f.engine.sample(old).unwrap();
    let new = f.engine.sample(new).unwrap();
    assert_eq!(old.content_hash, new.content_hash);
    assert_ne!(old.configuration_hash, new.configuration_hash);
    assert_ne!(
        old.agent_selections[0].sample_hash,
        new.agent_selections[0].sample_hash
    );
    assert!(matches!(
        old.status,
        crate::application::workflow::sample::SampleStatus::Completed
    ));
    assert!(matches!(
        new.status,
        crate::application::workflow::sample::SampleStatus::Completed
    ));
    assert_eq!(before, counts(&f).await);
    let published: i64 =
        sqlx::query_scalar("SELECT count(*) FROM agent_revisions WHERE company_id=$1")
            .bind(f.company)
            .fetch_one(f.persistence.pool())
            .await
            .unwrap();
    assert_eq!(
        published, 0,
        "capturing drafts never activates production revisions"
    );
    assert!(
        capture
            .authorize_release(capture_target(&f, f.stranger, workflow))
            .await
            .is_err()
    );
    assert!(
        capture
            .capture(
                capture_target(&f, f.stranger, workflow),
                crate::domain::workflow::DraftRevision::new(1).unwrap(),
                capture_request(workflow)
            )
            .await
            .is_err()
    );
}

async fn wait_for_member_lock(pool: &sqlx::PgPool, minimum: i64) {
    tokio::time::timeout(std::time::Duration::from_secs(2),async {
        loop {
            let waiting:i64=sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND pid<>pg_backend_pid() AND wait_event_type='Lock' AND query LIKE '%company_members%'").fetch_one(pool).await.unwrap();
            if waiting>=minimum {break;}
            tokio::task::yield_now().await;
        }
    }).await.expect("real competing member locks");
}
#[tokio::test]
async fn autonomous_agent_samples_release_rechecks_admin_after_competing_role_revocation() {
    use crate::application::workflow::sample_capture::SampleDefinitionCapture;
    use crate::use_cases::company_invite::CompanyInvitePersistence;
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.agent("Assistant", "assistant").await;
    let workflow = Uuid::new_v4();
    f.draft(workflow, "Release race", &source_of("agent.run", workflow))
        .await;
    let capture = Arc::new(f.engine.sample_capture(vec![], vec![]));
    let target = capture_target(&f, f.admin, workflow);
    let snapshot = capture
        .capture(
            target,
            crate::domain::workflow::DraftRevision::new(1).unwrap(),
            capture_request(workflow),
        )
        .await
        .unwrap();
    capture.authorize_release(target).await.unwrap();
    let mut locker = f.persistence.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM company_members WHERE company_id=$1 AND user_id=$2 FOR UPDATE")
        .bind(f.company)
        .bind(f.admin)
        .execute(&mut *locker)
        .await
        .unwrap();
    let p = f.persistence.clone();
    let company = f.company;
    let admin = f.admin;
    let demote = tokio::spawn(async move {
        p.update_member_role(
            company,
            admin,
            crate::entities::company_member::CompanyAccessRole::Member,
        )
        .await
        .unwrap()
        .unwrap()
    });
    wait_for_member_lock(f.persistence.pool(), 1).await;
    let release = {
        let capture = capture.clone();
        tokio::spawn(async move { capture.authorize_release(target).await })
    };
    wait_for_member_lock(f.persistence.pool(), 2).await;
    locker.commit().await.unwrap();
    demote.await.unwrap();
    assert!(
        release.await.unwrap().is_err(),
        "stale admin snapshot cannot release its result"
    );
    assert_eq!(
        f.engine.sample(snapshot).unwrap().status,
        crate::application::workflow::sample::SampleStatus::Completed
    );
    assert!(
        f.ui(f.admin)
            .sample_body(
                f.company,
                &f.ui(f.owner).detail(f.company, workflow).await.unwrap(),
                SampleState::default()
            )
            .await
            .is_err(),
        "actual rendering guard refuses current member"
    );
}
