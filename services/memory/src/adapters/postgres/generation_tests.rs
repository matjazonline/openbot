//! DB-backed `memory_generation_` cases (04B, D9, D13). Require MEMORY_TEST_DATABASE_URL.
//! Every provider call goes to the counted local provider, so each case can prove how many
//! (paid) calls a sequence of claims, retries, crashes, cancels and recoveries made.
use super::test_support::{TestDb, test_db};
use crate::application::contract::GenerationState;
use crate::{
    application::{
        generation_worker::{GenerationSettings, GenerationWorker, Maintenance, Pass},
        indexer::{Indexer, IndexerSettings},
        operations::{Surface, by_id},
        ports::{
            GenerationProvider, ProviderError, ProviderResponse, ProviderUsage, Settled,
            Settlement, StopReason,
        },
        service::{App, Principal},
        test_provider::{CountedProvider, Step, answer},
    },
    domain::{
        credential::{self, TokenKind},
        error::ErrorCode,
    },
};
use chrono::{Duration as Days, Utc};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

const USEFUL: &str = r#"{"outcome":"save","observations":[
    {"kind":"decision","title":"PostgreSQL chosen","body":"The team chose PostgreSQL.","event_ids":["e1","e2"]},
    {"kind":"fix","title":"Timeout raised","body":"The login timeout was raised.","event_ids":["e2"]}],"summary":null}"#;

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

/// One deadline for the whole run: `expires_at` is part of a request's identity, so a retry
/// must resend the same value.
fn later() -> String {
    static LATER: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    LATER
        .get_or_init(|| (Utc::now() + Days::hours(2)).to_rfc3339())
        .clone()
}

fn event(id: &str, revision: &str, level: i64, text: &str) -> Value {
    json!({"event_id": id, "source_ref": format!("chat:{id}"), "revision": revision,
           "occurred_at": "2026-11-08T10:00:00Z", "access_level": level, "role": "user",
           "text": text, "exclude": false})
}

fn window() -> Vec<Value> {
    vec![
        event("e1", "1", 2, "Should we use PostgreSQL?"),
        event(
            "e2",
            "1",
            4,
            "Yes. password=hunter2 and the login timeout is raised.",
        ),
    ]
}

fn request(key: &str, tag: &str, events: Vec<Value>) -> Value {
    json!({"idempotency_key": key, "expires_at": later(), "namespace": "ns", "session_id": null,
           "producing_ceiling": 6, "consumed": {"kind": "unknown"}, "description": tag,
           "events": events, "allowance_tokens": 100_000, "max_output_tokens": 1024,
           "summary": false})
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
    async fn run_detailed(
        &self,
        principal: &Principal,
        id: &str,
        input: Value,
    ) -> Result<Value, (ErrorCode, Value)> {
        self.app
            .execute(
                by_id(id).unwrap(),
                principal,
                input,
                Utc::now(),
                Uuid::now_v7(),
            )
            .await
            .map_err(|error| (error.code, error.details.unwrap_or_default()))
    }
    /// A tenant with namespace `ns`, a daily cap and a client allowed everything up to 9.
    async fn tenant(&self, tenant: &str, cap: u64) -> (String, Principal) {
        let op = &self.operator;
        self.run(op, "admin.tenants.create", json!({"external_id": tenant}))
            .await
            .unwrap();
        self.run(
            op,
            "admin.namespaces.create",
            json!({"tenant": tenant, "namespace": "ns"}),
        )
        .await
        .unwrap();
        self.run(
            op,
            "admin.tenants.settings",
            json!({"tenant": tenant, "generation_daily_token_cap": cap}),
        )
        .await
        .unwrap();
        let issued = self
            .run(
                op,
                "admin.credentials.issue",
                json!({"tenant": tenant, "label": "capture", "expires_at": null, "grants": [
                    {"namespace": "ns", "read_max": 9, "write_max": 9,
                     "operations": ["generate", "generation_status", "save", "fetch", "search",
                                    "sources"]}]}),
            )
            .await
            .unwrap();
        let token = issued["token"].as_str().unwrap().to_owned();
        let client = self
            .app
            .authenticate(Some(&token), Surface::Client, Utc::now())
            .await
            .unwrap();
        (token, client)
    }
    fn worker(&self) -> GenerationWorker {
        let provider: Arc<dyn GenerationProvider> = self.provider.clone();
        GenerationWorker::new(self.app.store().clone(), provider, settings())
    }
    async fn pass(&self) -> Pass {
        self.worker()
            .pass(&tokio_util::sync::CancellationToken::new())
            .await
            .unwrap()
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
    async fn state(&self, op: &str) -> (String, Option<String>, i64, i64) {
        sqlx::query_as(
            "SELECT state, outcome_code, reserved_tokens, used_tokens \
             FROM generation_operations WHERE id = $1::uuid",
        )
        .bind(op)
        .fetch_one(self.db.pool())
        .await
        .unwrap()
    }
    async fn spend(&self, tenant: &str) -> (i64, i64) {
        sqlx::query_as(
            "SELECT coalesce(sum(d.reserved), 0)::bigint, coalesce(sum(d.used), 0)::bigint \
               FROM tenant_spend_days d JOIN tenants t ON t.id = d.tenant_id \
              WHERE t.external_id = $1",
        )
        .bind(tenant)
        .fetch_one(self.db.pool())
        .await
        .unwrap()
    }
    async fn status(&self, client: &Principal, op: &str, ceiling: i64) -> Result<Value, ErrorCode> {
        self.run(
            client,
            "generations.status",
            json!({"namespace": "ns", "operation_id": op, "read_ceiling": ceiling}),
        )
        .await
    }
}

fn op_id(accepted: &Value) -> String {
    accepted["operation_id"].as_str().unwrap().to_owned()
}

/// Acceptance stores the operation, its immutable request and its window as source revisions
/// in one transaction (04B.1): privacy before capture and before the identity hash, conflicts
/// against stored revisions, retention-nulled evidence refused, privacy-only windows finished
/// without a call, the daily cap as the off switch, and duplicate requests giving one operation.
#[tokio::test]
async fn memory_generation_acceptance_is_atomic_private_and_idempotent() {
    let f = fixture(CountedProvider::new(vec![])).await;
    let (_, client) = f.tenant("t", 0).await;
    // Cap 0 (the default) keeps generation off; so does a service without a provider.
    let refused = f
        .run_detailed(&client, "generations.request", request("k0", "t", window()))
        .await
        .unwrap_err();
    assert_eq!(
        refused,
        (
            ErrorCode::GenerationDisabled,
            json!({"reason": "daily_cap_zero"})
        )
    );
    let bare = App::new(
        f.app.store().clone(),
        crate::application::test_embedder::fake_pool(),
    );
    let refused = bare
        .execute(
            by_id("generations.request").unwrap(),
            &client,
            request("k0", "t", window()),
            Utc::now(),
            Uuid::now_v7(),
        )
        .await
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::GenerationDisabled);
    f.run(
        &f.operator,
        "admin.tenants.settings",
        json!({"tenant": "t", "generation_daily_token_cap": 1_000_000}),
    )
    .await
    .unwrap();
    // An excluded event is never stored; secrets are redacted before capture.
    let mut events = window();
    events.push(
        json!({"event_id": "e3", "source_ref": "chat:e3", "revision": "1",
                       "occurred_at": "2026-11-08T10:01:00Z", "access_level": 6,
                       "role": "user", "text": "excluded secret plan", "exclude": true}),
    );
    let accepted = f
        .run(
            &client,
            "generations.request",
            request("k1", "t", events.clone()),
        )
        .await
        .unwrap();
    assert_eq!(
        (accepted["state"].clone(), accepted["replayed"].clone()),
        (json!("accepted"), json!(false))
    );
    let op = op_id(&accepted);
    assert_eq!(f.scalar("SELECT count(*) FROM sources").await, 2);
    assert_eq!(
        f.scalar("SELECT count(*) FROM sources WHERE content LIKE '%hunter2%' OR content LIKE '%excluded%'")
            .await,
        0
    );
    assert_eq!(
        f.scalar("SELECT count(*) FROM generation_operation_sources")
            .await,
        2
    );
    assert_eq!(
        f.scalar(
            "SELECT count(*) FROM generation_operations WHERE input_level = 6 \
               AND watermark_kind = 'ceiling_fallback' AND source_max_level = 4 \
               AND jsonb_array_length(request->'events') = 2 AND reservation > 0"
        )
        .await,
        1
    );
    // A retry differing only in removed text replays; other visible text conflicts.
    let mut retry = events.clone();
    retry[1]["text"] = json!("Yes. password=other and the login timeout is raised.");
    retry[2]["text"] = json!("a different excluded text");
    let replay = f
        .run(&client, "generations.request", request("k1", "t", retry))
        .await
        .unwrap();
    assert_eq!(
        (op_id(&replay), replay["replayed"].clone()),
        (op.clone(), json!(true))
    );
    let mut other = window();
    other[0]["text"] = json!("Should we use MySQL?");
    assert_eq!(
        f.run(
            &client,
            "generations.request",
            request("k1", "t", other.clone())
        )
        .await
        .unwrap_err(),
        ErrorCode::IdempotencyConflict
    );
    // The same revision with other content conflicts with the stored one; nothing is kept.
    assert_eq!(
        f.run(&client, "generations.request", request("k2", "t", other))
            .await
            .unwrap_err(),
        ErrorCode::Conflict
    );
    assert_eq!(
        f.scalar("SELECT count(*) FROM generation_operations").await,
        1
    );
    assert_eq!(f.scalar("SELECT count(*) FROM sources").await, 2);
    // Evidence retention already nulled refuses a window naming it.
    f.exec(
        "UPDATE sources SET content = NULL, content_nulled_at = now() WHERE external_ref = 'chat:e1'",
    )
    .await;
    let refused = f
        .run_detailed(&client, "generations.request", request("k3", "t", window()))
        .await
        .unwrap_err();
    assert_eq!(
        refused,
        (ErrorCode::Conflict, json!({"reason": "source_unavailable"}))
    );
    // Privacy-only input finishes as a skip without a provider call.
    let private = f
        .run(
            &client,
            "generations.request",
            request(
                "k4",
                "t",
                vec![event("p1", "1", 1, "<private>only private</private>")],
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        (private["state"].clone(), private["outcome"].clone()),
        (json!("completed"), json!("nothing_to_process"))
    );
    // Competing identical requests give one operation.
    let tasks: Vec<_> = (0..8)
        .map(|_| {
            let app = f.app.clone();
            let client = client.clone();
            let body = request("k5", "t", vec![event("c1", "1", 1, "Concurrent window.")]);
            tokio::spawn(async move {
                app.execute(
                    by_id("generations.request").unwrap(),
                    &client,
                    body,
                    Utc::now(),
                    Uuid::now_v7(),
                )
                .await
                .map(|value| op_id(&value))
            })
        })
        .collect();
    let mut ids = std::collections::BTreeSet::new();
    for task in tasks {
        ids.insert(task.await.unwrap().unwrap());
    }
    assert_eq!(ids.len(), 1);
    assert_eq!(
        f.scalar("SELECT count(*) FROM generation_operations WHERE idempotency_key = 'k5'")
            .await,
        1
    );
    assert_eq!(f.provider.calls(), 0, "acceptance never calls the provider");
    // Status needs the read ceiling of the operation's level.
    assert_eq!(
        f.status(&client, &op, 5).await.unwrap_err(),
        ErrorCode::NotFound
    );
    assert_eq!(
        f.status(&client, &op, 6).await.unwrap()["state"],
        "accepted"
    );
}

/// A worker commits the generated observations, their links, the spend settlement and
/// completion in one transaction, labelled with the frozen evidence; they enter indexing.
/// Nothing calls the provider again afterwards, and explicit-save delivery retries never do.
#[tokio::test]
async fn memory_generation_worker_saves_labelled_output_once_and_indexes_it() {
    let f = fixture(CountedProvider::new(vec![answer(USEFUL)])).await;
    let (_, client) = f.tenant("t", 1_000_000).await;
    let op = op_id(
        &f.run(&client, "generations.request", request("k1", "t", window()))
            .await
            .unwrap(),
    );
    assert_eq!(f.pass().await, Pass::Settled(GenerationState::Completed));
    assert_eq!(f.provider.calls(), 1);
    let sent = &f.provider.requests()[0].user;
    assert!(sent.contains("[REDACTED:secret]") && !sent.contains("hunter2"));
    let (state, outcome, reserved, used) = f.state(&op).await;
    assert_eq!(
        (state.as_str(), outcome.as_deref(), reserved, used),
        ("completed", Some("saved"), 0, 1200)
    );
    assert_eq!(f.spend("t").await, (0, 1200));
    assert_eq!(
        f.scalar(
            "SELECT count(*) FROM observations WHERE origin = 'generated' AND access_level = 6 \
               AND watermark_kind = 'ceiling_fallback' AND source_max_level = 4 \
               AND index_state = 'pending' AND output_ordinal IN (0, 1)"
        )
        .await,
        2
    );
    assert_eq!(f.scalar("SELECT count(*) FROM observation_links").await, 3);
    let status = f.status(&client, &op, 6).await.unwrap();
    assert_eq!(status["outcome"], "saved");
    assert_eq!(status["observations"].as_array().unwrap().len(), 2);
    // The generated observations enter the indexing pipeline.
    let indexer = Indexer::new(
        f.app.store().clone(),
        f.app.embeddings().clone(),
        IndexerSettings::default(),
    );
    assert_eq!(indexer.run_once().await.unwrap().indexed, 2);
    // No further inference: more passes, maintenance and explicit-save retries are idle.
    let save = json!({"idempotency_key": "save-1", "expires_at": later(), "namespace": "ns",
        "session_id": null, "context": {"producing_ceiling": 6, "consumed": {"kind": "unknown"},
        "sources": [], "external_sources": [], "expected_access_level": null},
        "items": [{"kind": "note", "title": "n", "body": "b", "occurred_at": "2026-11-08T10:00:00Z",
                   "expires_at": null}]});
    for _ in 0..3 {
        f.run(&client, "observations.save", save.clone())
            .await
            .unwrap();
        assert_eq!(f.pass().await, Pass::Idle);
    }
    Maintenance::new(f.app.store().clone(), settings())
        .tick()
        .await
        .unwrap();
    assert_eq!(f.provider.calls(), 1);
}

/// Simultaneous claimants: at most one operation in flight per tenant, claims spread across
/// tenants, and sequential claims rotate between tenants (round-robin fairness).
#[tokio::test]
async fn memory_generation_competing_claimants_keep_one_in_flight_per_tenant_fairly() {
    let f = fixture(CountedProvider::new(vec![])).await;
    let mut tenants = Vec::new();
    for tenant in ["a", "b"] {
        let (_, client) = f.tenant(tenant, 1_000_000).await;
        for i in 0..3 {
            f.run(
                &client,
                "generations.request",
                request(
                    &format!("{tenant}{i}"),
                    tenant,
                    vec![event(&format!("{tenant}{i}"), "1", 1, "window text")],
                ),
            )
            .await
            .unwrap();
        }
        tenants.push(client);
    }
    let store = f.app.store().clone();
    let claims: Vec<_> = (0..8)
        .map(|_| {
            let store = store.clone();
            tokio::spawn(async move {
                store
                    .claim_generation(Uuid::now_v7(), Duration::from_secs(30))
                    .await
                    .unwrap()
            })
        })
        .collect();
    let mut jobs = Vec::new();
    for claim in claims {
        jobs.extend(claim.await.unwrap());
    }
    let tenants_claimed: std::collections::BTreeSet<_> =
        jobs.iter().map(|job| job.tenant_id).collect();
    assert_eq!((jobs.len(), tenants_claimed.len()), (2, 2), "{jobs:?}");
    // Each claim reserved its spend; giving them back releases it.
    for job in &jobs {
        assert_eq!(
            store
                .settle_generation(job, &Settlement::Release)
                .await
                .unwrap(),
            Settled::Done(GenerationState::Accepted)
        );
    }
    assert_eq!(f.spend("a").await.0 + f.spend("b").await.0, 0);
    // Sequential claims alternate tenants.
    let mut order = Vec::new();
    for _ in 0..4 {
        let job = store
            .claim_generation(Uuid::now_v7(), Duration::from_secs(30))
            .await
            .unwrap()
            .unwrap();
        order.push(job.tenant_id);
        store
            .settle_generation(&job, &Settlement::Release)
            .await
            .unwrap();
    }
    assert!(
        order.windows(2).all(|pair| pair[0] != pair[1]),
        "round-robin: {order:?}"
    );
}

/// Lease loss cancels the real provider call: a cancel requested while dispatched fails the
/// heartbeat, the call is dropped (aborted) and the operation is cancelled with its cost
/// charged. A stale worker can neither dispatch nor commit after the reaper or another
/// claimant moved the fence.
#[tokio::test]
async fn memory_generation_lease_loss_aborts_the_call_and_stale_workers_cannot_commit() {
    let f = fixture(CountedProvider::steps(vec![
        Step::Hang,
        Step::Answer(answer(USEFUL)),
    ]))
    .await;
    let (_, client) = f.tenant("t", 1_000_000).await;
    let op = op_id(
        &f.run(&client, "generations.request", request("k1", "t", window()))
            .await
            .unwrap(),
    );
    let worker = f.worker();
    let running = tokio::spawn(async move {
        worker
            .pass(&tokio_util::sync::CancellationToken::new())
            .await
            .unwrap()
    });
    while f.provider.calls() == 0 {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let cancelled = f
        .run(
            &client,
            "generations.cancel",
            json!({"idempotency_key": "c1", "expires_at": later(), "namespace": "ns",
                   "operation_id": op}),
        )
        .await
        .unwrap();
    assert_eq!(
        cancelled["state"], "dispatched",
        "a dispatched call is flagged"
    );
    let pass = tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .expect("the heartbeat aborts the call")
        .unwrap();
    assert_eq!(
        pass,
        Pass::Abandoned("lease_lost", Settled::Done(GenerationState::Cancelled))
    );
    assert_eq!(f.provider.dropped(), 1, "the provider call was aborted");
    let (state, outcome, reserved, used) = f.state(&op).await;
    let reservation = f
        .scalar("SELECT reservation FROM generation_operations")
        .await;
    assert_eq!(
        (state.as_str(), outcome.as_deref(), reserved, used),
        ("cancelled", Some("client_cancelled"), 0, reservation)
    );
    // Stale dispatch: a claim whose lease expired is reaped back and claimed again (fence + 1).
    let second = op_id(
        &f.run(
            &client,
            "generations.request",
            request("k2", "t", vec![event("e9", "1", 1, "x")]),
        )
        .await
        .unwrap(),
    );
    let store = f.app.store().clone();
    let old = store
        .claim_generation(Uuid::now_v7(), Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    f.exec("UPDATE generation_operations SET lease_until = now() - interval '1 second' WHERE state = 'claimed'")
        .await;
    let maintenance = Maintenance::new(store.clone(), settings());
    assert_eq!(maintenance.tick().await.unwrap().reaped.released, 1);
    let new = store
        .claim_generation(Uuid::now_v7(), Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(new.fence, old.fence + 1);
    assert!(matches!(
        store
            .dispatch_generation(&old, Duration::from_secs(30))
            .await
            .unwrap(),
        crate::application::ports::Dispatch::Stale
    ));
    // Stale commit: the reaper settles a dispatched operation past its lease as unknown, and
    // the late worker's output is refused.
    assert!(matches!(
        store
            .dispatch_generation(&new, Duration::from_secs(30))
            .await
            .unwrap(),
        crate::application::ports::Dispatch::Send(_)
    ));
    f.exec("UPDATE generation_operations SET lease_until = now() - interval '1 second' WHERE state = 'dispatched'")
        .await;
    assert_eq!(maintenance.tick().await.unwrap().reaped.abandoned, 1);
    let late = Settlement::Saved(Vec::new(), ProviderUsage::default());
    assert_eq!(
        store.settle_generation(&new, &late).await.unwrap(),
        Settled::Stale
    );
    let (state, outcome, ..) = f.state(&second).await;
    assert_eq!(
        (state.as_str(), outcome.as_deref()),
        ("unknown", Some("lease_expired"))
    );
    assert_eq!(f.scalar("SELECT count(*) FROM observations").await, 0);
    assert_eq!(f.provider.calls(), 1);
}

fn envelope(kind: &str) -> Result<ProviderResponse, ProviderError> {
    Err(ProviderError::Rejected(kind.into()))
}

/// Ambiguity becomes an observable, charged `unknown` and is never re-sent; definite
/// rejections retry with backoff up to three dispatches; terminal provider errors (quota,
/// authentication) stop the operation, and nothing revives it: a recovered provider,
/// disable/enable, maintenance or later explicit saves make no further call. Only a new
/// request with new authorization runs again.
#[tokio::test]
async fn memory_generation_ambiguity_retries_and_terminal_errors_never_resend() {
    let f = fixture(
        CountedProvider::steps(vec![
            Step::Answer(Err(ProviderError::Ambiguous)),
            Step::Answer(envelope("overloaded_error")),
            Step::Answer(envelope("rate_limit_error")),
            Step::Answer(answer(USEFUL)),
            Step::Answer(Err(ProviderError::Terminal("invalid_request_error".into()))),
        ])
        .then_always(Step::Answer(answer(USEFUL))),
    )
    .await;
    let (_, client) = f.tenant("t", 10_000_000).await;
    let first = op_id(
        &f.run(&client, "generations.request", request("k1", "t", window()))
            .await
            .unwrap(),
    );
    assert_eq!(f.pass().await, Pass::Settled(GenerationState::Unknown));
    let reservation = f
        .scalar("SELECT reservation FROM generation_operations")
        .await;
    assert_eq!(
        f.state(&first).await,
        (
            "unknown".into(),
            Some("provider_unknown".into()),
            0,
            reservation
        )
    );
    // Two definite rejections, then success: three dispatches of one logical operation.
    let second = op_id(
        &f.run(
            &client,
            "generations.request",
            request("k2", "t", vec![event("r1", "1", 1, "x")]),
        )
        .await
        .unwrap(),
    );
    for _ in 0..2 {
        assert_eq!(f.pass().await, Pass::Settled(GenerationState::Accepted));
        assert_eq!(
            f.scalar("SELECT count(*) FROM generation_operations WHERE next_attempt_at > now() + interval '20 seconds'")
                .await,
            1,
            "backs off"
        );
        assert_eq!(f.pass().await, Pass::Idle, "not due yet");
        f.exec("UPDATE generation_operations SET next_attempt_at = now() WHERE state = 'accepted'")
            .await;
    }
    assert_eq!(
        f.spend("t").await.0,
        0,
        "rejections release the reservation"
    );
    // The model cited e1/e2, which are not in this window: malformed, a terminal failure.
    assert_eq!(f.pass().await, Pass::Settled(GenerationState::Failed));
    assert_eq!(
        f.state(&second).await.1.as_deref(),
        Some("malformed_output")
    );
    assert_eq!(
        f.scalar("SELECT attempts::bigint FROM generation_operations WHERE state = 'failed'")
            .await,
        3
    );
    // A terminal provider error stops the operation. (A rejection of the service's credentials
    // pauses instead: `memory_operations_auth_breaker_…`.)
    let third = op_id(
        &f.run(&client, "generations.request", request("k3", "t", window()))
            .await
            .unwrap(),
    );
    assert_eq!(f.pass().await, Pass::Settled(GenerationState::Failed));
    assert_eq!(
        f.state(&third).await.1.as_deref(),
        Some("invalid_request_error")
    );
    let calls = f.provider.calls();
    assert_eq!(calls, 5);
    // The provider recovered: nothing old is revived.
    f.run(&f.operator, "admin.tenants.disable", json!({"tenant": "t"}))
        .await
        .unwrap();
    f.run(&f.operator, "admin.tenants.enable", json!({"tenant": "t"}))
        .await
        .unwrap();
    let maintenance = Maintenance::new(f.app.store().clone(), settings());
    for _ in 0..3 {
        maintenance.tick().await.unwrap();
        assert_eq!(f.pass().await, Pass::Idle);
    }
    assert_eq!(
        f.provider.calls(),
        calls,
        "no unrequested inference resumes"
    );
    assert!(
        f.exec_err("UPDATE generation_operations SET state = 'accepted', finished_at = NULL WHERE state = 'failed'")
            .await
            .contains("never revived")
    );
    // Explicit recovery is a new, authorized request.
    f.run(&client, "generations.request", request("k4", "t", window()))
        .await
        .unwrap();
    assert_eq!(f.pass().await, Pass::Settled(GenerationState::Completed));
    assert_eq!(f.provider.calls(), calls + 1);
}

impl Fixture {
    async fn exec_err(&self, sql: &str) -> String {
        sqlx::query(sql)
            .execute(self.db.pool())
            .await
            .unwrap_err()
            .to_string()
    }
}

/// Spend: the reservation is taken at claim against the per-tenant daily cap; a day that
/// cannot hold it fails the operation terminally (raising the cap later does not revive it).
/// Usage above the reservation is recorded and counted by the early-failing metric.
#[tokio::test]
async fn memory_generation_spend_cap_is_terminal_and_overshoot_is_counted() {
    let big = Ok(ProviderResponse {
        stop: StopReason::EndTurn,
        text: USEFUL.into(),
        usage: ProviderUsage {
            input_tokens: 50_000,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
            output_tokens: 70_000,
        },
    });
    let f = fixture(CountedProvider::new(vec![big])).await;
    let (_, client) = f.tenant("t", 125_000).await;
    let first = op_id(
        &f.run(&client, "generations.request", request("k1", "t", window()))
            .await
            .unwrap(),
    );
    let before = super::usage_exceeded_reservation_total();
    assert_eq!(f.pass().await, Pass::Settled(GenerationState::Completed));
    assert!(super::usage_exceeded_reservation_total() > before);
    assert_eq!(f.state(&first).await.3, 120_000, "actual usage is recorded");
    assert_eq!(f.spend("t").await, (0, 120_000));
    // The day now cannot hold another reservation: failed, terminally.
    let second = op_id(
        &f.run(&client, "generations.request", request("k2", "t", window()))
            .await
            .unwrap(),
    );
    assert_eq!(f.pass().await, Pass::Idle);
    assert_eq!(
        f.state(&second).await,
        ("failed".into(), Some("spend_cap_reached".into()), 0, 0)
    );
    f.run(
        &f.operator,
        "admin.tenants.settings",
        json!({"tenant": "t", "generation_daily_token_cap": 10_000_000}),
    )
    .await
    .unwrap();
    assert_eq!(f.pass().await, Pass::Idle);
    assert_eq!(f.state(&second).await.0, "failed");
    assert_eq!(f.provider.calls(), 1);
}

/// Revocation, credential expiry, request expiry and tenant deletion stop operations at
/// claim, dispatch and commit; a deletion racing competing workers leaves nothing live and
/// no observation committed after it.
#[tokio::test]
async fn memory_generation_revocation_expiry_and_deletion_stop_operations() {
    let f = fixture(
        CountedProvider::new(vec![])
            .then_always(Step::After(Duration::from_millis(50), answer(USEFUL))),
    )
    .await;
    let (token, client) = f.tenant("t", 100_000_000).await;
    // Credential expiry is seen at claim: cancelled, nothing sent.
    let op = op_id(
        &f.run(&client, "generations.request", request("k1", "t", window()))
            .await
            .unwrap(),
    );
    f.exec("UPDATE credentials SET expires_at = now() - interval '1 second'")
        .await;
    assert_eq!(f.pass().await, Pass::Idle);
    assert_eq!(f.state(&op).await.1.as_deref(), Some("credential_expired"));
    f.exec("UPDATE credentials SET expires_at = NULL").await;
    // Request expiry: pending past its deadline is expired by the reaper.
    let mut short = request("k2", "t", window());
    short["expires_at"] = json!((Utc::now() + Days::milliseconds(300)).to_rfc3339());
    let expiring = op_id(&f.run(&client, "generations.request", short).await.unwrap());
    tokio::time::sleep(Duration::from_millis(400)).await;
    let maintenance = Maintenance::new(f.app.store().clone(), settings());
    assert_eq!(maintenance.tick().await.unwrap().reaped.expired, 1);
    assert_eq!(f.state(&expiring).await.0, "expired");
    // Revocation cancels queued work atomically.
    let queued = op_id(
        &f.run(&client, "generations.request", request("k3", "t", window()))
            .await
            .unwrap(),
    );
    let id = credential::parse(&token).unwrap().id;
    f.run(
        &f.operator,
        "admin.credentials.revoke",
        json!({"tenant": "t", "credential_id": id}),
    )
    .await
    .unwrap();
    assert_eq!(
        f.state(&queued).await.1.as_deref(),
        Some("credential_revoked")
    );
    assert_eq!(f.provider.calls(), 0);
    // Deletion racing competing workers.
    let (_, client) = f.tenant("d", 100_000_000).await;
    for i in 0..12 {
        f.run(
            &client,
            "generations.request",
            request(
                &format!("d{i}"),
                "d",
                vec![event(&format!("e{i}"), "1", 1, "x")],
            ),
        )
        .await
        .unwrap();
    }
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let worker = f.worker();
            tokio::spawn(async move {
                let shutdown = tokio_util::sync::CancellationToken::new();
                for _ in 0..6 {
                    let _ = worker.pass(&shutdown).await;
                }
            })
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(120)).await;
    f.run(&f.operator, "admin.tenants.delete", json!({"tenant": "d"}))
        .await
        .unwrap();
    for worker in workers {
        worker.await.unwrap();
    }
    maintenance.tick().await.unwrap();
    assert_eq!(
        f.scalar(
            "SELECT count(*) FROM generation_operations o JOIN tenants t ON t.id = o.tenant_id \
              WHERE t.external_id = 'd' AND o.finished_at IS NULL AND o.cancel_requested_at IS NULL"
        )
        .await,
        0,
        "nothing live survives the deletion"
    );
    assert_eq!(
        f.scalar(
            "SELECT count(*) FROM observations ob JOIN generation_operations o \
               ON o.id = ob.generation_operation_id \
              JOIN tenants t ON t.id = o.tenant_id \
             WHERE t.external_id = 'd' AND ob.created_at > t.state_changed_at"
        )
        .await,
        0,
        "no output committed after the deletion"
    );
    assert!(
        f.provider.max_in_flight("d") <= 1,
        "one call in flight per tenant"
    );
}

/// Restart: a worker that dies after dispatch leaves the call to the reaper (`unknown`, never
/// re-sent); one that dies after claiming but before dispatch is reclaimed and sent once.
/// A changed classification cannot reach a retried operation: its label is immutable.
#[tokio::test]
async fn memory_generation_restart_never_resends_and_labels_stay_frozen() {
    let f = fixture(
        CountedProvider::steps(vec![Step::Hang, Step::Answer(envelope("overloaded_error"))])
            .then_always(Step::Answer(answer(USEFUL))),
    )
    .await;
    let (_, client) = f.tenant("t", 100_000_000).await;
    let crashed = op_id(
        &f.run(&client, "generations.request", request("k1", "t", window()))
            .await
            .unwrap(),
    );
    let worker = f.worker();
    let task = tokio::spawn(async move {
        worker
            .pass(&tokio_util::sync::CancellationToken::new())
            .await
    });
    while f.provider.calls() == 0 {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    task.abort();
    let _ = task.await;
    assert_eq!(f.state(&crashed).await.0, "dispatched");
    assert_eq!(
        f.pass().await,
        Pass::Idle,
        "the tenant's slot is still held"
    );
    tokio::time::sleep(settings().lease).await;
    let maintenance = Maintenance::new(f.app.store().clone(), settings());
    assert_eq!(maintenance.tick().await.unwrap().reaped.abandoned, 1);
    assert_eq!(f.state(&crashed).await.1.as_deref(), Some("lease_expired"));
    // A retried operation keeps its frozen label even if the client now claims less.
    let retried = op_id(
        &f.run(&client, "generations.request", request("k2", "t", window()))
            .await
            .unwrap(),
    );
    assert_eq!(f.pass().await, Pass::Settled(GenerationState::Accepted));
    assert!(
        f.exec_err("UPDATE generation_operations SET input_level = 4 WHERE state = 'accepted'")
            .await
            .contains("immutable")
    );
    let mut lower = request("k2", "t", window());
    lower["consumed"] = json!({"kind": "exact", "level": 4});
    assert_eq!(
        f.run(&client, "generations.request", lower)
            .await
            .unwrap_err(),
        ErrorCode::IdempotencyConflict
    );
    f.exec("UPDATE generation_operations SET next_attempt_at = now() WHERE state = 'accepted'")
        .await;
    // A worker that dies between claim and dispatch: reclaimed and sent exactly once.
    let store = f.app.store().clone();
    store
        .claim_generation(Uuid::now_v7(), Duration::from_millis(100))
        .await
        .unwrap()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(maintenance.tick().await.unwrap().reaped.released, 1);
    assert_eq!(f.pass().await, Pass::Settled(GenerationState::Completed));
    assert_eq!(f.provider.calls(), 3);
    assert_eq!(
        f.scalar(&format!(
            "SELECT count(*) FROM observations WHERE generation_operation_id = '{retried}' \
               AND access_level = 6"
        ))
        .await,
        2
    );
}

impl Fixture {
    /// `count` recorded revisions `old:{i}` recorded 31 days ago in tenant `tenant`'s `ns`.
    async fn old_sources(&self, tenant: &str, prefix: &str, count: i64) {
        sqlx::query(
            "INSERT INTO sources (tenant_id, namespace_id, id, external_ref, revision, access_level, \
             content, content_sha256, occurred_at, recorded_at) \
             SELECT t.id, n.id, gen_random_uuid(), $2 || i, '1', 1, 'Old evidence.', \
                    sha256(convert_to('Old evidence.', 'UTF8')), now(), now() - interval '31 days' \
               FROM tenants t JOIN namespaces n ON n.tenant_id = t.id, generate_series(1, $3) AS i \
              WHERE t.external_id = $1",
        )
        .bind(tenant)
        .bind(prefix)
        .bind(count)
        .execute(self.db.pool())
        .await
        .unwrap();
    }
}

fn old_event(reference: &str) -> Value {
    json!({"event_id": reference, "source_ref": reference, "revision": "1",
           "occurred_at": "2026-11-08T10:00:00Z", "access_level": 1, "role": "note",
           "text": "Old evidence.", "exclude": false})
}

/// Retention (06A.1 for generation inputs and raw evidence): bounded batches, content a live
/// operation references is never nulled, requests go 30 days after their operation finished,
/// and a poisoned row makes the maintenance task back off instead of hot-spinning.
#[tokio::test]
async fn memory_generation_retention_is_bounded_spares_live_inputs_and_backs_off() {
    let f = fixture(CountedProvider::new(vec![])).await;
    let (_, client) = f.tenant("t", 100_000_000).await;
    f.old_sources("t", "old:", 600).await;
    // A live operation reuses one old revision (identical text, level and session).
    let live = op_id(
        &f.run(
            &client,
            "generations.request",
            request("k1", "t", vec![old_event("old:1")]),
        )
        .await
        .unwrap(),
    );
    let store = f.app.store().clone();
    let first = store.sweep_retention(500).await.unwrap();
    let second = store.sweep_retention(500).await.unwrap();
    assert_eq!((first.sources, second.sources), (500, 99));
    assert_eq!(
        f.scalar("SELECT count(*) FROM sources WHERE content IS NOT NULL")
            .await,
        1,
        "only the live operation's evidence is kept"
    );
    // Finished recently: still kept (30 days after the operation finished).
    f.run(
        &client,
        "generations.cancel",
        json!({"idempotency_key": "c1", "expires_at": later(), "namespace": "ns",
               "operation_id": live}),
    )
    .await
    .unwrap();
    assert_eq!(store.sweep_retention(500).await.unwrap().sources, 0);
    // 31 days after it finished, its request and its evidence go.
    f.exec("ALTER TABLE generation_operations DISABLE TRIGGER generation_requests_immutable")
        .await;
    f.exec("UPDATE generation_operations SET finished_at = now() - interval '31 days'")
        .await;
    f.exec("ALTER TABLE generation_operations ENABLE TRIGGER generation_requests_immutable")
        .await;
    let swept = store.sweep_retention(500).await.unwrap();
    assert_eq!((swept.requests, swept.sources), (1, 1));
    assert_eq!(
        f.scalar("SELECT count(*) FROM generation_operations WHERE request IS NULL AND inputs_nulled_at IS NOT NULL")
            .await,
        1
    );
    // Identity, level and digest stay for provenance.
    assert_eq!(
        f.scalar(
            "SELECT count(*) FROM sources WHERE content IS NULL AND content_sha256 IS NOT NULL"
        )
        .await,
        600
    );
    // A poisoned row fails every sweep: the task backs off (1 s, doubling) instead of
    // re-running every 100 ms.
    f.old_sources("t", "poison:", 1).await;
    f.exec(
        "CREATE FUNCTION poison() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
           IF OLD.external_ref LIKE 'poison:%' THEN RAISE EXCEPTION 'poisoned row'; END IF; \
           RETURN NEW; END $$",
    )
    .await;
    f.exec("CREATE TRIGGER poison BEFORE UPDATE ON sources FOR EACH ROW EXECUTE FUNCTION poison()")
        .await;
    let maintenance = Maintenance::new(store.clone(), settings());
    let shutdown = tokio_util::sync::CancellationToken::new();
    let running = tokio::spawn({
        let maintenance = maintenance.clone();
        let shutdown = shutdown.clone();
        async move { maintenance.run(shutdown).await }
    });
    tokio::time::sleep(Duration::from_millis(1_600)).await;
    shutdown.cancel();
    running.await.unwrap();
    assert!(
        (1..=3).contains(&maintenance.ticks()),
        "backed off: {} ticks",
        maintenance.ticks()
    );
}

/// D9 n3: retention racing acceptance and dispatch. Every acceptance either reuses evidence
/// that is still there (and the sweeper then never nulls it while the operation lives) or is
/// refused `source_unavailable`; no accepted window is ever sent incomplete.
#[tokio::test]
async fn memory_generation_retention_races_acceptance_and_dispatch_safely() {
    let nothing = r#"{"outcome":"nothing_to_save","observations":[],"summary":null}"#;
    let f = fixture(CountedProvider::new(vec![]).then_always(Step::Answer(answer(nothing)))).await;
    let (_, client) = f.tenant("t", 100_000_000).await;
    f.old_sources("t", "race:", 40).await;
    let store = f.app.store().clone();
    let sweeper = tokio::spawn({
        let store = store.clone();
        async move {
            for _ in 0..40 {
                store.sweep_retention(3).await.unwrap();
                tokio::task::yield_now().await;
            }
        }
    });
    let acceptances: Vec<_> = (1..=40)
        .map(|i| {
            let app = f.app.clone();
            let client = client.clone();
            let body = request(&format!("r{i}"), "t", vec![old_event(&format!("race:{i}"))]);
            tokio::spawn(async move {
                app.execute(
                    by_id("generations.request").unwrap(),
                    &client,
                    body,
                    Utc::now(),
                    Uuid::now_v7(),
                )
                .await
                .map_err(|error| (error.code, error.details))
            })
        })
        .collect();
    let (mut accepted, mut refused) = (0, 0);
    for task in acceptances {
        match task.await.unwrap() {
            Ok(_) => accepted += 1,
            Err((ErrorCode::Conflict, Some(details))) => {
                assert_eq!(details["reason"], "source_unavailable");
                refused += 1;
            }
            Err(other) => panic!("{other:?}"),
        }
    }
    sweeper.await.unwrap();
    assert_eq!(accepted + refused, 40);
    assert_eq!(
        f.scalar(
            "SELECT count(*) FROM generation_operation_sources g \
               JOIN generation_operations o ON o.id = g.operation_id \
               JOIN sources s ON s.id = g.source_id \
              WHERE o.finished_at IS NULL AND s.content IS NULL"
        )
        .await,
        0,
        "no live operation lost its evidence"
    );
    while f.pass().await != Pass::Idle {}
    assert_eq!(
        f.scalar(
            "SELECT count(*) FROM generation_operations WHERE outcome_code = 'source_unavailable'"
        )
        .await,
        0
    );
    assert_eq!(f.provider.calls(), accepted);
    assert!(
        f.provider
            .requests()
            .iter()
            .all(|request| request.user.contains("Old evidence."))
    );
}

/// Shutdown: a worker drops its in-flight call (the connection is aborted) and records the
/// operation as `unknown`, charged and never re-sent; a claim taken while stopping is given
/// back untouched.
#[tokio::test]
async fn memory_generation_shutdown_aborts_in_flight_calls_and_returns_claims() {
    let f = fixture(CountedProvider::steps(vec![
        Step::Hang,
        Step::After(Duration::from_millis(100), answer(USEFUL)),
    ]))
    .await;
    let (_, client) = f.tenant("t", 100_000_000).await;
    let op = op_id(
        &f.run(&client, "generations.request", request("k1", "t", window()))
            .await
            .unwrap(),
    );
    let shutdown = tokio_util::sync::CancellationToken::new();
    let worker = f.worker();
    let running = tokio::spawn({
        let shutdown = shutdown.clone();
        async move { worker.pass(&shutdown).await.unwrap() }
    });
    while f.provider.calls() == 0 {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    shutdown.cancel();
    assert_eq!(
        running.await.unwrap(),
        Pass::Abandoned("shutdown", Settled::Done(GenerationState::Unknown))
    );
    assert_eq!(f.provider.dropped(), 1);
    assert_eq!(f.state(&op).await.1.as_deref(), Some("shutdown"));
    f.run(&client, "generations.request", request("k2", "t", window()))
        .await
        .unwrap();
    assert_eq!(
        f.worker().pass(&shutdown).await.unwrap(),
        Pass::Abandoned("shutdown", Settled::Done(GenerationState::Accepted))
    );
    assert_eq!(
        f.spend("t").await.0,
        0,
        "the returned claim holds no reservation"
    );
    assert_eq!(f.provider.calls(), 1);
    // Review p6b-code-r1 n3: a call that answers within the drain (80 % of the grace) is
    // saved, not turned into an unknown.
    let shutdown = tokio_util::sync::CancellationToken::new();
    let worker = f.worker();
    let running = tokio::spawn({
        let shutdown = shutdown.clone();
        async move { worker.pass(&shutdown).await.unwrap() }
    });
    while f.provider.calls() == 1 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    shutdown.cancel();
    assert_eq!(
        running.await.unwrap(),
        Pass::Settled(GenerationState::Completed)
    );
    assert_eq!((f.provider.calls(), f.provider.dropped()), (2, 1));
}

fn tight(lease_ms: u64, heartbeat_ms: u64, margin_ms: u64) -> GenerationSettings {
    GenerationSettings {
        lease: Duration::from_millis(lease_ms),
        heartbeat: Duration::from_millis(heartbeat_ms),
        margin: Duration::from_millis(margin_ms),
        ..settings()
    }
}

/// Review p6b-code-r1/r2 M1 (D10 "a stalled heartbeat against a slow provider never yields two
/// in-flight calls per tenant"), with real competitors: a provider slower than the lease; the
/// worker's renewals fail (injected in the store, touching no row, so the failed-renewal branch
/// runs); a second worker with a second same-tenant operation and the reaper at grace 0 run
/// free on their own connections. The local deadline must drop the first call while the
/// database lease still stands, so the reaper frees the slot, and the second call starts, only
/// after that drop.
#[tokio::test]
async fn memory_generation_stalled_heartbeat_with_a_slow_provider_never_doubles_calls() {
    let f = fixture(CountedProvider::steps(vec![
        Step::After(Duration::from_secs(30), answer(USEFUL)),
        Step::Answer(answer(USEFUL)),
    ]))
    .await;
    let (_, client) = f.tenant("t", 100_000_000).await;
    let first = op_id(
        &f.run(&client, "generations.request", request("k1", "t", window()))
            .await
            .unwrap(),
    );
    let second = op_id(
        &f.run(&client, "generations.request", request("k2", "t", window()))
            .await
            .unwrap(),
    );
    // Its own fault flag: the competitors' store (a clone) renews normally.
    let faulty = super::PgStore {
        renew_fault: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        ..f.db.store.clone()
    };
    let provider: Arc<dyn GenerationProvider> = f.provider.clone();
    let stalled = GenerationWorker::new(Arc::new(faulty), provider, settings());
    let running = tokio::spawn(async move {
        stalled
            .pass(&tokio_util::sync::CancellationToken::new())
            .await
    });
    // The stalled worker dispatches first; then the competitors start, on their own
    // connections, free to act at the lease boundary.
    while f.provider.calls() == 0 {
        tokio::time::sleep(Duration::from_millis(2)).await;
    }

    let stop = tokio_util::sync::CancellationToken::new();
    let competitor = {
        let worker = f.worker();
        let stop = stop.clone();
        tokio::spawn(async move {
            while !stop.is_cancelled() {
                let _ = worker.pass(&stop).await;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
    };
    let reaper = {
        let maintenance = Maintenance::new(f.app.store().clone(), settings());
        let stop = stop.clone();
        tokio::spawn(async move {
            while !stop.is_cancelled() {
                maintenance.tick().await.unwrap();
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
    };
    // Renewals fail, so lease_until stays where the dispatch put it.
    let lease_until: chrono::DateTime<Utc> =
        sqlx::query_scalar("SELECT lease_until FROM generation_operations WHERE id = $1::uuid")
            .bind(&first)
            .fetch_one(f.db.pool())
            .await
            .unwrap();
    while f.provider.dropped() == 0 {
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    let dropped_at: chrono::DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(f.db.pool())
        .await
        .unwrap();
    assert!(
        dropped_at < lease_until,
        "the local deadline dropped the call before the database lease ended \
         ({dropped_at} vs {lease_until})"
    );
    assert_eq!(
        running.await.unwrap().unwrap(),
        Pass::Abandoned("deadline", Settled::Done(GenerationState::Unknown))
    );
    let freed = std::time::Instant::now();
    while f.state(&second).await.0 != "completed" {
        assert!(
            freed.elapsed() < Duration::from_secs(10),
            "the second operation ran"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    stop.cancel();
    competitor.await.unwrap();
    reaper.await.unwrap();
    assert_eq!(f.state(&first).await.1.as_deref(), Some("deadline"));
    assert_eq!(f.provider.calls(), 2);
    assert_eq!(f.provider.max_in_flight("t"), 1);
    assert_eq!(
        f.provider.drops_at_start(),
        vec![0, 1],
        "the second call started only after the first was dropped"
    );
}

/// M1 reaper boundary: the reaper frees a dispatched slot only once `lease_until + grace` has
/// passed by the database clock. Every tick that abandons it must end after that instant.
#[tokio::test]
async fn memory_generation_reaper_frees_a_dispatched_slot_only_after_lease_and_grace() {
    let f = fixture(CountedProvider::new(vec![])).await;
    let (_, client) = f.tenant("t", 100_000_000).await;
    let store = f.app.store().clone();
    let grace = Duration::from_millis(700);
    let maintenance = Maintenance::new(
        store.clone(),
        GenerationSettings {
            reaper_grace: grace,
            ..settings()
        },
    );
    let clock = || async {
        sqlx::query_scalar::<_, chrono::DateTime<Utc>>("SELECT clock_timestamp()")
            .fetch_one(f.db.pool())
            .await
            .unwrap()
    };
    // Each attempt dispatches a fresh operation with a 1 s lease and ticks the reaper until it
    // frees the slot. No tick may free it before `lease_until + grace` (by the database clock).
    // An attempt also needs one quiet tick that started within 450 ms of that boundary: its
    // transaction ran after that instant, so a reaper acting 0.5 s early would have freed the
    // slot there. Under heavy load a tick may not land in that window; the attempt is then
    // repeated (a reaper that frees early can never pass).
    let mut qualified = false;
    for attempt in 0..5 {
        let op = op_id(
            &f.run(
                &client,
                "generations.request",
                request(&format!("b{attempt}"), "t", window()),
            )
            .await
            .unwrap(),
        );
        let job = store
            .claim_generation(Uuid::now_v7(), Duration::from_secs(1))
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            store
                .dispatch_generation(&job, Duration::from_secs(1))
                .await
                .unwrap(),
            crate::application::ports::Dispatch::Send(_)
        ));
        let lease_until: chrono::DateTime<Utc> =
            sqlx::query_scalar("SELECT lease_until FROM generation_operations WHERE id = $1::uuid")
                .bind(&op)
                .fetch_one(f.db.pool())
                .await
                .unwrap();
        let boundary = lease_until + Days::milliseconds(700);
        let mut last_quiet_start = None;
        loop {
            let before = clock().await;
            let abandoned = maintenance.tick().await.unwrap().reaped.abandoned;
            let after = clock().await;
            if abandoned == 1 {
                assert!(after >= boundary, "freed early: {after} < {boundary}");
                break;
            }
            last_quiet_start = Some(before);
            assert!(after < boundary + Days::seconds(10), "never freed");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(f.state(&op).await.1.as_deref(), Some("lease_expired"));
        if last_quiet_start.is_some_and(|start| start >= boundary - Days::milliseconds(450)) {
            qualified = true;
            break;
        }
    }
    assert!(
        qualified,
        "the slot was never seen held within 450 ms of lease_until + grace"
    );
}

/// M1 "reaper vs heartbeat": a provider call lasting 2.5 leases while two reapers tick every
/// 10 ms with grace 0 on their own connections. Renewals keep the lease ahead of it, so the
/// operation is never reaped, the call is never dropped and the output is saved once.
#[tokio::test]
async fn memory_generation_reaper_racing_live_heartbeats_never_reaps_a_held_lease() {
    let f = fixture(CountedProvider::steps(vec![Step::After(
        Duration::from_millis(2_500),
        answer(USEFUL),
    )]))
    .await;
    let (_, client) = f.tenant("t", 100_000_000).await;
    let op = op_id(
        &f.run(&client, "generations.request", request("k1", "t", window()))
            .await
            .unwrap(),
    );
    let stop = tokio_util::sync::CancellationToken::new();
    let reapers: Vec<_> = (0..2)
        .map(|_| {
            let maintenance = Maintenance::new(f.app.store().clone(), settings());
            let stop = stop.clone();
            let pool = f.db.pool().clone();
            tokio::spawn(async move {
                let clock = || async {
                    sqlx::query_scalar::<_, chrono::DateTime<Utc>>("SELECT clock_timestamp()")
                        .fetch_one(&pool)
                        .await
                        .unwrap()
                };
                // (released, abandoned, database-clock span of each tick)
                let mut total = (0, 0, Vec::new());
                while !stop.is_cancelled() {
                    let before = clock().await;
                    let reaped = maintenance.tick().await.unwrap().reaped;
                    total.0 += reaped.released;
                    total.1 += reaped.abandoned;
                    total.2.push((before, clock().await));
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                total
            })
        })
        .collect();
    let provider: Arc<dyn GenerationProvider> = f.provider.clone();
    let worker = GenerationWorker::new(f.app.store().clone(), provider, tight(1_000, 150, 300));
    let pass = worker
        .pass(&tokio_util::sync::CancellationToken::new())
        .await
        .unwrap();
    stop.cancel();
    // The race is real when a reaper tick ran entirely inside the window where an unrenewed
    // lease would already have been reaped: after dispatch + lease (1 s) + grace (0) and
    // before the operation finished. Measured by the database clock, not by tick counts.
    let (dispatched, finished): (chrono::DateTime<Utc>, chrono::DateTime<Utc>) = sqlx::query_as(
        "SELECT dispatched_at, finished_at FROM generation_operations WHERE id = $1::uuid",
    )
    .bind(&op)
    .fetch_one(f.db.pool())
    .await
    .unwrap();
    let mut raced = 0;
    for reaper in reapers {
        let (released, abandoned, ticks) = reaper.await.unwrap();
        assert_eq!((released, abandoned), (0, 0));
        raced += ticks
            .iter()
            .filter(|(start, end)| *start > dispatched + Days::seconds(1) && *end < finished)
            .count();
    }
    assert!(
        raced > 0,
        "a reaper tick ran where an unrenewed lease would have been reaped"
    );
    assert_eq!(pass, Pass::Settled(GenerationState::Completed));
    assert_eq!(f.state(&op).await.1.as_deref(), Some("saved"));
    assert_eq!((f.provider.calls(), f.provider.dropped()), (1, 0));
}

/// M1 "competing sweepers": four sweepers on their own connections over the same 1,200
/// candidates skip each other's locks; the counts sum exactly and nothing errors.
#[tokio::test]
async fn memory_generation_competing_sweepers_split_the_work_exactly() {
    let f = fixture(CountedProvider::new(vec![])).await;
    f.tenant("t", 100_000_000).await;
    f.old_sources("t", "swept:", 1_200).await;
    let pool = f.db.pool().clone();
    let sweepers: Vec<_> = (0..4)
        .map(|_| {
            let store = f.app.store().clone();
            let pool = pool.clone();
            tokio::spawn(async move {
                let (mut total, mut batches) = (0u64, 0u64);
                loop {
                    let swept = store.sweep_retention(50).await.unwrap().sources;
                    total += swept;
                    batches += u64::from(swept > 0);
                    if swept == 0 {
                        let left: i64 = sqlx::query_scalar(
                            "SELECT count(*) FROM sources WHERE content IS NOT NULL",
                        )
                        .fetch_one(&pool)
                        .await
                        .unwrap();
                        if left == 0 {
                            return (total, batches);
                        }
                    }
                    tokio::task::yield_now().await;
                }
            })
        })
        .collect();
    let mut totals = Vec::new();
    for sweeper in sweepers {
        totals.push(sweeper.await.unwrap());
    }
    assert_eq!(
        totals.iter().map(|(total, _)| total).sum::<u64>(),
        1_200,
        "{totals:?}"
    );
    assert!(totals.iter().all(|(total, batches)| *total <= batches * 50));
    assert_eq!(
        f.scalar(
            "SELECT count(*) FROM sources WHERE content IS NULL AND content_nulled_at IS NOT NULL"
        )
        .await,
        1_200
    );
}

/// Review p6b-code-r1 m1: old evidence kept by a recently finished operation never fills the
/// candidate batch, so an expired revision behind more than a batch of it is still nulled.
#[tokio::test]
async fn memory_generation_retention_does_not_stall_behind_recently_reused_evidence() {
    let f = fixture(CountedProvider::new(vec![])).await;
    let (_, client) = f.tenant("t", 100_000_000).await;
    let op = op_id(
        &f.run(&client, "generations.request", request("k1", "t", window()))
            .await
            .unwrap(),
    );
    f.run(
        &client,
        "generations.cancel",
        json!({"idempotency_key": "c1", "expires_at": later(), "namespace": "ns",
               "operation_id": op}),
    )
    .await
    .unwrap();
    f.old_sources("t", "reused:", 600).await;
    f.exec(
        "INSERT INTO generation_operation_sources (tenant_id, namespace_id, operation_id, source_id) \
         SELECT s.tenant_id, s.namespace_id, o.id, s.id FROM sources s, generation_operations o \
          WHERE s.external_ref LIKE 'reused:%'",
    )
    .await;
    f.exec(
        "INSERT INTO sources (tenant_id, namespace_id, id, external_ref, revision, access_level, \
         content, content_sha256, occurred_at, expires_at) \
         SELECT tenant_id, namespace_id, gen_random_uuid(), 'expired:1', '1', 1, 'x', \
                sha256(convert_to('x', 'UTF8')), now(), now() - interval '1 second' \
           FROM sources WHERE external_ref = 'reused:1'",
    )
    .await;
    let swept = f.app.store().sweep_retention(500).await.unwrap();
    assert_eq!(swept.sources, 1);
    assert_eq!(
        f.scalar(
            "SELECT count(*) FROM sources WHERE external_ref = 'expired:1' AND content IS NULL"
        )
        .await,
        1
    );
    assert_eq!(
        f.scalar("SELECT count(*) FROM sources WHERE external_ref LIKE 'reused:%' AND content IS NOT NULL")
            .await,
        600
    );
}

/// Review p6b-code-r1 n6: D3 allows a write grant above the read grant. A stored revision
/// above the caller's `read_max` is a conflict whatever the content offered, on both
/// `sources.record` and acceptance, so a write-up credential cannot confirm guesses about
/// what it cannot read; revisions it can read still match.
#[tokio::test]
async fn memory_generation_write_up_grants_cannot_probe_unreadable_revisions() {
    let f = fixture(CountedProvider::new(vec![])).await;
    let (_, client) = f.tenant("t", 100_000_000).await;
    f.run(&client, "generations.request", request("k1", "t", window()))
        .await
        .unwrap();
    let issued = f
        .run(
            &f.operator,
            "admin.credentials.issue",
            json!({"tenant": "t", "label": "write-up", "expires_at": null, "grants": [
                {"namespace": "ns", "read_max": 2, "write_max": 6,
                 "operations": ["sources", "generate", "search"]}]}),
        )
        .await
        .unwrap();
    let writer = f
        .app
        .authenticate(issued["token"].as_str(), Surface::Client, Utc::now())
        .await
        .unwrap();
    let record = |key: &str, reference: &str, level: i64, content: &str| {
        json!({"idempotency_key": key, "expires_at": later(), "namespace": "ns",
               "session_id": null,
               "sources": [{"external_ref": reference, "revision": "1", "access_level": level,
                            "occurred_at": "2026-11-08T10:00:00Z", "content": content,
                            "expires_at": null}]})
    };
    let stored = "Yes. password=hunter2 and the login timeout is raised.";
    for (key, content) in [("s1", stored), ("s2", "a wrong guess")] {
        assert_eq!(
            f.run(
                &writer,
                "sources.record",
                record(key, "chat:e2", 4, content)
            )
            .await
            .unwrap_err(),
            ErrorCode::Conflict,
            "{content}"
        );
    }
    // A readable revision (level 2) still matches identically.
    f.run(
        &writer,
        "sources.record",
        record("s3", "chat:e1", 2, "Should we use PostgreSQL?"),
    )
    .await
    .unwrap();
    assert_eq!(
        f.run(&writer, "generations.request", request("k2", "t", window()))
            .await
            .unwrap_err(),
        ErrorCode::Conflict
    );
    // Review p6b-code-r2 m2: the recording credential reuses its own revisions above its read
    // grant: record then generate, overlapping windows, and re-recording under a new key.
    let own = |id: &str| event(id, "1", 5, &format!("Write-up evidence {id}."));
    f.run(
        &writer,
        "sources.record",
        record("w1", "chat:w1", 5, "Write-up evidence w1."),
    )
    .await
    .unwrap();
    f.run(
        &writer,
        "sources.record",
        record("w2", "chat:w1", 5, "Write-up evidence w1."),
    )
    .await
    .unwrap();
    f.run(
        &writer,
        "generations.request",
        request("g1", "t", vec![own("w1")]),
    )
    .await
    .unwrap();
    f.run(
        &writer,
        "generations.request",
        request("g2", "t", vec![own("w1"), own("w2")]),
    )
    .await
    .unwrap();
    // Another write-up credential probing those revisions gets the uniform conflict, right
    // guess or wrong.
    let other = f
        .run(
            &f.operator,
            "admin.credentials.issue",
            json!({"tenant": "t", "label": "other", "expires_at": null, "grants": [
                {"namespace": "ns", "read_max": 2, "write_max": 6,
                 "operations": ["sources", "generate", "search"]}]}),
        )
        .await
        .unwrap();
    let other = f
        .app
        .authenticate(other["token"].as_str(), Surface::Client, Utc::now())
        .await
        .unwrap();
    for (key, content) in [("o1", "Write-up evidence w1."), ("o2", "wrong")] {
        assert_eq!(
            f.run(&other, "sources.record", record(key, "chat:w1", 5, content))
                .await
                .unwrap_err(),
            ErrorCode::Conflict
        );
    }
    assert_eq!(
        f.run(
            &other,
            "generations.request",
            request("o3", "t", vec![own("w1")])
        )
        .await
        .unwrap_err(),
        ErrorCode::Conflict
    );
}
