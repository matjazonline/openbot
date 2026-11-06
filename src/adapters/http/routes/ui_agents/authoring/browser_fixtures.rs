//! Browser inputs are produced by the real scoped handlers over the owned test database.
use super::test_support::{Fixture, fields, form};
use super::*;
use crate::application::workflow::{http::*, publication::ToolSnapshot, registry::ToolContract};
use crate::domain::workflow::TypeName;
use crate::use_cases::agent::AgentPersistence;
use std::path::Path as FilePath;

async fn selected_catalog(f: &Fixture, kind: AgentToolKind, connection: Option<Uuid>) -> String {
    catalog::catalog_form(
        f.workspace(f.f.owner.id),
        Path(f.agent.id),
        Query(catalog::CatalogQuery {
            company_id: f.f.company.id,
            kind,
            connection,
            after_connection: None,
            after_operation: None,
        }),
    )
    .await
    .unwrap()
    .0
}

async fn http_connection(f: &Fixture) -> Uuid {
    let connection =
        f.f.persistence
            .create_http(
                f.scope().company,
                f.scope().actor,
                HttpConnectionWrite {
                    origin: HttpOrigin::parse("http://127.0.0.1:9").unwrap(),
                    bearer: false,
                },
            )
            .await
            .unwrap();
    f.f.persistence.register_http_operation(f.scope().company, f.scope().actor, connection.id, connection.revision, HttpOperation {
        tool: ToolSnapshot { company_id: f.scope().company, workflow: None,
            contract: ToolContract { name: TypeName::parse("http.fetch").unwrap(), connection: None,
                input_schema: serde_json::json!({"type":"object","properties":{"method":{"type":"string"},"path":{"type":"string"}},"required":["method","path"],"additionalProperties":false}),
                output_schema: serde_json::json!(true) },
            policy: ApprovedActionPolicy { capability: TypeName::parse("http.request").unwrap(), policy_revision: 1,
                effect: ActionEffect::Read, recovery: ActionRecovery::Reconcile } }, approval_required: true,
    }).await.unwrap();
    connection.id
}

fn save(root: &FilePath, name: &str, value: &str) {
    std::fs::write(root.join(name), value).unwrap();
}

struct BrowserFixture {
    fixture: Fixture,
    alpha: Uuid,
    zeta: Uuid,
    other: crate::entities::agent::Agent,
    initial: AgentAuthoringState,
    key: String,
}
struct BrowserSubmission {
    fields: Vec<(&'static str, String)>,
    current_revision: crate::entities::agent_capability::AgentDraftRevision,
}
fn set_field(fields: &mut [(&str, String)], name: &str, value: impl Into<String>) {
    fields
        .iter_mut()
        .find(|(field, _)| *field == name)
        .unwrap()
        .1 = value.into();
}

async fn initial_browser_fixture(root: &FilePath) -> BrowserFixture {
    let f = Fixture::new().await.expect("owned PostgreSQL is required");
    let alpha = f.skill("alpha", Vec::new()).await;
    let zeta = f.skill("zeta", Vec::new()).await;
    let other = AgentPersistence::create(
        f.f.persistence.as_ref(),
        f.f.company.id,
        AgentWrite {
            name: "Navigator".into(),
            slug: "navigator".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let mut initial = fields(f.agent.draft_revision, "[]");
    set_field(&mut initial, "name", "Authoring");
    set_field(
        &mut initial,
        "skill_ids",
        format!("{},{}", zeta.id, alpha.id),
    );
    super::super::update_agent(
        f.workspace(f.f.owner.id),
        Path(f.agent.id),
        f.query(),
        Form(form(&initial).await),
    )
    .await
    .unwrap();
    let initial = f.state().await;
    let page = super::super::agents_page(
        f.workspace(f.f.owner.id),
        Query(WorkspaceQuery {
            company_id: Some(f.f.company.id),
            agent_id: Some(f.agent.id),
            new: None,
        }),
    )
    .await
    .unwrap()
    .0;
    let key = page
        .split("name=\"key\" value=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_owned();
    save(root, "page.html", &page);
    save(root, "app.js", &pages::application_javascript());
    save(root, "theme-init.js", pages::theme_init_javascript());
    BrowserFixture {
        fixture: f,
        alpha: alpha.id,
        zeta: zeta.id,
        other,
        initial,
        key,
    }
}

async fn catalog_fixtures(root: &FilePath, f: &Fixture) -> Uuid {
    save(
        root,
        "builtin.html",
        &selected_catalog(f, AgentToolKind::Builtin, None).await,
    );
    let connection = http_connection(f).await;
    save(
        root,
        "http.html",
        &selected_catalog(f, AgentToolKind::Http, None).await,
    );
    save(
        root,
        "http-selected.html",
        &selected_catalog(f, AgentToolKind::Http, Some(connection)).await,
    );
    save(
        root,
        "workflow.html",
        &selected_catalog(f, AgentToolKind::Workflow, None).await,
    );
    connection
}

async fn rejected_fixtures(root: &FilePath, state: &BrowserFixture) -> BrowserSubmission {
    let f = &state.fixture;
    let mut concurrent = fields(state.initial.agent.draft_revision, "[]");
    set_field(&mut concurrent, "name", "Concurrent winner");
    super::super::update_agent(
        f.workspace(f.f.owner.id),
        Path(f.agent.id),
        f.query(),
        Form(form(&concurrent).await),
    )
    .await
    .unwrap();
    let current = f.state().await;
    let view = f.workspace(f.f.owner.id);
    save(
        root,
        "fresh-pane.html",
        &view
            .view(&f.f.company)
            .edit_pane(&current.agent, None, None)
            .await
            .unwrap(),
    );
    let tools = r#"[{"binding":null,"alias":"respond","choice":{"kind":"builtin","tool":"message_reply"}}]"#;
    let mut submitted = fields(state.initial.agent.draft_revision, tools);
    set_field(&mut submitted, "name", "Draft <browser>");
    set_field(
        &mut submitted,
        "system_prompt",
        "browser unsaved prompt marker",
    );
    set_field(
        &mut submitted,
        "skill_ids",
        format!("{},{}", state.alpha, state.zeta),
    );
    let rejected = super::super::update_agent(
        f.workspace(f.f.owner.id),
        Path(f.agent.id),
        f.query(),
        Form(form(&submitted).await),
    )
    .await
    .unwrap();
    save(root, "stale-pane.html", &test_support::text(rejected).await);
    let refused = publication::publish_form(
        f.workspace(f.f.owner.id),
        Path(f.agent.id),
        f.query(),
        Form(publication::PublicationForm {
            expected_draft_revision: state.initial.agent.draft_revision,
            expected_active_revision: String::new(),
            key: AgentPublicationKey::parse(&state.key).unwrap(),
        }),
    )
    .await
    .unwrap();
    save(root, "publication.html", &refused.0);
    BrowserSubmission {
        fields: submitted,
        current_revision: current.agent.draft_revision,
    }
}

async fn accepted_fixtures(
    root: &FilePath,
    state: &BrowserFixture,
    mut submitted: BrowserSubmission,
    connection: Uuid,
) {
    let f = &state.fixture;
    set_field(
        &mut submitted.fields,
        "expected_revision",
        submitted.current_revision.get().to_string(),
    );
    let response = super::super::update_agent(
        f.workspace(f.f.owner.id),
        Path(f.agent.id),
        f.query(),
        Form(form(&submitted.fields).await),
    )
    .await
    .unwrap()
    .into_response();
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_str().unwrap().to_string()))
        .collect::<Vec<_>>();
    let response_body = String::from_utf8(
        axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    save(
        root,
        "late-save.json",
        &serde_json::to_string(&serde_json::json!({"headers":headers,"body":response_body}))
            .unwrap(),
    );
    let view = f.workspace(f.f.owner.id);
    save(
        root,
        "other-pane.html",
        &view
            .view(&f.f.company)
            .edit_pane(&state.other, None, None)
            .await
            .unwrap(),
    );
    save(root, "metadata.json", &serde_json::to_string(&serde_json::json!({"company":f.f.company.id,"agent":f.agent.id,"other":state.other.id,"connection":connection,"alpha":state.alpha,"zeta":state.zeta,"initial_revision":state.initial.agent.draft_revision,"key":state.key})).unwrap());
}

#[tokio::test]
#[ignore = "exports native authoring responses for the browser gate"]
async fn dump_autonomous_agent_authoring_browser_fixtures() {
    let directory = std::env::var("AGENT_AUTHORING_BROWSER_FIXTURES")
        .expect("explicit fixture output directory");
    let root = FilePath::new(&directory);
    std::fs::create_dir_all(root).unwrap();
    // Box each native fixture transition so the browser export keeps the stock test stack budget.
    let state = Box::pin(initial_browser_fixture(root)).await;
    let connection = Box::pin(catalog_fixtures(root, &state.fixture)).await;
    let submitted = Box::pin(rejected_fixtures(root, &state)).await;
    Box::pin(accepted_fixtures(root, &state, submitted, connection)).await;
}
