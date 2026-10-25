//! The authoring workspace over the production engine and a database of its own: the fixture
//! the area modules share, and the library — the sidebar, keyed creates and copies, and the
//! catalogue.
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

    /// `source` saved over the draft (creating it when new) and published; the version's id.
    async fn published(&self, workflow: Uuid, title: &str, source: &str) -> Uuid {
        let actor = WorkflowActor::authenticated(self.owner).unwrap();
        let current = self
            .engine
            .reads()
            .definition(CompanyId::new(self.company), WorkflowId::new(workflow))
            .await
            .unwrap()
            .map(|detail| detail.summary.revision);
        let draft = save_draft(
            &self.engine,
            self.company,
            actor,
            workflow,
            current,
            DraftContent {
                title: title.into(),
                description: String::new(),
                source: source.into(),
            },
        )
        .await
        .unwrap();
        let key = Uuid::new_v4().to_string();
        let (version, _) = publish_draft(
            &self.engine,
            self.company,
            actor,
            workflow,
            PublishRequest {
                expected_revision: draft.revision().get(),
                command_key: &key,
                http: &[],
                mcp: &[],
            },
        )
        .await
        .unwrap();
        version.as_uuid()
    }

    /// A channel of the company; `restricted` makes it one an admin who is not on its allowlist
    /// cannot see (the owner still can).
    async fn channel(&self, name: &str, slug: &str, restricted: bool) -> Uuid {
        use crate::use_cases::channel::{ChannelPersistence, ChannelWrite};
        let channel = ChannelPersistence::create(
            &self.persistence,
            self.company,
            ChannelWrite {
                name: name.into(),
                slug: slug.into(),
                enabled: false,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap()
        .id;
        if restricted {
            sqlx::query("UPDATE channels SET access_mode = 'allowlist' WHERE id = $1")
                .bind(channel)
                .execute(self.persistence.pool())
                .await
                .unwrap();
        }
        channel
    }

    /// A company agent with a model, by slug.
    async fn agent(&self, name: &str, slug: &str) -> Uuid {
        sqlx::query(
            "INSERT INTO company_model_connections(company_id, provider, api_key, models) \
             VALUES ($1, 'openai', 'fixture-key', '{gpt-test}') ON CONFLICT DO NOTHING",
        )
        .bind(self.company)
        .execute(self.persistence.pool())
        .await
        .unwrap();
        AgentPersistence::create(
            &self.persistence,
            self.company,
            AgentWrite {
                name: name.into(),
                slug: slug.into(),
                provider: Some("openai".into()),
                model: Some("gpt-test".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id
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
            tools: None,
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

/// A real MCP connection with a reviewed discovered tool and its approval, the way the catalog
/// writes them (p08 `mcp_test_support`), so capture accepts it.
async fn reviewed_connection(f: &Fixture) -> Uuid {
    reviewed_connection_at(f, "https://mcp.example.test/rpc/super-secret-token").await
}
async fn reviewed_connection_at(f: &Fixture, endpoint: &str) -> Uuid {
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
            endpoint: endpoint.to_string().try_into().unwrap(),
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

#[path = "access_tests.rs"]
mod access_tests;
#[path = "bindings_tests.rs"]
mod bindings_tests;
#[path = "editor_tests.rs"]
mod editor_tests;
#[path = "publish_tests.rs"]
mod publish_tests;
#[path = "tools_tests.rs"]
mod tools_tests;

#[path = "destination_tests.rs"]
mod destination_tests;

#[path = "sample_tests.rs"]
mod sample_tests;
