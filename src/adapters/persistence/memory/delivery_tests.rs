//! Promised memory deliveries (E2, E6, E8, E9) against PostgreSQL and the contract fake. Every
//! test owns its database: the delivery claim sweeps the whole table.
use super::deliveries::*;
use super::tests::{Fixture, backend_pid, blocked_by};
use crate::adapters::memory_service::MemoryServiceHttp;
use crate::adapters::memory_service::contract_fake::ContractFake;
use crate::adapters::persistence::credentials::envelope::CredentialContext;
use crate::app_error::AppResult;
use crate::application::memory::*;
use crate::domain::workflow::{ExecutionId, RunId};
use crate::entities::access_level::AccessLevel;
use crate::infra::memory_config::{MemoryServiceConfig, ProvisionerToken};
use secrecy::SecretString;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[path = "capture_tests.rs"]
mod capture_intents;

const ADMIN: &str =
    "msk_adm_0190f1a2b3c44d5e8f9a0b1c2d3e4f50_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

fn http(fake: &ContractFake) -> Arc<MemoryServiceHttp> {
    Arc::new(
        MemoryServiceHttp::new(&MemoryServiceConfig {
            base_url: fake.base_url.clone(),
            provisioner: ProvisionerToken::parse(SecretString::from(ADMIN)).unwrap(),
            request_timeout: Duration::from_secs(5),
            recall_deadline: Duration::from_secs(1),
            interactive_in_flight: 2,
            background_in_flight: 2,
        })
        .unwrap(),
    )
}

fn worker(f: &Fixture, fake: &ContractFake) -> MemoryDeliveryWorker {
    MemoryDeliveryWorker::new(Arc::new(f.persistence.clone()), http(fake))
}

async fn pass(worker: &MemoryDeliveryWorker) -> bool {
    worker.pass(&CancellationToken::new()).await.unwrap()
}

/// Enable memory and make `owner`'s namespace ready with a sealed client token the fake grants.
async fn ready(f: &Fixture, fake: &ContractFake, owner: NamespaceOwner) -> Uuid {
    if f.tenant().await.is_none() {
        f.enable().await.unwrap();
    }
    assert_eq!(f.ensure(owner).await, NamespaceReadiness::NotReady);
    let kind = match owner {
        NamespaceOwner::Company => "company",
        NamespaceOwner::Agent(_) => "agent",
        NamespaceOwner::User(_) => "user",
    };
    let namespace = f.namespaces(kind).await[0].0;
    let token = fake.client_token(f.company, namespace);
    let envelope = f
        .persistence
        .credential_cipher()
        .unwrap()
        .seal_envelope(
            &CredentialContext::memory_client_credential(f.company, namespace),
            &SecretString::from(token),
        )
        .unwrap();
    sqlx::query(
        "UPDATE memory_namespaces SET state = 'ready', credential_id = gen_random_uuid(), \
         credential_envelope = $3 WHERE company_id = $1 AND id = $2",
    )
    .bind(f.company)
    .bind(namespace)
    .bind(envelope)
    .execute(f.pool())
    .await
    .unwrap();
    namespace
}

fn run(content: AccessLevel, exact: bool) -> LockedRun {
    LockedRun {
        ceiling: AccessLevel::L6,
        content,
        content_exact: exact,
    }
}

async fn freeze(f: &Fixture, scope: SaveScope, key: &str, levels: LockedRun) -> SaveEnqueue {
    let facts = vec!["Renewal is due in March.\nThe customer asked for net 30.".to_owned()];
    let mut tx = f.pool().begin().await.unwrap();
    let saved = freeze_save_on(&mut tx, &intent(f, scope, key, &facts), levels)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    saved
}

fn intent<'a>(f: &Fixture, scope: SaveScope, key: &'a str, facts: &'a [String]) -> SaveIntent<'a> {
    SaveIntent {
        company: f.scope(),
        run: RunId::new(Uuid::new_v4()),
        execution: ExecutionId::new(Uuid::new_v4()),
        invocation: None,
        wait: None,
        scope,
        key,
        facts,
        external_sources: Vec::new(),
    }
}

#[derive(Debug, PartialEq, sqlx::FromRow)]
struct Row {
    state: String,
    outcome_code: Option<String>,
    attempts: i32,
    result: Option<Value>,
    request: Option<Value>,
}

async fn row_by_id(f: &Fixture, id: Uuid) -> Row {
    sqlx::query_as(
        "SELECT state, outcome_code, attempts, result, request FROM memory_deliveries WHERE id = $1",
    )
    .bind(id)
    .fetch_one(f.pool())
    .await
    .unwrap()
}

async fn row(f: &Fixture, key: &str) -> Row {
    sqlx::query_as(
        "SELECT state, outcome_code, attempts, result, request FROM memory_deliveries \
         WHERE company_id = $1 AND idempotency_key = $2",
    )
    .bind(f.company)
    .bind(key)
    .fetch_one(f.pool())
    .await
    .unwrap()
}

async fn make_due(f: &Fixture) {
    sqlx::query(
        "UPDATE memory_deliveries SET available_at = clock_timestamp() WHERE company_id = $1",
    )
    .bind(f.company)
    .execute(f.pool())
    .await
    .unwrap();
}

/// A ceiling-6 run that consumed exactly level 4 (a schedule run reading level-4 knowledge) saves
/// at 4 into the agent scope, but at its ceiling into the company scope (U2 addendum); an
/// inexact run saves at the ceiling into any scope.
#[tokio::test]
async fn memory_access_level_saved_label_follows_scope_and_exactness() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    ready(&f, &fake, NamespaceOwner::Agent(f.agent)).await;
    ready(&f, &fake, NamespaceOwner::Company).await;
    let worker = worker(&f, &fake);
    for (scope, key, levels, expected) in [
        (
            SaveScope::Agent(f.agent),
            "ob.s.agent-exact",
            run(AccessLevel::L4, true),
            4,
        ),
        (
            SaveScope::Company,
            "ob.s.company-exact",
            run(AccessLevel::L4, true),
            6,
        ),
        (
            SaveScope::Agent(f.agent),
            "ob.s.agent-inexact",
            run(AccessLevel::L4, false),
            6,
        ),
    ] {
        assert!(matches!(
            freeze(&f, scope, key, levels).await,
            SaveEnqueue::Enqueued {
                replayed: false,
                ..
            }
        ));
        let frozen = row(&f, key)
            .await
            .request
            .expect("an open intent holds its request");
        assert!(pass(&worker).await);
        let saved = row(&f, key).await;
        assert_eq!(saved.state, "delivered", "{key}");
        assert_eq!(
            saved.request, None,
            "a settled intent drops the memory text"
        );
        let context = &frozen["body"]["context"];
        assert_eq!(context["producing_ceiling"], 6);
        assert_eq!(
            context["consumed"]["kind"],
            if expected == 4 { "exact" } else { "unknown" },
            "{key}"
        );
        assert_eq!(
            saved.result.unwrap()["observations"][0]["access_level"],
            expected,
            "{key}"
        );
    }
    assert!(!pass(&worker).await);
}

/// One atomic request per logical save; an ambiguous outcome is resent with the same key and
/// body and settles as a replay; a repeated freeze replays its intent instead of re-freezing.
#[tokio::test]
async fn memory_integration_delivery_resends_the_same_request_after_an_ambiguous_outcome() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    ready(&f, &fake, NamespaceOwner::Company).await;
    let worker = worker(&f, &fake);
    let first = freeze(
        &f,
        SaveScope::Company,
        "ob.s.ambiguous",
        run(AccessLevel::L0, true),
    )
    .await;
    let SaveEnqueue::Enqueued {
        id,
        replayed: false,
    } = first
    else {
        panic!("{first:?}");
    };
    assert_eq!(
        freeze(
            &f,
            SaveScope::Company,
            "ob.s.ambiguous",
            run(AccessLevel::L6, true)
        )
        .await,
        SaveEnqueue::Enqueued { id, replayed: true },
        "a retry replays its frozen intent and label"
    );
    let frozen = row(&f, "ob.s.ambiguous").await.request;
    fake.drop_after_write("observations/save");
    assert!(pass(&worker).await);
    let retried = row(&f, "ob.s.ambiguous").await;
    assert_eq!((retried.state.as_str(), retried.attempts), ("pending", 0));
    assert_eq!(retried.request, frozen);
    make_due(&f).await;
    assert!(pass(&worker).await);
    let delivered = row(&f, "ob.s.ambiguous").await;
    assert_eq!(delivered.state, "delivered");
    assert_eq!(delivered.result.unwrap()["replayed"], true);
    let saves = fake.state.lock().unwrap().saves.len();
    assert_eq!(saves, 1, "the service executed the key once");
    assert_eq!(
        fake.calls()
            .iter()
            .filter(|call| *call == "observations/save")
            .count(),
        2
    );
}

/// Expired, stale-epoch and namespace-less intents settle at the claim without a request, and a
/// batch of them leaves the due set: a second pass reclaims none (poison batch).
#[tokio::test]
async fn memory_integration_delivery_settles_unsendable_intents_once_poison_batch() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    ready(&f, &fake, NamespaceOwner::Company).await;
    let worker = worker(&f, &fake);
    for index in 0..4 {
        freeze(
            &f,
            SaveScope::Company,
            &format!("ob.s.poison-{index}"),
            run(AccessLevel::L0, true),
        )
        .await;
    }
    sqlx::query("UPDATE memory_deliveries SET disable_epoch = disable_epoch + 1 WHERE company_id = $1 AND idempotency_key IN ('ob.s.poison-0','ob.s.poison-1')")
        .bind(f.company).execute(f.pool()).await.unwrap();
    sqlx::query("UPDATE memory_deliveries SET expires_at = created_at + interval '1 microsecond' WHERE company_id = $1 AND idempotency_key = 'ob.s.poison-2'")
        .bind(f.company).execute(f.pool()).await.unwrap();
    sqlx::query("UPDATE memory_namespaces SET state = 'removing', credential_id = NULL, credential_envelope = NULL WHERE company_id = $1")
        .bind(f.company).execute(f.pool()).await.unwrap();
    for _ in 0..4 {
        assert!(pass(&worker).await);
    }
    assert!(!pass(&worker).await, "nothing settled is reclaimed");
    assert!(!pass(&worker).await);
    let mut outcomes = Vec::new();
    for index in 0..4 {
        let saved = row(&f, &format!("ob.s.poison-{index}")).await;
        outcomes.push((saved.state, saved.outcome_code.unwrap()));
    }
    assert_eq!(
        outcomes,
        [
            ("failed".to_owned(), "disabled".to_owned()),
            ("failed".to_owned(), "disabled".to_owned()),
            ("expired".to_owned(), "expired".to_owned()),
            ("failed".to_owned(), "namespace_unavailable".to_owned()),
        ]
    );
    assert!(fake.calls().iter().all(|call| call != "observations/save"));
}

/// Lease expiry spends an attempt; the eighth is terminal. Two competing claimants never own
/// the same intent, and a stale claimant's completion is fenced out.
#[tokio::test]
async fn memory_integration_delivery_fences_competing_claimants_and_bounds_attempts() {
    let Some(mut f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    ready(&f, &fake, NamespaceOwner::Company).await;
    freeze(
        &f,
        SaveScope::Company,
        "ob.s.lease-a",
        run(AccessLevel::L0, true),
    )
    .await;
    // The competing intent belongs to another company: a company with a save in flight is
    // not claimable (F7), so two claimants of one company would find one intent.
    let first = f.company;
    f.company = super::tests::company(&f.persistence, f.owner).await;
    ready(&f, &fake, NamespaceOwner::Company).await;
    freeze(
        &f,
        SaveScope::Company,
        "ob.s.lease-b",
        run(AccessLevel::L0, true),
    )
    .await;
    f.company = first;
    let store = f.persistence.clone();
    let (a, b) = tokio::join!(
        store.claim_delivery(Uuid::new_v4(), DELIVERY_LEASE),
        store.claim_delivery(Uuid::new_v4(), DELIVERY_LEASE)
    );
    let (DeliveryClaimOutcome::Claimed(a), DeliveryClaimOutcome::Claimed(b)) =
        (a.unwrap(), b.unwrap())
    else {
        panic!("both claimants find an intent");
    };
    assert_ne!(a.id, b.id);
    sqlx::query("UPDATE memory_deliveries SET lease_until = clock_timestamp() - interval '1 second' WHERE company_id = $1 AND id = $2")
        .bind(a.company).bind(a.id).execute(f.pool()).await.unwrap();
    let DeliveryClaimOutcome::Claimed(again) = store
        .claim_delivery(Uuid::new_v4(), DELIVERY_LEASE)
        .await
        .unwrap()
    else {
        panic!("an expired lease is reclaimable");
    };
    assert_eq!((again.id, again.fence), (a.id, a.fence + 1));
    assert_eq!(
        store
            .complete_delivery(&a, DeliveryResult::Failed("conflict"))
            .await
            .unwrap(),
        DeliveryCompletion::Lost,
        "the superseded claimant completes nothing"
    );
    assert_eq!(row_by_id(&f, a.id).await.attempts, 1);
    sqlx::query("UPDATE memory_deliveries SET attempts = 7, lease_until = clock_timestamp() - interval '1 second' WHERE company_id = $1 AND id = $2")
        .bind(a.company).bind(a.id).execute(f.pool()).await.unwrap();
    assert!(matches!(
        store
            .claim_delivery(Uuid::new_v4(), DELIVERY_LEASE)
            .await
            .unwrap(),
        DeliveryClaimOutcome::Settled
    ));
    let exhausted = row_by_id(&f, a.id).await;
    assert_eq!(
        (exhausted.state.as_str(), exhausted.outcome_code.as_deref()),
        ("failed", Some("lease_expired"))
    );
    assert_eq!(
        store
            .complete_delivery(&b, DeliveryResult::Failed("conflict"))
            .await
            .unwrap(),
        DeliveryCompletion::Applied
    );
}

/// Disable fails open intents (their waits wake terminally) and owner removal cancels them; a
/// later enable revives nothing.
#[tokio::test]
async fn memory_integration_disable_fails_and_owner_removal_cancels_open_intents() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    ready(&f, &fake, NamespaceOwner::Agent(f.agent)).await;
    ready(&f, &fake, NamespaceOwner::Company).await;
    freeze(
        &f,
        SaveScope::Agent(f.agent),
        "ob.s.agent",
        run(AccessLevel::L0, true),
    )
    .await;
    let mut tx = f.pool().begin().await.unwrap();
    super::retire_namespaces_on(&mut tx, f.company, NamespaceOwner::Agent(f.agent))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let cancelled = row(&f, "ob.s.agent").await;
    assert_eq!(
        (cancelled.state.as_str(), cancelled.outcome_code.as_deref()),
        ("cancelled", Some("owner_removed"))
    );
    freeze(
        &f,
        SaveScope::Company,
        "ob.s.company",
        run(AccessLevel::L0, true),
    )
    .await;
    f.disable().await.unwrap();
    f.enable().await.unwrap();
    let failed = row(&f, "ob.s.company").await;
    assert_eq!(
        (failed.state.as_str(), failed.outcome_code.as_deref()),
        ("failed", Some("disabled"))
    );
    assert!(!pass(&worker(&f, &fake)).await);
    assert!(fake.calls().iter().all(|call| call != "observations/save"));
}

/// The capture of one accepted window into the agent scope (E8), for the lifecycle races.
fn captured_window(f: &Fixture, exchange: Uuid) -> CaptureIntent<'static> {
    CaptureIntent {
        company: f.scope(),
        run: RunId::new(Uuid::new_v4()),
        execution: ExecutionId::new(Uuid::new_v4()),
        exchange,
        agent: f.agent,
        kind: MemoryCaptureKind::Threads,
        scope: SaveScope::Agent(f.agent),
        input: "\"question\"",
        output: "\"answer\"",
    }
}

/// E9: a save freeze, a capture freeze and a disable race on the tenant row, with real
/// concurrent transactions in both orders. Either the freezes commit first and the disable fails
/// both intents, or they wait and find memory disabled. After re-enabling, nothing is delivered.
#[tokio::test]
async fn memory_integration_disable_races_save_and_capture_then_reenable() {
    use crate::application::memory::MemorySettingsStore;
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    ready(&f, &fake, NamespaceOwner::Company).await;
    ready(&f, &fake, NamespaceOwner::Agent(f.agent)).await;
    sqlx::query("UPDATE memory_tenants SET generation_cap = 100000 WHERE company_id = $1")
        .bind(f.company)
        .execute(f.pool())
        .await
        .unwrap();
    fake.state.lock().unwrap().generation_cap = 100_000;
    f.persistence
        .set_memory_capture(
            f.scope(),
            Fixture::actor(f.owner),
            &MemoryCaptureChange {
                agent: f.agent,
                kind: MemoryCaptureKind::Threads,
                enabled: true,
                limits: CaptureLimits::DEFAULT,
                reason: "Race".into(),
                expected_revision: 0,
            },
        )
        .await
        .unwrap();
    let facts = vec!["A fact".to_owned()];

    // Order 1: the freezes hold the tenant FOR SHARE; the disable waits, then fails both intents.
    let mut save = f.pool().begin().await.unwrap();
    let saved = freeze_save_on(
        &mut save,
        &intent(&f, SaveScope::Company, "ob.s.race-first", &facts),
        run(AccessLevel::L0, true),
    )
    .await
    .unwrap();
    assert!(matches!(saved, SaveEnqueue::Enqueued { .. }));
    let first_exchange = Uuid::new_v4();
    let captured = freeze_capture_on(
        &mut save,
        &captured_window(&f, first_exchange),
        run(AccessLevel::L0, false),
    )
    .await
    .unwrap();
    assert!(matches!(captured, CaptureEnqueue::Enqueued { .. }));
    let holder = backend_pid(&mut save).await;
    let disabling = tokio::spawn({
        let persistence = f.persistence.clone();
        let (scope, owner) = (f.scope(), f.owner);
        let revision = f.revision().await;
        async move {
            use crate::application::memory::MemorySettingsStore;
            persistence
                .disable_memory(scope, Fixture::actor(owner), revision)
                .await
        }
    });
    blocked_by(f.pool(), holder).await;
    save.commit().await.unwrap();
    disabling.await.unwrap().unwrap();
    for key in [
        "ob.s.race-first".to_owned(),
        format!("ob.g.{}", first_exchange.simple()),
    ] {
        let failed = row(&f, &key).await;
        assert_eq!(
            (failed.state.as_str(), failed.outcome_code.as_deref()),
            ("failed", Some("disabled")),
            "{key}"
        );
    }

    // Order 2: the disable holds the tenant FOR UPDATE; the freezes wait and find it disabled.
    f.enable().await.unwrap();
    let mut disable = f.pool().begin().await.unwrap();
    super::tenants::disable_on(
        &mut disable,
        f.scope(),
        Fixture::actor(f.owner),
        f.revision().await,
    )
    .await
    .unwrap();
    let holder = backend_pid(&mut disable).await;
    let freezing = tokio::spawn({
        let pool = f.pool().clone();
        let company = f.scope();
        async move {
            let facts = vec!["A fact".to_owned()];
            let mut tx = pool.begin().await.unwrap();
            let intent = SaveIntent {
                company,
                run: RunId::new(Uuid::new_v4()),
                execution: ExecutionId::new(Uuid::new_v4()),
                invocation: None,
                wait: None,
                scope: SaveScope::Company,
                key: "ob.s.race-second",
                facts: &facts,
                external_sources: Vec::new(),
            };
            let saved = freeze_save_on(&mut tx, &intent, run(AccessLevel::L0, true))
                .await
                .unwrap();
            tx.commit().await.unwrap();
            saved
        }
    });
    let capturing = tokio::spawn({
        let pool = f.pool().clone();
        let window = captured_window(&f, Uuid::new_v4());
        async move {
            let mut tx = pool.begin().await.unwrap();
            let captured = freeze_capture_on(&mut tx, &window, run(AccessLevel::L0, false))
                .await
                .unwrap();
            tx.commit().await.unwrap();
            captured
        }
    });
    crate::adapters::persistence::test_support::wait_until_backends_are_blocked(f.pool(), 2).await;
    blocked_by(f.pool(), holder).await;
    disable.commit().await.unwrap();
    assert_eq!(freezing.await.unwrap(), SaveEnqueue::NotEnabled);
    assert_eq!(
        capturing.await.unwrap(),
        CaptureEnqueue::Skipped("not_enabled")
    );

    f.enable().await.unwrap();
    let worker = worker(&f, &fake);
    while pass(&worker).await {}
    let delivered: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM memory_deliveries WHERE company_id = $1 AND state = 'delivered'",
    )
    .bind(f.company)
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert_eq!(delivered, 0);
    assert!(
        fake.calls()
            .iter()
            .all(|call| call != "observations/save" && call != "generations/request")
    );
}

/// F6: an envelope this process cannot open settles its intent (`credential_unavailable`) at the
/// claim instead of rolling back, so it never heads the queue again and others are delivered.
#[tokio::test]
async fn memory_integration_delivery_settles_an_unopenable_credential_poison_batch() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    let poisoned = ready(&f, &fake, NamespaceOwner::Company).await;
    ready(&f, &fake, NamespaceOwner::Agent(f.agent)).await;
    freeze(
        &f,
        SaveScope::Company,
        "ob.s.poisoned",
        run(AccessLevel::L0, true),
    )
    .await;
    freeze(
        &f,
        SaveScope::Agent(f.agent),
        "ob.s.healthy",
        run(AccessLevel::L0, true),
    )
    .await;
    // Same shape, wrong key material: the cipher refuses it.
    let other = crate::adapters::persistence::credentials::CredentialCipher::for_test_with_keys(
        &[(1, [9_u8; 32])],
        1,
    )
    .seal_envelope(
        &CredentialContext::memory_client_credential(f.company, poisoned),
        &SecretString::from("msk_cli_x"),
    )
    .unwrap();
    sqlx::query(
        "UPDATE memory_namespaces SET credential_envelope = $3 WHERE company_id = $1 AND id = $2",
    )
    .bind(f.company)
    .bind(poisoned)
    .bind(other)
    .execute(f.pool())
    .await
    .unwrap();
    let worker = worker(&f, &fake);
    assert!(pass(&worker).await);
    assert!(pass(&worker).await);
    assert!(!pass(&worker).await, "nothing is reclaimed");
    let bad = row(&f, "ob.s.poisoned").await;
    assert_eq!(
        (bad.state.as_str(), bad.outcome_code.as_deref(), bad.request),
        ("failed", Some("credential_unavailable"), None)
    );
    assert_eq!(row(&f, "ob.s.healthy").await.state, "delivered");
}

/// F7 (E1, E8): the claim skips a company that already has a save in flight, so a second lane
/// serves another company even though the busy company's backlog is older; once the busy
/// company's attempt completes, its next intent is claimable again.
#[tokio::test]
async fn memory_integration_delivery_claims_are_fair_across_companies() {
    let Some(mut f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    let busy = f.company;
    ready(&f, &fake, NamespaceOwner::Company).await;
    for index in 0..3 {
        freeze(
            &f,
            SaveScope::Company,
            &format!("ob.s.burst-{index}"),
            run(AccessLevel::L2, true),
        )
        .await;
    }
    sqlx::query("UPDATE memory_deliveries SET available_at = available_at - interval '1 hour' WHERE company_id = $1")
        .bind(busy).execute(f.pool()).await.unwrap();
    let quiet = super::tests::company(&f.persistence, f.owner).await;
    f.company = quiet;
    ready(&f, &fake, NamespaceOwner::Company).await;
    freeze(
        &f,
        SaveScope::Company,
        "ob.s.quiet",
        run(AccessLevel::L2, true),
    )
    .await;
    let store: &dyn MemoryDeliveryStore = &f.persistence;
    let claim = || async {
        match store
            .claim_delivery(Uuid::new_v4(), DELIVERY_LEASE)
            .await
            .unwrap()
        {
            DeliveryClaimOutcome::Claimed(claim) => Some(claim),
            DeliveryClaimOutcome::Empty => None,
            DeliveryClaimOutcome::Settled => panic!("nothing settles here"),
        }
    };
    let first = claim().await.expect("the oldest intent");
    assert_eq!(first.company, busy);
    let second = claim().await.expect("another company's intent");
    assert_eq!(
        second.company, quiet,
        "the busy company's backlog waits for its lane"
    );
    assert!(claim().await.is_none(), "both companies are at their share");
    let retry = DeliveryResult::Retry {
        code: "service_unavailable",
        retry_after: None,
    };
    assert_eq!(
        store.complete_delivery(&first, retry).await.unwrap(),
        DeliveryCompletion::Applied
    );
    let third = claim().await.expect("the busy company's next intent");
    assert_eq!(third.company, busy);
    assert_ne!(third.id, first.id, "the retried intent backs off");
}

/// Delegates to the service client, holding each save until `width` saves are in flight at
/// once (or a second passes, which then fails the test's expectation).
struct Overlap {
    inner: Arc<MemoryServiceHttp>,
    together: Arc<tokio::sync::Barrier>,
}

#[async_trait::async_trait]
impl MemoryClient for Overlap {
    async fn request_generation(
        &self,
        credential: &ClientCredential,
        request: &crate::application::memory::MemoryGenerationRequest,
    ) -> Result<crate::application::memory::MemoryGenerationReceipt, MemoryServiceError> {
        self.inner.request_generation(credential, request).await
    }
    async fn save(
        &self,
        credential: &ClientCredential,
        request: &MemorySaveRequest,
    ) -> Result<MemorySaveReceipt, MemoryServiceError> {
        tokio::time::timeout(Duration::from_secs(1), self.together.wait())
            .await
            .map_err(|_| MemoryServiceError::Unavailable)?;
        self.inner.save(credential, request).await
    }
    async fn search(
        &self,
        credential: &ClientCredential,
        search: &MemorySearch,
    ) -> Result<MemorySearchResult, MemoryServiceError> {
        self.inner.search(credential, search).await
    }
    async fn context_pack(
        &self,
        packs: &[crate::application::memory::ScopedPack<'_>],
    ) -> Result<Vec<crate::application::memory::MemoryPack>, MemoryServiceError> {
        self.inner.context_pack(packs).await
    }
    async fn context_validate(
        &self,
        credential: &ClientCredential,
        namespace: crate::application::memory::NamespaceExternalId,
        read_ceiling: u8,
        items: &[crate::application::memory::PackedRevision],
    ) -> Result<Vec<crate::application::memory::PackItemStatus>, MemoryServiceError> {
        self.inner
            .context_validate(credential, namespace, read_ceiling, items)
            .await
    }
    async fn search_scopes(
        &self,
        searches: &[crate::application::memory::ScopedSearch<'_>],
    ) -> Result<Vec<crate::application::memory::MemorySearchResult>, MemoryServiceError> {
        self.inner.search_scopes(searches).await
    }
    async fn timeline(
        &self,
        credential: &ClientCredential,
        timeline: &crate::application::memory::MemoryTimeline,
    ) -> Result<MemorySearchResult, MemoryServiceError> {
        self.inner.timeline(credential, timeline).await
    }
    async fn fetch(
        &self,
        credential: &ClientCredential,
        namespace: NamespaceExternalId,
        read_ceiling: u8,
        ids: &[Uuid],
    ) -> Result<MemoryFetchResult, MemoryServiceError> {
        self.inner
            .fetch(credential, namespace, read_ceiling, ids)
            .await
    }
}

/// Counts the worker's concurrent store calls; each holds its connection a little longer so
/// unserialized lanes would overlap.
struct Counted {
    inner: crate::adapters::persistence::PostgresPersistence,
    now: std::sync::atomic::AtomicUsize,
    peak: std::sync::atomic::AtomicUsize,
}

impl Counted {
    async fn counted<T>(&self, call: impl std::future::Future<Output = T>) -> T {
        use std::sync::atomic::Ordering::SeqCst;
        let now = self.now.fetch_add(1, SeqCst) + 1;
        self.peak.fetch_max(now, SeqCst);
        tokio::time::sleep(Duration::from_millis(30)).await;
        let result = call.await;
        self.now.fetch_sub(1, SeqCst);
        result
    }
}

#[async_trait::async_trait]
impl MemoryDeliveryStore for Counted {
    async fn claim_delivery(
        &self,
        worker: Uuid,
        lease: Duration,
    ) -> AppResult<DeliveryClaimOutcome> {
        self.counted(self.inner.claim_delivery(worker, lease)).await
    }
    async fn complete_delivery(
        &self,
        claim: &DeliveryClaim,
        result: DeliveryResult,
    ) -> AppResult<DeliveryCompletion> {
        self.counted(self.inner.complete_delivery(claim, result))
            .await
    }
}

/// F7 (E8): the worker sends one save per lane at once. Two companies' intents are delivered
/// only if both saves are in flight together, which a single lane never achieves. F14: the
/// lanes still take turns on the store, so the worker never holds more than one connection.
#[tokio::test]
async fn memory_integration_delivery_lanes_send_companies_concurrently() {
    for lanes in [2, 1] {
        let Some(mut f) = Fixture::new().await else {
            return;
        };
        let fake = ContractFake::start(ADMIN).await;
        ready(&f, &fake, NamespaceOwner::Company).await;
        freeze(
            &f,
            SaveScope::Company,
            "ob.s.lane-a",
            run(AccessLevel::L2, true),
        )
        .await;
        f.company = super::tests::company(&f.persistence, f.owner).await;
        ready(&f, &fake, NamespaceOwner::Company).await;
        freeze(
            &f,
            SaveScope::Company,
            "ob.s.lane-b",
            run(AccessLevel::L2, true),
        )
        .await;
        let client = Arc::new(Overlap {
            inner: http(&fake),
            together: Arc::new(tokio::sync::Barrier::new(2)),
        });
        let store = Arc::new(Counted {
            inner: f.persistence.clone(),
            now: Default::default(),
            peak: Default::default(),
        });
        let worker = MemoryDeliveryWorker::new(store.clone(), client).with_lanes(lanes);
        let stop = CancellationToken::new();
        let delivered = async {
            loop {
                let count: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM memory_deliveries WHERE state = 'delivered'",
                )
                .fetch_one(f.pool())
                .await
                .unwrap();
                if count == 2 {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        };
        let outcome = tokio::select! {
            _ = worker.run(&stop) => unreachable!("the worker runs until stopped"),
            done = tokio::time::timeout(Duration::from_millis(1500), delivered) => done.is_ok(),
        };
        stop.cancel();
        assert_eq!(outcome, lanes == 2, "lanes={lanes}");
        assert_eq!(
            store.peak.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "lanes={lanes}: concurrent store calls"
        );
    }
}

/// F5 (01 D4): a frozen intent whose trusted ceiling is missing is never sent; the claim settles
/// it as invalid instead of letting the service guess a level.
#[tokio::test]
async fn memory_access_level_delivery_refuses_an_intent_without_its_trusted_ceiling() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    ready(&f, &fake, NamespaceOwner::Company).await;
    freeze(
        &f,
        SaveScope::Company,
        "ob.s.ceiling",
        run(AccessLevel::L4, true),
    )
    .await;
    let tampered = sqlx::query(
        "UPDATE memory_deliveries SET request = request #- '{body,context,producing_ceiling}' \
         WHERE company_id = $1 AND idempotency_key = 'ob.s.ceiling' \
           AND request->'body'->'context' ? 'producing_ceiling'",
    )
    .bind(f.company)
    .execute(f.pool())
    .await
    .unwrap();
    assert_eq!(tampered.rows_affected(), 1);
    assert!(pass(&worker(&f, &fake)).await);
    let refused = row(&f, "ob.s.ceiling").await;
    assert_eq!(
        (
            refused.state.as_str(),
            refused.outcome_code.as_deref(),
            refused.request
        ),
        ("failed", Some("invalid_intent"), None)
    );
    assert!(fake.calls().iter().all(|call| call != "observations/save"));
}

/// F5 (E9): owner removal and a save freeze race on the namespace row, with real concurrent
/// transactions in both orders. Either the freeze commits first and the removal cancels its
/// intent, or the freeze waits out the removal and finds the namespace not ready.
#[tokio::test]
async fn memory_integration_owner_removal_races_save_freeze_in_both_orders() {
    for freeze_first in [true, false] {
        let Some(f) = Fixture::new().await else {
            return;
        };
        let fake = ContractFake::start(ADMIN).await;
        ready(&f, &fake, NamespaceOwner::Agent(f.agent)).await;
        let facts = vec!["A fact".to_owned()];
        let (company, agent, pool) = (f.scope(), f.agent, f.pool().clone());
        if freeze_first {
            let mut save = f.pool().begin().await.unwrap();
            let saved = freeze_save_on(
                &mut save,
                &intent(&f, SaveScope::Agent(agent), "ob.s.owner-race", &facts),
                run(AccessLevel::L0, true),
            )
            .await
            .unwrap();
            assert!(matches!(saved, SaveEnqueue::Enqueued { .. }));
            let holder = backend_pid(&mut save).await;
            let removing = tokio::spawn(async move {
                let mut tx = pool.begin().await.unwrap();
                super::retire_namespaces_on(
                    &mut tx,
                    company.as_uuid(),
                    NamespaceOwner::Agent(agent),
                )
                .await
                .unwrap();
                tx.commit().await.unwrap();
            });
            blocked_by(f.pool(), holder).await;
            save.commit().await.unwrap();
            removing.await.unwrap();
            let cancelled = row(&f, "ob.s.owner-race").await;
            assert_eq!(
                (
                    cancelled.state.as_str(),
                    cancelled.outcome_code.as_deref(),
                    cancelled.request
                ),
                ("cancelled", Some("owner_removed"), None)
            );
        } else {
            let mut removal = f.pool().begin().await.unwrap();
            super::retire_namespaces_on(&mut removal, f.company, NamespaceOwner::Agent(agent))
                .await
                .unwrap();
            let holder = backend_pid(&mut removal).await;
            let freezing = tokio::spawn(async move {
                let facts = vec!["A fact".to_owned()];
                let mut tx = pool.begin().await.unwrap();
                let intent = SaveIntent {
                    company,
                    run: RunId::new(Uuid::new_v4()),
                    execution: ExecutionId::new(Uuid::new_v4()),
                    invocation: None,
                    wait: None,
                    scope: SaveScope::Agent(agent),
                    key: "ob.s.owner-race",
                    facts: &facts,
                    external_sources: Vec::new(),
                };
                let saved = freeze_save_on(&mut tx, &intent, run(AccessLevel::L0, true))
                    .await
                    .unwrap();
                tx.commit().await.unwrap();
                saved
            });
            blocked_by(f.pool(), holder).await;
            removal.commit().await.unwrap();
            assert_eq!(freezing.await.unwrap(), SaveEnqueue::NotReady);
            let intents: i64 = sqlx::query_scalar("SELECT count(*) FROM memory_deliveries")
                .fetch_one(f.pool())
                .await
                .unwrap();
            assert_eq!(intents, 0);
        }
        assert!(
            !pass(&worker(&f, &fake)).await,
            "freeze_first={freeze_first}"
        );
        assert!(fake.calls().iter().all(|call| call != "observations/save"));
    }
}

/// F5 (E9): disabling memory while a claimed save is in flight settles the intent at once; the
/// worker's completion then finds its fence gone and changes nothing (`Lost`), even though the
/// service accepted the request.
#[tokio::test]
async fn memory_integration_disable_during_an_in_flight_delivery_loses_its_completion() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    ready(&f, &fake, NamespaceOwner::Company).await;
    freeze(
        &f,
        SaveScope::Company,
        "ob.s.in-flight",
        run(AccessLevel::L0, true),
    )
    .await;
    fake.state
        .lock()
        .unwrap()
        .delays
        .insert("observations/save".into(), Duration::from_millis(700));
    let store = f.persistence.clone();
    let worker = MemoryDeliveryWorker::new(Arc::new(store.clone()), http(&fake));
    let sending = tokio::spawn(async move { worker.pass(&CancellationToken::new()).await });
    tokio::time::timeout(Duration::from_secs(5), async {
        while row(&f, "ob.s.in-flight").await.state != "claimed" {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the worker claims the intent");
    f.disable().await.unwrap();
    assert!(sending.await.unwrap().unwrap());
    let settled = row(&f, "ob.s.in-flight").await;
    assert_eq!(
        (
            settled.state.as_str(),
            settled.outcome_code.as_deref(),
            settled.result,
            settled.request
        ),
        ("failed", Some("disabled"), None, None),
        "the completion was fenced out"
    );
    assert_eq!(
        fake.calls()
            .iter()
            .filter(|call| *call == "observations/save")
            .count(),
        1
    );
}

/// Review MEM-P05-B2-F16: a completion waiting on a row another transaction holds (a disable,
/// owner removal or run-cancel settling it) gives up after the claim's 2 s lock timeout and changes
/// nothing, so it cannot stall every lane behind the store mutex; after its lease the intent is
/// reclaimed and delivered with the same key.
#[tokio::test]
async fn memory_integration_a_completion_behind_a_row_lock_gives_up_and_is_reclaimed() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    ready(&f, &fake, NamespaceOwner::Company).await;
    freeze(
        &f,
        SaveScope::Company,
        "ob.s.locked-completion",
        run(AccessLevel::L0, true),
    )
    .await;
    let DeliveryClaimOutcome::Claimed(claim) = f
        .persistence
        .claim_delivery(Uuid::new_v4(), Duration::from_secs(60))
        .await
        .unwrap()
    else {
        panic!("claimed");
    };
    let mut holder = f.pool().begin().await.unwrap();
    sqlx::query("SELECT 1 FROM memory_deliveries WHERE id = $1 FOR UPDATE")
        .bind(claim.id)
        .execute(&mut *holder)
        .await
        .unwrap();
    let started = std::time::Instant::now();
    // Bounded here too, so a completion that waits on the lock fails the test instead of hanging it.
    let completion = tokio::time::timeout(
        Duration::from_secs(10),
        f.persistence
            .complete_delivery(&claim, DeliveryResult::Failed("refused")),
    )
    .await
    .expect("the completion gave up on the lock")
    .unwrap();
    assert_eq!(completion, DeliveryCompletion::Lost);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "bounded by the lock timeout: {:?}",
        started.elapsed()
    );
    holder.rollback().await.unwrap();
    assert_eq!(row(&f, "ob.s.locked-completion").await.state, "claimed");
    sqlx::query(
        "UPDATE memory_deliveries SET lease_until = clock_timestamp() - interval '1 second' \
         WHERE id = $1",
    )
    .bind(claim.id)
    .execute(f.pool())
    .await
    .unwrap();
    assert!(pass(&worker(&f, &fake)).await);
    assert_eq!(row(&f, "ob.s.locked-completion").await.state, "delivered");
}

/// Review MEM-P05-B2-F16: a pass that starts after shutdown claims nothing, so no intent is
/// released as `interrupted` and no transient retry is spent.
#[tokio::test]
async fn memory_integration_a_stopped_worker_claims_nothing() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    ready(&f, &fake, NamespaceOwner::Company).await;
    freeze(
        &f,
        SaveScope::Company,
        "ob.s.stopped",
        run(AccessLevel::L0, true),
    )
    .await;
    let stop = CancellationToken::new();
    stop.cancel();
    assert!(!worker(&f, &fake).pass(&stop).await.unwrap());
    let (state, retries): (String, i32) = sqlx::query_as(
        "SELECT state, transient_retries FROM memory_deliveries WHERE idempotency_key = 'ob.s.stopped'",
    )
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert_eq!((state.as_str(), retries), ("pending", 0));
    assert!(fake.calls().iter().all(|call| call != "observations/save"));
}
