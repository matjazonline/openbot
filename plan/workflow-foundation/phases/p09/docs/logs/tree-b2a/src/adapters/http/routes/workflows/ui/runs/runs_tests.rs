//! The runs workspace over the production engine and a database of its own: lineage and steps
//! of a run with a child, paging over runs on channels an admin cannot see, other tenants and
//! plain members, hostile content and the CSRF boundary. The controls, the event stream and the
//! page renderings have their own files below.
use super::*;
use crate::adapters::http::routes::workflows::bindings::{
    BindingRequest, configure_binding, set_binding_activity, start_command,
};
use crate::adapters::http::routes::workflows::definitions::{
    PublishRequest, publish_draft, save_draft,
};
use crate::adapters::http::routes::workflows::tests::{example, owner_and_company, teammate};
use crate::adapters::persistence::{
    PostgresPersistence,
    test_support::{OwnDatabase, own_database},
};
use crate::application::workflow::RelatedChannelId;
use crate::application::workflow::lifecycle::DraftContent;
use crate::infra::events::{MailboxEvent, MailboxEvents, RunScope};
use crate::use_cases::{agent::AgentUseCases, company::CompanyUseCases, user::UserUseCases};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use std::time::Duration;
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

#[path = "controls_tests.rs"]
mod controls_tests;
#[path = "events_tests.rs"]
mod events_tests;
#[path = "pages_tests.rs"]
mod pages_tests;

struct Fixture {
    _db: OwnDatabase,
    persistence: PostgresPersistence,
    engine: Engine,
    owner: Uuid,
    company: Uuid,
    admin: Uuid,
    member: Uuid,
    stranger: Uuid,
    /// The stranger's own company.
    other: Uuid,
}

impl Fixture {
    async fn new() -> Option<Self> {
        let db = own_database().await?;
        let persistence = PostgresPersistence::new(db.pool.clone());
        let (owner, company) = owner_and_company(&persistence, "Runs").await;
        let (stranger, other) = owner_and_company(&persistence, "Elsewhere").await;
        let admin = teammate(&persistence, company, "admin").await;
        let member = teammate(&persistence, company, "member").await;
        Some(Self {
            _db: db,
            engine: Arc::new(WorkflowEngine::for_tests(persistence.clone())),
            persistence,
            owner,
            company,
            admin,
            member,
            stranger,
            other,
        })
    }

    fn ui(&self, user: Uuid) -> Ui {
        ui_over(&self.persistence, &self.engine, user)
    }

    fn query(&self) -> Query<CompanyQuery> {
        Query(CompanyQuery {
            company_id: self.company,
        })
    }

    fn actor(&self) -> WorkflowActor {
        WorkflowActor::authenticated(self.owner).unwrap()
    }

    /// `source` saved as a new draft and published; the version's id.
    async fn published(&self, workflow: Uuid, title: &str, source: &Value) -> Uuid {
        let draft = save_draft(
            &self.engine,
            self.company,
            self.actor(),
            workflow,
            None,
            DraftContent {
                title: title.into(),
                description: String::new(),
                source: serde_json::to_string_pretty(source).unwrap(),
            },
        )
        .await
        .unwrap();
        let key = Uuid::new_v4().to_string();
        publish_draft(
            &self.engine,
            self.company,
            self.actor(),
            workflow,
            PublishRequest {
                expected_revision: draft.revision().get(),
                command_key: &key,
                http: &[],
                mcp: &[],
            },
        )
        .await
        .unwrap()
        .0
        .as_uuid()
    }

    /// An active binding of `version` in `association`.
    async fn bound(&self, version: Uuid, association: RelatedAssociation) -> Uuid {
        let binding = Uuid::new_v4();
        configure_binding(
            &self.engine,
            self.company,
            self.actor(),
            binding,
            BindingRequest {
                expected_state_revision: None,
                association,
                version,
                params: json!({}),
                resources: BTreeMap::new(),
                run_as: None,
                message_trigger: None,
            },
        )
        .await
        .unwrap();
        set_binding_activity(&self.engine, self.company, self.actor(), binding, 1, true)
            .await
            .unwrap();
        binding
    }

    async fn start(&self, binding: Uuid, input: Value) -> Uuid {
        start_command(
            &self.engine,
            self.company,
            self.actor(),
            binding,
            None,
            input,
            Uuid::new_v4().to_string(),
            None,
        )
        .await
        .unwrap()
        .0
        .as_uuid()
    }

    async fn state(&self, run: Uuid) -> (String, u64) {
        let (state, revision): (String, i64) =
            sqlx::query_as("SELECT state, revision FROM workflow_runs WHERE id = $1")
                .bind(run)
                .fetch_one(self.persistence.pool())
                .await
                .unwrap();
        (state, revision as u64)
    }

    /// Run the production workers until every run in `runs` is terminal.
    async fn run_until_done(&self, runs: &[Uuid]) {
        let shutdown = CancellationToken::new();
        let handles = self.engine.spawn(&shutdown);
        let settled = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let mut done = true;
                for run in runs {
                    let (state, _) = self.state(*run).await;
                    done &= matches!(state.as_str(), "succeeded" | "failed" | "cancelled");
                }
                if done {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await;
        shutdown.cancel();
        for (name, handle) in handles {
            tokio::time::timeout(Duration::from_secs(10), handle)
                .await
                .unwrap_or_else(|_| panic!("{name} did not stop"))
                .unwrap();
        }
        assert!(settled.is_ok(), "the runs never finished");
    }

    async fn pane(&self, user: Uuid, run: Uuid) -> AppResult<String> {
        pane(self.ui(user), Path(run), pane_query(self.company, None))
            .await
            .map(|html| html.0)
    }

    /// The body a wake-up reloads.
    async fn body(&self, user: Uuid, run: Uuid) -> String {
        pane(
            self.ui(user),
            Path(run),
            pane_query(self.company, Some(PanePart::Body)),
        )
        .await
        .unwrap()
        .0
    }

    async fn list(&self, user: Uuid, after: Option<Uuid>) -> String {
        list(
            self.ui(user),
            Query(ListQuery {
                company_id: self.company,
                binding_id: None,
                state: None,
                after,
                more: after.map(|_| "1".into()),
            }),
        )
        .await
        .unwrap()
        .0
    }

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
}

fn pane_query(company_id: Uuid, part: Option<PanePart>) -> Query<PaneQuery> {
    Query(PaneQuery { company_id, part })
}

/// The runs UI for `user` over `persistence` and its engine.
fn ui_over(persistence: &PostgresPersistence, engine: &Engine, user: Uuid) -> Ui {
    let shared = Arc::new(persistence.clone());
    Ui {
        engine: engine.clone(),
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

fn form(fields: &[(&str, &str)]) -> axum::body::Bytes {
    axum::body::Bytes::from(
        url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(fields)
            .finish(),
    )
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

/// A child that maps its text input, and a parent that calls it.
async fn parent_and_child(f: &Fixture) -> (Uuid, Uuid) {
    let child = Uuid::new_v4();
    let mut source = example("data.map", child);
    source["input_schema"] = json!({"type": "object", "properties": {"text": {"type": "string", "maxLength": 128}}, "required": ["text"]});
    source["output_schema"] = json!(true);
    let child_version = f.published(child, "Child", &source).await;
    let parent = Uuid::new_v4();
    let mut source = example("workflow.call", parent);
    source["steps"]["start"]["child_workflow_id"] = json!(child.to_string());
    source["steps"]["start"]["child_version_id"] = json!(child_version.to_string());
    let parent_version = f.published(parent, "Parent", &source).await;
    (parent_version, child_version)
}

#[tokio::test]
async fn a_run_with_a_child_shows_its_lineage_steps_and_controls_through_the_parent() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (parent_version, _) = parent_and_child(&f).await;
    let binding = f.bound(parent_version, RelatedAssociation::Company).await;
    let run = f.start(binding, json!({"note": "first"})).await;
    f.run_until_done(&[run]).await;
    let child: Uuid = sqlx::query_scalar(
        "SELECT child_run_id FROM workflow_run_parents WHERE parent_run_id = $1",
    )
    .bind(run)
    .fetch_one(f.persistence.pool())
    .await
    .unwrap();

    let parent_pane = f.pane(f.owner, run).await.unwrap();
    assert!(
        parent_pane.starts_with(r#"<section id="run-pane""#),
        "{parent_pane}"
    );
    // Wake-ups reload the body alone; a finished run has no tick, and its finished steps keep
    // their place across those reloads.
    assert!(
        parent_pane.contains(&format!(r#"hx-trigger="sse:run-{run}, sse:resync""#)),
        "{parent_pane}"
    );
    assert!(parent_pane.contains(&format!(
        "/ui/workflow-runs/{run}?company_id={}&amp;part=body",
        f.company
    )));
    // The reload's sync sits on an empty leaf: nothing inherits it, so no write joins it.
    assert!(parent_pane.contains(
        r##"hx-target="#run-body" hx-swap="outerHTML" hx-sync="#run-body:replace"></span>"##
    ));
    assert!(!parent_pane.contains("this:replace"), "{parent_pane}");
    assert!(parent_pane.contains(r#"hx-preserve="true""#));
    let reloaded = f.body(f.owner, run).await;
    assert!(
        reloaded.starts_with(r#"<div id="run-body""#) && !reloaded.contains(r#"id="run-pane""#),
        "{reloaded}"
    );
    assert!(parent_pane.contains("succeeded") && parent_pane.contains("Child runs"));
    assert!(parent_pane.contains(&pages::short_id(child)));
    assert!(parent_pane.contains("Rerun") && !parent_pane.contains("Cancel run"));
    assert!(parent_pane.contains("Timeline") && parent_pane.contains("Run created"));
    assert!(
        parent_pane.contains("<svg"),
        "the graph from the run's bundle"
    );

    let child_pane = f.pane(f.owner, child).await.unwrap();
    assert!(child_pane.contains("Child of run"), "{child_pane}");
    assert!(child_pane.contains("controlled through its parent"));
    assert!(!child_pane.contains("/cancel?") && !child_pane.contains("/rerun?"));

    // The parent's step opens to what it froze, its output and the child it started.
    let execution: Uuid = sqlx::query_scalar(
        "SELECT parent_execution_id FROM workflow_run_parents WHERE child_run_id = $1",
    )
    .bind(child)
    .fetch_one(f.persistence.pool())
    .await
    .unwrap();
    let body = step(f.ui(f.owner), Path((run, execution)), f.query())
        .await
        .unwrap()
        .0;
    assert!(
        body.contains("Inputs (frozen when the step started)"),
        "{body}"
    );
    assert!(body.contains("Output") && body.contains("Child runs"));
    assert!(body.contains(&pages::short_id(child)));
    // A child's own control is refused with the parent named.
    let refused = cancel(
        f.ui(f.owner),
        Path(child),
        f.query(),
        form(&[("expected_revision", "1"), ("command_key", "child-cancel")]),
    )
    .await
    .unwrap()
    .0;
    assert!(
        refused.contains("controlled through its parent run"),
        "{refused}"
    );
}

#[tokio::test]
async fn the_list_pages_over_runs_an_admin_cannot_see_and_labels_reruns_and_children() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    let version = f
        .published(workflow, "Map", &example("data.map", workflow))
        .await;
    let board = f.channel("Board", "board", true).await;
    let shown = f.bound(version, RelatedAssociation::Company).await;
    let hidden = f
        .bound(
            version,
            RelatedAssociation::Channel(RelatedChannelId::new(board)),
        )
        .await;
    // 51 queued runs (no workers), the oldest few on the hidden channel.
    let mut runs = Vec::new();
    for index in 0..51 {
        let binding = if index < 3 { hidden } else { shown };
        runs.push(f.start(binding, json!({})).await);
    }
    let owner_first = f.list(f.owner, None).await;
    assert!(owner_first.contains(r#"id="run-list""#), "{owner_first}");
    assert!(owner_first.contains(r#"hx-trigger="sse:runs, sse:resync""#));
    assert!(owner_first.contains(
        r##"hx-target="#run-list" hx-swap="outerHTML" hx-sync="#run-list:replace"></span>"##
    ));
    assert!(!owner_first.contains("this:replace"));
    assert!(owner_first.contains("Load more"));
    // The admin's first page holds the newest 50; its second page holds the oldest, hidden one.
    let admin_first = f.list(f.admin, None).await;
    assert!(admin_first.contains("Load more"), "{admin_first}");
    assert!(admin_first.contains("2 runs of this page are on channels you cannot see"));
    let next = field_after(&admin_first, "&amp;after=");
    let admin_second = f.list(f.admin, Some(next)).await;
    assert!(!admin_second.contains(r#"id="run-list""#), "entries only");
    assert!(admin_second.contains("1 run of this page is on channels you cannot see"));
    assert!(!admin_second.contains("Load more"));
    assert!(matches!(
        f.pane(f.admin, runs[0]).await,
        Err(AppError::NotFound(_))
    ));
    // A rerun is labelled in the list and on its page.
    let cancelled = runs[50];
    let (_, revision) = f.state(cancelled).await;
    let answered = cancel(
        f.ui(f.owner),
        Path(cancelled),
        f.query(),
        form(&[
            ("expected_revision", &revision.to_string()),
            ("command_key", "cancel-last"),
        ]),
    )
    .await
    .unwrap()
    .0;
    assert!(answered.contains("Cancelled (revision"), "{answered}");
    let rerun = rerun(
        f.ui(f.owner),
        Path(cancelled),
        f.query(),
        form(&[("command_key", "rerun-last")]),
    )
    .await
    .unwrap();
    let pushed = rerun
        .headers()
        .get("HX-Push-Url")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let page = String::from_utf8(
        axum::body::to_bytes(rerun.into_body(), 8 << 20)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(page.contains("as a rerun of"), "{page}");
    assert!(page.contains("A rerun of"));
    let new_run: Uuid = pushed.rsplit("run_id=").next().unwrap().parse().unwrap();
    let rerun_of: Option<Uuid> =
        sqlx::query_scalar("SELECT rerun_of_run_id FROM workflow_runs WHERE id = $1")
            .bind(new_run)
            .fetch_one(f.persistence.pool())
            .await
            .unwrap();
    assert_eq!(rerun_of, Some(cancelled));
    let first = f.list(f.owner, None).await;
    assert!(first.contains(&format!("rerun of {}", pages::short_id(cancelled))));
}

/// The cursor after `marker` in a page (up to the next `&` or `"`).
fn field_after(html: &str, marker: &str) -> Uuid {
    let start = html.find(marker).unwrap() + marker.len();
    let end = start + html[start..].find(['&', '"']).unwrap();
    html[start..end].parse().unwrap()
}

#[tokio::test]
async fn other_tenants_and_plain_members_reach_no_run_and_hostile_content_is_escaped() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let hostile = format!("<{}>alert(1)</script>", "script");
    let escaped = "&lt;script&gt;alert(1)&lt;/script&gt;";
    let workflow = Uuid::new_v4();
    let version = f
        .published(workflow, &hostile, &example("data.map", workflow))
        .await;
    let binding = f.bound(version, RelatedAssociation::Company).await;
    let run = f.start(binding, json!({"note": hostile})).await;
    f.run_until_done(&[run]).await;
    let shown = f.pane(f.owner, run).await.unwrap();
    assert!(
        !shown.contains(&hostile) && shown.contains(escaped),
        "{shown}"
    );
    let execution: Uuid =
        sqlx::query_scalar("SELECT id FROM workflow_executions WHERE run_id = $1")
            .bind(run)
            .fetch_one(f.persistence.pool())
            .await
            .unwrap();
    let body = step(f.ui(f.owner), Path((run, execution)), f.query())
        .await
        .unwrap()
        .0;
    assert!(!body.contains(&hostile), "{body}");

    for (user, company) in [(f.member, f.company), (f.stranger, f.other)] {
        let query = || {
            Query(CompanyQuery {
                company_id: company,
            })
        };
        for part in [None, Some(PanePart::Body)] {
            assert!(matches!(
                pane(f.ui(user), Path(run), pane_query(company, part)).await,
                Err(AppError::NotFound(_))
            ));
        }
        assert!(matches!(
            step(f.ui(user), Path((run, execution)), query()).await,
            Err(AppError::NotFound(_))
        ));
        let cancelled = cancel(
            f.ui(user),
            Path(run),
            query(),
            form(&[("expected_revision", "1"), ("command_key", "x-cancel")]),
        )
        .await;
        assert!(matches!(
            cancelled,
            Err(WorkflowError::App(AppError::NotFound(_)))
        ));
    }
}

#[tokio::test]
async fn every_runs_write_refuses_a_cross_origin_cookie_post() {
    let mut app = Router::new();
    for pattern in WRITE_ROUTES {
        app = app.route(pattern, post(|| async { StatusCode::NO_CONTENT }));
    }
    let app = app.layer(axum::middleware::from_fn_with_state(
        crate::adapters::http::routes::ui::stream_tests::test_config(),
        crate::adapters::http::security::browser_security,
    ));
    for pattern in WRITE_ROUTES {
        let path = pattern.replace("{run_id}", &Uuid::new_v4().to_string());
        assert!(!path.contains('{'), "{path}");
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
