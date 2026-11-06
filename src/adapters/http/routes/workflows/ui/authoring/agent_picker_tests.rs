use super::super::agent_picker::{PickerForm, apply, picker};
use super::*;
use crate::application::agent::*;
use crate::entities::agent_capability::*;

fn input(source: &str, agent: Option<Uuid>) -> axum::Form<PickerForm> {
    axum::Form(
        serde_json::from_value(json!({"source":source,"step":"start","agent":agent})).unwrap(),
    )
}

#[tokio::test]
async fn autonomous_agent_authoring_workflow_picker_preserves_unsaved_source_and_selected_agent_outside_page()
 {
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.agent("Original", "assistant").await;
    for index in 0..51 {
        f.agent(&format!("Agent {index}"), &format!("a{index:02}"))
            .await;
    }
    let target = f
        .agent("Picked </option><img src=x onerror=alert(1)>", "zz-target")
        .await;
    let workflow = Uuid::new_v4();
    let source = source_of("agent.run", workflow);
    f.draft(workflow, "Picker", &source).await;
    let unsaved = format!("{source}\n# keep this unsaved comment\n");
    let html = picker(
        f.ui(f.owner),
        Path(workflow),
        f.query(),
        input(&unsaved, Some(target)),
    )
    .await
    .unwrap()
    .0;
    assert!(html.contains("Picked &lt;/option&gt;&lt;img") && !html.contains("<img src=x"));
    assert!(html.contains(&format!(r#"value="{target}" selected"#)));
    assert!(html.contains("More agents") && html.contains("Inactive"));
    assert!(!html.contains("fixture-key"));
    let changed = apply(
        f.ui(f.owner),
        Path(workflow),
        f.query(),
        input(&unsaved, Some(target)),
    )
    .await
    .unwrap()
    .0;
    let expected = unsaved.replacen(r#""literal": "assistant""#, r#""literal": "zz-target""#, 1);
    assert_ne!(expected, unsaved);
    assert!(changed.contains(&pages::escape_html_attr(&expected)));
    let stored = f.ui(f.owner).detail(f.company, workflow).await.unwrap();
    assert_eq!(
        stored.source, source,
        "selection changes only the browser editor"
    );
    assert_eq!(stored.summary.revision, 1);
    assert!(stored.versions.is_empty());
}

async fn published_message_agent(f: &Fixture) -> PublishedAgentRevision {
    let id = f.agent("Picker target", "picked").await;
    let cases = f.ui(f.owner).agents;
    let stored = cases
        .get_company_agent(f.owner, f.company, id)
        .await
        .unwrap()
        .unwrap();
    let saved = cases
        .update_agent(
            f.owner,
            f.company,
            CompanyAgentUpdate {
                agent: AgentId::new(id),
                expected_revision: stored.draft_revision,
                write: AgentWrite {
                    name: stored.name,
                    slug: stored.slug,
                    provider: stored.provider,
                    model: stored.model,
                    ..Default::default()
                },
                tools: Some(vec![AgentToolAssignment {
                    binding: None,
                    alias: AgentToolAlias::parse("message.reply").unwrap(),
                    choice: AgentToolChoice::Builtin {
                        tool: AgentBuiltinTool::MessageReply,
                    },
                }]),
            },
        )
        .await
        .unwrap();
    f.engine
        .agent_publications()
        .publish_agent(PublishAgent {
            scope: AgentPublicationScope {
                company: CompanyId::new(f.company),
                actor: WorkflowActor::authenticated(f.owner).unwrap(),
                agent: AgentId::new(id),
            },
            expected_draft_revision: saved.draft_revision,
            expected_active_revision: None,
            key: AgentPublicationKey::parse(Uuid::new_v4().to_string()).unwrap(),
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn autonomous_agent_authoring_workflow_picker_uses_current_hint_and_keeps_model_only_steps_tool_free()
 {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let published = published_message_agent(&f).await;
    for (kind, tools) in [
        ("agent.run", true),
        ("ai.classify", false),
        ("decision.agent", false),
    ] {
        let workflow = Uuid::new_v4();
        let source = source_of(kind, workflow);
        f.draft(workflow, "Model step", &source).await;
        let html = picker(
            f.ui(f.owner),
            Path(workflow),
            f.query(),
            input(&source, Some(published.definition.agent_id.as_uuid())),
        )
        .await
        .unwrap()
        .0;
        assert!(html.contains(&published.id.as_uuid().to_string()));
        assert_eq!(html.contains("Published tools"), tools, "{kind}");
        assert_eq!(html.contains("message.reply"), tools, "{kind}");
        if !tools {
            assert!(
                html.contains("model-only step")
                    && html.contains("Tools and skills are unavailable")
            );
        }
        assert!(!html.contains("fixture-key"));
    }
}

#[tokio::test]
async fn autonomous_agent_authoring_workflow_picker_refuses_foreign_actor_agent_workflow_and_non_model_step()
 {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let target = f.agent("Own", "picked").await;
    let foreign = AgentPersistence::create(
        &f.persistence,
        f.other,
        AgentWrite {
            name: "Foreign".into(),
            slug: "foreign".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let workflow = Uuid::new_v4();
    let source = source_of("agent.run", workflow);
    f.draft(workflow, "Scoped picker", &source).await;
    for user in [f.member, f.stranger] {
        assert!(matches!(
            picker(
                f.ui(user),
                Path(workflow),
                f.query(),
                input(&source, Some(target))
            )
            .await,
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            apply(
                f.ui(user),
                Path(workflow),
                f.query(),
                input(&source, Some(target))
            )
            .await,
            Err(AppError::NotFound(_))
        ));
    }
    assert!(matches!(
        apply(
            f.ui(f.owner),
            Path(workflow),
            f.query(),
            input(&source, Some(foreign.id))
        )
        .await,
        Err(AppError::NotFound(_))
    ));
    assert!(matches!(
        picker(
            f.ui(f.stranger),
            Path(workflow),
            Query(CompanyQuery {
                company_id: f.other
            }),
            input(&source, Some(foreign.id))
        )
        .await,
        Err(AppError::NotFound(_))
    ));
    let changed_kind = source_of("data.map", workflow);
    let refused = apply(
        f.ui(f.owner),
        Path(workflow),
        f.query(),
        input(&changed_kind, Some(target)),
    )
    .await
    .unwrap()
    .0;
    assert!(
        refused.contains("Select a model step") && !refused.contains("data-workflow-agent-source")
    );
    assert_eq!(
        f.ui(f.owner)
            .detail(f.company, workflow)
            .await
            .unwrap()
            .source,
        source
    );
}

fn browser_fixture(root: &std::path::Path, name: &str, content: impl AsRef<[u8]>) {
    std::fs::write(root.join(name), content).unwrap();
}

async fn export_picker_responses(
    f: &Fixture,
    root: &std::path::Path,
    workflow: Uuid,
    initial: &str,
    unsaved: &str,
    agent: Uuid,
) {
    let original = axum::Form(serde_json::from_value(json!({"source":initial})).unwrap());
    let initial_html = picker(f.ui(f.owner), Path(workflow), f.query(), original)
        .await
        .unwrap()
        .0;
    browser_fixture(root, "initial-picker.html", initial_html);
    let selected = picker(
        f.ui(f.owner),
        Path(workflow),
        f.query(),
        input(unsaved, Some(agent)),
    )
    .await
    .unwrap()
    .0;
    browser_fixture(root, "selected-picker.html", selected);
    let applied = apply(
        f.ui(f.owner),
        Path(workflow),
        f.query(),
        input(unsaved, Some(agent)),
    )
    .await
    .unwrap()
    .0;
    browser_fixture(root, "applied.html", applied);
}

#[tokio::test]
#[ignore = "exports native workflow agent picker responses for the browser gate"]
async fn dump_autonomous_workflow_agent_picker_browser_fixtures() {
    let directory =
        std::env::var("WORKFLOW_AGENT_BROWSER_FIXTURES").expect("explicit owned fixture directory");
    let root = std::path::Path::new(&directory);
    std::fs::create_dir_all(root).unwrap();
    let f = Fixture::new().await.expect("owned PostgreSQL is required");
    f.agent("Original", "assistant").await;
    let target = published_message_agent(&f).await;
    let workflow = Uuid::new_v4();
    let source = source_of("agent.run", workflow);
    let unsaved = format!("{source}\n# browser unsaved comment\n");
    f.draft(workflow, "Picker browser", &source).await;
    let other = Uuid::new_v4();
    let other_source = source_of("data.map", other);
    f.draft(other, "Other workflow", &other_source).await;
    let page = super::super::workspace(
        f.ui(f.owner),
        Query(super::super::WorkspaceQuery {
            company_id: Some(f.company),
            workflow_id: Some(workflow),
            tab: None,
            new: None,
            tools: None,
        }),
    )
    .await
    .unwrap()
    .0;
    browser_fixture(root, "page.html", page);
    browser_fixture(
        root,
        "other-pane.html",
        f.pane(f.owner, other, "editor").await.unwrap(),
    );
    browser_fixture(root, "app.js", pages::application_javascript());
    browser_fixture(root, "theme-init.js", pages::theme_init_javascript());
    let agent = target.definition.agent_id.as_uuid();
    export_picker_responses(&f, root, workflow, &source, &unsaved, agent).await;
    browser_fixture(
        root,
        "metadata.json",
        serde_json::to_vec_pretty(&json!({
            "company":f.company, "workflow":workflow, "other":other, "agent":agent,
            "source":source, "unsaved":unsaved, "other_source":other_source,
            "applied":unsaved.replacen(r#""literal": "assistant""#, r#""literal": "picked""#, 1),
            "revision":target.id.as_uuid(),
        }))
        .unwrap(),
    );
    let stored = f.ui(f.owner).detail(f.company, workflow).await.unwrap();
    assert_eq!(stored.source, source);
    assert_eq!(stored.summary.revision, 1);
    assert!(stored.versions.is_empty());
}
