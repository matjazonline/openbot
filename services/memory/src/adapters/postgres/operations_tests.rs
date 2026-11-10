//! DB-backed `memory_operations_` cases (06A.1, 06A.2; p06 gate A1): purge, removal of deleted
//! namespaces and tenants after inference settles, no resurrection, age-based retention,
//! tenant reassignment and paged credential listing. Require MEMORY_TEST_DATABASE_URL.
use super::test_support::{TestDb, superuser_sql_in, test_db};
use crate::{
    application::{
        generation_worker::{GenerationSettings, GenerationWorker, Maintenance, Pass},
        operations::{Surface, by_id},
        ports::{GenerationProvider, IndexCommit},
        service::{App, Principal},
        test_embedder::FAKE_MODEL,
        test_provider::{CountedProvider, Step, answer},
    },
    domain::{
        admin_policy::AdminScope,
        credential::{self, TokenKind},
        error::ErrorCode,
    },
};
use chrono::{Duration as Days, Utc};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const USEFUL: &str = r#"{"outcome":"save","observations":[
    {"kind":"decision","title":"PostgreSQL chosen","body":"The team chose PostgreSQL.","event_ids":["e1","e2"]}],"summary":null}"#;

const ALL: [&str; 12] = [
    "save",
    "search",
    "timeline",
    "fetch",
    "correct",
    "delete",
    "context",
    "sources",
    "sessions",
    "generate",
    "generation_status",
    "purge",
];

struct Fixture {
    db: TestDb,
    app: App,
    provider: Arc<CountedProvider>,
    operator: Principal,
}

fn settings() -> GenerationSettings {
    GenerationSettings {
        workers: 1,
        lease: Duration::from_secs(2),
        heartbeat: Duration::from_millis(200),
        margin: Duration::from_millis(500),
        reaper_grace: Duration::ZERO,
        idle: Duration::from_millis(10),
        maintenance_tick: Duration::from_millis(50),
        batch: 500,
        shutdown_grace: Duration::from_millis(300),
        ..GenerationSettings::default()
    }
}

async fn fixture(provider: CountedProvider) -> Fixture {
    let db = test_db().await;
    let provider = Arc::new(provider);
    let app = App::new(
        Arc::new(db.store.clone()),
        crate::application::test_embedder::fake_pool(),
    )
    .with_generation(provider.clone());
    let issued = credential::issue(TokenKind::Admin).unwrap();
    app.store()
        .bootstrap_operator(issued.id, "operator", issued.verifier)
        .await
        .unwrap();
    let operator = app
        .authenticate(Some(&issued.token), Surface::Admin, Utc::now())
        .await
        .unwrap();
    Fixture {
        db,
        app,
        provider,
        operator,
    }
}

fn later() -> String {
    static LATER: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    LATER
        .get_or_init(|| (Utc::now() + Days::hours(2)).to_rfc3339())
        .clone()
}

fn context(sources: Value) -> Value {
    json!({"producing_ceiling": 3, "consumed": {"kind": "exact", "level": 3},
           "sources": sources, "external_sources": [], "expected_access_level": null})
}

fn save(key: &str, namespace: &str, count: usize, sources: Value) -> Value {
    let items: Vec<Value> = (0..count)
        .map(|i| {
            json!({"kind": "fact", "title": format!("fact {i}"), "body": "PostgreSQL",
                   "occurred_at": "2026-11-08T10:00:00Z", "expires_at": null})
        })
        .collect();
    json!({"idempotency_key": key, "expires_at": later(), "namespace": namespace,
           "session_id": null, "context": context(sources), "items": items})
}

fn generation(key: &str, namespace: &str, expires_at: &str) -> Value {
    json!({"idempotency_key": key, "expires_at": expires_at, "namespace": namespace,
           "session_id": null, "producing_ceiling": 6, "consumed": {"kind": "unknown"},
           "description": "capture",
           "events": [
               {"event_id": "e1", "source_ref": format!("chat:{key}:1"), "revision": "1",
                "occurred_at": "2026-11-08T10:00:00Z", "access_level": 2, "role": "user",
                "text": "Should we use PostgreSQL?", "exclude": false},
               {"event_id": "e2", "source_ref": format!("chat:{key}:2"), "revision": "1",
                "occurred_at": "2026-11-08T10:01:00Z", "access_level": 2, "role": "assistant",
                "text": "Yes, PostgreSQL.", "exclude": false}],
           "allowance_tokens": 100_000, "max_output_tokens": 1024, "summary": false})
}

fn purge(key: &str, namespace: &str) -> Value {
    json!({"idempotency_key": key, "expires_at": later(), "namespace": namespace})
}

impl Fixture {
    async fn run(&self, principal: &Principal, id: &str, input: Value) -> Result<Value, ErrorCode> {
        self.app
            .execute(
                by_id(id).unwrap(),
                principal,
                input,
                Utc::now(),
                Uuid::now_v7(),
            )
            .await
            .map_err(|error| error.code)
    }
    /// A tenant with `namespaces` and a generation cap.
    async fn tenant(&self, tenant: &str, namespaces: &[&str]) {
        let op = &self.operator;
        self.run(op, "admin.tenants.create", json!({"external_id": tenant}))
            .await
            .unwrap();
        for namespace in namespaces {
            self.run(
                op,
                "admin.namespaces.create",
                json!({"tenant": tenant, "namespace": namespace}),
            )
            .await
            .unwrap();
        }
        self.run(
            op,
            "admin.tenants.settings",
            json!({"tenant": tenant, "generation_daily_token_cap": 100_000_000}),
        )
        .await
        .unwrap();
    }
    /// A client allowed everything up to level 9 on `namespaces`.
    async fn client(&self, tenant: &str, namespaces: &[&str]) -> (String, Principal) {
        let grants: Vec<Value> = namespaces
            .iter()
            .map(|ns| json!({"namespace": ns, "read_max": 9, "write_max": 9, "operations": ALL}))
            .collect();
        let issued = self
            .run(
                &self.operator,
                "admin.credentials.issue",
                json!({"tenant": tenant, "label": "client", "expires_at": null, "grants": grants}),
            )
            .await
            .unwrap();
        let token = issued["token"].as_str().unwrap().to_owned();
        let principal = self.login(&token).await.unwrap();
        (token, principal)
    }
    async fn login(&self, token: &str) -> Result<Principal, ErrorCode> {
        self.app
            .authenticate(Some(token), Surface::Client, Utc::now())
            .await
            .map_err(|error| error.code)
    }
    async fn admin(&self, scope: AdminScope) -> (Uuid, Principal) {
        let issued = self
            .run(
                &self.operator,
                "admin.admin_credentials.issue",
                json!({"scope": scope, "label": "admin", "expires_at": null}),
            )
            .await
            .unwrap();
        let token = issued["token"].as_str().unwrap();
        let principal = self
            .app
            .authenticate(Some(token), Surface::Admin, Utc::now())
            .await
            .unwrap();
        (credential::parse(token).unwrap().id, principal)
    }
    async fn scalar(&self, sql: &str) -> i64 {
        sqlx::query_scalar(sql)
            .fetch_one(self.db.pool())
            .await
            .unwrap()
    }
    async fn exec(&self, sql: &str) {
        sqlx::query(sql).execute(self.db.pool()).await.unwrap();
    }
    fn worker(&self) -> GenerationWorker {
        self.worker_with(settings())
    }
    fn worker_with(&self, settings: GenerationSettings) -> GenerationWorker {
        let provider: Arc<dyn GenerationProvider> = self.provider.clone();
        GenerationWorker::new(self.app.store().clone(), provider, settings)
    }
    fn maintenance(&self) -> Maintenance {
        Maintenance::new(self.app.store().clone(), settings())
    }
    /// Rows of `namespace` (tenant `tenant`) left in every content table.
    async fn content(&self, tenant: &str, namespace: &str) -> i64 {
        let mut total = 0;
        for table in [
            "observations",
            "generation_operations",
            "generation_operation_sources",
            "sources",
            "sessions",
        ] {
            total += self
                .scalar(&format!(
                    "SELECT count(*) FROM {table} AS x JOIN namespaces AS n \
                       ON (n.tenant_id, n.id) = (x.tenant_id, x.namespace_id) \
                       JOIN tenants AS t ON t.id = n.tenant_id \
                      WHERE t.external_id = '{tenant}' AND n.external_id = '{namespace}'"
                ))
                .await;
        }
        total += self
            .scalar(&format!(
                "SELECT count(*) FROM observation_parts AS x JOIN namespaces AS n \
                   ON (n.tenant_id, n.id) = (x.tenant_id, x.namespace_id) \
                   JOIN tenants AS t ON t.id = n.tenant_id \
                  WHERE t.external_id = '{tenant}' AND n.external_id = '{namespace}'"
            ))
            .await;
        total
    }
    async fn removed(&self, tenant: &str, namespace: &str) -> bool {
        self.scalar(&format!(
            "SELECT count(*) FROM namespaces AS n JOIN tenants AS t ON t.id = n.tenant_id \
              WHERE t.external_id = '{tenant}' AND n.external_id = '{namespace}' \
                AND n.content_removed_at IS NOT NULL"
        ))
        .await
            == 1
    }
    async fn op_state(&self, op: &str) -> (String, Option<String>) {
        sqlx::query_as("SELECT state, outcome_code FROM generation_operations WHERE id = $1::uuid")
            .bind(op)
            .fetch_one(self.db.pool())
            .await
            .unwrap()
    }
    async fn wait_dispatched(&self, op: &str) {
        for _ in 0..200 {
            if self.op_state(op).await.0 == "dispatched" {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the operation was never dispatched");
    }
}

fn op_id(accepted: &Value) -> String {
    accepted["operation_id"].as_str().unwrap().to_owned()
}

/// A1-1/A1-2: purge tombstones at once, cancels queued inference and flags the dispatched call,
/// whose late `Saved` answer is discarded. No content is removed while that call is unfinished;
/// then every content row goes, the tombstone stays, re-creation is refused and a retry replays.
#[tokio::test]
async fn memory_operations_purge_cancels_inference_then_removes_every_content_row() {
    let f = fixture(
        CountedProvider::new(vec![])
            .then_always(Step::After(Duration::from_millis(700), answer(USEFUL))),
    )
    .await;
    f.tenant("t", &["a", "b"]).await;
    let (token, client) = f.client("t", &["a", "b"]).await;
    f.run(&client, "observations.save", save("s1", "a", 3, json!([])))
        .await
        .unwrap();
    f.run(&client, "observations.save", save("s2", "b", 1, json!([])))
        .await
        .unwrap();
    f.run(
        &client,
        "sessions.open",
        json!({"idempotency_key": "o1", "expires_at": later(), "namespace": "a",
               "external_id": "run-1"}),
    )
    .await
    .unwrap();
    let dispatched = op_id(
        &f.run(
            &client,
            "generations.request",
            generation("g1", "a", &later()),
        )
        .await
        .unwrap(),
    );
    // No heartbeat before the answer: the flagged call stays unfinished until it answers.
    let worker = f.worker_with(GenerationSettings {
        lease: Duration::from_secs(10),
        heartbeat: Duration::from_secs(5),
        ..settings()
    });
    let running = tokio::spawn(async move { worker.pass(&CancellationToken::new()).await });
    f.wait_dispatched(&dispatched).await;
    let queued = op_id(
        &f.run(
            &client,
            "generations.request",
            generation("g2", "a", &later()),
        )
        .await
        .unwrap(),
    );
    let purged = f
        .run(&client, "namespaces.purge", purge("p1", "a"))
        .await
        .unwrap();
    assert_eq!(purged["purged"], json!(true));
    assert_eq!(purged["replayed"], json!(false));
    assert_eq!(
        f.op_state(&queued).await,
        ("cancelled".into(), Some("namespace_deleted".into()))
    );
    // Reads stop at once: a request authenticated before the purge finds nothing (reads
    // require an active namespace), and a new request no longer has the grant.
    let search =
        json!({"namespaces": ["a"], "read_ceiling": 9, "query": "PostgreSQL", "limit": 10});
    let stale = f
        .run(&client, "observations.search", search.clone())
        .await
        .unwrap();
    assert_eq!(stale["items"], json!([]));
    let fresh = f.login(&token).await.unwrap();
    assert_eq!(
        f.run(&fresh, "observations.search", search)
            .await
            .unwrap_err(),
        ErrorCode::Forbidden
    );
    // Inference first: nothing of `a` is removed while its call is unfinished.
    let before = f.content("t", "a").await;
    let pass = f.maintenance().tick().await.unwrap();
    assert_eq!(pass.removed.observations, 0, "{pass:?}");
    assert_eq!(f.content("t", "a").await, before, "{pass:?}");
    assert!(matches!(running.await.unwrap().unwrap(), Pass::Settled(_)));
    assert_eq!(f.op_state(&dispatched).await.0, "cancelled");
    assert_eq!(
        f.scalar(&format!(
            "SELECT count(*) FROM observations WHERE generation_operation_id = '{dispatched}'"
        ))
        .await,
        0,
        "the late answer was discarded"
    );
    for _ in 0..3 {
        f.maintenance().tick().await.unwrap();
    }
    assert_eq!(f.content("t", "a").await, 0);
    assert!(f.removed("t", "a").await);
    assert_eq!(
        f.content("t", "b").await,
        1,
        "the other namespace keeps its content"
    );
    // The tombstone stays and refuses re-creation; a retry replays for the purger.
    assert_eq!(
        f.run(
            &f.operator,
            "admin.namespaces.create",
            json!({"tenant": "t", "namespace": "a"}),
        )
        .await
        .unwrap_err(),
        ErrorCode::NamespaceDeleted
    );
    let replay = f
        .run(&client, "namespaces.purge", purge("p1", "a"))
        .await
        .unwrap();
    assert_eq!(replay["replayed"], json!(true));
    assert_eq!(f.provider.calls(), 1);
}

/// F2/F8/F9: identical purges by one credential race to one execution and one replay; by two
/// credentials, the loser is 403 as a fresh execution would be. Separate connections.
#[tokio::test]
async fn memory_operations_identical_purges_race_to_one_execution_and_one_replay() {
    let f = Arc::new(fixture(CountedProvider::new(vec![])).await);
    f.tenant("t", &["a", "x"]).await;
    let (_, one) = f.client("t", &["a", "x"]).await;
    let (_, two) = f.client("t", &["x"]).await;
    let race = |first: Principal, second: Principal, namespace: &'static str| {
        let f = f.clone();
        async move {
            // `hold-` keeps the winner's claimed record uncommitted for 400 ms.
            let body = purge(&format!("hold-{namespace}"), namespace);
            let (a, b) = tokio::join!(
                f.run(&first, "namespaces.purge", body.clone()),
                f.run(&second, "namespaces.purge", body)
            );
            (a, b)
        }
    };
    let (a, b) = race(one.clone(), one.clone(), "a").await;
    let replays: Vec<bool> = [a.unwrap(), b.unwrap()]
        .iter()
        .map(|result| result["replayed"].as_bool().unwrap())
        .collect();
    assert_eq!(
        replays.iter().filter(|replayed| **replayed).count(),
        1,
        "one execution, one replay: {replays:?}"
    );
    let (a, b) = race(one.clone(), two.clone(), "x").await;
    let outcomes = [a, b];
    assert_eq!(
        outcomes.iter().filter(|outcome| outcome.is_ok()).count(),
        1,
        "{outcomes:?}"
    );
    assert!(
        outcomes.contains(&Err(ErrorCode::Forbidden)),
        "{outcomes:?}"
    );
    assert_eq!(
        f.scalar("SELECT count(*) FROM audit_events WHERE action = 'namespace.purge'")
            .await,
        2
    );
    // After the commit: the executing credential replays, the other one stays refused.
    let executed: Uuid =
        sqlx::query_scalar("SELECT credential_id FROM idempotency_records WHERE key = 'hold-x'")
            .fetch_one(f.db.pool())
            .await
            .unwrap();
    let (winner, loser) = if executed == one_id(&one) {
        (one, two)
    } else {
        (two, one)
    };
    assert_eq!(
        f.run(&winner, "namespaces.purge", purge("hold-x", "x"))
            .await
            .unwrap()["replayed"],
        json!(true)
    );
    assert_eq!(
        f.run(&loser, "namespaces.purge", purge("hold-x", "x"))
            .await
            .unwrap_err(),
        ErrorCode::Forbidden
    );
}

fn one_id(principal: &Principal) -> Uuid {
    match principal {
        Principal::Client(client) => client.credential_id,
        Principal::Admin(_) => panic!("a client principal"),
    }
}

/// F3: a copy in another namespace keeps its own text and label when its parent's namespace
/// is purged; only the parent link goes (implementation-decisions.md 762-768).
#[tokio::test]
async fn memory_operations_purge_leaves_a_cross_namespace_copy_and_drops_its_parent_link() {
    let f = fixture(CountedProvider::new(vec![])).await;
    f.tenant("t", &["a", "b"]).await;
    let (_, client) = f.client("t", &["a", "b"]).await;
    let parent = f
        .run(&client, "observations.save", save("p", "a", 1, json!([])))
        .await
        .unwrap();
    let parent: Uuid =
        serde_json::from_value(parent["observations"][0]["observation_id"].clone()).unwrap();
    let copy = f
        .run(
            &client,
            "observations.save",
            save(
                "c",
                "b",
                1,
                json!([{"kind": "observation", "observation_id": parent, "revision": 1}]),
            ),
        )
        .await
        .unwrap();
    let copy: Uuid =
        serde_json::from_value(copy["observations"][0]["observation_id"].clone()).unwrap();
    assert_eq!(
        f.scalar(&format!(
            "SELECT count(*) FROM observation_links WHERE observation_id = '{copy}'"
        ))
        .await,
        1
    );
    f.run(&client, "namespaces.purge", purge("pa", "a"))
        .await
        .unwrap();
    for _ in 0..3 {
        f.maintenance().tick().await.unwrap();
    }
    assert!(f.removed("t", "a").await);
    let fetched = f
        .run(
            &client,
            "observations.fetch",
            json!({"namespaces": ["b"], "read_ceiling": 9, "ids": [copy]}),
        )
        .await
        .unwrap();
    assert_eq!(fetched["items"][0]["title"], json!("fact 0"), "{fetched}");
    assert_eq!(
        f.scalar(&format!(
            "SELECT count(*) FROM observation_links WHERE observation_id = '{copy}'"
        ))
        .await,
        0,
        "the parent link went with the parent"
    );
}

/// A1-1, F4, F7: tenant delete tombstones its namespaces; removal empties them, blanks every
/// credential label beyond one batch, removes the tenant rows and marks the tenant only after
/// the last label. An old client token still answers `credential_revoked`.
#[tokio::test]
async fn memory_operations_tenant_removal_blanks_every_credential_label_beyond_one_batch() {
    let f = fixture(CountedProvider::new(vec![])).await;
    f.tenant("big", &["a", "b"]).await;
    let (token, client) = f.client("big", &["a", "b"]).await;
    f.run(&client, "observations.save", save("s", "a", 2, json!([])))
        .await
        .unwrap();
    f.exec(
        "INSERT INTO credentials (tenant_id, id, label, verifier, issued_by) \
         SELECT t.id, gen_random_uuid(), 'bulk ' || g, decode(repeat('00', 32), 'hex'), \
                (SELECT id FROM admin_credentials LIMIT 1) \
           FROM tenants AS t, generate_series(1, 1200) AS g WHERE t.external_id = 'big'",
    )
    .await;
    f.run(
        &f.operator,
        "admin.tenants.delete",
        json!({"tenant": "big"}),
    )
    .await
    .unwrap();
    assert_eq!(
        f.scalar(
            "SELECT count(*) FROM namespaces n JOIN tenants t ON t.id = n.tenant_id \
              WHERE t.external_id = 'big' AND n.state = 'deleted'"
        )
        .await,
        2
    );
    assert_eq!(f.scalar("SELECT count(*) FROM credential_grants").await, 0);
    assert_eq!(
        f.login(&token).await.unwrap_err(),
        ErrorCode::CredentialRevoked
    );
    let marked = || {
        f.scalar(
            "SELECT count(*) FROM tenants WHERE external_id = 'big' \
               AND content_removed_at IS NOT NULL",
        )
    };
    let mut labels = Vec::new();
    for _ in 0..3 {
        let pass = f.maintenance().tick().await.unwrap();
        labels.push(pass.removed.labels);
        if labels.len() < 3 {
            assert_eq!(marked().await, 0, "not marked before the last label");
        }
    }
    assert_eq!(labels, vec![500, 500, 201]);
    assert_eq!(marked().await, 1);
    assert_eq!(
        f.scalar("SELECT count(*) FROM credentials WHERE label <> 'removed'")
            .await,
        0
    );
    for table in [
        "idempotency_records",
        "tenant_spend_days",
        "tenant_generation_schedule",
        "observations",
    ] {
        assert_eq!(
            f.scalar(&format!("SELECT count(*) FROM {table}")).await,
            0,
            "{table}"
        );
    }
    assert_eq!(
        f.login(&token).await.unwrap_err(),
        ErrorCode::CredentialRevoked
    );
}

/// A1-3 (P6b n2): a `Saved` settlement whose credential row is missing is a stop, never a save.
/// The row is removed under `session_replication_role = replica`, which the foreign keys
/// otherwise forbid: this is defence in depth behind them.
#[tokio::test]
async fn memory_operations_a_saved_settlement_without_its_credential_row_is_a_stop() {
    let f = fixture(
        CountedProvider::new(vec![])
            .then_always(Step::After(Duration::from_millis(500), answer(USEFUL))),
    )
    .await;
    f.tenant("t", &["a"]).await;
    let (_, client) = f.client("t", &["a"]).await;
    let op = op_id(
        &f.run(
            &client,
            "generations.request",
            generation("g", "a", &later()),
        )
        .await
        .unwrap(),
    );
    let worker = f.worker();
    let running = tokio::spawn(async move { worker.pass(&CancellationToken::new()).await });
    f.wait_dispatched(&op).await;
    superuser_sql_in(
        &f.db.name,
        &[
            "SET session_replication_role = replica",
            "DELETE FROM credentials WHERE label = 'client'",
        ],
    )
    .await;
    running.await.unwrap().unwrap();
    assert_eq!(
        f.op_state(&op).await,
        ("cancelled".into(), Some("authority_missing".into()))
    );
    assert_eq!(
        f.scalar("SELECT count(*) FROM observations WHERE generation_operation_id IS NOT NULL")
            .await,
        0
    );
}

/// No resurrection: an indexer that claimed a row before its namespace was purged commits
/// `Stale` after removal and writes no vectors.
#[tokio::test]
async fn memory_operations_a_stale_indexer_commit_after_removal_writes_nothing() {
    let f = fixture(CountedProvider::new(vec![])).await;
    f.tenant("t", &["a"]).await;
    let (_, client) = f.client("t", &["a"]).await;
    f.run(&client, "observations.save", save("s", "a", 1, json!([])))
        .await
        .unwrap();
    let store = f.app.store().clone();
    let job = store
        .claim_index_jobs(8, Duration::from_secs(60))
        .await
        .unwrap()
        .remove(0);
    let parts = f
        .app
        .embeddings()
        .embed_parts(job.title.clone(), job.body.clone(), Default::default())
        .await
        .unwrap()
        .unwrap();
    f.run(&client, "namespaces.purge", purge("p", "a"))
        .await
        .unwrap();
    for _ in 0..3 {
        f.maintenance().tick().await.unwrap();
    }
    assert!(f.removed("t", "a").await);
    assert_eq!(
        store.commit_index(&job, FAKE_MODEL, &parts).await.unwrap(),
        IndexCommit::Stale
    );
    assert_eq!(f.scalar("SELECT count(*) FROM observation_parts").await, 0);
    assert_eq!(f.scalar("SELECT count(*) FROM observations").await, 0);
}

/// A1-4: expired observations are removed with their parts and links, others kept; competing
/// sweepers on separate connections remove each row once.
#[tokio::test]
async fn memory_operations_expired_observations_are_removed_once_by_competing_sweepers() {
    let f = fixture(CountedProvider::new(vec![])).await;
    f.tenant("t", &["a"]).await;
    let (_, client) = f.client("t", &["a"]).await;
    for i in 0..5 {
        f.run(
            &client,
            "observations.save",
            save(&format!("s{i}"), "a", 250, json!([])),
        )
        .await
        .unwrap();
    }
    f.exec(
        "UPDATE observations SET expires_at = now() - interval '1 second' \
          WHERE id IN (SELECT id FROM observations ORDER BY id LIMIT 1200)",
    )
    .await;
    let store = f.app.store().clone();
    let sweepers: Vec<_> = (0..4)
        .map(|_| {
            let store = store.clone();
            tokio::spawn(async move {
                let mut removed = 0;
                loop {
                    let aged = store.sweep_aged(100).await.unwrap();
                    removed += aged.observations;
                    if aged.observations == 0 {
                        return removed;
                    }
                }
            })
        })
        .collect();
    let mut total = 0;
    for sweeper in sweepers {
        total += sweeper.await.unwrap();
    }
    assert_eq!(total, 1200);
    assert_eq!(f.scalar("SELECT count(*) FROM observations").await, 50);
}

/// A1-4 (D5): idempotency records age out after 7 days and their deadline; a retry carrying the
/// deadline then gets 410, never a second execution; a generation key reused with a new deadline
/// after its record went is 409 while its operation exists.
#[tokio::test]
async fn memory_operations_idempotency_records_age_out_and_keys_never_execute_twice() {
    let f = fixture(CountedProvider::new(vec![])).await;
    f.tenant("t", &["a"]).await;
    let (_, client) = f.client("t", &["a"]).await;
    let soon = (Utc::now() + Days::milliseconds(400)).to_rfc3339();
    let mut body = save("k1", "a", 1, json!([]));
    body["expires_at"] = json!(soon);
    f.run(&client, "observations.save", body.clone())
        .await
        .unwrap();
    f.run(&client, "generations.request", generation("g1", "a", &soon))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    // Six days old: kept.
    f.exec("UPDATE idempotency_records SET created_at = now() - interval '6 days'")
        .await;
    assert_eq!(
        f.app
            .store()
            .sweep_aged(500)
            .await
            .unwrap()
            .idempotency_records,
        0
    );
    f.exec("UPDATE idempotency_records SET created_at = now() - interval '8 days'")
        .await;
    assert_eq!(
        f.app
            .store()
            .sweep_aged(500)
            .await
            .unwrap()
            .idempotency_records,
        2
    );
    assert_eq!(
        f.run(&client, "observations.save", body).await.unwrap_err(),
        ErrorCode::RequestExpired
    );
    assert_eq!(f.scalar("SELECT count(*) FROM observations").await, 1);
    assert_eq!(
        f.run(
            &client,
            "generations.request",
            generation("g1", "a", &later())
        )
        .await
        .unwrap_err(),
        ErrorCode::IdempotencyConflict
    );
}

/// A1-4: audit events and spend days age out at 400 days; the schema refuses deleting a younger
/// audit event.
#[tokio::test]
async fn memory_operations_audit_and_spend_rows_age_out_at_400_days() {
    let f = fixture(CountedProvider::new(vec![])).await;
    f.tenant("t", &["a"]).await;
    f.exec(
        "INSERT INTO audit_events (id, at, actor_kind, action, target_kind, target_id, outcome) \
         VALUES (gen_random_uuid(), now() - interval '401 days', 'system', 'old', 'x', 'x', 'ok'), \
                (gen_random_uuid(), now() - interval '399 days', 'system', 'young', 'x', 'x', 'ok')",
    )
    .await;
    f.exec(
        "INSERT INTO tenant_spend_days (tenant_id, day) \
         SELECT id, (now() AT TIME ZONE 'UTC')::date - d FROM tenants, (VALUES (401), (10)) AS v(d)",
    )
    .await;
    let aged = f.app.store().sweep_aged(500).await.unwrap();
    assert_eq!((aged.audit_events, aged.spend_days), (1, 1));
    assert_eq!(
        f.scalar("SELECT count(*) FROM audit_events WHERE action IN ('old', 'young')")
            .await,
        1
    );
    let refused = sqlx::query("DELETE FROM audit_events WHERE action = 'young'")
        .execute(f.db.pool())
        .await
        .unwrap_err()
        .to_string();
    assert!(refused.contains("kept for 400 days"), "{refused}");
}

/// F5: finished operations no observation names, and nulled sources nothing names, go 30 days
/// later; a referenced operation stays.
#[tokio::test]
async fn memory_operations_unreferenced_operations_and_nulled_sources_age_out() {
    let f = fixture(CountedProvider::new(vec![]).then_always(Step::Answer(answer(USEFUL)))).await;
    f.tenant("t", &["a"]).await;
    let (_, client) = f.client("t", &["a"]).await;
    let saved = op_id(
        &f.run(
            &client,
            "generations.request",
            generation("g1", "a", &later()),
        )
        .await
        .unwrap(),
    );
    assert!(matches!(
        f.worker().pass(&CancellationToken::new()).await.unwrap(),
        Pass::Settled(_)
    ));
    let cancelled = op_id(
        &f.run(
            &client,
            "generations.request",
            generation("g2", "a", &later()),
        )
        .await
        .unwrap(),
    );
    f.run(
        &client,
        "generations.cancel",
        json!({"idempotency_key": "c", "expires_at": later(), "namespace": "a",
               "operation_id": cancelled}),
    )
    .await
    .unwrap();
    f.exec("ALTER TABLE generation_operations DISABLE TRIGGER generation_requests_immutable")
        .await;
    f.exec("UPDATE generation_operations SET finished_at = now() - interval '31 days'")
        .await;
    f.exec("ALTER TABLE generation_operations ENABLE TRIGGER generation_requests_immutable")
        .await;
    f.exec("ALTER TABLE sources DISABLE TRIGGER sources_immutable")
        .await;
    f.exec("UPDATE sources SET recorded_at = now() - interval '62 days'")
        .await;
    // The input sweep nulls the requests and evidence first.
    f.app.store().sweep_retention(500).await.unwrap();
    f.exec(
        "UPDATE sources SET content_nulled_at = now() - interval '31 days' WHERE content IS NULL",
    )
    .await;
    f.exec("ALTER TABLE sources ENABLE TRIGGER sources_immutable")
        .await;
    assert_eq!(
        f.scalar("SELECT count(*) FROM sources WHERE content IS NULL")
            .await,
        4
    );
    let aged = f.app.store().sweep_aged(500).await.unwrap();
    assert_eq!(aged.operations, 1, "only the operation nothing names");
    assert_eq!(aged.sources, 2, "the cancelled operation's evidence");
    assert_eq!(
        f.op_state(&saved).await.0,
        "completed",
        "the saved operation stays with its observation"
    );
    assert_eq!(
        f.scalar(&format!(
            "SELECT count(*) FROM generation_operations WHERE id = '{cancelled}'"
        ))
        .await,
        0
    );
}

/// A1-5: an operator moves a manager's tenants in batches; the old provisioner loses them, the
/// new one manages them, bad targets and non-operators are refused, and each move is audited.
#[tokio::test]
async fn memory_operations_reassign_moves_tenants_in_batches_and_refuses_bad_targets() {
    let f = fixture(CountedProvider::new(vec![])).await;
    let (old_id, old) = f.admin(AdminScope::Provisioner).await;
    let (new_id, new) = f.admin(AdminScope::Provisioner).await;
    for tenant in ["t1", "t2", "t3"] {
        f.run(&old, "admin.tenants.create", json!({"external_id": tenant}))
            .await
            .unwrap();
    }
    f.run(&old, "admin.tenants.delete", json!({"tenant": "t3"}))
        .await
        .unwrap();
    let reassign =
        |from: Uuid, to: Uuid, limit: u32| json!({"from": from, "to": to, "limit": limit});
    assert_eq!(
        f.run(&new, "admin.tenants.reassign", reassign(old_id, new_id, 2))
            .await
            .unwrap_err(),
        ErrorCode::Forbidden,
        "operator only"
    );
    let operator_id = match &f.operator {
        Principal::Admin(admin) => admin.id,
        Principal::Client(_) => unreachable!(),
    };
    for (to, why) in [
        (old_id, "same"),
        (operator_id, "operator"),
        (Uuid::now_v7(), "unknown"),
    ] {
        assert_eq!(
            f.run(
                &f.operator,
                "admin.tenants.reassign",
                reassign(old_id, to, 2)
            )
            .await
            .unwrap_err(),
            ErrorCode::Conflict,
            "{why}"
        );
    }
    let first = f
        .run(
            &f.operator,
            "admin.tenants.reassign",
            reassign(old_id, new_id, 2),
        )
        .await
        .unwrap();
    assert_eq!(first, json!({"moved": 2, "remaining": true}));
    let second = f
        .run(
            &f.operator,
            "admin.tenants.reassign",
            reassign(old_id, new_id, 2),
        )
        .await
        .unwrap();
    assert_eq!(second, json!({"moved": 1, "remaining": false}));
    assert_eq!(
        f.run(&old, "admin.tenants.disable", json!({"tenant": "t1"}))
            .await
            .unwrap_err(),
        ErrorCode::NotFound
    );
    f.run(&new, "admin.tenants.disable", json!({"tenant": "t1"}))
        .await
        .unwrap();
    // The new manager owns the tombstone too (it may list its credentials).
    f.run(
        &new,
        "admin.credentials.list",
        json!({"tenant": "t3", "limit": 1}),
    )
    .await
    .unwrap();
    assert_eq!(
        f.scalar("SELECT count(*) FROM audit_events WHERE action = 'tenant.reassign'")
            .await,
        3
    );
    // A revoked target is refused.
    f.run(
        &f.operator,
        "admin.admin_credentials.revoke",
        json!({"credential_id": old_id}),
    )
    .await
    .unwrap();
    assert_eq!(
        f.run(
            &f.operator,
            "admin.tenants.reassign",
            reassign(new_id, old_id, 2)
        )
        .await
        .unwrap_err(),
        ErrorCode::Conflict
    );
}

/// F2: a reassignment waits for a request the old provisioner already holds a tenant lock for,
/// then moves the tenant; the old provisioner's next request is `not_found`. Concurrent old-
/// provisioner writes each commit before the move or are refused after it.
#[tokio::test]
async fn memory_operations_reassign_waits_for_in_flight_requests_and_races_old_writes() {
    let f = Arc::new(fixture(CountedProvider::new(vec![])).await);
    let (old_id, old) = f.admin(AdminScope::Provisioner).await;
    let (new_id, _) = f.admin(AdminScope::Provisioner).await;
    f.run(&old, "admin.tenants.create", json!({"external_id": "t"}))
        .await
        .unwrap();
    // A held tenant lock stands in for an in-flight old-provisioner request.
    let mut held = f.db.pool().begin().await.unwrap();
    sqlx::query("SELECT 1 FROM tenants WHERE external_id = 't' FOR SHARE")
        .execute(&mut *held)
        .await
        .unwrap();
    let moving = {
        let f = f.clone();
        tokio::spawn(async move {
            f.run(
                &f.operator,
                "admin.tenants.reassign",
                json!({"from": old_id, "to": new_id}),
            )
            .await
        })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!moving.is_finished(), "the move waits for the held request");
    held.commit().await.unwrap();
    assert_eq!(
        moving.await.unwrap().unwrap(),
        json!({"moved": 1, "remaining": false})
    );
    assert_eq!(
        f.run(
            &old,
            "admin.namespaces.create",
            json!({"tenant": "t", "namespace": "n"})
        )
        .await
        .unwrap_err(),
        ErrorCode::NotFound
    );
    // Racing writes by the old manager against a move.
    for tenant in ["r1", "r2", "r3", "r4"] {
        f.run(
            &f.operator,
            "admin.tenants.create",
            json!({"external_id": tenant}),
        )
        .await
        .unwrap();
    }
    f.exec(&format!(
        "UPDATE tenants SET managed_by = '{old_id}' WHERE external_id LIKE 'r%'"
    ))
    .await;
    // The old credential must be live to race: it still is (only reassigned, never revoked).
    let writers: Vec<_> = ["r1", "r2", "r3", "r4"]
        .into_iter()
        .map(|tenant| {
            let f = f.clone();
            let old = old.clone();
            tokio::spawn(async move {
                f.run(
                    &old,
                    "admin.namespaces.create",
                    json!({"tenant": tenant, "namespace": "w"}),
                )
                .await
            })
        })
        .collect();
    let mover = {
        let f = f.clone();
        tokio::spawn(async move {
            f.run(
                &f.operator,
                "admin.tenants.reassign",
                json!({"from": old_id, "to": new_id}),
            )
            .await
        })
    };
    let mut created = 0;
    for writer in writers {
        match writer.await.unwrap() {
            Ok(_) => created += 1,
            Err(code) => assert_eq!(code, ErrorCode::NotFound),
        }
    }
    assert_eq!(mover.await.unwrap().unwrap()["remaining"], json!(false));
    assert_eq!(
        f.scalar(
            "SELECT count(*) FROM namespaces n JOIN tenants t ON t.id = n.tenant_id \
              WHERE t.external_id LIKE 'r%' AND n.external_id = 'w'"
        )
        .await,
        created,
        "every success committed before the move, every refusal came after it"
    );
    assert_eq!(
        f.scalar(&format!(
            "SELECT count(*) FROM tenants WHERE managed_by = '{new_id}'"
        ))
        .await,
        5,
        "no tenant is half-moved"
    );
}

/// A1-6: listing pages in id order with `next_after`, filters by exact label and by activity,
/// and covers every credential exactly once.
#[tokio::test]
async fn memory_operations_credential_listing_pages_by_label_and_activity() {
    let f = fixture(CountedProvider::new(vec![])).await;
    f.tenant("t", &["a"]).await;
    let mut ids = Vec::new();
    for i in 0..5 {
        let label = if i % 2 == 0 { "ob:a" } else { "other" };
        let issued = f
            .run(
                &f.operator,
                "admin.credentials.issue",
                json!({"tenant": "t", "label": label, "expires_at": null,
                       "grants": [{"namespace": "a", "read_max": 3, "write_max": 3,
                                   "operations": ["save"]}]}),
            )
            .await
            .unwrap();
        ids.push(issued["credential_id"].as_str().unwrap().to_owned());
    }
    f.run(
        &f.operator,
        "admin.credentials.revoke",
        json!({"tenant": "t", "credential_id": ids[0]}),
    )
    .await
    .unwrap();
    let mut seen = Vec::new();
    let mut after = Value::Null;
    loop {
        let page = f
            .run(
                &f.operator,
                "admin.credentials.list",
                json!({"tenant": "t", "after": after, "limit": 2}),
            )
            .await
            .unwrap();
        for credential in page["credentials"].as_array().unwrap() {
            seen.push(credential["credential_id"].as_str().unwrap().to_owned());
            assert_eq!(credential["grants"].as_array().unwrap().len(), 1);
        }
        after = page["next_after"].clone();
        if after.is_null() {
            break;
        }
    }
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(seen, sorted, "every credential once, in id order");
    let labelled = f
        .run(
            &f.operator,
            "admin.credentials.list",
            json!({"tenant": "t", "label": "ob:a", "active_only": true}),
        )
        .await
        .unwrap();
    let labelled: Vec<&str> = labelled["credentials"]
        .as_array()
        .unwrap()
        .iter()
        .map(|credential| credential["credential_id"].as_str().unwrap())
        .collect();
    let mut expected = vec![ids[2].as_str(), ids[4].as_str()];
    expected.sort();
    assert_eq!(labelled, expected, "label match, revoked excluded");
    for bad in [
        json!({"tenant": "t", "limit": 0}),
        json!({"tenant": "t", "limit": 1001}),
    ] {
        assert_eq!(
            f.run(&f.operator, "admin.credentials.list", bad)
                .await
                .unwrap_err(),
            ErrorCode::InvalidRequest
        );
    }
}

/// A4-1, F2, F3: a rejection of the service's credentials opens one breaker shared by competing
/// workers. While it is open nothing is sent; after the window exactly one probe calls; a
/// probe's rejection doubles the window; a probe that succeeds closes it, and then every
/// operation is sent once. A paused operation's dispatch is not counted. The timing is asserted
/// only as lower bounds between recorded call starts, so a slow runner cannot fail it (review
/// B2 F1a).
#[tokio::test]
async fn memory_operations_auth_breaker_pauses_competing_workers_and_closes_on_success() {
    use crate::application::{
        contract::GenerationState, metrics::METRICS, ports::ProviderError,
        provider_breaker::ProviderBreaker,
    };
    use tokio::time::Instant;
    let refused = || Step::Answer(Err(ProviderError::Terminal("authentication_error".into())));
    let f = fixture(
        CountedProvider::steps(vec![refused(), refused()])
            .then_always(Step::After(Duration::from_millis(20), answer(USEFUL))),
    )
    .await;
    let mut ops = Vec::new();
    for tenant in ["t1", "t2", "t3"] {
        f.tenant(tenant, &["a"]).await;
        let (_, client) = f.client(tenant, &["a"]).await;
        for n in 0..2 {
            let key = format!("{tenant}-g{n}");
            ops.push(op_id(
                &f.run(
                    &client,
                    "generations.request",
                    generation(&key, "a", &later()),
                )
                .await
                .unwrap(),
            ));
        }
    }
    let failures = || {
        METRICS.get(
            "memory_provider_auth_failures_total",
            &[("kind", "authentication_error")],
        )
    };
    let before = failures();
    let base = Duration::from_millis(200);
    let breaker = Arc::new(ProviderBreaker::new(base, Duration::from_secs(5)));
    let worker = |breaker: &Arc<ProviderBreaker>| f.worker().with_breaker(breaker.clone());
    // The first rejection: the operation goes back to `accepted`, its dispatch not counted.
    let first = worker(&breaker)
        .pass(&CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(first, Pass::Settled(GenerationState::Accepted));
    assert!(breaker.is_open());
    assert_eq!(f.provider.calls(), 1);
    assert_eq!(
        f.scalar(
            "SELECT count(*) FROM generation_operations WHERE state = 'accepted' \
                    AND attempts = 0 AND next_attempt_at > now() AND reserved_tokens = 0"
        )
        .await,
        1,
        "paused: back to accepted, not counted, waiting for the window"
    );
    let stop = CancellationToken::new();
    let running: Vec<_> = (0..3)
        .map(|_| {
            let worker = worker(&breaker);
            let stop = stop.clone();
            tokio::spawn(async move { worker.run(stop).await })
        })
        .collect();
    // The probe after the first window is refused again (the window doubles); the probe after
    // the second succeeds and closes the breaker, and then every operation is sent once.
    let deadline = Instant::now() + Duration::from_secs(10);
    while f
        .scalar("SELECT count(*) FROM generation_operations WHERE state = 'completed'")
        .await
        < ops.len() as i64
    {
        assert!(Instant::now() < deadline, "every operation completes");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    stop.cancel();
    for task in running {
        task.await.unwrap();
    }
    assert!(!breaker.is_open());
    assert_eq!(f.provider.calls(), 2 + ops.len());
    // A window starts when its rejection answered, after that call started. So call 1 (the first
    // probe) starts at least `base` after call 0: nothing was sent while open. Call 2 starts at
    // least `2 × base` after call 1: one probe in the half-open state, and a doubled window.
    let started = f.provider.started();
    let gap = |i: usize| started[i].duration_since(started[i - 1]);
    assert!(gap(1) >= base, "nothing sent while open: {:?}", gap(1));
    assert!(
        gap(2) >= base * 2,
        "one probe, then a doubled window: {:?}",
        gap(2)
    );
    assert_eq!(
        f.scalar("SELECT count(*) FROM generation_operations WHERE attempts = 1")
            .await,
        ops.len() as i64,
        "each operation was dispatched once; the paused dispatches were not counted"
    );
    assert_eq!(
        f.scalar(
            "SELECT count(*) FROM audit_events \
              WHERE action = 'generation.settle' AND outcome = 'paused'"
        )
        .await,
        2
    );
    assert!(failures() >= before + 2);
}

/// A4-2 (F2): after-restore finishes every unfinished operation as `cancelled(restored)`, its
/// reservation released, and leaves finished ones untouched. A second run changes nothing, a
/// worker then finds nothing to send, and the provider is never called again.
#[tokio::test]
async fn memory_operations_after_restore_finishes_every_unfinished_operation() {
    use crate::application::ports::Dispatch;
    let f = fixture(CountedProvider::new(vec![answer(USEFUL)])).await;
    let mut clients = Vec::new();
    for tenant in ["t1", "t2", "t3"] {
        f.tenant(tenant, &["a"]).await;
        clients.push(f.client(tenant, &["a"]).await.1);
    }
    let done = op_id(
        &f.run(
            &clients[0],
            "generations.request",
            generation("done", "a", &later()),
        )
        .await
        .unwrap(),
    );
    assert!(matches!(
        f.worker().pass(&CancellationToken::new()).await.unwrap(),
        Pass::Settled(_)
    ));
    for (n, client) in clients.iter().enumerate() {
        f.run(
            client,
            "generations.request",
            generation(&format!("open{n}"), "a", &later()),
        )
        .await
        .unwrap();
    }
    let store = f.app.store();
    let lease = Duration::from_secs(60);
    let job = store
        .claim_generation(Uuid::now_v7(), lease)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        store.dispatch_generation(&job, lease).await.unwrap(),
        Dispatch::Send(_)
    ));
    store
        .claim_generation(Uuid::now_v7(), lease)
        .await
        .unwrap()
        .unwrap();
    let states = "SELECT count(DISTINCT state) FROM generation_operations \
                   WHERE state IN ('accepted', 'claimed', 'dispatched')";
    assert_eq!(
        f.scalar(states).await,
        3,
        "one operation in each unfinished state"
    );
    assert!(
        f.scalar("SELECT coalesce(sum(reserved), 0)::bigint FROM tenant_spend_days")
            .await
            > 0
    );
    let finished = "SELECT count(*) FROM generation_operations \
                     WHERE state = 'cancelled' AND outcome_code = 'restored' \
                       AND finished_at IS NOT NULL AND reserved_tokens = 0";
    assert_eq!(store.after_restore().await.unwrap(), 3);
    assert_eq!(f.scalar(finished).await, 3);
    assert_eq!(
        f.op_state(&done).await,
        ("completed".into(), Some("saved".into()))
    );
    assert_eq!(
        f.scalar("SELECT coalesce(sum(reserved), 0)::bigint FROM tenant_spend_days")
            .await,
        0,
        "every reservation released"
    );
    assert_eq!(store.after_restore().await.unwrap(), 0, "idempotent");
    assert_eq!(
        f.worker().pass(&CancellationToken::new()).await.unwrap(),
        Pass::Idle
    );
    assert_eq!(f.provider.calls(), 1, "nothing was sent again");
    assert_eq!(
        f.scalar("SELECT count(*) FROM audit_events WHERE action = 'service.after_restore'")
            .await,
        1
    );
}

/// `MEMORY_TEST_DATABASE_URL` with its database replaced by `database`.
fn url_for(database: &str) -> String {
    let url = std::env::var(super::test_support::URL_VAR).unwrap();
    let (base, query) = match url.split_once('?') {
        Some((base, query)) => (base.to_owned(), format!("?{query}")),
        None => (url.clone(), String::new()),
    };
    let (scheme, rest) = base.split_once("://").unwrap();
    let authority = rest.split('/').next().unwrap();
    format!("{scheme}://{authority}/{database}{query}")
}

/// Run `tool` from `MEMORY_PG_BIN`, failing the test with its stderr.
fn pg_tool(tool: &str, args: &[&str]) {
    let bin = std::env::var("MEMORY_PG_BIN")
        .expect("MEMORY_PG_BIN must name the directory of pg_dump and pg_restore");
    let out = std::process::Command::new(std::path::Path::new(&bin).join(tool))
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("{tool} could not run: {error}"));
    assert!(
        out.status.success(),
        "{tool} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A4-3: a custom-format dump restored into a fresh database keeps every content row, label
/// and search result. After-restore then finishes the unfinished operations, and nothing is
/// sent again. Run by `scripts/backup-restore-check.sh`, which supplies `MEMORY_PG_BIN`.
#[tokio::test]
#[ignore = "needs pg_dump and pg_restore; run by scripts/backup-restore-check.sh"]
async fn memory_operations_backup_restore_keeps_content_and_never_replays() {
    use crate::application::{
        indexer::{Indexer, IndexerSettings},
        ports::Dispatch,
    };
    let f = fixture(CountedProvider::new(vec![answer(USEFUL)])).await;
    let mut clients = Vec::new();
    for tenant in ["t1", "t2", "t3"] {
        f.tenant(tenant, &["a"]).await;
        clients.push(f.client(tenant, &["a"]).await);
    }
    let (token, client) = clients[0].clone();
    f.run(&client, "observations.save", save("s1", "a", 3, json!([])))
        .await
        .unwrap();
    f.run(
        &client,
        "generations.request",
        generation("done", "a", &later()),
    )
    .await
    .unwrap();
    assert!(matches!(
        f.worker().pass(&CancellationToken::new()).await.unwrap(),
        Pass::Settled(_)
    ));
    let indexer = Indexer::new(
        f.app.store().clone(),
        f.app.embeddings().clone(),
        IndexerSettings::default(),
    );
    while indexer.run_once().await.unwrap().claimed > 0 {}
    for (n, (_, client)) in clients.iter().enumerate() {
        f.run(
            client,
            "generations.request",
            generation(&format!("open{n}"), "a", &later()),
        )
        .await
        .unwrap();
    }
    let store = f.app.store();
    let lease = Duration::from_secs(60);
    let job = store
        .claim_generation(Uuid::now_v7(), lease)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        store.dispatch_generation(&job, lease).await.unwrap(),
        Dispatch::Send(_)
    ));
    store
        .claim_generation(Uuid::now_v7(), lease)
        .await
        .unwrap()
        .unwrap();
    let search =
        json!({"namespaces": ["a"], "read_ceiling": 9, "query": "PostgreSQL", "limit": 10});
    let searched = f
        .run(&client, "observations.search", search.clone())
        .await
        .unwrap();
    assert!(!searched["items"].as_array().unwrap().is_empty());

    let dump = std::env::temp_dir().join(format!("{}.dump", f.db.name));
    pg_tool(
        "pg_dump",
        &[
            "-Fc",
            "--file",
            dump.to_str().unwrap(),
            "--dbname",
            &url_for(&f.db.name),
        ],
    );
    let (restored_name, restored_options) = super::test_support::empty_database(false).await;
    struct Dropped(String, std::path::PathBuf);
    impl Drop for Dropped {
        fn drop(&mut self) {
            super::test_support::drop_database(&self.0);
            let _ = std::fs::remove_file(&self.1);
        }
    }
    let _cleanup = Dropped(restored_name.clone(), dump.clone());
    pg_tool(
        "pg_restore",
        &[
            "--exit-on-error",
            "--dbname",
            &url_for(&restored_name),
            dump.to_str().unwrap(),
        ],
    );
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect_with(restored_options)
        .await
        .unwrap();
    let restored = App::new(
        Arc::new(super::PgStore::new(pool.clone())),
        crate::application::test_embedder::fake_pool(),
    );
    let count = |pool: sqlx::PgPool, sql: String| async move {
        sqlx::query_scalar::<_, i64>(&sql)
            .fetch_one(&pool)
            .await
            .unwrap()
    };
    for table in [
        "tenants",
        "namespaces",
        "credentials",
        "observations",
        "observation_parts",
        "observation_links",
        "sources",
        "sessions",
        "generation_operations",
        "audit_events",
    ] {
        let sql = format!("SELECT count(*) FROM {table}");
        assert_eq!(
            count(pool.clone(), sql.clone()).await,
            count(f.db.pool().clone(), sql).await,
            "{table} restored"
        );
    }
    let labels = "SELECT coalesce(string_agg(label, ',' ORDER BY id), '') FROM credentials";
    let label_list = |pool: sqlx::PgPool| async move {
        sqlx::query_scalar::<_, String>(labels)
            .fetch_one(&pool)
            .await
            .unwrap()
    };
    assert_eq!(
        label_list(pool.clone()).await,
        label_list(f.db.pool().clone()).await
    );
    let principal = restored
        .authenticate(Some(&token), Surface::Client, Utc::now())
        .await
        .unwrap();
    let found = restored
        .execute(
            by_id("observations.search").unwrap(),
            &principal,
            search,
            Utc::now(),
            Uuid::now_v7(),
        )
        .await
        .unwrap();
    assert_eq!(found["items"], searched["items"], "the same search results");

    assert_eq!(restored.store().after_restore().await.unwrap(), 3);
    assert_eq!(
        count(
            pool.clone(),
            "SELECT count(*) FROM generation_operations WHERE state IN ('accepted', 'claimed', \
               'dispatched') OR (state = 'cancelled' AND outcome_code <> 'restored')"
                .into()
        )
        .await,
        0,
        "every unfinished operation is cancelled(restored)"
    );
    let provider: Arc<dyn GenerationProvider> = f.provider.clone();
    let worker = GenerationWorker::new(restored.store().clone(), provider, settings());
    assert_eq!(
        worker.pass(&CancellationToken::new()).await.unwrap(),
        Pass::Idle
    );
    assert_eq!(
        f.provider.calls(),
        1,
        "the provider saw no call after the restore"
    );
}

/// A4-5, U8 (option C): a measurement, not a gate. Each of 4 tenants' namespaces is first seeded
/// with 10,000 varied memories (200 per save) and indexed, as a mature agent's would be. Then the
/// tenants concurrently make 100 saves each, the indexer catches up, and they make 200 searches
/// each. p50/p95 latencies are printed for the record. Run with
/// `MEMORY_MODEL_DIR=assets/model cargo test --release --lib -- --ignored --nocapture
/// memory_operations_representative_load` (without the variable a fake embedder is used).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "a measurement; run by hand and record its output"]
async fn memory_operations_representative_load_is_measured() {
    use crate::application::indexer::{Indexer, IndexerSettings};
    use std::time::Instant;
    let mut f = fixture(CountedProvider::new(vec![])).await;
    // With MEMORY_MODEL_DIR the real model embeds (its CPU cost is most of a search).
    let embedder = match std::env::var("MEMORY_MODEL_DIR") {
        Ok(dir) => {
            let loaded = tokio::task::spawn_blocking(move || {
                crate::adapters::embedding::load(std::path::Path::new(&dir))
            })
            .await
            .unwrap()
            .expect("the model in MEMORY_MODEL_DIR loads");
            f.app = App::new(f.app.store().clone(), Arc::new(loaded.pool().unwrap()));
            "real model"
        }
        Err(_) => "fake embedder",
    };
    let f = Arc::new(f);
    let mut clients = Vec::new();
    for n in 0..4 {
        let tenant = format!("load{n}");
        f.tenant(&tenant, &["a"]).await;
        clients.push(f.client(&tenant, &["a"]).await.1);
    }
    // Seed: 10,000 distinct memories per namespace, then index them all.
    const SEED: usize = 10_000;
    const BATCH: usize = 200;
    let topics = [
        "billing",
        "deploys",
        "onboarding",
        "security review",
        "database migration",
        "pricing",
        "support escalation",
        "release notes",
        "hiring",
        "vendor contract",
        "incident",
        "roadmap",
    ];
    let seeding = Instant::now();
    for (c, client) in clients.iter().enumerate() {
        for batch in 0..SEED / BATCH {
            let items: Vec<Value> = (0..BATCH)
                .map(|i| {
                    let n = batch * BATCH + i;
                    let topic = topics[n % topics.len()];
                    json!({"kind": "fact", "title": format!("{topic} note {n}"),
                           "body": format!("Customer {} asked about {topic}; follow-up item {n} \
                                            was assigned to team {}.", n % 97, n % 13),
                           "occurred_at": "2026-11-08T10:00:00Z", "expires_at": null})
                })
                .collect();
            let body = json!({"idempotency_key": format!("seed{c}-{batch}"), "expires_at": later(),
                              "namespace": "a", "session_id": null,
                              "context": context(json!([])), "items": items});
            f.run(client, "observations.save", body).await.unwrap();
        }
    }
    let seeded_in = seeding.elapsed();
    let seed_indexer = Indexer::new(
        f.app.store().clone(),
        f.app.embeddings().clone(),
        IndexerSettings::default(),
    );
    let indexing_seed = Instant::now();
    while seed_indexer.run_once().await.unwrap().claimed > 0 {}
    let seed_indexed_in = indexing_seed.elapsed();
    let percentiles = |mut samples: Vec<Duration>| {
        samples.sort();
        let at = |p: f64| samples[((samples.len() as f64 * p) as usize).min(samples.len() - 1)];
        (samples.len(), at(0.50), at(0.95))
    };
    async fn timed(
        f: Arc<Fixture>,
        clients: Vec<Principal>,
        per_client: usize,
        build: fn(usize, usize) -> (&'static str, Value),
    ) -> Vec<Duration> {
        let mut tasks = Vec::new();
        for (c, client) in clients.into_iter().enumerate() {
            let f = f.clone();
            tasks.push(tokio::spawn(async move {
                let mut samples = Vec::new();
                for i in 0..per_client {
                    let (op, body) = build(c, i);
                    let started = Instant::now();
                    f.run(&client, op, body).await.unwrap();
                    samples.push(started.elapsed());
                }
                samples
            }));
        }
        let mut all = Vec::new();
        for task in tasks {
            all.extend(task.await.unwrap());
        }
        all
    }
    let saves = timed(f.clone(), clients.clone(), 100, |c, i| {
        (
            "observations.save",
            save(&format!("load{c}-{i}"), "a", 1, json!([])),
        )
    })
    .await;
    let indexer = Indexer::new(
        f.app.store().clone(),
        f.app.embeddings().clone(),
        IndexerSettings::default(),
    );
    let started = Instant::now();
    while indexer.run_once().await.unwrap().claimed > 0 {}
    let indexing = started.elapsed();
    let searches = timed(f.clone(), clients, 200, |_, i| {
        let text = ["PostgreSQL", "fact 0", "database choice", "decision"][i % 4];
        (
            "observations.search",
            json!({"namespaces": ["a"], "read_ceiling": 9, "query": text, "limit": 10}),
        )
    })
    .await;
    println!(
        "load: seeded {} memories per namespace × 4 in {seeded_in:?}; indexed in \
         {seed_indexed_in:?} ({embedder})",
        SEED
    );
    let (n, p50, p95) = percentiles(saves);
    println!("load: saves n={n} p50={p50:?} p95={p95:?}");
    println!("load: indexing 400 observations took {indexing:?} ({embedder})");
    let (n, p50, p95) = percentiles(searches);
    println!("load: searches n={n} p50={p50:?} p95={p95:?} ({embedder})");
}
