//! The authoring workspace over the production engine and a database of its own: keyed creates
//! and copies, located validation that keeps the typed source, concurrent saves, publication
//! that moves no binding, compare, archive, the catalogue, MCP selections, hostile text, other
//! tenants and plain members, and the CSRF boundary of every write.
use super::*;
use crate::adapters::http::routes::workflows::tests::{example, owner_and_company, teammate};
use crate::adapters::persistence::{
    PostgresPersistence,
    test_support::{OwnDatabase, own_database},
};
use crate::use_cases::agent::{AgentPersistence, AgentWrite};
use axum::extract::State;
use axum::http::{Request, header};
use tower::ServiceExt;

struct Fixture {
    _db: OwnDatabase,
    persistence: PostgresPersistence,
    engine: Engine,
    owner: Uuid,
    company: Uuid,
    member: Uuid,
    admin: Uuid,
    stranger: Uuid,
    /// The stranger's own company.
    other: Uuid,
}

impl Fixture {
    async fn new() -> Option<Self> {
        let db = own_database().await?;
        let persistence = PostgresPersistence::new(db.pool.clone());
        let (owner, company) = owner_and_company(&persistence, "Authoring").await;
        let (stranger, other) = owner_and_company(&persistence, "Elsewhere").await;
        let member = teammate(&persistence, company, "member").await;
        let admin = teammate(&persistence, company, "admin").await;
        Some(Self {
            _db: db,
            engine: Arc::new(WorkflowEngine::for_tests(persistence.clone())),
            persistence,
            owner,
            company,
            member,
            admin,
            stranger,
            other,
        })
    }

    fn ui(&self, user: Uuid) -> Ui {
        let shared = Arc::new(self.persistence.clone());
        Ui {
            engine: self.engine.clone(),
            companies: Arc::new(CompanyUseCases::new(shared.clone())),
            users: Arc::new(UserUseCases::new(
                Arc::new(crate::infra::argon2_password_hasher()),
                shared.clone(),
            )),
            agents: Arc::new(AgentUseCases::new(
                shared.clone(),
                shared.clone(),
                shared.clone(),
            )),
            config: crate::adapters::http::routes::ui::stream_tests::test_config(),
            user,
        }
    }

    fn query(&self) -> Query<CompanyQuery> {
        Query(CompanyQuery {
            company_id: self.company,
        })
    }

    async fn pane(&self, user: Uuid, workflow: Uuid, tab: &str) -> AppResult<String> {
        pane(
            self.ui(user),
            Path(workflow),
            Query(PaneQuery {
                company_id: self.company,
                tab: Some(tab.into()),
            }),
        )
        .await
        .map(|html| html.0)
    }

    async fn save(&self, workflow: Uuid, fields: &[(&str, &str)]) -> WorkflowResult<String> {
        save(
            self.ui(self.owner),
            Path(workflow),
            self.query(),
            form(fields),
        )
        .await
        .map(|html| html.0)
    }

    /// A stored draft of `source` at revision 1, saved through the shared command.
    async fn draft(&self, workflow: Uuid, title: &str, source: &str) {
        save_draft(
            &self.engine,
            self.company,
            WorkflowActor::authenticated(self.owner).unwrap(),
            workflow,
            None,
            DraftContent {
                title: title.into(),
                description: String::new(),
                source: source.into(),
            },
        )
        .await
        .unwrap();
    }
}

fn form(fields: &[(&str, &str)]) -> Bytes {
    Bytes::from(
        url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(fields)
            .finish(),
    )
}

async fn text(response: Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), 8 << 20)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

fn source_of(name: &str, workflow: Uuid) -> String {
    serde_json::to_string_pretty(&example(name, workflow)).unwrap()
}

/// The value of the first `name="<field>" value="…"` in a page.
fn field_value(html: &str, field: &str) -> String {
    let marker = format!(r#"name="{field}" value=""#);
    let start = html
        .find(&marker)
        .unwrap_or_else(|| panic!("{field} in {html}"))
        + marker.len();
    html[start..start + html[start..].find('"').unwrap()].to_owned()
}

#[tokio::test]
async fn the_sidebar_pages_through_every_draft() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    for index in 0..(LIST_PAGE + 1) {
        let workflow = Uuid::new_v4();
        f.draft(
            workflow,
            &format!("Draft {index:03}"),
            &blank_source(workflow),
        )
        .await;
    }
    let Html(page) = workspace(
        f.ui(f.owner),
        Query(WorkspaceQuery {
            company_id: Some(f.company),
            workflow_id: None,
            tab: None,
            new: None,
        }),
    )
    .await
    .unwrap();
    assert!(page.contains("Draft 000") && page.contains("Draft 049"));
    assert!(!page.contains("Draft 050"), "the first page holds 50");
    assert!(page.contains("Load more"));
    let after = {
        let marker = "after=";
        let start = page.find(marker).unwrap() + marker.len();
        page[start..start + 36].parse::<Uuid>().unwrap()
    };
    let Html(next) = list_page(
        f.ui(f.owner),
        Query(ListQuery {
            company_id: f.company,
            after: Some(after),
        }),
    )
    .await
    .unwrap();
    assert!(next.contains("Draft 050") && !next.contains("Draft 049"));
    assert!(!next.contains("Load more"), "the last page has no next");
}

#[tokio::test]
async fn a_blank_draft_is_created_once_per_key_and_validates_as_a_one_step_graph() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let create = || async {
        text(
            create_draft(
                f.ui(f.owner),
                f.query(),
                form(&[("title", "Triage"), ("command_key", "blank-1")]),
            )
            .await
            .unwrap(),
        )
        .await
    };
    let first = create().await;
    let second = create().await;
    assert!(first.contains("Draft created."), "{first}");
    assert!(first.contains(r#"id="workflow-menu" hx-swap-oob="true""#));
    let workflow = keyed_id("workflow.ui_draft", CompanyId::new(f.company), "blank-1");
    assert!(first.contains(&workflow.to_string()) && second.contains(&workflow.to_string()));
    let page = f
        .engine
        .reads()
        .list_definitions(
            CompanyId::new(f.company),
            PageRequest::new(None, None).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        page.items.len(),
        1,
        "a retry of the key makes no second draft"
    );

    let saved = f
        .save(
            workflow,
            &[
                ("expected_revision", "1"),
                ("title", "Triage"),
                ("description", ""),
                ("source", &blank_source(workflow)),
                ("intent", "validate"),
            ],
        )
        .await
        .unwrap();
    assert!(saved.contains("Saved revision 2; it is valid."), "{saved}");
    assert!(saved.contains("Revision 2 is valid"));
    assert!(saved.contains("<svg role=\"img\""), "the graph is drawn");
    assert!(saved.contains("Publish revision 2"));
    assert!(saved.contains("No resource slots"));
}

#[tokio::test]
async fn a_template_copy_is_keyed_and_a_stale_catalogue_revision_is_refused() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let catalogue = f.engine.template_catalogue();
    let template = catalogue
        .template(crate::application::workflow::templates::REVIEWED_SUPPORT)
        .unwrap();
    let revision = template.revision().get().to_string();
    let copy = |revision: String, key: &'static str| {
        let ui = f.ui(f.owner);
        async move {
            text(
                copy_template(
                    ui,
                    Path(crate::application::workflow::templates::REVIEWED_SUPPORT.to_owned()),
                    Query(CompanyQuery {
                        company_id: f.company,
                    }),
                    form(&[("revision", &revision), ("command_key", key)]),
                )
                .await
                .unwrap(),
            )
            .await
        }
    };
    let first = copy(revision.clone(), "copy-1").await;
    assert!(first.contains("Template copied"), "{first}");
    assert!(
        first.contains("decision.human"),
        "the copy's source is in the editor"
    );
    assert!(first.contains("copied from template"));
    let replay = copy(revision.clone(), "copy-1").await;
    assert!(replay.contains("Template copied"));
    let stale = copy((template.revision().get() + 1).to_string(), "copy-2").await;
    assert!(stale.contains("The template was not copied"), "{stale}");
    let page = f
        .engine
        .reads()
        .list_definitions(
            CompanyId::new(f.company),
            PageRequest::new(None, None).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
}

#[tokio::test]
async fn an_invalid_source_is_saved_kept_and_located() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    f.draft(workflow, "Broken", &blank_source(workflow)).await;
    // Line 13 names a step that does not exist.
    let broken =
        blank_source(workflow).replace("routes: {success: $end}", "routes: {success: nowhere}");
    let line = broken
        .lines()
        .position(|line| line.contains("nowhere"))
        .unwrap()
        + 1;
    let html = f
        .save(
            workflow,
            &[
                ("expected_revision", "1"),
                ("title", "Broken"),
                ("source", &broken),
                ("intent", "validate"),
            ],
        )
        .await
        .unwrap();
    assert!(
        html.contains("Saved revision 2; it does not validate yet."),
        "{html}"
    );
    assert!(html.contains(r#"id="workflow-diagnostic""#));
    assert!(html.contains(&format!("Go to line {line}")), "{html}");
    assert!(
        html.contains("routes: {success: nowhere}"),
        "the saved text is in the form"
    );
    assert!(!html.contains("Revision 2 is valid"));
    let stored = f
        .engine
        .reads()
        .definition(CompanyId::new(f.company), WorkflowId::new(workflow))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.summary.revision, 2);
    assert_eq!(stored.source, broken);
}

#[tokio::test]
async fn a_concurrent_save_keeps_the_authors_text_and_shows_what_the_other_saved() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    let source = blank_source(workflow);
    f.draft(workflow, "Shared", &source).await;
    // Someone else saves revision 2 while the author still has revision 1 open.
    let theirs = source.replace("max_steps: 10", "max_steps: 11");
    save_draft(
        &f.engine,
        f.company,
        WorkflowActor::authenticated(f.admin).unwrap(),
        workflow,
        Some(1),
        DraftContent {
            title: "Shared".into(),
            description: String::new(),
            source: theirs,
        },
    )
    .await
    .unwrap();
    let mine = source.replace("max_steps: 10", "max_steps: 12");
    let html = f
        .save(
            workflow,
            &[
                ("expected_revision", "1"),
                ("title", "Mine"),
                ("source", &mine),
                ("intent", "save"),
            ],
        )
        .await
        .unwrap();
    assert!(html.contains("Someone else saved revision 2"), "{html}");
    assert!(
        html.contains("max_steps: 12"),
        "the author's text stays in the form"
    );
    assert!(
        html.contains("- limits: {max_steps: 11"),
        "the diff shows theirs"
    );
    assert!(
        html.contains("+ limits: {max_steps: 12"),
        "and the author's"
    );
    assert_eq!(field_value(&html, "expected_revision"), "2");
    let stored = f
        .engine
        .reads()
        .definition(CompanyId::new(f.company), WorkflowId::new(workflow))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.summary.revision, 2,
        "nothing of the author's was saved"
    );
    // Saving again deliberately replaces revision 2.
    let again = f
        .save(
            workflow,
            &[
                ("expected_revision", "2"),
                ("title", "Mine"),
                ("source", &mine),
                ("intent", "save"),
            ],
        )
        .await
        .unwrap();
    assert!(again.contains("Saved revision 3."), "{again}");
}

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
async fn hostile_text_is_escaped_in_the_list_the_editor_and_the_preview() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    // Built from parts: the HTTP sources are scanned for literal script tags (pages tests).
    let script = format!("<{}>alert(1)", "script");
    let hostile = &format!(r#"{script}</script>"><img src=x onerror=alert(2)>"#);
    let source = format!("# {hostile}\n{}", blank_source(workflow));
    f.draft(workflow, hostile, &source).await;
    let html = f
        .save(
            workflow,
            &[
                ("expected_revision", "1"),
                ("title", hostile),
                ("description", hostile),
                ("source", &source),
                ("intent", "validate"),
            ],
        )
        .await
        .unwrap();
    let Html(page) = workspace(
        f.ui(f.owner),
        Query(WorkspaceQuery {
            company_id: Some(f.company),
            workflow_id: Some(workflow),
            tab: None,
            new: None,
        }),
    )
    .await
    .unwrap();
    for rendered in [&html, &page] {
        assert!(!rendered.contains(&script), "{rendered}");
        assert!(!rendered.contains("<img src=x"), "{rendered}");
        assert!(rendered.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    }
}

#[tokio::test]
async fn other_tenants_and_plain_members_reach_no_workflow_and_admins_do() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    f.draft(workflow, "Private", &blank_source(workflow)).await;
    for user in [f.stranger, f.member] {
        assert!(matches!(
            f.pane(user, workflow, "editor").await,
            Err(AppError::NotFound(_))
        ));
        let ui = f.ui(user);
        assert!(matches!(
            save(
                ui,
                Path(workflow),
                f.query(),
                form(&[("expected_revision", "1")])
            )
            .await,
            Err(WorkflowError::App(AppError::NotFound(_)))
        ));
        assert!(matches!(
            publish(
                f.ui(user),
                Path(workflow),
                f.query(),
                form(&[("expected_revision", "1"), ("command_key", "k")])
            )
            .await,
            Err(WorkflowError::App(AppError::NotFound(_)))
        ));
        assert!(matches!(
            archive(
                f.ui(user),
                Path(workflow),
                f.query(),
                form(&[("expected_revision", "1")])
            )
            .await,
            Err(WorkflowError::App(AppError::NotFound(_)))
        ));
        assert!(matches!(
            compare(
                f.ui(user),
                Path(workflow),
                Query(CompareQuery {
                    company_id: f.company,
                    from: "draft".into(),
                    to: "draft".into()
                })
            )
            .await,
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            catalogue(f.ui(user), f.query()).await,
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            list_page(
                f.ui(user),
                Query(ListQuery {
                    company_id: f.company,
                    after: None
                })
            )
            .await,
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            new_pane(f.ui(user), f.query()).await,
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            create_draft(
                f.ui(user),
                f.query(),
                form(&[("title", "x"), ("command_key", "k")])
            )
            .await,
            Err(WorkflowError::App(AppError::NotFound(_)))
        ));
        // A page they can open shows only companies they manage: none here.
        let Html(page) = workspace(
            f.ui(user),
            Query(WorkspaceQuery {
                company_id: Some(f.company),
                workflow_id: Some(workflow),
                tab: None,
                new: None,
            }),
        )
        .await
        .unwrap();
        assert!(!page.contains("Private"), "{page}");
    }
    // A workflow id of this company asked through another company's scope is not found.
    assert!(matches!(
        pane(
            f.ui(f.stranger),
            Path(workflow),
            Query(PaneQuery {
                company_id: f.other,
                tab: None
            })
        )
        .await,
        Err(AppError::NotFound(_))
    ));
    assert!(
        f.pane(f.admin, workflow, "editor")
            .await
            .unwrap()
            .contains("Private")
    );
}

#[tokio::test]
async fn archiving_refuses_a_stale_revision_then_locks_the_draft() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    f.draft(workflow, "Old", &blank_source(workflow)).await;
    let stale = text(
        archive(
            f.ui(f.owner),
            Path(workflow),
            f.query(),
            form(&[("expected_revision", "7")]),
        )
        .await
        .unwrap(),
    )
    .await;
    assert!(stale.contains("nothing was archived"), "{stale}");
    let done = text(
        archive(
            f.ui(f.owner),
            Path(workflow),
            f.query(),
            form(&[("expected_revision", "1")]),
        )
        .await
        .unwrap(),
    )
    .await;
    assert!(done.contains("Archived."), "{done}");
    assert!(done.contains(r#"<span class="badge badge-ghost">archived</span>"#));
    assert!(done.contains("This workflow is archived"));
    let refused = f
        .save(
            workflow,
            &[
                ("expected_revision", "2"),
                ("title", "Old"),
                ("source", &blank_source(workflow)),
                ("intent", "save"),
            ],
        )
        .await
        .unwrap();
    assert!(refused.contains("This workflow was archived"), "{refused}");
}

#[tokio::test]
async fn the_catalogue_lists_every_runnable_type_and_the_agents_a_step_can_name() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    for (slug, model) in [("assistant", Some("gpt-test")), ("plain", None)] {
        AgentPersistence::create(
            &f.persistence,
            f.company,
            AgentWrite {
                name: format!("Agent {slug}"),
                slug: slug.into(),
                provider: model.map(|_| "openai".to_owned()),
                model: model.map(str::to_owned),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    let Html(html) = catalogue(f.ui(f.owner), f.query()).await.unwrap();
    for entry in f.engine.step_catalogue() {
        assert!(html.contains(entry.type_name), "{}", entry.type_name);
    }
    assert!(!html.contains("memory.load"));
    assert!(html.contains("assistant") && html.contains("openai / gpt-test"));
    assert!(html.contains("no explicit model: workflows refuse this agent"));
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

/// An approved MCP operation inserted as rows (no reviewed discovery): enough for the editor's
/// listing, which reads only the approval rows.
async fn approved_rows(f: &Fixture, slug: &str, tools: &[String]) -> Uuid {
    let connection = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO company_mcp_connections(company_id, id, slug, endpoint_url, enabled, auth_type) \
         VALUES ($1, $2, $3, 'https://mcp.example.test/rpc/super-secret-token', true, 'none')",
    )
    .bind(f.company)
    .bind(connection)
    .bind(slug)
    .execute(f.persistence.pool())
    .await
    .unwrap();
    for tool in tools {
        sqlx::query(
            "INSERT INTO company_mcp_tool_grants(company_id, connection_id, tool_name) VALUES ($1, $2, $3)",
        )
        .bind(f.company)
        .bind(connection)
        .bind(tool)
        .execute(f.persistence.pool())
        .await
        .unwrap();
        let snapshot = json!({
            "company_id": f.company,
            "contract": {"name": tool, "connection": null, "input_schema": {"type": "object"}, "output_schema": {"type": "object"}},
            "policy": {"capability": tool, "policy_revision": 1, "effect": "read", "recovery": "reconcile"}
        });
        sqlx::query(
            "INSERT INTO workflow_mcp_operations(company_id, connection_id, name, snapshot, approval_required, version) \
             VALUES ($1, $2, $3, $4, false, $5)",
        )
        .bind(f.company)
        .bind(connection)
        .bind(tool)
        .bind(&snapshot)
        .bind(Uuid::new_v4())
        .execute(f.persistence.pool())
        .await
        .unwrap();
    }
    connection
}

#[tokio::test]
async fn the_editor_offers_a_needed_mcp_tool_past_any_company_wide_count_and_cuts_per_tool() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    // 201 other approved tools sort before the needed `lookup` (WF-P09-F002).
    let mut tools: Vec<String> = (0..201).map(|index| format!("a{index:03}")).collect();
    tools.push("lookup".into());
    let first = approved_rows(&f, "server-000", &tools).await;
    let workflow = Uuid::new_v4();
    f.draft(workflow, "Lookup", &source_of("mcp.call", workflow))
        .await;
    let html = f.pane(f.owner, workflow, "editor").await.unwrap();
    assert!(html.contains(r#"name="mcp:service:lookup""#), "{html}");
    assert!(!html.contains("no company MCP connection has this tool approved"));
    assert!(
        html.contains(&format!(r#"<option value="{first}" selected>"#)),
        "the only candidate"
    );
    assert!(
        !html.contains("a200"),
        "only the needed tool's approvals are read"
    );
    assert!(!html.contains("super-secret-token") && !html.contains("mcp.example.test"));
    // Past the per-tool bound the list says it was cut.
    for index in 1..=crate::application::workflow::reads::MAX_CONNECTIONS_PER_TOOL {
        approved_rows(&f, &format!("server-{index:03}"), &["lookup".to_owned()]).await;
    }
    let html = f.pane(f.owner, workflow, "editor").await.unwrap();
    assert!(
        html.contains("More connections offer this tool than the 20 listed here."),
        "{html}"
    );
    let select = &html[html.find(r#"name="mcp:service:lookup""#).unwrap()..];
    let select = &select[..select.find("</select>").unwrap()];
    assert_eq!(
        select.matches("<option value=\"").count(),
        21,
        "none plus 20 listed"
    );
    assert!(
        !html.contains(r#"selected>"#),
        "with several candidates nothing is preselected"
    );
}

/// A real MCP connection with a reviewed discovered tool and its approval, the way the catalog
/// writes them (p08 `mcp_test_support`), so capture accepts it.
async fn reviewed_connection(f: &Fixture) -> Uuid {
    use crate::application::workflow::mcp::{McpOperation, McpOperationWrite, McpOperations};
    use crate::application::workflow::publication::{
        ActionEffect, ActionRecovery, ApprovedActionPolicy, ToolSnapshot,
    };
    use crate::entities::mcp::{McpAuth, McpDiscoveredTool};
    use crate::use_cases::mcp::{McpConnectionWrite, McpPersistence};
    let input = json!({"type": "object", "properties": {"key": {"type": "string", "maxLength": 128}},
        "required": ["key"], "additionalProperties": false});
    let output = json!({"type": "object", "properties": {"result": {"type": "string", "maxLength": 128}},
        "required": ["result"], "additionalProperties": false});
    let connection = McpPersistence::create_mcp_connection(
        &f.persistence,
        f.company,
        McpConnectionWrite {
            slug: "tickets-server".into(),
            endpoint: "https://mcp.example.test/rpc/super-secret-token"
                .to_string()
                .try_into()
                .unwrap(),
            enabled: true,
            auth: McpAuth::None,
            discovered_tools: vec![McpDiscoveredTool {
                name: "lookup".to_string().try_into().unwrap(),
                description: "Find a ticket".into(),
                input_schema: input.clone(),
                output_schema: Some(output.clone()),
            }],
            tool_grants: vec!["lookup".to_string().try_into().unwrap()],
        },
    )
    .await
    .unwrap();
    f.persistence
        .register_mcp_operation(
            CompanyId::new(f.company),
            WorkflowActor::authenticated(f.owner).unwrap(),
            McpOperationWrite {
                connection: connection.id,
                catalog_revision: connection.revision,
                expected_version: None,
                operation: McpOperation {
                    tool: ToolSnapshot {
                        workflow: None,
                        company_id: CompanyId::new(f.company),
                        contract: crate::application::workflow::registry::ToolContract {
                            name: crate::domain::workflow::TypeName::parse("lookup").unwrap(),
                            connection: None,
                            input_schema: input,
                            output_schema: output,
                        },
                        policy: ApprovedActionPolicy {
                            capability: crate::domain::workflow::TypeName::parse("mcp.lookup")
                                .unwrap(),
                            policy_revision: 1,
                            effect: ActionEffect::Read,
                            recovery: ActionRecovery::Reconcile,
                        },
                    },
                    approval_required: false,
                },
            },
        )
        .await
        .unwrap();
    connection.id
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
async fn the_editor_guards_unsaved_text_and_reports_only_the_work_it_does() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    f.draft(workflow, "Guarded", &blank_source(workflow)).await;
    let html = f.pane(f.owner, workflow, "editor").await.unwrap();
    // F003: the editor is the dirty-state owner, and its own reload is the one exempt control.
    assert!(html.contains(r#"<form id="workflow-editor" class="space-y-4" data-workflow-editor"#));
    assert!(html.contains("data-editor-reload"));
    let script = crate::adapters::http::pages::application_javascript();
    assert!(script.contains("The editor has changes that are not saved. Discard them?"));
    assert!(
        script.contains("target.id !== 'workflow-pane'"),
        "only pane replacements ask"
    );
    assert!(script.contains("beforeunload"));
    // F004: neither submit button owns a form-scoped spinner; the status says which work runs.
    let form = &html[html.find(r#"id="workflow-editor""#).unwrap()..];
    let form = &form[..form.find("</form>").unwrap()];
    assert!(!form.contains("[.htmx-request_&]"), "{form}");
    assert!(form.contains(r#"id="workflow-editor-status""#));
    assert!(script.contains("'Saving…' : 'Saving and validating…'"));
    assert!(html.contains(r##"hx-target="#workflow-reference-body" hx-swap="outerHTML""##));
    let Html(reference) = catalogue(f.ui(f.owner), f.query()).await.unwrap();
    assert!(
        reference.starts_with(r#"<div id="workflow-reference-body""#),
        "{reference}"
    );
    assert!(
        !reference.contains("aria-busy"),
        "the loaded reference is not busy"
    );
}

#[tokio::test]
async fn a_source_and_description_that_start_with_a_newline_keep_it() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    let source = format!("\n{}", blank_source(workflow));
    save_draft(
        &f.engine,
        f.company,
        WorkflowActor::authenticated(f.owner).unwrap(),
        workflow,
        None,
        DraftContent {
            title: "Leading".into(),
            description: "\nsecond line".into(),
            source: source.clone(),
        },
    )
    .await
    .unwrap();
    let html = f.pane(f.owner, workflow, "editor").await.unwrap();
    // The parser drops one newline after the start tag; the markup puts it there itself.
    assert!(
        html.contains("aria-describedby=\"workflow-source-help\">\n\nformat_version"),
        "{html}"
    );
    assert!(html.contains("maxlength=\"4096\">\n\nsecond line"));
}

#[tokio::test]
async fn an_admin_of_another_company_reaches_no_write_or_version_of_this_one() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    f.draft(workflow, "Victim", &source_of("data.map", workflow))
        .await;
    let own = Query(CompanyQuery {
        company_id: f.other,
    });
    let fields = [
        ("expected_revision", "1"),
        ("title", "Taken"),
        ("source", "x"),
        ("intent", "validate"),
        ("command_key", "foreign-1"),
    ];
    assert!(matches!(
        save(
            f.ui(f.stranger),
            Path(workflow),
            Query(CompanyQuery {
                company_id: f.other
            }),
            form(&fields)
        )
        .await,
        Err(WorkflowError::App(AppError::NotFound(_)))
    ));
    assert!(matches!(
        publish(
            f.ui(f.stranger),
            Path(workflow),
            Query(CompanyQuery {
                company_id: f.other
            }),
            form(&fields)
        )
        .await,
        Err(WorkflowError::App(AppError::NotFound(_)))
    ));
    assert!(matches!(
        archive(f.ui(f.stranger), Path(workflow), own, form(&fields)).await,
        Err(WorkflowError::App(AppError::NotFound(_)))
    ));
    assert!(matches!(
        compare(
            f.ui(f.stranger),
            Path(workflow),
            Query(CompareQuery {
                company_id: f.other,
                from: "draft".into(),
                to: "draft".into()
            })
        )
        .await,
        Err(AppError::NotFound(_))
    ));
    let stored = f
        .engine
        .reads()
        .definition(CompanyId::new(f.company), WorkflowId::new(workflow))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (stored.summary.revision, stored.summary.archived),
        (1, false)
    );
    assert!(stored.versions.is_empty() && stored.summary.title == "Victim");
    // A version of another workflow, named through this workflow's compare, is not found.
    let other = Uuid::new_v4();
    let other_source = source_of("data.map", other);
    f.draft(other, "Other", &other_source).await;
    let (version, _) = publish_draft(
        &f.engine,
        f.company,
        WorkflowActor::authenticated(f.owner).unwrap(),
        other,
        PublishRequest {
            expected_revision: 1,
            command_key: "other-1",
            http: &[],
            mcp: &[],
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        compare(
            f.ui(f.owner),
            Path(workflow),
            Query(CompareQuery {
                company_id: f.company,
                from: version.as_uuid().to_string(),
                to: "draft".into()
            })
        )
        .await,
        Err(AppError::NotFound(_))
    ));
}

#[tokio::test]
async fn a_draft_that_moves_after_the_save_keeps_the_text_and_says_both() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    let source = blank_source(workflow);
    f.draft(workflow, "Moving", &source).await;
    // Someone saves revision 2 between the author's save (revision 1 here) and its validation.
    let theirs = source.replace("max_steps: 10", "max_steps: 11");
    save_draft(
        &f.engine,
        f.company,
        WorkflowActor::authenticated(f.admin).unwrap(),
        workflow,
        Some(1),
        DraftContent {
            title: "Moving".into(),
            description: String::new(),
            source: theirs,
        },
    )
    .await
    .unwrap();
    let mine = source.replace("max_steps: 10", "max_steps: 12");
    let state = validated(
        &f.ui(f.owner),
        f.company,
        workflow,
        1,
        Typed {
            title: "Moving".into(),
            description: String::new(),
            source: mine,
            expected_revision: 1,
        },
        pages::WorkflowSelections::default(),
    )
    .await
    .unwrap();
    let html = f
        .ui(f.owner)
        .pane_html(f.company, workflow, pages::WorkflowTab::Editor, state)
        .await
        .unwrap();
    assert!(
        html.contains(
            "Saved revision 1, but someone else saved revision 2 before it could be validated."
        ),
        "{html}"
    );
    assert!(html.contains("max_steps: 12"), "the author's text stays");
    assert_eq!(field_value(&html, "expected_revision"), "2");
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
async fn a_save_refreshes_the_sidebar_and_a_changed_publication_is_flagged() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    f.draft(workflow, "Before", &blank_source(workflow)).await;
    let html = f
        .save(
            workflow,
            &[
                ("expected_revision", "1"),
                ("title", "After"),
                ("source", &blank_source(workflow)),
                ("intent", "save"),
            ],
        )
        .await
        .unwrap();
    let menu = &html[html
        .find(r#"<ul id="workflow-menu" hx-swap-oob="true""#)
        .expect("the sidebar refresh")..];
    assert!(
        menu.contains("After") && menu.contains("draft revision 2"),
        "{menu}"
    );
    let published = text(
        publish(
            f.ui(f.owner),
            Path(workflow),
            f.query(),
            form(&[
                ("expected_revision", "2"),
                ("command_key", "flagged"),
                ("previewed_dependencies", &"0".repeat(64)),
            ]),
        )
        .await
        .unwrap(),
    )
    .await;
    assert!(
        published.contains("Its content differs from the preview you validated"),
        "{published}"
    );
}

#[test]
fn selections_read_only_well_formed_connection_fields() {
    let connection = Uuid::new_v4().to_string();
    let selections = pages::WorkflowSelections::from_fields([
        ("mcp:tickets:lookup", connection.as_str()),
        ("http:send", connection.as_str()),
        ("mcp:missing-tool", connection.as_str()),
        ("mcp:bad slot:lookup", connection.as_str()),
        ("http:<b>", connection.as_str()),
        ("http:none", ""),
        ("title", connection.as_str()),
    ]);
    assert_eq!(selections.mcp.len(), 1, "invalid names select nothing");
    assert_eq!(selections.http.len(), 1);
    let hidden = selections.hidden_fields();
    assert!(
        hidden.contains(r#"name="mcp:tickets:lookup""#) && hidden.contains(r#"name="http:send""#)
    );
    let (http, mcp) = selection_bodies(&selections);
    assert_eq!(
        (http[0].operation.as_str(), http[0].slot.as_deref()),
        ("send", None)
    );
    assert_eq!(
        (mcp[0].slot.as_str(), mcp[0].tool.as_str()),
        ("tickets", "lookup")
    );
}

#[tokio::test]
async fn every_workspace_write_refuses_a_cross_origin_cookie_post() {
    let mut app = Router::new();
    for pattern in WRITE_ROUTES {
        app = app.route(pattern, post(|| async { StatusCode::NO_CONTENT }));
    }
    let app = app.layer(axum::middleware::from_fn_with_state(
        crate::adapters::http::routes::ui::stream_tests::test_config(),
        crate::adapters::http::security::browser_security,
    ));
    for pattern in WRITE_ROUTES {
        let path = pattern
            .replace("{workflow_id}", &Uuid::new_v4().to_string())
            .replace("{template_id}", "reviewed-support");
        let request = |origin: &str| {
            Request::post(path.as_str())
                .header(header::HOST, "localhost")
                .header(header::COOKIE, "session=signed")
                .header(header::ORIGIN, origin)
                .body(axum::body::Body::empty())
                .unwrap()
        };
        let foreign = app
            .clone()
            .oneshot(request("https://foreign.example"))
            .await
            .unwrap();
        assert_eq!(foreign.status(), StatusCode::FORBIDDEN, "{path}");
        let same = app
            .clone()
            .oneshot(request("http://localhost"))
            .await
            .unwrap();
        assert_eq!(same.status(), StatusCode::NO_CONTENT, "{path}");
    }
}
