//! Capture intents (phase 05 E8, E9) at the freeze and delivery layer, against PostgreSQL and
//! the contract fake: the switch, operator cap, readiness and bounds that gate a freeze, the
//! frozen generation request and its single promised delivery, replay on retry, terminal refusals,
//! competing claimants, and the switch-off and agent-delete races in both orders.
use super::*;
use crate::adapters::persistence::agent::{finish_agent_delete_on, lock_agent_delete_on};
use crate::application::memory::MemorySettingsStore;
use serde_json::json;

const INPUT: &str = r#"{"task":"CAPTURE-INPUT renewal terms"}"#;
const OUTPUT: &str = r#"{"answer":"CAPTURE-OUTPUT net 30"}"#;

/// Turn one capture policy on or off through the owner's settings command (actor, reason, time).
async fn switch(f: &Fixture, kind: MemoryCaptureKind, enabled: bool, limits: CaptureLimits) {
    let revision: i64 = sqlx::query_scalar(
        "SELECT COALESCE(max(revision), 0) FROM memory_agent_capture \
         WHERE company_id = $1 AND agent_id = $2 AND kind = $3",
    )
    .bind(f.company)
    .bind(f.agent)
    .bind(kind.as_str())
    .fetch_one(f.pool())
    .await
    .unwrap();
    f.persistence
        .set_memory_capture(
            f.scope(),
            Fixture::actor(f.owner),
            &MemoryCaptureChange {
                agent: f.agent,
                kind,
                enabled,
                limits,
                reason: "Pilot thread capture".into(),
                expected_revision: revision,
            },
        )
        .await
        .unwrap();
}

async fn cap(f: &Fixture, cap: Option<i64>) {
    sqlx::query("UPDATE memory_tenants SET generation_cap = $2 WHERE company_id = $1")
        .bind(f.company)
        .bind(cap)
        .execute(f.pool())
        .await
        .unwrap();
}

const LIMITS: CaptureLimits = CaptureLimits {
    allowance_tokens: 5_000,
    max_output_tokens: 300,
    expiry_hours: 2,
};

/// A ready agent namespace, generation allowed by the operator, and the agent's thread switch.
async fn capturing(f: &Fixture, fake: &ContractFake) -> Uuid {
    let namespace = ready(f, fake, NamespaceOwner::Agent(f.agent)).await;
    cap(f, Some(100_000)).await;
    fake.state.lock().unwrap().generation_cap = 100_000;
    switch(f, MemoryCaptureKind::Threads, true, LIMITS).await;
    namespace
}

fn window<'a>(f: &Fixture, exchange: Uuid, scope: SaveScope) -> CaptureIntent<'a> {
    CaptureIntent {
        company: f.scope(),
        run: RunId::new(Uuid::new_v4()),
        execution: ExecutionId::new(Uuid::new_v4()),
        exchange,
        agent: f.agent,
        kind: MemoryCaptureKind::Threads,
        scope,
        input: INPUT,
        output: OUTPUT,
    }
}

async fn capture(f: &Fixture, intent: &CaptureIntent<'_>, levels: LockedRun) -> CaptureEnqueue {
    let mut tx = f.pool().begin().await.unwrap();
    let captured = freeze_capture_on(&mut tx, intent, levels).await.unwrap();
    tx.commit().await.unwrap();
    captured
}

async fn generation_rows(f: &Fixture) -> Vec<(Uuid, String, Option<Value>, String)> {
    sqlx::query_as(
        "SELECT id, idempotency_key, request, state FROM memory_deliveries \
         WHERE company_id = $1 AND kind = 'generation' ORDER BY created_at, id",
    )
    .bind(f.company)
    .fetch_all(f.pool())
    .await
    .unwrap()
}

/// E8: the freeze writes one generation intent with the frozen request (the window's two texts
/// at the run's content level, the producing ceiling, `unknown` consumed for an inexact run, the
/// policy's allowance, output limit and expiry under `ob.g.<exchange>`); a replayed acceptance
/// freezes nothing again; the worker sends it once and records the accepted operation.
#[tokio::test]
async fn memory_capture_freezes_one_generation_intent_and_delivers_it_once() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    let namespace = capturing(&f, &fake).await;
    let exchange = Uuid::new_v4();
    let intent = window(&f, exchange, SaveScope::Agent(f.agent));
    let before = chrono::Utc::now();
    let CaptureEnqueue::Enqueued { id, replayed } =
        capture(&f, &intent, run(AccessLevel::L3, false)).await
    else {
        panic!("captured");
    };
    assert!(!replayed);
    let rows = generation_rows(&f).await;
    assert_eq!(rows.len(), 1);
    let (_, key, request, state) = &rows[0];
    assert_eq!(key, &format!("ob.g.{}", exchange.simple()));
    assert_eq!(state, "pending");
    let body = &request.as_ref().unwrap()["body"];
    assert_eq!(body["namespace"], json!(namespace.hyphenated().to_string()));
    assert_eq!(body["producing_ceiling"], json!(6));
    assert_eq!(body["consumed"], json!({"kind":"unknown"}));
    assert_eq!(body["allowance_tokens"], json!(5_000));
    assert_eq!(body["max_output_tokens"], json!(300));
    let events = body["events"].as_array().unwrap();
    assert_eq!(events.len(), 2);
    // Distinct sources: the service refuses one source revision with two texts (live finding).
    assert_ne!(
        (&events[0]["source_ref"], &events[0]["revision"]),
        (&events[1]["source_ref"], &events[1]["revision"])
    );
    assert_eq!(
        (
            &events[0]["role"],
            &events[0]["text"],
            &events[0]["access_level"]
        ),
        (&json!("user"), &json!(INPUT), &json!(3))
    );
    assert_eq!(
        (
            &events[1]["role"],
            &events[1]["text"],
            &events[1]["access_level"]
        ),
        (&json!("assistant"), &json!(OUTPUT), &json!(3))
    );
    let expires: chrono::DateTime<chrono::Utc> =
        serde_json::from_value(body["expires_at"].clone()).unwrap();
    let lifetime = expires - before;
    assert!(
        lifetime > chrono::Duration::minutes(119) && lifetime < chrono::Duration::minutes(121),
        "the policy's 2 h expiry: {lifetime}"
    );
    // An exact run sends its content level as consumed.
    let exact = capture(
        &f,
        &window(&f, Uuid::new_v4(), SaveScope::Agent(f.agent)),
        run(AccessLevel::L2, true),
    )
    .await;
    assert!(matches!(exact, CaptureEnqueue::Enqueued { .. }));
    assert_eq!(
        generation_rows(&f).await[1].2.as_ref().unwrap()["body"]["consumed"],
        json!({"kind":"exact","level":2})
    );
    sqlx::query("DELETE FROM memory_deliveries WHERE company_id = $1 AND id <> $2")
        .bind(f.company)
        .bind(id)
        .execute(f.pool())
        .await
        .unwrap();

    // A replayed acceptance finds the key: the same intent, nothing frozen again.
    assert_eq!(
        capture(&f, &intent, run(AccessLevel::L5, true)).await,
        CaptureEnqueue::Enqueued { id, replayed: true }
    );
    assert_eq!(generation_rows(&f).await.len(), 1);

    let worker = worker(&f, &fake);
    assert!(pass(&worker).await);
    let delivered = row_by_id(&f, id).await;
    assert_eq!(delivered.state, "delivered");
    let result = delivered.result.unwrap();
    assert!(result["operation_id"].as_str().is_some());
    assert_eq!(result["replayed"], json!(false));
    {
        let state = fake.state.lock().unwrap();
        assert_eq!(state.generations.len(), 1);
        let (sent_to, sent, _) = &state.generations[key];
        assert_eq!(sent_to, &namespace.hyphenated().to_string());
        assert_eq!(sent, body, "the frozen request, byte for byte");
    }
    assert!(!pass(&worker).await);
    assert_eq!(
        fake.calls()
            .iter()
            .filter(|call| *call == "generations/request")
            .count(),
        1
    );
}

/// E8: an outage after the service acted (the connection dropped) retries with the same key and
/// body, so the service replays its first answer instead of starting a second generation.
#[tokio::test]
async fn memory_capture_a_retry_replays_the_frozen_request() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    capturing(&f, &fake).await;
    let CaptureEnqueue::Enqueued { id, .. } = capture(
        &f,
        &window(&f, Uuid::new_v4(), SaveScope::Agent(f.agent)),
        run(AccessLevel::L1, false),
    )
    .await
    else {
        panic!("captured");
    };
    fake.state
        .lock()
        .unwrap()
        .drops
        .push("generations/request".into());
    let worker = worker(&f, &fake);
    assert!(pass(&worker).await);
    let retrying = row_by_id(&f, id).await;
    assert_eq!((retrying.state.as_str(), retrying.attempts), ("pending", 0));
    sqlx::query("UPDATE memory_deliveries SET available_at = clock_timestamp() WHERE id = $1")
        .bind(id)
        .execute(f.pool())
        .await
        .unwrap();
    assert!(pass(&worker).await);
    let delivered = row_by_id(&f, id).await;
    assert_eq!(delivered.state, "delivered");
    assert_eq!(delivered.result.unwrap()["replayed"], json!(true));
    let state = fake.state.lock().unwrap();
    assert_eq!(state.generations.len(), 1, "one generation, replayed");
    assert!(state.violations.is_empty(), "{:?}", state.violations);
}

/// E8: the service's `generation_disabled` is terminal; an intent past its policy expiry is
/// settled `expired` at the claim and never sent.
#[tokio::test]
async fn memory_capture_generation_disabled_is_terminal_and_an_expired_intent_is_never_sent() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    capturing(&f, &fake).await;
    // OpenBot's cached cap still allows it; the service's current cap does not.
    fake.state.lock().unwrap().generation_cap = 0;
    let CaptureEnqueue::Enqueued { id, .. } = capture(
        &f,
        &window(&f, Uuid::new_v4(), SaveScope::Agent(f.agent)),
        run(AccessLevel::L0, true),
    )
    .await
    else {
        panic!("captured");
    };
    let worker = worker(&f, &fake);
    assert!(pass(&worker).await);
    let refused = row_by_id(&f, id).await;
    assert_eq!(
        (
            refused.state.as_str(),
            refused.outcome_code.as_deref(),
            refused.request
        ),
        ("failed", Some("generation_disabled"), None)
    );
    fake.state.lock().unwrap().generation_cap = 100_000;
    let CaptureEnqueue::Enqueued { id, .. } = capture(
        &f,
        &window(&f, Uuid::new_v4(), SaveScope::Agent(f.agent)),
        run(AccessLevel::L0, true),
    )
    .await
    else {
        panic!("captured");
    };
    sqlx::query(
        "UPDATE memory_deliveries SET created_at = clock_timestamp() - interval '1 hour', \
           expires_at = clock_timestamp() - interval '1 minute' WHERE id = $1",
    )
    .bind(id)
    .execute(f.pool())
    .await
    .unwrap();
    let calls = fake.calls().len();
    assert!(pass(&worker).await);
    let expired = row_by_id(&f, id).await;
    assert_eq!(
        (expired.state.as_str(), expired.outcome_code.as_deref()),
        ("expired", Some("expired"))
    );
    assert_eq!(fake.calls().len(), calls, "nothing sent");
}

/// E8 (U3): nothing is frozen unless the agent's switch for this kind is on, the operator's cap
/// allows generation, memory is enabled, the target namespace is ready, the window is within the
/// producing ceiling and the service's text bound.
#[tokio::test]
async fn memory_capture_switch_cap_readiness_and_bounds_gate_the_freeze() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    ready(&f, &fake, NamespaceOwner::Agent(f.agent)).await;
    let agent = SaveScope::Agent(f.agent);
    let levels = run(AccessLevel::L1, true);
    let skipped = |reason| CaptureEnqueue::Skipped(reason);
    cap(&f, Some(100_000)).await;
    // Never set, then the other kind on only.
    assert_eq!(
        capture(&f, &window(&f, Uuid::new_v4(), agent), levels).await,
        skipped("off")
    );
    switch(&f, MemoryCaptureKind::PrivateSessions, true, LIMITS).await;
    assert_eq!(
        capture(&f, &window(&f, Uuid::new_v4(), agent), levels).await,
        skipped("off")
    );
    switch(&f, MemoryCaptureKind::Threads, true, LIMITS).await;
    switch(&f, MemoryCaptureKind::Threads, false, LIMITS).await;
    assert_eq!(
        capture(&f, &window(&f, Uuid::new_v4(), agent), levels).await,
        skipped("off")
    );
    switch(&f, MemoryCaptureKind::Threads, true, LIMITS).await;
    // The operator's cap: 0 or not yet known.
    for unknown in [Some(0), None] {
        cap(&f, unknown).await;
        assert_eq!(
            capture(&f, &window(&f, Uuid::new_v4(), agent), levels).await,
            skipped("generation_off")
        );
    }
    cap(&f, Some(100_000)).await;
    // A scope without a ready namespace.
    let user = SaveScope::User(f.owner);
    assert_eq!(
        capture(&f, &window(&f, Uuid::new_v4(), user), levels).await,
        skipped("not_ready")
    );
    // Content above the producing ceiling, and a window over the service's text bound.
    let above = LockedRun {
        ceiling: AccessLevel::L2,
        content: AccessLevel::L4,
        content_exact: true,
    };
    assert_eq!(
        capture(&f, &window(&f, Uuid::new_v4(), agent), above).await,
        skipped("above_ceiling")
    );
    let large = "x".repeat(MAX_CAPTURE_TEXT_BYTES);
    let mut oversized = window(&f, Uuid::new_v4(), agent);
    oversized.output = &large;
    assert_eq!(capture(&f, &oversized, levels).await, skipped("too_large"));
    // Memory disabled.
    f.disable().await.unwrap();
    assert_eq!(
        capture(&f, &window(&f, Uuid::new_v4(), agent), levels).await,
        skipped("not_enabled")
    );
    assert!(generation_rows(&f).await.is_empty());
    assert!(
        fake.calls()
            .iter()
            .all(|call| call != "generations/request")
    );
}

/// E8 with competing claimants: two workers race for one capture intent; the claim's row lock
/// and the in-flight skip let exactly one send it.
#[tokio::test]
async fn memory_capture_competing_delivery_claimants_send_once() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    capturing(&f, &fake).await;
    let CaptureEnqueue::Enqueued { id, .. } = capture(
        &f,
        &window(&f, Uuid::new_v4(), SaveScope::Agent(f.agent)),
        run(AccessLevel::L0, true),
    )
    .await
    else {
        panic!("captured");
    };
    fake.state
        .lock()
        .unwrap()
        .holds
        .insert("generations/request".into(), Duration::from_millis(300));
    let (first, second) = (Arc::new(worker(&f, &fake)), Arc::new(worker(&f, &fake)));
    let racing = |worker: Arc<MemoryDeliveryWorker>| {
        tokio::spawn(async move { worker.pass(&CancellationToken::new()).await.unwrap() })
    };
    let (a, b) = (racing(first.clone()), racing(second.clone()));
    let (a, b) = (a.await.unwrap(), b.await.unwrap());
    assert!(a || b);
    while pass(&first).await || pass(&second).await {}
    assert_eq!(row_by_id(&f, id).await.state, "delivered");
    let state = fake.state.lock().unwrap();
    assert_eq!(
        state
            .calls
            .iter()
            .filter(|call| *call == "generations/request")
            .count(),
        1
    );
    assert_eq!(state.held_peak.get("generations/request"), Some(&1));
}

/// E8/E9: switching capture off and a freeze serialize on the policy row, with real concurrent
/// transactions in both orders. A freeze that committed first stays deliverable (the switch
/// governs new windows); a freeze that waited finds the switch off.
#[tokio::test]
async fn memory_capture_switch_off_races_the_freeze_in_both_orders() {
    for freeze_first in [true, false] {
        let Some(f) = Fixture::new().await else {
            return;
        };
        let fake = ContractFake::start(ADMIN).await;
        capturing(&f, &fake).await;
        let revision: i64 = sqlx::query_scalar(
            "SELECT revision FROM memory_agent_capture WHERE company_id = $1 AND kind = 'threads'",
        )
        .bind(f.company)
        .fetch_one(f.pool())
        .await
        .unwrap();
        let off = MemoryCaptureChange {
            agent: f.agent,
            kind: MemoryCaptureKind::Threads,
            enabled: false,
            limits: LIMITS,
            reason: "Pause capture".into(),
            expected_revision: revision,
        };
        let (company, agent, pool) = (f.scope(), f.agent, f.pool().clone());
        if freeze_first {
            let mut tx = f.pool().begin().await.unwrap();
            let captured = freeze_capture_on(
                &mut tx,
                &window(&f, Uuid::new_v4(), SaveScope::Agent(agent)),
                run(AccessLevel::L0, true),
            )
            .await
            .unwrap();
            assert!(matches!(captured, CaptureEnqueue::Enqueued { .. }));
            let holder = backend_pid(&mut tx).await;
            let switching = tokio::spawn({
                let persistence = f.persistence.clone();
                let owner = Fixture::actor(f.owner);
                async move { persistence.set_memory_capture(company, owner, &off).await }
            });
            blocked_by(f.pool(), holder).await;
            tx.commit().await.unwrap();
            switching.await.unwrap().unwrap();
            let rows = generation_rows(&f).await;
            assert_eq!((rows.len(), rows[0].3.as_str()), (1, "pending"));
            assert!(pass(&worker(&f, &fake)).await);
            assert_eq!(row_by_id(&f, rows[0].0).await.state, "delivered");
        } else {
            let mut tx = f.pool().begin().await.unwrap();
            super::super::capture::set_capture_on(&mut tx, company, Fixture::actor(f.owner), &off)
                .await
                .unwrap();
            let holder = backend_pid(&mut tx).await;
            let freezing = tokio::spawn(async move {
                let mut tx = pool.begin().await.unwrap();
                let intent = CaptureIntent {
                    company,
                    run: RunId::new(Uuid::new_v4()),
                    execution: ExecutionId::new(Uuid::new_v4()),
                    exchange: Uuid::new_v4(),
                    agent,
                    kind: MemoryCaptureKind::Threads,
                    scope: SaveScope::Agent(agent),
                    input: INPUT,
                    output: OUTPUT,
                };
                let captured = freeze_capture_on(&mut tx, &intent, run(AccessLevel::L0, true))
                    .await
                    .unwrap();
                tx.commit().await.unwrap();
                captured
            });
            blocked_by(f.pool(), holder).await;
            tx.commit().await.unwrap();
            assert_eq!(freezing.await.unwrap(), CaptureEnqueue::Skipped("off"));
            assert!(generation_rows(&f).await.is_empty());
        }
    }
}

/// E9: an agent's final delete and a freeze race on the namespace row, in both orders. Either
/// the freeze commits first and the delete cancels its intent (and removes the agent's policies),
/// or the freeze waits out the delete and finds no ready namespace. Nothing is ever sent.
#[tokio::test]
async fn memory_capture_agent_delete_races_the_freeze_in_both_orders() {
    for freeze_first in [true, false] {
        let Some(f) = Fixture::new().await else {
            return;
        };
        let fake = ContractFake::start(ADMIN).await;
        capturing(&f, &fake).await;
        let (company, agent, pool) = (f.scope(), f.agent, f.pool().clone());
        let deleting = move |pool: sqlx::PgPool| async move {
            let mut tx = pool.begin().await.unwrap();
            let locks = lock_agent_delete_on(&mut tx, agent).await.unwrap().unwrap();
            finish_agent_delete_on(&mut tx, locks).await.unwrap();
            tx
        };
        if freeze_first {
            let mut tx = f.pool().begin().await.unwrap();
            let captured = freeze_capture_on(
                &mut tx,
                &window(&f, Uuid::new_v4(), SaveScope::Agent(agent)),
                run(AccessLevel::L0, true),
            )
            .await
            .unwrap();
            assert!(matches!(captured, CaptureEnqueue::Enqueued { .. }));
            let holder = backend_pid(&mut tx).await;
            let removal = tokio::spawn(async move { deleting(pool).await.commit().await.unwrap() });
            blocked_by(f.pool(), holder).await;
            tx.commit().await.unwrap();
            removal.await.unwrap();
            let rows = generation_rows(&f).await;
            let cancelled = row_by_id(&f, rows[0].0).await;
            assert_eq!(
                (
                    cancelled.state.as_str(),
                    cancelled.outcome_code.as_deref(),
                    cancelled.request
                ),
                ("cancelled", Some("owner_removed"), None)
            );
        } else {
            let mut removal = deleting(pool.clone()).await;
            let holder = backend_pid(&mut removal).await;
            let freezing = tokio::spawn(async move {
                let mut tx = pool.begin().await.unwrap();
                let intent = CaptureIntent {
                    company,
                    run: RunId::new(Uuid::new_v4()),
                    execution: ExecutionId::new(Uuid::new_v4()),
                    exchange: Uuid::new_v4(),
                    agent,
                    kind: MemoryCaptureKind::Threads,
                    scope: SaveScope::Agent(agent),
                    input: INPUT,
                    output: OUTPUT,
                };
                let captured = freeze_capture_on(&mut tx, &intent, run(AccessLevel::L0, true))
                    .await
                    .unwrap();
                tx.commit().await.unwrap();
                captured
            });
            blocked_by(f.pool(), holder).await;
            removal.commit().await.unwrap();
            assert_eq!(
                freezing.await.unwrap(),
                CaptureEnqueue::Skipped("not_ready")
            );
            assert!(generation_rows(&f).await.is_empty());
        }
        let policies: i64 =
            sqlx::query_scalar("SELECT count(*) FROM memory_agent_capture WHERE company_id = $1")
                .bind(f.company)
                .fetch_one(f.pool())
                .await
                .unwrap();
        assert_eq!(
            policies, 0,
            "the agent's policies went with it (freeze_first={freeze_first})"
        );
        assert!(!pass(&worker(&f, &fake)).await);
        assert!(
            fake.calls()
                .iter()
                .all(|call| call != "generations/request")
        );
    }
}

/// E9 lock order, interleaved: the agent delete has locked the agent and retired its namespace
/// (the first half of `finish_agent_delete_on`) when a freeze starts and blocks on the namespace;
/// the delete then finishes (`principals` demotion and `DELETE FROM agents`, whose cascade locks
/// the policy rows). Because the freeze takes the namespace before the policy row, it holds no
/// policy lock while it waits, so the delete commits and the freeze finds no ready namespace. A
/// freeze that locked the policy first would deadlock here and one side would be aborted.
#[tokio::test]
async fn memory_capture_agent_delete_interleaved_with_a_freeze_does_not_deadlock() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    capturing(&f, &fake).await;
    let (company, agent, pool) = (f.scope(), f.agent, f.pool().clone());
    let mut removal = f.pool().begin().await.unwrap();
    let _locks = lock_agent_delete_on(&mut removal, agent)
        .await
        .unwrap()
        .unwrap();
    super::super::retire_namespaces_on(&mut removal, f.company, NamespaceOwner::Agent(agent))
        .await
        .unwrap();
    let holder = backend_pid(&mut removal).await;
    let freezing = tokio::spawn(async move {
        let mut tx = pool.begin().await.unwrap();
        let intent = CaptureIntent {
            company,
            run: RunId::new(Uuid::new_v4()),
            execution: ExecutionId::new(Uuid::new_v4()),
            exchange: Uuid::new_v4(),
            agent,
            kind: MemoryCaptureKind::Threads,
            scope: SaveScope::Agent(agent),
            input: INPUT,
            output: OUTPUT,
        };
        let captured = freeze_capture_on(&mut tx, &intent, run(AccessLevel::L0, true)).await;
        tx.commit().await.unwrap();
        captured
    });
    blocked_by(f.pool(), holder).await;
    // The rest of `finish_agent_delete_on`, now that the freeze waits.
    sqlx::query(
        "UPDATE principals SET kind = 'external', agent_id = NULL, updated_at = CURRENT_TIMESTAMP \
         WHERE company_id = $1 AND agent_id = $2",
    )
    .bind(f.company)
    .bind(agent)
    .execute(&mut *removal)
    .await
    .unwrap();
    sqlx::query("DELETE FROM agents WHERE id = $1")
        .bind(agent)
        .execute(&mut *removal)
        .await
        .expect("the cascade does not wait on the freeze");
    removal.commit().await.unwrap();
    assert_eq!(
        freezing.await.unwrap().unwrap(),
        CaptureEnqueue::Skipped("not_ready")
    );
    assert!(generation_rows(&f).await.is_empty());
}

/// E9/E11 delete vs delivery: the agent's final delete while its capture is in flight at the
/// service settles the intent `cancelled(owner_removed)` at once (settlement is kind-agnostic);
/// the worker's completion then finds its fence gone and changes nothing.
#[tokio::test]
async fn memory_capture_agent_delete_during_an_in_flight_delivery_loses_its_completion() {
    use crate::application::use_cases::agent::AgentPersistence;
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    capturing(&f, &fake).await;
    let CaptureEnqueue::Enqueued { id, .. } = capture(
        &f,
        &window(&f, Uuid::new_v4(), SaveScope::Agent(f.agent)),
        run(AccessLevel::L0, true),
    )
    .await
    else {
        panic!("captured");
    };
    fake.state
        .lock()
        .unwrap()
        .holds
        .insert("generations/request".into(), Duration::from_millis(800));
    let worker = Arc::new(worker(&f, &fake));
    let sending = tokio::spawn({
        let worker = worker.clone();
        async move { worker.pass(&CancellationToken::new()).await.unwrap() }
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while fake
            .state
            .lock()
            .unwrap()
            .held_now
            .get("generations/request")
            .copied()
            .unwrap_or(0)
            == 0
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the capture is in flight");
    f.persistence.delete(f.agent).await.unwrap();
    let cancelled = row_by_id(&f, id).await;
    assert_eq!(
        (
            cancelled.state.as_str(),
            cancelled.outcome_code.as_deref(),
            cancelled.request
        ),
        ("cancelled", Some("owner_removed"), None)
    );
    assert!(sending.await.unwrap());
    let after = row_by_id(&f, id).await;
    assert_eq!(
        (after.state.as_str(), after.result),
        ("cancelled", None),
        "the completion was fenced out"
    );
}
