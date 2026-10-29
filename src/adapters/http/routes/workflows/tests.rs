//! The workflow routes over the production engine: each route against another tenant's ids and a
//! member who is not an admin, revision conflicts, key replays and conflicts, workflow tools and
//! agent grants (WF-C003), and the decision page a review notice links to.
use super::*;
#[path = "jev_tests.rs"]
mod jev_tests;
use crate::adapters::http::session::SessionAuthority;
use crate::adapters::persistence::{
    PostgresPersistence,
    test_support::{OwnDatabase, own_database},
};
use crate::application::workflow::registry;
use crate::infra::config::AppConfig;
use crate::use_cases::{
    agent::{AgentPersistence, AgentWrite},
    channel::{ChannelPersistence, ChannelWrite},
    company::{CompanyPersistence, CompanyWrite},
    user::UserPersistence,
};
use axum::{
    Form,
    extract::{Path, Query, State},
};
use serde::de::DeserializeOwned;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Each test has a database of its own: the decision and child tests run the engine's workers,
/// whose claims are unscoped (persistence `AGENTS.md`), and every test's runs are claimable work.
struct Fixture {
    _db: OwnDatabase,
    persistence: PostgresPersistence,
    engine: Engine,
    owner: Uuid,
    company: Uuid,
    /// A plain member of `company`, and an admin of it: both with a principal, as the app makes.
    member: Uuid,
    admin: Uuid,
    /// The owner of `other`, and nothing of `company`.
    stranger: Uuid,
    other: Uuid,
}

pub(super) async fn owner_and_company(p: &PostgresPersistence, name: &str) -> (Uuid, Uuid) {
    let suffix = Uuid::new_v4().simple().to_string();
    let owner = p
        .create_user(&suffix, &format!("{suffix}@example.test"), "hash")
        .await
        .unwrap()
        .id;
    let company = CompanyPersistence::create(
        p,
        owner,
        CompanyWrite {
            name: name.into(),
            slug: suffix,
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    (owner, company)
}

/// A user of `company` with `role` and their principal.
pub(super) async fn teammate(p: &PostgresPersistence, company: Uuid, role: &str) -> Uuid {
    let suffix = Uuid::new_v4().simple().to_string();
    let user = p
        .create_user(&suffix, &format!("{suffix}@example.test"), "hash")
        .await
        .unwrap()
        .id;
    sqlx::query(
        "INSERT INTO company_members(id, company_id, user_id, role) VALUES ($1, $2, $3, $4)",
    )
    .bind(Uuid::new_v4())
    .bind(company)
    .bind(user)
    .bind(role)
    .execute(p.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO principals(id, company_id, kind, user_id, display_label) \
         VALUES ($1, $2, 'person', $3, $4)",
    )
    .bind(Uuid::new_v4())
    .bind(company)
    .bind(user)
    .bind(role)
    .execute(p.pool())
    .await
    .unwrap();
    user
}

async fn answer(response: impl IntoResponse) -> (StatusCode, Value) {
    let response = response.into_response();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 4 << 20)
        .await
        .unwrap();
    let body = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    (status, body)
}

fn body<T: DeserializeOwned>(value: Value) -> Json<T> {
    Json(serde_json::from_value(value).unwrap())
}

fn by(id: Uuid) -> AuthenticatedUser {
    AuthenticatedUser { id }
}

fn uuid(value: &Value) -> Uuid {
    value.as_str().unwrap().parse().unwrap()
}

/// A registered example's source, as a fresh workflow's.
pub(super) fn example(name: &str, workflow: Uuid) -> Value {
    let mut source: Value = serde_json::from_str(&registry::example(name).unwrap().source).unwrap();
    source["workflow_id"] = json!(workflow.to_string());
    source
}

impl Fixture {
    async fn new() -> Option<Self> {
        let db = own_database().await?;
        let persistence = PostgresPersistence::new(db.pool.clone());
        let (owner, company) = owner_and_company(&persistence, "Workflows").await;
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

    fn state(&self) -> State<Engine> {
        State(self.engine.clone())
    }

    async fn save(
        &self,
        user: Uuid,
        company: Uuid,
        workflow: Uuid,
        expected: Option<u64>,
        source: &Value,
    ) -> (StatusCode, Value) {
        answer(
            definitions::save(
                self.state(),
                by(user),
                Path((company, workflow)),
                body(json!({"expected_revision": expected, "title": "Report",
                    "source": source.to_string()})),
            )
            .await,
        )
        .await
    }

    async fn publish(
        &self,
        user: Uuid,
        company: Uuid,
        workflow: Uuid,
        revision: u64,
        key: &str,
    ) -> (StatusCode, Value) {
        answer(
            definitions::publish(
                self.state(),
                by(user),
                Path((company, workflow)),
                body(json!({"expected_revision": revision, "command_key": key})),
            )
            .await,
        )
        .await
    }

    /// `source` saved and published as a new workflow of `company` by `owner`: (workflow, version).
    async fn published(&self, owner: Uuid, company: Uuid, source: Value) -> (Uuid, Uuid) {
        let workflow = uuid(&source["workflow_id"]);
        let (status, saved) = self.save(owner, company, workflow, None, &source).await;
        assert_eq!(status, StatusCode::OK, "{saved}");
        // Publish keys are the company's, so each workflow's first publish has its own.
        let key = format!("publish-{}", workflow.simple());
        let (status, published) = self.publish(owner, company, workflow, 1, &key).await;
        assert_eq!(status, StatusCode::OK, "{published}");
        (workflow, uuid(&published["version_id"]))
    }

    async fn configure(
        &self,
        user: Uuid,
        company: Uuid,
        binding: Uuid,
        expected: Option<u64>,
        version: Uuid,
    ) -> (StatusCode, Value) {
        self.configure_on(
            user,
            company,
            binding,
            expected,
            version,
            json!({"kind": "company"}),
        )
        .await
    }

    async fn configure_on(
        &self,
        user: Uuid,
        company: Uuid,
        binding: Uuid,
        expected: Option<u64>,
        version: Uuid,
        association: Value,
    ) -> (StatusCode, Value) {
        answer(
            bindings::configure(
                self.state(),
                by(user),
                Path((company, binding)),
                body(json!({"expected_state_revision": expected,
                    "association": association, "version_id": version})),
            )
            .await,
        )
        .await
    }

    async fn activate(
        &self,
        user: Uuid,
        company: Uuid,
        binding: Uuid,
        expected: u64,
    ) -> (StatusCode, Value) {
        answer(
            bindings::activate(
                self.state(),
                by(user),
                Path((company, binding)),
                body(json!({"expected_state_revision": expected})),
            )
            .await,
        )
        .await
    }

    /// An active company binding of `version`, by `owner`.
    async fn bound(&self, owner: Uuid, company: Uuid, version: Uuid) -> Uuid {
        let binding = Uuid::new_v4();
        let (status, configured) = self.configure(owner, company, binding, None, version).await;
        assert_eq!(status, StatusCode::OK, "{configured}");
        let revision = configured["state_revision"].as_u64().unwrap();
        let (status, active) = self.activate(owner, company, binding, revision).await;
        assert_eq!(status, StatusCode::OK, "{active}");
        binding
    }

    async fn start(
        &self,
        user: Uuid,
        company: Uuid,
        binding: Uuid,
        key: &str,
        input: Value,
    ) -> (StatusCode, Value) {
        answer(
            bindings::start_run(
                self.state(),
                by(user),
                Path((company, binding)),
                body(json!({"input": input, "command_key": key})),
            )
            .await,
        )
        .await
    }

    async fn run(&self, user: Uuid, company: Uuid, run: Uuid) -> (StatusCode, Value) {
        answer(bindings::run(self.state(), by(user), Path((company, run))).await).await
    }

    /// A published, bound workflow of `company` with one started run: its ids.
    async fn resources(&self, owner: Uuid, company: Uuid) -> Ids {
        let (workflow, version) = self
            .published(owner, company, example("data.map", Uuid::new_v4()))
            .await;
        let binding = self.bound(owner, company, version).await;
        let (status, started) = self.start(owner, company, binding, "run", json!({})).await;
        assert_eq!(status, StatusCode::CREATED, "{started}");
        let agent = AgentPersistence::create(
            &self.persistence,
            company,
            AgentWrite {
                name: "Probe".into(),
                slug: "probe".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        Ids {
            workflow,
            version,
            binding,
            run: uuid(&started["run_id"]),
            agent,
        }
    }

    /// One listing page's `(items, next)`.
    fn items(page: &Value) -> (Vec<Value>, Option<Uuid>) {
        (
            page["items"].as_array().unwrap().clone(),
            page["next"].as_str().map(|next| next.parse().unwrap()),
        )
    }

    /// Run the production workers until `done` holds, then stop and join them.
    async fn run_until(&self, what: &str, done: impl AsyncFn() -> bool) {
        let shutdown = CancellationToken::new();
        let handles = self.engine.spawn(&shutdown);
        let settled = tokio::time::timeout(Duration::from_secs(30), async {
            while !done().await {
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
        assert!(settled.is_ok(), "the engine never reached: {what}");
    }

    async fn run_state(&self, run: Uuid) -> String {
        sqlx::query_scalar("SELECT state FROM workflow_runs WHERE id = $1")
            .bind(run)
            .fetch_one(self.persistence.pool())
            .await
            .unwrap()
    }
}

#[derive(Clone, Copy)]
struct Ids {
    workflow: Uuid,
    version: Uuid,
    binding: Uuid,
    run: Uuid,
    agent: Uuid,
}

/// The workflow routes mount beside every other route without a path conflict.
#[test]
fn workflow_routes_mount_in_the_app_router() {
    let sessions = Arc::new(SessionAuthority::for_test(&AppConfig::for_test()));
    let _ = crate::adapters::http::routes::router(sessions);
}

#[path = "decision_tests.rs"]
mod decision_tests;
#[path = "lifecycle_tests.rs"]
mod lifecycle_tests;
#[path = "reconciliation_tests.rs"]
mod reconciliation_tests;
#[path = "tenancy_tests.rs"]
mod tenancy_tests;
#[path = "tool_tests.rs"]
mod tool_tests;
