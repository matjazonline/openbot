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
    let granted = grant_agent_tools(
        f.ui(f.owner),
        Path(agent),
        f.query(),
        form(&[("expected", ""), ("tools", "sample.send")]),
    )
    .await
    .unwrap();
    assert!(!granted.0.contains("alert-error"), "{}", granted.0);
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
