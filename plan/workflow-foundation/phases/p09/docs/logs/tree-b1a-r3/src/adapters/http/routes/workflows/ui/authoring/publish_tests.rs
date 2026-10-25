//! Publication from the editor: the preview of frozen agents and selected connections, a publish
//! that moves no binding, compare, stale and conflicting publishes, and MCP selections.
use super::*;

#[tokio::test]
async fn publishing_moves_no_binding_and_versions_compare_line_by_line() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    let source = source_of("data.map", workflow);
    f.draft(workflow, "Map", &source).await;
    let validated = f
        .save(
            workflow,
            &[
                ("expected_revision", "1"),
                ("title", "Map"),
                ("source", &source),
                ("intent", "validate"),
            ],
        )
        .await
        .unwrap();
    let key = field_value(&validated, "command_key");
    let publish_with = |revision: &'static str, key: String| {
        let ui = f.ui(f.owner);
        async move {
            text(
                publish(
                    ui,
                    Path(workflow),
                    Query(CompanyQuery {
                        company_id: f.company,
                    }),
                    form(&[("expected_revision", revision), ("command_key", &key)]),
                )
                .await
                .unwrap(),
            )
            .await
        }
    };
    let published = publish_with("2", key.clone()).await;
    assert!(published.contains("Published version"), "{published}");
    assert!(published.contains("Publishing changes no binding"));
    assert!(published.contains("No binding uses this workflow yet"));
    // A retry of the key publishes nothing new.
    publish_with("2", key).await;
    let detail = f
        .engine
        .reads()
        .definition(CompanyId::new(f.company), WorkflowId::new(workflow))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(detail.versions.len(), 1);
    let first = detail.versions[0].version.as_uuid();

    // A binding on the first version, then a second version.
    let binding = Uuid::new_v4();
    let _configured = super::super::bindings::configure(
        State(f.engine.clone()),
        AuthenticatedUser { id: f.owner },
        Path((f.company, binding)),
        Json(
            serde_json::from_value(
                json!({"association": {"kind": "company"}, "version_id": first}),
            )
            .unwrap(),
        ),
    )
    .await
    .unwrap();
    let changed = source.replace("hello", "goodbye");
    let validated = f
        .save(
            workflow,
            &[
                ("expected_revision", "2"),
                ("title", "Map"),
                ("source", &changed),
                ("intent", "validate"),
            ],
        )
        .await
        .unwrap();
    let published = publish_with("3", field_value(&validated, "command_key")).await;
    assert!(published.contains("stays on version"), "{published}");
    assert!(published.contains("Company-wide"));
    assert!(published.contains("paused: admits nothing"));

    let Html(diff) = compare(
        f.ui(f.owner),
        Path(workflow),
        Query(CompareQuery {
            company_id: f.company,
            from: first.to_string(),
            to: "draft".into(),
        }),
    )
    .await
    .unwrap();
    assert!(diff.contains("hello") && diff.contains("goodbye"), "{diff}");
    assert!(diff.contains("bg-error/10") && diff.contains("bg-success/10"));
    let Html(same) = compare(
        f.ui(f.owner),
        Path(workflow),
        Query(CompareQuery {
            company_id: f.company,
            from: "draft".into(),
            to: "draft".into(),
        }),
    )
    .await
    .unwrap();
    assert!(same.contains("No differences."));
}

#[tokio::test]
async fn a_publish_of_a_revision_that_is_no_longer_the_draft_is_refused_in_the_editor() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    f.draft(workflow, "Stale", &blank_source(workflow)).await;
    f.save(
        workflow,
        &[
            ("expected_revision", "1"),
            ("title", "Stale"),
            ("source", &blank_source(workflow)),
            ("intent", "save"),
        ],
    )
    .await
    .unwrap();
    let html = text(
        publish(
            f.ui(f.owner),
            Path(workflow),
            f.query(),
            form(&[("expected_revision", "1"), ("command_key", "stale-1")]),
        )
        .await
        .unwrap(),
    )
    .await;
    assert!(
        html.contains("Revision 1 is no longer the draft (now revision 2"),
        "{html}"
    );
}

#[tokio::test]
async fn an_agent_step_previews_the_agents_frozen_tools_and_skills() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    sqlx::query(
        "INSERT INTO company_model_connections(company_id, provider, api_key, models) \
         VALUES ($1, 'openai', 'fixture-key', '{gpt-test}')",
    )
    .bind(f.company)
    .execute(f.persistence.pool())
    .await
    .unwrap();
    AgentPersistence::create(
        &f.persistence,
        f.company,
        AgentWrite {
            name: "Assistant".into(),
            slug: "assistant".into(),
            provider: Some("openai".into()),
            model: Some("gpt-test".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let workflow = Uuid::new_v4();
    let source = source_of("agent.run", workflow);
    f.draft(workflow, "Agent", &source).await;
    let html = f
        .save(
            workflow,
            &[
                ("expected_revision", "1"),
                ("title", "Agent"),
                ("source", &source),
                ("intent", "validate"),
            ],
        )
        .await
        .unwrap();
    assert!(html.contains("Revision 2 is valid"), "{html}");
    assert!(html.contains("Uses the agent's tools and skills"));
    assert!(html.contains("source: the agent, frozen at publication"));
    assert!(html.contains("tools: no tools") && html.contains("skills: no skills"));
    assert!(html.contains("openai / gpt-test"));
}

#[tokio::test]
async fn an_mcp_step_validates_publishes_through_the_selected_slot_and_locates_bad_arguments() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let connection = reviewed_connection(&f).await;
    let workflow = Uuid::new_v4();
    let source = source_of("mcp.call", workflow);
    f.draft(workflow, "Lookup", &source).await;
    let editor = f.pane(f.owner, workflow, "editor").await.unwrap();
    assert!(
        editor.contains(&format!(r#"<option value="{connection}" selected>"#)),
        "{editor}"
    );
    assert!(editor.contains("Frozen contract of lookup on tickets-server"));
    assert!(!editor.contains("super-secret-token") && !editor.contains("mcp.example.test"));
    let field = connection.to_string();
    let validated = f
        .save(
            workflow,
            &[
                ("expected_revision", "1"),
                ("title", "Lookup"),
                ("source", &source),
                ("intent", "validate"),
                ("mcp:service:lookup", &field),
            ],
        )
        .await
        .unwrap();
    assert!(
        validated.contains("Saved revision 2; it is valid."),
        "{validated}"
    );
    assert!(validated.contains("through slot service"));
    assert!(validated.contains("Connections this publish freezes"));
    assert!(
        validated.contains("on tickets-server"),
        "the preview names the selected connection"
    );
    // The publish form carries exactly the validated selection.
    let preview = &validated[validated.find(r#"id="workflow-preview""#).unwrap()..];
    assert_eq!(field_value(preview, "mcp:service:lookup"), field);
    let published = text(
        publish(
            f.ui(f.owner),
            Path(workflow),
            f.query(),
            form(&[
                ("expected_revision", "2"),
                ("command_key", &field_value(preview, "command_key")),
                (
                    "previewed_dependencies",
                    &field_value(preview, "previewed_dependencies"),
                ),
                ("mcp:service:lookup", &field),
            ]),
        )
        .await
        .unwrap(),
    )
    .await;
    assert!(published.contains("Published version"), "{published}");
    assert!(
        !published.contains("differs from the preview"),
        "nothing changed in between"
    );
    let detail = f
        .engine
        .reads()
        .definition(CompanyId::new(f.company), WorkflowId::new(workflow))
        .await
        .unwrap()
        .unwrap();
    let bundle = crate::application::workflow::lifecycle::WorkflowDefinitions::selectable_version(
        &f.persistence,
        CompanyId::new(f.company),
        detail.versions[0].version,
    )
    .await
    .unwrap()
    .unwrap();
    let tool = bundle
        .snapshots()
        .tools
        .iter()
        .find(|tool| tool.contract.name.as_str() == "lookup")
        .expect("the selected tool is frozen");
    assert_eq!(
        tool.contract.connection.as_ref().map(|slot| slot.as_str()),
        Some("service")
    );
    // A wrong argument name is a located diagnostic.
    let wrong = source.replace("\"key\"", "\"wrong\"");
    let refused = f
        .save(
            workflow,
            &[
                ("expected_revision", "2"),
                ("title", "Lookup"),
                ("source", &wrong),
                ("intent", "validate"),
                ("mcp:service:lookup", &field),
            ],
        )
        .await
        .unwrap();
    assert!(refused.contains(r#"id="workflow-diagnostic""#), "{refused}");
    assert!(refused.contains("Go to line"));
}

#[tokio::test]
async fn a_parameter_selected_agent_step_previews_every_frozen_candidate() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    sqlx::query(
        "INSERT INTO company_model_connections(company_id, provider, api_key, models) \
         VALUES ($1, 'openai', 'fixture-key', '{gpt-test}')",
    )
    .bind(f.company)
    .execute(f.persistence.pool())
    .await
    .unwrap();
    for slug in ["assistant", "helper"] {
        AgentPersistence::create(
            &f.persistence,
            f.company,
            AgentWrite {
                name: format!("Agent {slug}"),
                slug: slug.into(),
                provider: Some("openai".into()),
                model: Some("gpt-test".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    let workflow = Uuid::new_v4();
    let mut source = example("agent.run", workflow);
    source["parameter_schema"] = json!({"type": "object", "properties": {"agent":
        {"type": "string", "enum": ["assistant", "helper"]}}, "required": ["agent"],
        "additionalProperties": false});
    source["steps"]["start"]["with"]["agent"] = json!({"ref": "/params/agent"});
    let source = serde_json::to_string_pretty(&source).unwrap();
    f.draft(workflow, "Choice", &source).await;
    let html = f
        .save(
            workflow,
            &[
                ("expected_revision", "1"),
                ("title", "Choice"),
                ("source", &source),
                ("intent", "validate"),
            ],
        )
        .await
        .unwrap();
    assert!(html.contains("Revision 2 is valid"), "{html}");
    assert!(html.contains("Agent assistant") && html.contains("Agent helper"));
    assert_eq!(
        html.matches("tools: no tools").count(),
        2,
        "each candidate's tools"
    );
    assert!(html.contains(r#"the agent named by <span class="font-mono">/params/agent</span> at run time, one of @assistant, @helper"#));
}

#[tokio::test]
async fn a_publish_conflict_that_is_not_a_moved_draft_says_what_the_command_said() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    f.draft(workflow, "Keyed", &blank_source(workflow)).await;
    let first = text(
        publish(
            f.ui(f.owner),
            Path(workflow),
            f.query(),
            form(&[("expected_revision", "1"), ("command_key", "same-key")]),
        )
        .await
        .unwrap(),
    )
    .await;
    assert!(first.contains("Published version"), "{first}");
    // The same key with other selections names another version: the command refuses it.
    let refused = text(
        publish(
            f.ui(f.owner),
            Path(workflow),
            f.query(),
            form(&[
                ("expected_revision", "1"),
                ("command_key", "same-key"),
                ("http:send", &Uuid::new_v4().to_string()),
            ]),
        )
        .await
        .unwrap(),
    )
    .await;
    assert!(
        refused.contains("Revision 1 was not published: "),
        "{refused}"
    );
    assert!(!refused.contains("no longer the draft"));
}

#[tokio::test]
async fn a_parameter_selected_step_lists_its_own_parameters_agents() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    sqlx::query(
        "INSERT INTO company_model_connections(company_id, provider, api_key, models) \
         VALUES ($1, 'openai', 'fixture-key', '{gpt-test}')",
    )
    .bind(f.company)
    .execute(f.persistence.pool())
    .await
    .unwrap();
    for slug in ["assistant", "helper"] {
        AgentPersistence::create(
            &f.persistence,
            f.company,
            AgentWrite {
                name: format!("Agent {slug}"),
                slug: slug.into(),
                provider: Some("openai".into()),
                model: Some("gpt-test".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    // `start` names @assistant literally; `second` picks from an enum holding only @helper.
    let workflow = Uuid::new_v4();
    let mut source = example("agent.run", workflow);
    source["parameter_schema"] = json!({"type": "object", "properties": {"agent":
        {"type": "string", "enum": ["helper"]}}, "required": ["agent"],
        "additionalProperties": false});
    let mut second = source["steps"]["start"].clone();
    second["with"]["agent"] = json!({"ref": "/params/agent"});
    source["steps"]["start"]["routes"] = json!({"success": "second"});
    source["steps"]["second"] = second;
    let source = serde_json::to_string_pretty(&source).unwrap();
    f.draft(workflow, "Two", &source).await;
    let html = f
        .save(
            workflow,
            &[
                ("expected_revision", "1"),
                ("title", "Two"),
                ("source", &source),
                ("intent", "validate"),
            ],
        )
        .await
        .unwrap();
    assert!(html.contains("Revision 2 is valid"), "{html}");
    assert!(html.contains("at run time, one of @helper, "), "{html}");
    assert!(
        !html.contains("one of @assistant"),
        "the literal step's agent is not a choice here"
    );
    assert!(html.contains(r#"agent <span class="font-mono">@assistant</span>"#));
}
