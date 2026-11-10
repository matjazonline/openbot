//! DB-backed 02B cases: `memory_storage_`, `memory_idempotency_`, the `memory_access_level_` save
//! rows, and HTTP/MCP parity of the real memory operations (`memory_contract_`, `memory_mcp_`).
//! Require MEMORY_TEST_DATABASE_URL.
use super::test_support::{TestDb, test_db};
use crate::{
    adapters::http::{HttpSettings, router},
    application::{
        operations::{Surface, by_id},
        service::{App, Principal},
    },
    domain::{
        credential::{self, TokenKind},
        error::ErrorCode,
    },
};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use chrono::{Duration, Utc};
use serde_json::{Value, json};
use std::sync::{Arc, atomic::AtomicBool};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;
use uuid::Uuid;

pub(super) struct Fixture {
    pub(super) db: TestDb,
    pub(super) app: App,
    pub(super) operator: Principal,
}

pub(super) type Outcome = Result<Value, (ErrorCode, Option<Value>)>;

pub(super) async fn fixture() -> Fixture {
    fixture_with(crate::application::test_embedder::fake_pool(), None).await
}

/// A fixture whose app uses `pool` and, optionally, a lowered exact-scan cap.
pub(super) async fn fixture_with(
    pool: Arc<crate::application::embedding::EmbeddingPool>,
    exact_scan_cap: Option<i64>,
) -> Fixture {
    let mut db = test_db().await;
    if let Some(cap) = exact_scan_cap {
        db.store = db.store.clone().with_exact_scan_cap(cap);
    }
    let app = App::new(Arc::new(db.store.clone()), pool);
    let issued = credential::issue(TokenKind::Admin).unwrap();
    app.store()
        .bootstrap_operator(issued.id, "operator", issued.verifier)
        .await
        .unwrap();
    let operator = app
        .authenticate(Some(&issued.token), Surface::Admin, Utc::now())
        .await
        .unwrap();
    Fixture { db, app, operator }
}

pub(super) fn all_ops() -> Value {
    json!([
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
        "purge"
    ])
}

impl Fixture {
    pub(super) async fn run(&self, principal: &Principal, id: &str, input: Value) -> Outcome {
        self.app
            .execute(
                by_id(id).unwrap(),
                principal,
                input,
                Utc::now(),
                Uuid::now_v7(),
            )
            .await
            .map_err(|error| (error.code, error.details))
    }
    pub(super) async fn tenant(&self, tenant: &str, namespaces: &[&str]) {
        self.run(
            &self.operator,
            "admin.tenants.create",
            json!({"external_id": tenant}),
        )
        .await
        .unwrap();
        for namespace in namespaces {
            self.run(
                &self.operator,
                "admin.namespaces.create",
                json!({"tenant": tenant, "namespace": namespace}),
            )
            .await
            .unwrap();
        }
    }
    /// A client with every operation on each `(namespace, read_max, write_max)`.
    pub(super) async fn client(
        &self,
        tenant: &str,
        grants: &[(&str, i64, i64)],
    ) -> (String, Principal) {
        let grants: Vec<Value> = grants
            .iter()
            .map(|(ns, read, write)| {
                json!({"namespace": ns, "read_max": read, "write_max": write, "operations": all_ops()})
            })
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
        let principal = self
            .app
            .authenticate(Some(&token), Surface::Client, Utc::now())
            .await
            .unwrap();
        (token, principal)
    }
    pub(super) async fn scalar(&self, sql: &str) -> i64 {
        sqlx::query_scalar(sql)
            .fetch_one(self.db.pool())
            .await
            .unwrap()
    }
}

/// One fixed deadline per test process: the deadline is part of the canonical request (D5),
/// so a retry must resend it, and bodies built twice must be identical.
pub(super) fn later() -> String {
    static LATER: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    LATER
        .get_or_init(|| (Utc::now() + Duration::days(1)).to_rfc3339())
        .clone()
}

pub(super) fn context(ceiling: Value, consumed: Value) -> Value {
    json!({"producing_ceiling": ceiling, "consumed": consumed, "sources": [],
           "external_sources": [], "expected_access_level": null})
}

pub(super) fn exact(level: i64) -> Value {
    json!({"kind": "exact", "level": level})
}

pub(super) fn unknown() -> Value {
    json!({"kind": "unknown"})
}

pub(super) fn save(key: &str, namespace: &str, context: Value, items: usize) -> Value {
    let items: Vec<Value> = (0..items)
        .map(|i| {
            json!({"kind": "fact", "title": format!("fact {i}"), "body": "PostgreSQL",
                        "occurred_at": "2026-11-08T10:00:00Z", "expires_at": null})
        })
        .collect();
    json!({"idempotency_key": key, "expires_at": later(), "namespace": namespace,
           "session_id": null, "context": context, "items": items})
}

pub(super) fn saved_id(result: &Value) -> Uuid {
    serde_json::from_value(result["observations"][0]["observation_id"].clone()).unwrap()
}

pub(super) fn refusal(outcome: Outcome) -> String {
    match outcome {
        Err((ErrorCode::ClassificationRefused, Some(details))) => {
            details["reason"].as_str().unwrap().to_owned()
        }
        other => panic!("expected classification_refused, got {other:?}"),
    }
}

pub(super) fn code(outcome: Outcome) -> ErrorCode {
    outcome.expect_err("expected a refusal").0
}

pub(super) async fn record_source(
    f: &Fixture,
    client: &Principal,
    namespace: &str,
    level: i64,
) -> (Uuid, String) {
    let reference = format!("doc:{}", Uuid::now_v7());
    let result = f
        .run(
            client,
            "sources.record",
            json!({"idempotency_key": Uuid::now_v7().to_string(), "expires_at": later(),
                   "namespace": namespace, "session_id": null,
                   "sources": [{"external_ref": reference, "revision": "1", "access_level": level,
                                "occurred_at": "2026-11-08T10:00:00Z", "content": "evidence",
                                "expires_at": null}]}),
        )
        .await
        .unwrap();
    (
        serde_json::from_value(result["sources"][0]["source_id"].clone()).unwrap(),
        "1".to_owned(),
    )
}

pub(super) fn correct(
    key: &str,
    namespace: &str,
    id: Uuid,
    revision: i64,
    ceiling: i64,
    context: Value,
) -> Value {
    json!({"idempotency_key": key, "expires_at": later(), "namespace": namespace,
           "read_ceiling": ceiling, "observation_id": id, "expected_revision": revision,
           "title": "corrected", "body": "new text", "context": context})
}

pub(super) fn fetch(namespaces: &[&str], ceiling: i64, ids: &[Uuid]) -> Value {
    json!({"namespaces": namespaces, "read_ceiling": ceiling, "ids": ids})
}

pub(super) fn delete(key: &str, namespace: &str, id: Uuid, ceiling: i64) -> Value {
    json!({"idempotency_key": key, "expires_at": later(), "namespace": namespace,
           "read_ceiling": ceiling, "observation_id": id})
}

/// Plan 01 table rows and refusals, persisted with their evidence.
#[tokio::test]
async fn memory_access_level_save_labels_follow_the_contract_and_persist_evidence() {
    let f = fixture().await;
    f.tenant("t", &["ns"]).await;
    let (_, client) = f.client("t", &[("ns", 9, 9)]).await;
    let (_, narrow) = f.client("t", &[("ns", 9, 3)]).await;
    let level = |result: &Value| result["observations"][0]["access_level"].as_i64().unwrap();
    // Ceiling 6, proven consumption 4 → 4; missing evidence → the ceiling, 6.
    let four = f
        .run(
            &client,
            "observations.save",
            save("k1", "ns", context(json!(6), exact(4)), 1),
        )
        .await
        .unwrap();
    assert_eq!(level(&four), 4);
    let six = f
        .run(
            &client,
            "observations.save",
            save("k2", "ns", context(json!(6), unknown()), 1),
        )
        .await
        .unwrap();
    assert_eq!(level(&six), 6);
    let row: (i16, i16, String, i16, i16) = sqlx::query_as(
        "SELECT access_level::smallint, producing_ceiling::smallint, watermark_kind, \
         watermark_level::smallint, source_max_level::smallint FROM observations WHERE id = $1",
    )
    .bind(saved_id(&four))
    .fetch_one(f.db.pool())
    .await
    .unwrap();
    assert_eq!(row, (4, 6, "exact".into(), 4, 0));
    let row: (i16, String, i16) = sqlx::query_as(
        "SELECT access_level::smallint, watermark_kind, watermark_level::smallint FROM observations WHERE id = $1",
    )
    .bind(saved_id(&six))
    .fetch_one(f.db.pool())
    .await
    .unwrap();
    assert_eq!(row, (6, "ceiling_fallback".into(), 6));
    // An inherited level-6 context whose only known source is level 0 still saves at 6.
    let mut inherited = context(json!(6), unknown());
    inherited["external_sources"] = json!([{"reference": "note:1", "access_level": 0}]);
    let inherited = f
        .run(&client, "observations.save", save("k3", "ns", inherited, 1))
        .await
        .unwrap();
    assert_eq!(level(&inherited), 6);
    // An explicitly proven public input saves at 0.
    let public = f
        .run(
            &client,
            "observations.save",
            save("k4", "ns", context(json!(9), exact(0)), 1),
        )
        .await
        .unwrap();
    assert_eq!(level(&public), 0);
    // Refusals.
    assert_eq!(
        refusal(
            f.run(
                &client,
                "observations.save",
                save("r1", "ns", context(json!(6), exact(7)), 1)
            )
            .await
        ),
        "consumed_exceeds_ceiling"
    );
    assert_eq!(
        refusal(
            f.run(
                &client,
                "observations.save",
                save("r2", "ns", context(Value::Null, exact(0)), 1)
            )
            .await
        ),
        "missing_trusted_ceiling"
    );
    let mut forged = context(json!(6), exact(4));
    forged["expected_access_level"] = json!(2);
    assert_eq!(
        refusal(
            f.run(&client, "observations.save", save("r3", "ns", forged, 1))
                .await
        ),
        "inconsistent_classification"
    );
    let mut external = context(json!(6), exact(1));
    external["external_sources"] = json!([{"reference": "x", "access_level": 7}]);
    assert_eq!(
        refusal(
            f.run(&client, "observations.save", save("r4", "ns", external, 1))
                .await
        ),
        "source_above_ceiling"
    );
    let (source, revision) = record_source(&f, &client, "ns", 7).await;
    let mut known = context(json!(6), exact(1));
    known["sources"] = json!([{"kind": "source", "source_id": source, "revision": revision}]);
    assert_eq!(
        refusal(
            f.run(&client, "observations.save", save("r5", "ns", known, 1))
                .await
        ),
        "source_above_ceiling"
    );
    // A derived level above the write grant is 403.
    assert_eq!(
        code(
            f.run(
                &narrow,
                "observations.save",
                save("r6", "ns", context(json!(6), exact(4)), 1)
            )
            .await
        ),
        ErrorCode::Forbidden
    );
    // A replay returns the frozen label; relabelling under the same key is a conflict.
    let replay = f
        .run(
            &client,
            "observations.save",
            save("k1", "ns", context(json!(6), exact(4)), 1),
        )
        .await
        .unwrap();
    assert_eq!(replay["replayed"], true);
    assert_eq!(replay["observations"], four["observations"]);
    assert_eq!(
        code(
            f.run(
                &client,
                "observations.save",
                save("k1", "ns", context(json!(6), exact(2)), 1)
            )
            .await
        ),
        ErrorCode::IdempotencyConflict
    );
    assert_eq!(f.scalar("SELECT count(*) FROM observations").await, 4);
}

/// Copies and corrections never declassify; above-ceiling records stay invisible.
#[tokio::test]
async fn memory_access_level_copies_and_corrections_never_declassify() {
    let f = fixture().await;
    f.tenant("t", &["a", "b"]).await;
    let (_, client) = f.client("t", &[("a", 9, 9), ("b", 9, 9)]).await;
    let parent = saved_id(
        &f.run(
            &client,
            "observations.save",
            save("p", "a", context(json!(6), unknown()), 1),
        )
        .await
        .unwrap(),
    );
    // A copy into another namespace carries the parent's level even with public consumption.
    let mut copy = context(json!(6), exact(0));
    copy["sources"] = json!([{"kind": "observation", "observation_id": parent, "revision": 1}]);
    let copied = f
        .run(
            &client,
            "observations.save",
            save("c", "b", copy.clone(), 1),
        )
        .await
        .unwrap();
    assert_eq!(copied["observations"][0]["access_level"], 6);
    // A copy produced under a lower ceiling than the parent is refused, not relabelled.
    let mut low = copy.clone();
    low["producing_ceiling"] = json!(4);
    assert_eq!(
        refusal(
            f.run(&client, "observations.save", save("c2", "b", low, 1))
                .await
        ),
        "source_above_ceiling"
    );
    // A stale parent revision conflicts.
    let mut stale = copy;
    stale["sources"][0]["revision"] = json!(2);
    assert_eq!(
        code(
            f.run(&client, "observations.save", save("c3", "b", stale, 1))
                .await
        ),
        ErrorCode::RevisionConflict
    );
    // A correction with public evidence keeps 6; one with level-8 evidence raises it.
    let kept = f
        .run(
            &client,
            "observations.correct",
            correct("x1", "a", parent, 1, 9, context(json!(9), exact(0))),
        )
        .await
        .unwrap();
    assert_eq!(
        (kept["revision"].as_i64(), kept["access_level"].as_i64()),
        (Some(2), Some(6))
    );
    let raised = f
        .run(
            &client,
            "observations.correct",
            correct("x2", "a", parent, 2, 9, context(json!(8), exact(8))),
        )
        .await
        .unwrap();
    assert_eq!(raised["access_level"], 8);
    // Correcting needs a ceiling that can see the record: below it, the id is not found.
    assert_eq!(
        code(
            f.run(
                &client,
                "observations.correct",
                correct("x3", "a", parent, 3, 7, context(json!(9), exact(0)))
            )
            .await
        ),
        ErrorCode::NotFound
    );
    let view = f
        .run(&client, "observations.fetch", fetch(&["a"], 7, &[parent]))
        .await
        .unwrap();
    assert_eq!(view["unavailable"], json!([parent]));
    assert_eq!(view["exposed_level"], 0);
    let view = f
        .run(&client, "observations.fetch", fetch(&["a"], 8, &[parent]))
        .await
        .unwrap();
    assert_eq!(view["items"][0]["revision"], 3);
    assert_eq!(view["exposed_level"], 8);
}

/// Fetch filters tenant, namespace, ceiling and expiry before returning anything; filtered ids
/// are one indistinguishable `unavailable` list.
#[tokio::test]
async fn memory_storage_fetch_filters_before_returning_and_reports_provenance() {
    let f = fixture().await;
    f.tenant("t", &["a", "b"]).await;
    f.tenant("u", &["a"]).await;
    let (_, client) = f.client("t", &[("a", 6, 6), ("b", 6, 6)]).await;
    let (_, foreign) = f.client("u", &[("a", 9, 9)]).await;
    let (source, revision) = record_source(&f, &client, "a", 2).await;
    let mut ctx = context(json!(6), exact(3));
    ctx["sources"] = json!([{"kind": "source", "source_id": source, "revision": revision}]);
    let mine = saved_id(
        &f.run(&client, "observations.save", save("s1", "a", ctx, 1))
            .await
            .unwrap(),
    );
    let other_ns = saved_id(
        &f.run(
            &client,
            "observations.save",
            save("s2", "b", context(json!(6), exact(1)), 1),
        )
        .await
        .unwrap(),
    );
    let theirs = saved_id(
        &f.run(
            &foreign,
            "observations.save",
            save("s1", "a", context(json!(9), exact(1)), 1),
        )
        .await
        .unwrap(),
    );
    let mut expiring = save("s3", "a", context(json!(6), exact(1)), 1);
    expiring["items"][0]["expires_at"] = json!((Utc::now() + Duration::seconds(1)).to_rfc3339());
    let expired = saved_id(&f.run(&client, "observations.save", expiring).await.unwrap());
    sqlx::query("UPDATE observations SET expires_at = now() - interval '1 second' WHERE id = $1")
        .bind(expired)
        .execute(f.db.pool())
        .await
        .unwrap();
    let unknown_id = Uuid::now_v7();
    let view = f
        .run(
            &client,
            "observations.fetch",
            fetch(&["a"], 6, &[mine, other_ns, theirs, expired, unknown_id]),
        )
        .await
        .unwrap();
    assert_eq!(view["items"].as_array().unwrap().len(), 1);
    let item = &view["items"][0];
    assert_eq!(item["observation_id"], json!(mine));
    assert_eq!(item["namespace"], "a");
    assert_eq!(item["access_level"], 3);
    assert_eq!(item["index_state"], "pending");
    assert_eq!(
        item["sources"],
        json!([{"kind": "source", "source_id": source, "revision": "1"}])
    );
    assert_eq!(
        item["content_sha256"],
        super::content_sha256("fact 0", "PostgreSQL")
    );
    assert_eq!(
        view["unavailable"],
        json!([other_ns, theirs, expired, unknown_id])
    );
    assert_eq!(view["exposed_level"], 3);
    // Asking for a ceiling above the grant or an ungranted namespace is refused outright.
    assert_eq!(
        code(
            f.run(&client, "observations.fetch", fetch(&["a"], 7, &[mine]))
                .await
        ),
        ErrorCode::Forbidden
    );
    assert_eq!(
        code(
            f.run(&foreign, "observations.fetch", fetch(&["b"], 1, &[mine]))
                .await
        ),
        ErrorCode::Forbidden
    );
    // Another tenant cannot borrow this tenant's source either: it is not found.
    let mut borrowed = context(json!(9), exact(1));
    borrowed["sources"] = json!([{"kind": "source", "source_id": source, "revision": "1"}]);
    assert_eq!(
        code(
            f.run(&foreign, "observations.save", save("s9", "a", borrowed, 1))
                .await
        ),
        ErrorCode::NotFound
    );
}

/// Composite tenant-consistent keys refuse rows that borrow another tenant's parents.
#[tokio::test]
async fn memory_storage_cross_tenant_foreign_keys_reject_borrowed_rows() {
    let f = fixture().await;
    f.tenant("t", &["a"]).await;
    f.tenant("u", &["a"]).await;
    let (_, client) = f.client("t", &[("a", 9, 9)]).await;
    let (source, _) = record_source(&f, &client, "a", 1).await;
    let observation = saved_id(
        &f.run(
            &client,
            "observations.save",
            save("k", "a", context(json!(1), exact(1)), 1),
        )
        .await
        .unwrap(),
    );
    let u: Uuid = sqlx::query_scalar("SELECT id FROM tenants WHERE external_id = 'u'")
        .fetch_one(f.db.pool())
        .await
        .unwrap();
    let t_ns: Uuid = sqlx::query_scalar("SELECT n.id FROM namespaces n JOIN tenants t ON t.id = n.tenant_id WHERE t.external_id = 't'")
        .fetch_one(f.db.pool()).await.unwrap();
    for (what, sql) in [
        (
            "an observation in another tenant's namespace",
            "INSERT INTO observations (tenant_id, namespace_id, id, kind, title, body, access_level, \
          producing_ceiling, watermark_kind, watermark_level, source_max_level, origin, occurred_at) \
          VALUES ($1, $2, gen_random_uuid(), 'fact', 't', 'b', 1, 1, 'exact', 1, 0, 'explicit', now())",
        ),
        (
            "a source in another tenant's namespace",
            "INSERT INTO sources (tenant_id, namespace_id, id, external_ref, revision, access_level, \
          content, content_sha256, occurred_at) VALUES ($1, $2, gen_random_uuid(), 'r', '1', 1, 'c', \
          sha256('c'), now())",
        ),
    ] {
        let error = sqlx::query(sql)
            .bind(u)
            .bind(t_ns)
            .execute(f.db.pool())
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("foreign key"), "{what}: {error}");
    }
    let error = sqlx::query(
        "INSERT INTO observation_links (tenant_id, observation_id, ordinal, source_id, source_level) \
         VALUES ($1, $2, 9, $3, 1)",
    )
    .bind(u)
    .bind(observation)
    .bind(source)
    .execute(f.db.pool())
    .await
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("foreign key"),
        "a link borrowing another tenant's rows: {error}"
    );
}

/// Source-label tampering: a link must carry its source's real level, and a source revision
/// cannot be re-recorded with other content or level.
#[tokio::test]
async fn memory_storage_sources_and_sessions_are_immutable_grouped_and_bounded() {
    let f = fixture().await;
    f.tenant("t", &["a", "b"]).await;
    let (_, client) = f.client("t", &[("a", 9, 5), ("b", 9, 9)]).await;
    let session = f
        .run(&client, "sessions.open", json!({"idempotency_key": "o1", "expires_at": later(), "namespace": "a", "external_id": "run-1"}))
        .await
        .unwrap();
    let again = f
        .run(&client, "sessions.open", json!({"idempotency_key": "o2", "expires_at": later(), "namespace": "a", "external_id": "run-1"}))
        .await
        .unwrap();
    assert_eq!(
        session["session_id"], again["session_id"],
        "sessions are idempotent by external id"
    );
    let source = |key: &str, ns: &str, session: &Value, level: i64, content: &str| {
        json!({"idempotency_key": key, "expires_at": later(), "namespace": ns, "session_id": session,
               "sources": [{"external_ref": "doc:1", "revision": "1", "access_level": level,
                            "occurred_at": "2026-11-08T10:00:00Z", "content": content, "expires_at": null}]})
    };
    let first = f
        .run(
            &client,
            "sources.record",
            source("s1", "a", &session["session_id"], 3, "text"),
        )
        .await
        .unwrap();
    let same = f
        .run(
            &client,
            "sources.record",
            source("s2", "a", &session["session_id"], 3, "text"),
        )
        .await
        .unwrap();
    assert_eq!(
        first["sources"][0]["source_id"],
        same["sources"][0]["source_id"]
    );
    assert_eq!(
        code(
            f.run(
                &client,
                "sources.record",
                source("s3", "a", &session["session_id"], 3, "other")
            )
            .await
        ),
        ErrorCode::Conflict
    );
    assert_eq!(
        code(
            f.run(
                &client,
                "sources.record",
                source("s4", "a", &session["session_id"], 4, "text")
            )
            .await
        ),
        ErrorCode::Conflict
    );
    // Above the write grant, and a session of another namespace.
    assert_eq!(
        code(
            f.run(
                &client,
                "sources.record",
                source("s5", "a", &Value::Null, 6, "x")
            )
            .await
        ),
        ErrorCode::Forbidden
    );
    assert_eq!(
        code(
            f.run(
                &client,
                "sources.record",
                source("s6", "b", &session["session_id"], 1, "x")
            )
            .await
        ),
        ErrorCode::NotFound
    );
    // A link cannot claim a lower level than its source.
    let source_id: Uuid = serde_json::from_value(first["sources"][0]["source_id"].clone()).unwrap();
    let observation = saved_id(
        &f.run(
            &client,
            "observations.save",
            save("k", "a", context(json!(5), exact(1)), 1),
        )
        .await
        .unwrap(),
    );
    let tampered = sqlx::query(
        "INSERT INTO observation_links (tenant_id, observation_id, ordinal, source_id, source_level) \
         SELECT tenant_id, id, 5, $2, 1 FROM observations WHERE id = $1",
    )
    .bind(observation)
    .bind(source_id)
    .execute(f.db.pool())
    .await
    .unwrap_err()
    .to_string();
    assert!(tampered.contains("foreign key"), "{tampered}");
    // Closing is idempotent and the closed session keeps its id.
    for key in ["c1", "c2"] {
        let closed = f
            .run(&client, "sessions.close", json!({"idempotency_key": key, "expires_at": later(), "namespace": "a", "session_id": session["session_id"]}))
            .await
            .unwrap();
        assert_eq!(closed["state"], "closed");
    }
    assert_eq!(
        code(f.run(&client, "sessions.close", json!({"idempotency_key": "c3", "expires_at": later(), "namespace": "b", "session_id": session["session_id"]})).await),
        ErrorCode::NotFound
    );
}

async fn add_part(f: &Fixture, observation: Uuid) {
    sqlx::query(
        "INSERT INTO observation_parts (tenant_id, observation_id, namespace_id, access_level, \
         revision, ordinal, byte_start, byte_end, model_id, embedding) \
         SELECT tenant_id, id, namespace_id, access_level, revision, 0, 0, 1, 'test', \
                array_fill(0.1::real, ARRAY[384])::vector FROM observations WHERE id = $1",
    )
    .bind(observation)
    .execute(f.db.pool())
    .await
    .unwrap();
}

/// Correction drops derived parts and requeues indexing under a new fence; delete removes the
/// observation, its parts (vectors) and links, so nothing about it is retrievable.
#[tokio::test]
async fn memory_storage_correction_and_delete_clean_parts_and_links() {
    let f = fixture().await;
    f.tenant("t", &["a"]).await;
    let (_, client) = f.client("t", &[("a", 9, 9)]).await;
    let (source, revision) = record_source(&f, &client, "a", 2).await;
    let mut ctx = context(json!(6), exact(2));
    ctx["sources"] = json!([{"kind": "source", "source_id": source, "revision": revision}]);
    let id = saved_id(
        &f.run(&client, "observations.save", save("k", "a", ctx, 1))
            .await
            .unwrap(),
    );
    let child = {
        let mut copy = context(json!(6), exact(2));
        copy["sources"] = json!([{"kind": "observation", "observation_id": id, "revision": 1}]);
        saved_id(
            &f.run(&client, "observations.save", save("child", "a", copy, 1))
                .await
                .unwrap(),
        )
    };
    add_part(&f, id).await;
    sqlx::query("UPDATE observations SET index_state = 'indexed' WHERE id = $1")
        .bind(id)
        .execute(f.db.pool())
        .await
        .unwrap();
    f.run(
        &client,
        "observations.correct",
        correct("x", "a", id, 1, 9, context(json!(6), exact(2))),
    )
    .await
    .unwrap();
    let (parts, links, state, fence): (i64, i64, String, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM observation_parts WHERE observation_id = $1), \
                (SELECT count(*) FROM observation_links WHERE observation_id = $1), \
                index_state, index_fence FROM observations WHERE id = $1",
    )
    .bind(id)
    .fetch_one(f.db.pool())
    .await
    .unwrap();
    assert_eq!((parts, links, state.as_str(), fence), (0, 0, "pending", 1));
    add_part(&f, id).await;
    let deleted = f
        .run(&client, "observations.delete", delete("d", "a", id, 9))
        .await
        .unwrap();
    assert_eq!(deleted["deleted"], true);
    assert_eq!(f.scalar("SELECT count(*) FROM observation_parts").await, 0);
    let remaining = f
        .scalar(&format!("SELECT count(*) FROM observation_links WHERE observation_id = '{id}' OR parent_observation_id = '{id}'"))
        .await;
    assert_eq!(remaining, 0);
    let view = f
        .run(
            &client,
            "observations.fetch",
            fetch(&["a"], 9, &[id, child]),
        )
        .await
        .unwrap();
    assert_eq!(view["unavailable"], json!([id]));
    assert_eq!(
        view["items"][0]["sources"],
        json!([]),
        "the child's link to the deleted parent is gone"
    );
    // A replay reports the original result; a new delete of the gone id is not found.
    assert_eq!(
        f.run(&client, "observations.delete", delete("d", "a", id, 9))
            .await
            .unwrap()["replayed"],
        true
    );
    assert_eq!(
        code(
            f.run(&client, "observations.delete", delete("d2", "a", id, 9))
                .await
        ),
        ErrorCode::NotFound
    );
    let audited = f.scalar("SELECT count(*) FROM audit_events WHERE actor_kind = 'client' AND action = 'observation.delete'").await;
    assert_eq!(audited, 1);
}

/// Competing correctors at the same expected revision: exactly one wins, and the others
/// provably wait on its uncommitted row lock before they are refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn memory_storage_concurrent_corrections_serialize_on_the_revision() {
    let f = Arc::new(fixture().await);
    f.tenant("t", &["a"]).await;
    let (_, client) = f.client("t", &[("a", 9, 9)]).await;
    let id = saved_id(
        &f.run(
            &client,
            "observations.save",
            save("k", "a", context(json!(6), exact(2)), 1),
        )
        .await
        .unwrap(),
    );
    let tasks: Vec<_> = (0..8)
        .map(|i| {
            let (f, client) = (f.clone(), client.clone());
            tokio::spawn(async move {
                // `hold-` keys keep each claimant's transaction open after its row lock.
                f.run(
                    &client,
                    "observations.correct",
                    correct(
                        &format!("hold-x{i}"),
                        "a",
                        id,
                        1,
                        9,
                        context(json!(6), exact(2)),
                    ),
                )
                .await
            })
        })
        .collect();
    assert!(
        lock_waits_observed(&f).await,
        "losers must wait on the winner's row lock"
    );
    let mut won = 0;
    for task in tasks {
        match task.await.unwrap() {
            Ok(result) => {
                won += 1;
                assert_eq!(result["revision"], 2);
            }
            Err((code, _)) => assert_eq!(code, ErrorCode::RevisionConflict),
        }
    }
    assert_eq!(won, 1);
    assert_eq!(
        f.scalar(&format!(
            "SELECT revision FROM observations WHERE id = '{id}'"
        ))
        .await,
        2
    );
}

/// Poll this test database for a backend waiting on a heavyweight lock.
async fn lock_waits_observed(f: &Fixture) -> bool {
    // Its own connection: the claimants may hold every pooled one.
    use sqlx::Connection;
    let mut db = sqlx::PgConnection::connect_with(&f.db.owner_options)
        .await
        .unwrap();
    for _ in 0..80 {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity WHERE datname = current_database() AND wait_event_type = 'Lock'",
        )
        .fetch_one(&mut db)
        .await
        .unwrap();
        if waiting > 0 {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    false
}

/// Delete racing correction and copy: each loser is `not_found`, nothing is resurrected, and a
/// copy that won keeps its label without a dangling link.
#[tokio::test]
async fn memory_storage_delete_races_writes_without_resurrection() {
    let f = Arc::new(fixture().await);
    f.tenant("t", &["a"]).await;
    let (_, client) = f.client("t", &[("a", 9, 9)]).await;
    for round in 0..6 {
        let id = saved_id(
            &f.run(
                &client,
                "observations.save",
                save(&format!("k{round}"), "a", context(json!(6), unknown()), 1),
            )
            .await
            .unwrap(),
        );
        let spawn = |id_: &'static str, body: Value| {
            let (f, client) = (f.clone(), client.clone());
            tokio::spawn(async move { f.run(&client, id_, body).await })
        };
        let mut copy = context(json!(6), exact(0));
        copy["sources"] = json!([{"kind": "observation", "observation_id": id, "revision": 1}]);
        let corrector = spawn(
            "observations.correct",
            correct(
                &format!("x{round}"),
                "a",
                id,
                1,
                9,
                context(json!(6), exact(0)),
            ),
        );
        let copier = spawn(
            "observations.save",
            save(&format!("c{round}"), "a", copy, 1),
        );
        let deleter = spawn(
            "observations.delete",
            delete(&format!("d{round}"), "a", id, 9),
        );
        assert!(deleter.await.unwrap().is_ok());
        for outcome in [corrector.await.unwrap(), copier.await.unwrap()] {
            match outcome {
                Ok(result) => {
                    if let Some(level) = result["observations"][0]["access_level"].as_i64() {
                        assert_eq!(level, 6, "a copy keeps its parent's label");
                    }
                }
                Err((code, _)) => assert!(
                    matches!(code, ErrorCode::NotFound | ErrorCode::RevisionConflict),
                    "{code:?}"
                ),
            }
        }
        assert_eq!(
            f.scalar(&format!(
                "SELECT count(*) FROM observations WHERE id = '{id}'"
            ))
            .await,
            0
        );
        assert_eq!(
            f.scalar(&format!(
                "SELECT count(*) FROM observation_links WHERE parent_observation_id = '{id}'"
            ))
            .await,
            0
        );
    }
}

/// D5 with competing claimants: concurrent identical saves give one observation and one
/// original result; the others provably wait on the winner's uncommitted record, then replay.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn memory_idempotency_concurrent_identical_saves_create_one_observation() {
    let f = Arc::new(fixture().await);
    f.tenant("t", &["a"]).await;
    let (_, client) = f.client("t", &[("a", 9, 9)]).await;
    // The `hold-` key keeps the winner's claimed record uncommitted for 400 ms.
    let body = save("hold-same", "a", context(json!(6), exact(4)), 3);
    let tasks: Vec<_> = (0..8)
        .map(|_| {
            let (f, client, body) = (f.clone(), client.clone(), body.clone());
            tokio::spawn(async move { f.run(&client, "observations.save", body).await.unwrap() })
        })
        .collect();
    assert!(
        lock_waits_observed(&f).await,
        "identical saves must wait on the uncommitted record"
    );
    let mut results = Vec::new();
    for task in tasks {
        results.push(task.await.unwrap());
    }
    assert_eq!(results.iter().filter(|r| r["replayed"] == false).count(), 1);
    for result in &results {
        assert_eq!(result["observations"], results[0]["observations"]);
    }
    assert_eq!(f.scalar("SELECT count(*) FROM observations").await, 3);
    assert_eq!(
        f.scalar("SELECT count(*) FROM idempotency_records WHERE kind = 'observations.save'")
            .await,
        1
    );
}

/// Conflicting keys, request expiry, key scope and the 7-day window. The deadline is part of
/// the canonical request (D5): a retry must resend it, so a changed deadline conflicts.
#[tokio::test]
async fn memory_idempotency_conflicts_expiry_and_scope() {
    let f = fixture().await;
    f.tenant("t", &["a", "b"]).await;
    let (_, client) = f.client("t", &[("a", 9, 9), ("b", 9, 9)]).await;
    let mut first = save("k", "a", context(json!(6), exact(4)), 1);
    first["expires_at"] = json!((Utc::now() + Duration::milliseconds(1500)).to_rfc3339());
    let original = f
        .run(&client, "observations.save", first.clone())
        .await
        .unwrap();
    let mut changed = first.clone();
    changed["items"][0]["body"] = json!("other");
    assert_eq!(
        code(f.run(&client, "observations.save", changed).await),
        ErrorCode::IdempotencyConflict
    );
    let mut elsewhere = first.clone();
    elsewhere["namespace"] = json!("b");
    assert_eq!(
        code(f.run(&client, "observations.save", elsewhere).await),
        ErrorCode::IdempotencyConflict
    );
    let mut extended = first.clone();
    extended["expires_at"] = json!(later());
    assert_eq!(
        code(f.run(&client, "observations.save", extended).await),
        ErrorCode::IdempotencyConflict
    );
    let stored: chrono::DateTime<Utc> =
        sqlx::query_scalar("SELECT request_expires_at FROM idempotency_records WHERE key = 'k'")
            .fetch_one(f.db.pool())
            .await
            .unwrap();
    assert!(
        stored < Utc::now() + Duration::seconds(2),
        "the record keeps the request's own deadline"
    );
    // The key is scoped by use case: the same key opens a session.
    f.run(&client, "sessions.open", json!({"idempotency_key": "k", "expires_at": later(), "namespace": "a", "external_id": "s"})).await.unwrap();
    // A replay after the request's deadline still returns the original result.
    tokio::time::sleep(std::time::Duration::from_millis(1600)).await;
    let replay = f.run(&client, "observations.save", first).await.unwrap();
    assert_eq!(
        (replay["replayed"].clone(), replay["observations"].clone()),
        (json!(true), original["observations"].clone())
    );
    // A new execution past its deadline is refused and leaves no record behind.
    let mut expired = save("fresh", "a", context(json!(6), exact(4)), 1);
    expired["expires_at"] = json!((Utc::now() - Duration::seconds(5)).to_rfc3339());
    assert_eq!(
        code(f.run(&client, "observations.save", expired).await),
        ErrorCode::RequestExpired
    );
    assert_eq!(
        f.scalar("SELECT count(*) FROM idempotency_records WHERE key = 'fresh'")
            .await,
        0
    );
    let mut far = save("far", "a", context(json!(6), exact(4)), 1);
    far["expires_at"] = json!((Utc::now() + Duration::days(8)).to_rfc3339());
    assert_eq!(
        code(f.run(&client, "observations.save", far).await),
        ErrorCode::InvalidRequest
    );
    assert_eq!(f.scalar("SELECT count(*) FROM observations").await, 1);
}

/// A client write re-checks, inside its transaction, the state authentication read earlier.
#[tokio::test]
async fn memory_authority_client_writes_recheck_namespace_credential_and_tenant() {
    let f = fixture().await;
    f.tenant("t", &["a", "b"]).await;
    let (token, client) = f.client("t", &[("a", 9, 9), ("b", 9, 9)]).await;
    f.run(
        &f.operator,
        "admin.namespaces.delete",
        json!({"tenant": "t", "namespace": "b"}),
    )
    .await
    .unwrap();
    assert_eq!(
        code(
            f.run(
                &client,
                "observations.save",
                save("k1", "b", context(json!(1), exact(1)), 1)
            )
            .await
        ),
        ErrorCode::Forbidden
    );
    f.run(&f.operator, "admin.tenants.disable", json!({"tenant": "t"}))
        .await
        .unwrap();
    assert_eq!(
        code(
            f.run(
                &client,
                "observations.save",
                save("k2", "a", context(json!(1), exact(1)), 1)
            )
            .await
        ),
        ErrorCode::TenantDisabled
    );
    f.run(&f.operator, "admin.tenants.enable", json!({"tenant": "t"}))
        .await
        .unwrap();
    let id = credential::parse(&token).unwrap().id;
    f.run(
        &f.operator,
        "admin.credentials.revoke",
        json!({"tenant": "t", "credential_id": id}),
    )
    .await
    .unwrap();
    assert_eq!(
        code(
            f.run(
                &client,
                "observations.save",
                save("k3", "a", context(json!(1), exact(1)), 1)
            )
            .await
        ),
        ErrorCode::CredentialRevoked
    );
    assert_eq!(f.scalar("SELECT count(*) FROM observations").await, 0);
}

pub(super) async fn http(
    router: &axum::Router,
    path: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::HOST, "localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 8 << 20).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

pub(super) async fn tool(
    router: &axum::Router,
    token: &str,
    name: &str,
    arguments: Value,
) -> Value {
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                      "params": {"name": name, "arguments": arguments}});
    http(router, "/mcp", token, body).await.1["result"].clone()
}

pub(super) fn pg_router(f: &Fixture) -> axum::Router {
    router(
        f.app.clone(),
        Arc::new(AtomicBool::new(true)),
        &HttpSettings {
            max_in_flight: 64,
            request_deadline: std::time::Duration::from_secs(10),
            mcp_allowed_hosts: vec!["localhost".into()],
            mcp_allowed_origins: vec![],
        },
        CancellationToken::new(),
    )
}

/// The real memory operations answer the same results and the same denials over HTTP and MCP.
#[tokio::test]
async fn memory_mcp_memory_operations_match_http_over_postgres() {
    let f = fixture().await;
    f.tenant("t", &["a", "b"]).await;
    let (token, _) = f.client("t", &[("a", 6, 6)]).await;
    let router = pg_router(&f);
    let (status, saved) = http(
        &router,
        "/v1/observations/save",
        &token,
        save("h1", "a", context(json!(6), exact(4)), 1),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let id = saved_id(&saved);
    let via_mcp = tool(
        &router,
        &token,
        "memory_save",
        save("m1", "a", context(json!(6), exact(4)), 1),
    )
    .await;
    assert_eq!(
        via_mcp["structuredContent"]["observations"][0]["access_level"],
        4
    );
    let (_, by_http) = http(
        &router,
        "/v1/observations/fetch",
        &token,
        fetch(&["a"], 6, &[id]),
    )
    .await;
    let by_tool = tool(&router, &token, "memory_fetch", fetch(&["a"], 6, &[id])).await;
    assert_eq!(by_tool["structuredContent"], by_http);
    // Identical denials: an ungranted namespace and a refused classification.
    let (status, body) = http(
        &router,
        "/v1/observations/save",
        &token,
        save("h2", "b", context(json!(1), exact(1)), 1),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].clone()),
        (StatusCode::FORBIDDEN, json!("forbidden"))
    );
    let denied = tool(
        &router,
        &token,
        "memory_save",
        save("m2", "b", context(json!(1), exact(1)), 1),
    )
    .await;
    assert_eq!(
        (
            denied["isError"].clone(),
            denied["structuredContent"]["error"]["code"].clone()
        ),
        (json!(true), json!("forbidden"))
    );
    let (status, body) = http(
        &router,
        "/v1/observations/save",
        &token,
        save("h3", "a", context(json!(6), exact(7)), 1),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let refused = tool(
        &router,
        &token,
        "memory_save",
        save("m3", "a", context(json!(6), exact(7)), 1),
    )
    .await;
    assert_eq!(refused["structuredContent"]["error"], body["error"]);
}

/// HTTP statuses of the memory error taxonomy over the real store.
#[tokio::test]
async fn memory_contract_memory_operations_map_errors_to_statuses() {
    let f = fixture().await;
    f.tenant("t", &["a"]).await;
    let (token, _) = f.client("t", &[("a", 6, 6)]).await;
    let router = pg_router(&f);
    let (_, saved) = http(
        &router,
        "/v1/observations/save",
        &token,
        save("k", "a", context(json!(6), exact(4)), 1),
    )
    .await;
    let id = saved_id(&saved);
    let mut changed = save("k", "a", context(json!(6), exact(3)), 1);
    changed["items"][0]["title"] = json!("x");
    let mut expired = save("e", "a", context(json!(6), exact(1)), 1);
    expired["expires_at"] = json!((Utc::now() - Duration::seconds(1)).to_rfc3339());
    for (path, body, status, error) in [
        (
            "/v1/observations/save",
            changed,
            StatusCode::CONFLICT,
            "idempotency_conflict",
        ),
        (
            "/v1/observations/save",
            expired,
            StatusCode::GONE,
            "request_expired",
        ),
        (
            "/v1/observations/correct",
            correct("c", "a", id, 9, 6, context(json!(6), exact(4))),
            StatusCode::CONFLICT,
            "revision_conflict",
        ),
        (
            "/v1/observations/delete",
            delete("d", "a", Uuid::now_v7(), 6),
            StatusCode::NOT_FOUND,
            "not_found",
        ),
        (
            "/v1/observations/save",
            save("n", "a", context(Value::Null, exact(1)), 1),
            StatusCode::UNPROCESSABLE_ENTITY,
            "classification_refused",
        ),
    ] {
        let (got, response) = http(&router, path, &token, body).await;
        assert_eq!(
            (got, response["error"]["code"].as_str()),
            (status, Some(error)),
            "{path}: {response}"
        );
    }
}

pub(super) fn copy_of(parent: Uuid, revision: i64, ceiling: i64) -> Value {
    let mut ctx = context(json!(ceiling), exact(0));
    ctx["sources"] =
        json!([{"kind": "observation", "observation_id": parent, "revision": revision}]);
    ctx
}

pub(super) async fn issue(f: &Fixture, tenant: &str, grants: Value) -> Principal {
    let issued = f
        .run(
            &f.operator,
            "admin.credentials.issue",
            json!({"tenant": tenant, "label": "c", "expires_at": null, "grants": grants}),
        )
        .await
        .unwrap();
    f.app
        .authenticate(issued["token"].as_str(), Surface::Client, Utc::now())
        .await
        .unwrap()
}

/// F1: provenance in fetch passes the same filter as the records it names: granted active
/// namespace, current level within the ceiling, not expired. Anything else is omitted.
#[tokio::test]
async fn memory_storage_fetch_omits_provenance_the_reader_cannot_see() {
    let f = fixture().await;
    f.tenant("t", &["a", "b"]).await;
    let (_, writer) = f.client("t", &[("a", 9, 9), ("b", 9, 9)]).await;
    let (_, reader) = f.client("t", &[("b", 9, 9)]).await;
    let parent = saved_id(
        &f.run(
            &writer,
            "observations.save",
            save("p", "a", context(json!(6), unknown()), 1),
        )
        .await
        .unwrap(),
    );
    let copy = saved_id(
        &f.run(
            &writer,
            "observations.save",
            save("c", "b", copy_of(parent, 1, 6), 1),
        )
        .await
        .unwrap(),
    );
    let sources = |view: &Value| view["items"][0]["sources"].clone();
    let parent_ref = json!([{"kind": "observation", "observation_id": parent, "revision": 1}]);
    // Provenance is filtered by the namespaces the read names (D4), so the writer names both.
    assert_eq!(
        sources(
            &f.run(&writer, "observations.fetch", fetch(&["b"], 6, &[copy]))
                .await
                .unwrap()
        ),
        json!([])
    );
    assert_eq!(
        sources(
            &f.run(
                &writer,
                "observations.fetch",
                fetch(&["a", "b"], 6, &[copy])
            )
            .await
            .unwrap()
        ),
        parent_ref
    );
    // A reader without the parent's namespace sees the copy, but not the parent's id.
    assert_eq!(
        sources(
            &f.run(&reader, "observations.fetch", fetch(&["b"], 9, &[copy]))
                .await
                .unwrap()
        ),
        json!([])
    );
    // A parent raised above the ceiling disappears from provenance at that ceiling.
    f.run(
        &writer,
        "observations.correct",
        correct("x", "a", parent, 1, 9, context(json!(8), exact(8))),
    )
    .await
    .unwrap();
    assert_eq!(
        sources(
            &f.run(
                &writer,
                "observations.fetch",
                fetch(&["a", "b"], 6, &[copy])
            )
            .await
            .unwrap()
        ),
        json!([])
    );
    assert_eq!(
        sources(
            &f.run(
                &writer,
                "observations.fetch",
                fetch(&["a", "b"], 8, &[copy])
            )
            .await
            .unwrap()
        ),
        parent_ref
    );
    // An expired source disappears from provenance.
    let (source, revision) = record_source(&f, &writer, "b", 1).await;
    let mut cited = context(json!(6), exact(1));
    cited["sources"] = json!([{"kind": "source", "source_id": source, "revision": revision}]);
    let evidenced = saved_id(
        &f.run(&writer, "observations.save", save("e", "b", cited, 1))
            .await
            .unwrap(),
    );
    for sql in [
        "ALTER TABLE sources DISABLE TRIGGER sources_immutable",
        "UPDATE sources SET expires_at = now() - interval '1 second'",
        "ALTER TABLE sources ENABLE TRIGGER sources_immutable",
    ] {
        sqlx::query(sql).execute(f.db.pool()).await.unwrap();
    }
    assert_eq!(
        sources(
            &f.run(
                &writer,
                "observations.fetch",
                fetch(&["b"], 9, &[evidenced])
            )
            .await
            .unwrap()
        ),
        json!([])
    );
    // A deleted namespace's parents disappear too.
    f.run(
        &f.operator,
        "admin.namespaces.delete",
        json!({"tenant": "t", "namespace": "a"}),
    )
    .await
    .unwrap();
    assert_eq!(
        sources(
            &f.run(
                &writer,
                "observations.fetch",
                fetch(&["a", "b"], 9, &[copy])
            )
            .await
            .unwrap_or_else(|e| panic!("{e:?}"))
        ),
        json!([])
    );
}

/// F3, F4, m2, m8 and self-citation.
#[tokio::test]
async fn memory_authority_citation_needs_read_authority_and_deletion_write_authority() {
    let f = fixture().await;
    f.tenant("t", &["a", "b"]).await;
    let (_, writer) = f.client("t", &[("a", 9, 9), ("b", 9, 9)]).await;
    let parent = saved_id(
        &f.run(
            &writer,
            "observations.save",
            save("p", "a", context(json!(5), exact(5)), 1),
        )
        .await
        .unwrap(),
    );
    // F3: read_max without a read operation gives no citation authority, and reveals nothing:
    // the right revision, a wrong one and a low ceiling are all the same not_found.
    let saver = issue(
        &f,
        "t",
        json!([
        {"namespace": "a", "read_max": 9, "write_max": 9, "operations": ["save"]},
        {"namespace": "b", "read_max": 9, "write_max": 9, "operations": ["save", "fetch"]}]),
    )
    .await;
    for (key, revision, ceiling) in [("s1", 1, 6), ("s2", 2, 6), ("s3", 1, 3)] {
        assert_eq!(
            code(
                f.run(
                    &saver,
                    "observations.save",
                    save(key, "b", copy_of(parent, revision, ceiling), 1)
                )
                .await
            ),
            ErrorCode::NotFound
        );
    }
    // F4: deleting is a write at the record's level.
    let deleter = issue(
        &f,
        "t",
        json!([{"namespace": "a", "read_max": 9, "write_max": 0, "operations": ["delete"]}]),
    )
    .await;
    assert_eq!(
        code(
            f.run(&deleter, "observations.delete", delete("d", "a", parent, 9))
                .await
        ),
        ErrorCode::Forbidden
    );
    // m8: a replay by another credential re-runs that credential's authority.
    let body = save("shared", "b", copy_of(parent, 1, 6), 1);
    let original = f
        .run(&writer, "observations.save", body.clone())
        .await
        .unwrap();
    assert_eq!(original["observations"][0]["access_level"], 5);
    let (_, low) = f.client("t", &[("a", 9, 9), ("b", 9, 3)]).await;
    assert_eq!(
        code(f.run(&low, "observations.save", body.clone()).await),
        ErrorCode::Forbidden
    );
    // n3: a parent the replaying credential cannot read is not_found, as a fresh citation is.
    let (_, blind) = f.client("t", &[("b", 9, 9)]).await;
    assert_eq!(
        code(f.run(&blind, "observations.save", body.clone()).await),
        ErrorCode::NotFound
    );
    assert_eq!(
        code(
            f.run(
                &blind,
                "observations.save",
                save("fresh", "b", copy_of(parent, 1, 6), 1)
            )
            .await
        ),
        ErrorCode::NotFound
    );
    let (_, peer) = f.client("t", &[("a", 9, 9), ("b", 9, 9)]).await;
    assert_eq!(
        f.run(&peer, "observations.save", body.clone())
            .await
            .unwrap()["replayed"],
        true
    );
    // Once the parent is gone, every other credential's replay is not_found (a fresh request
    // would be too), so the replay reveals nothing; the executing credential still replays.
    let doomed = saved_id(
        &f.run(
            &writer,
            "observations.save",
            save("doomed", "a", context(json!(5), exact(5)), 1),
        )
        .await
        .unwrap(),
    );
    let copy = save("copy-doomed", "b", copy_of(doomed, 1, 6), 1);
    f.run(&writer, "observations.save", copy.clone())
        .await
        .unwrap();
    f.run(
        &writer,
        "observations.delete",
        delete("del-doomed", "a", doomed, 9),
    )
    .await
    .unwrap();
    for other in [&blind, &peer] {
        assert_eq!(
            code(f.run(other, "observations.save", copy.clone()).await),
            ErrorCode::NotFound
        );
    }
    assert_eq!(
        code(
            f.run(
                &peer,
                "observations.save",
                save("fresh-doomed", "b", copy_of(doomed, 1, 6), 1)
            )
            .await
        ),
        ErrorCode::NotFound
    );
    assert_eq!(
        f.run(&writer, "observations.save", copy).await.unwrap()["replayed"],
        true
    );
    // A correction cannot cite itself.
    let mut own = context(json!(9), exact(0));
    own["sources"] = json!([{"kind": "observation", "observation_id": parent, "revision": 1}]);
    assert_eq!(
        code(
            f.run(
                &writer,
                "observations.correct",
                correct("o", "a", parent, 1, 9, own)
            )
            .await
        ),
        ErrorCode::InvalidRequest
    );
    // m2: a cited namespace deleted after authentication is re-checked under its lock.
    f.run(
        &f.operator,
        "admin.namespaces.delete",
        json!({"tenant": "t", "namespace": "a"}),
    )
    .await
    .unwrap();
    assert_eq!(
        code(
            f.run(
                &writer,
                "observations.save",
                save("late", "b", copy_of(parent, 1, 6), 1)
            )
            .await
        ),
        ErrorCode::NotFound
    );
}

/// m1, m3, m4: a carried label stays explained; re-records with another retention conflict;
/// cited revisions are bounded.
#[tokio::test]
async fn memory_storage_carried_labels_source_retention_and_reference_bounds() {
    let f = fixture().await;
    f.tenant("t", &["a"]).await;
    let (_, client) = f.client("t", &[("a", 9, 9)]).await;
    let id = saved_id(
        &f.run(
            &client,
            "observations.save",
            save("k", "a", context(json!(6), unknown()), 1),
        )
        .await
        .unwrap(),
    );
    let evidence = |f: &Fixture, id: Uuid| {
        let pool = f.db.pool().clone();
        async move {
            sqlx::query_as::<_, (i16, i16, i16, Option<i16>)>(
                "SELECT access_level::smallint, watermark_level::smallint, source_max_level::smallint, \
                 carried_level::smallint FROM observations WHERE id = $1",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };
    f.run(
        &client,
        "observations.correct",
        correct("x1", "a", id, 1, 9, context(json!(6), exact(2))),
    )
    .await
    .unwrap();
    assert_eq!(
        evidence(&f, id).await,
        (6, 2, 0, Some(6)),
        "the retained 6 is carried, not unexplained"
    );
    f.run(
        &client,
        "observations.correct",
        correct("x2", "a", id, 2, 9, context(json!(8), exact(8))),
    )
    .await
    .unwrap();
    assert_eq!(evidence(&f, id).await, (8, 8, 0, None));
    let record = |key: &str, expires: Value| {
        json!({"idempotency_key": key, "expires_at": later(), "namespace": "a", "session_id": null,
               "sources": [{"external_ref": "doc:r", "revision": "1", "access_level": 1,
                            "occurred_at": "2026-11-08T10:00:00Z", "content": "c", "expires_at": expires}]})
    };
    f.run(&client, "sources.record", record("r1", Value::Null))
        .await
        .unwrap();
    assert_eq!(
        code(
            f.run(&client, "sources.record", record("r2", json!(later())))
                .await
        ),
        ErrorCode::Conflict
    );
    // n2: timestamps are compared at the stored microsecond precision, so a re-record whose
    // occurred_at/expires_at differ only below a microsecond names the same source, and the
    // same key with such a difference replays instead of conflicting.
    let day = (Utc::now() + Duration::days(2))
        .format("%Y-%m-%d")
        .to_string();
    let precise = |key: &str, occurred: &str, expires: &str| {
        json!({"idempotency_key": key, "expires_at": later(), "namespace": "a", "session_id": null,
               "sources": [{"external_ref": "doc:precise", "revision": "1", "access_level": 1,
                            "occurred_at": occurred, "content": "c",
                            "expires_at": format!("{day}T00:00:00{expires}Z")}]})
    };
    let first = f
        .run(
            &client,
            "sources.record",
            precise("p1", "2026-11-08T10:00:00.123456789Z", ".000000001"),
        )
        .await
        .unwrap();
    let again = f
        .run(
            &client,
            "sources.record",
            precise("p2", "2026-11-08T10:00:00.123456Z", ""),
        )
        .await
        .unwrap();
    assert_eq!(again["replayed"], false);
    assert_eq!(
        first["sources"][0]["source_id"],
        again["sources"][0]["source_id"]
    );
    let replay = f
        .run(
            &client,
            "sources.record",
            precise("p1", "2026-11-08T10:00:00.123456999Z", ".000000999"),
        )
        .await
        .unwrap();
    assert_eq!(replay["replayed"], true);
    assert_eq!(
        code(
            f.run(
                &client,
                "sources.record",
                precise("p3", "2026-11-08T10:00:00.123457Z", "")
            )
            .await
        ),
        ErrorCode::Conflict,
        "a microsecond is a real difference"
    );
    let mut long = context(json!(6), exact(1));
    long["sources"] =
        json!([{"kind": "source", "source_id": Uuid::now_v7(), "revision": "r".repeat(129)}]);
    assert_eq!(
        code(
            f.run(&client, "observations.save", save("long", "a", long, 1))
                .await
        ),
        ErrorCode::InvalidRequest
    );
}

/// Tenant-consistent keys also cover sessions, parent links and parts.
#[tokio::test]
async fn memory_storage_cross_tenant_keys_cover_sessions_parent_links_and_parts() {
    let f = fixture().await;
    f.tenant("t", &["a"]).await;
    f.tenant("u", &["a"]).await;
    let (_, t_client) = f.client("t", &[("a", 9, 9)]).await;
    let (_, u_client) = f.client("u", &[("a", 9, 9)]).await;
    let t_obs = saved_id(
        &f.run(
            &t_client,
            "observations.save",
            save("k", "a", context(json!(1), exact(1)), 1),
        )
        .await
        .unwrap(),
    );
    let u_obs = saved_id(
        &f.run(
            &u_client,
            "observations.save",
            save("k", "a", context(json!(1), exact(1)), 1),
        )
        .await
        .unwrap(),
    );
    let session = f
        .run(&t_client, "sessions.open", json!({"idempotency_key": "s", "expires_at": later(), "namespace": "a", "external_id": "s"}))
        .await
        .unwrap()["session_id"]
        .clone();
    let session: Uuid = serde_json::from_value(session).unwrap();
    let u: Uuid = sqlx::query_scalar("SELECT id FROM tenants WHERE external_id = 'u'")
        .fetch_one(f.db.pool())
        .await
        .unwrap();
    let cases = [
        (
            "an observation in another tenant's session",
            "UPDATE observations SET session_id = $3 WHERE tenant_id = $1 AND id = $2",
        ),
        (
            "a parent link to another tenant's observation",
            "INSERT INTO observation_links (tenant_id, observation_id, ordinal, parent_observation_id, \
          parent_revision, source_level) VALUES ($1, $2, 7, $4, 1, 1)",
        ),
        (
            "a part of another tenant's observation",
            "INSERT INTO observation_parts (tenant_id, observation_id, namespace_id, access_level, revision, \
          ordinal, byte_start, byte_end, model_id, embedding) SELECT $1, id, namespace_id, access_level, \
          revision, 0, 0, 1, 'm', array_fill(0.1::real, ARRAY[384])::vector FROM observations WHERE id = $4",
        ),
    ];
    for (what, sql) in cases {
        let error = sqlx::query(sql)
            .bind(u)
            .bind(u_obs)
            .bind(session)
            .bind(t_obs)
            .execute(f.db.pool())
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("foreign key"), "{what}: {error}");
    }
}

/// Correction and copy denials are identical over HTTP and MCP.
#[tokio::test]
async fn memory_mcp_correction_and_copy_denials_match_http() {
    let f = fixture().await;
    f.tenant("t", &["a"]).await;
    let (token, _) = f.client("t", &[("a", 6, 6)]).await;
    let router = pg_router(&f);
    let (_, saved) = http(
        &router,
        "/v1/observations/save",
        &token,
        save("p", "a", context(json!(6), unknown()), 1),
    )
    .await;
    let parent = saved_id(&saved);
    let cases = [
        (
            "/v1/observations/correct",
            "memory_correct",
            correct("c1", "a", parent, 9, 6, context(json!(6), exact(1))),
            "revision_conflict",
        ),
        (
            "/v1/observations/correct",
            "memory_correct",
            correct("c2", "a", Uuid::now_v7(), 1, 6, context(json!(6), exact(1))),
            "not_found",
        ),
        (
            "/v1/observations/correct",
            "memory_correct",
            correct("c3", "a", parent, 1, 5, context(json!(6), exact(1))),
            "not_found",
        ),
        (
            "/v1/observations/save",
            "memory_save",
            save("s1", "a", copy_of(Uuid::now_v7(), 1, 6), 1),
            "not_found",
        ),
        (
            "/v1/observations/save",
            "memory_save",
            save("s2", "a", copy_of(parent, 1, 4), 1),
            "classification_refused",
        ),
        (
            "/v1/observations/save",
            "memory_save",
            save("s3", "a", copy_of(parent, 2, 6), 1),
            "revision_conflict",
        ),
    ];
    for (path, name, body, expected) in cases {
        let (_, over_http) = http(&router, path, &token, body.clone()).await;
        let mut retried = body;
        retried["idempotency_key"] = json!(format!(
            "mcp-{}",
            retried["idempotency_key"].as_str().unwrap()
        ));
        let over_mcp = tool(&router, &token, name, retried).await;
        assert_eq!(over_http["error"]["code"], expected, "{path}: {over_http}");
        assert_eq!(over_mcp["isError"], true);
        assert_eq!(
            over_mcp["structuredContent"]["error"]["code"],
            over_http["error"]["code"]
        );
        assert_eq!(
            over_mcp["structuredContent"]["error"]["details"],
            over_http["error"]["details"]
        );
    }
}
