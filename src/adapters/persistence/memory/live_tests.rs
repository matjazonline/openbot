//! Live memory-service integration (phase 05 E11), run by `scripts/memory-integration.sh`
//! against the real service it builds and serves (`MEMORY_LIVE_URL`), with the provisioner and
//! operator credentials it issues (`MEMORY_LIVE_PROVISIONER_TOKEN`, `MEMORY_LIVE_OPERATOR_TOKEN`)
//! and a loopback stand-in for the Messages API behind the service's generation worker. OpenBot's
//! own lifecycle and delivery workers, save and capture freezes and client reads run unchanged.
//! `#[ignore]`: without that service they cannot run.
use super::deliveries::*;
use super::tests::Fixture;
use crate::adapters::memory_service::MemoryServiceHttp;
use crate::adapters::persistence::credentials::envelope::CredentialContext;
use crate::application::memory::*;
use crate::domain::workflow::{ExecutionId, RunId};
use crate::entities::access_level::AccessLevel;
use crate::infra::memory_config::{MemoryServiceConfig, ProvisionerToken};
use secrecy::SecretString;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub(crate) struct Live {
    pub(crate) url: String,
    pub(crate) provisioner: ProvisionerToken,
    pub(crate) operator: String,
}

pub(crate) fn live() -> Live {
    let var = |name: &str| {
        std::env::var(name)
            .unwrap_or_else(|_| panic!("{name} is set by scripts/memory-integration.sh"))
    };
    Live {
        url: var("MEMORY_LIVE_URL"),
        provisioner: ProvisionerToken::parse(SecretString::from(var(
            "MEMORY_LIVE_PROVISIONER_TOKEN",
        )))
        .unwrap(),
        operator: var("MEMORY_LIVE_OPERATOR_TOKEN"),
    }
}

pub(crate) fn client(live: &Live) -> Arc<MemoryServiceHttp> {
    Arc::new(
        MemoryServiceHttp::new(&MemoryServiceConfig {
            base_url: live.url.clone(),
            provisioner: live.provisioner.clone(),
            request_timeout: Duration::from_secs(10),
            recall_deadline: Duration::from_secs(3),
            interactive_in_flight: 2,
            background_in_flight: 2,
        })
        .unwrap(),
    )
}

/// The reconciler until nothing is due (bounded).
async fn reconcile(f: &Fixture, live: &Live, http: &Arc<MemoryServiceHttp>) {
    reconcile_with(&f.persistence, live, http).await;
}

pub(crate) async fn reconcile_with(
    persistence: &crate::adapters::persistence::PostgresPersistence,
    live: &Live,
    http: &Arc<MemoryServiceHttp>,
) {
    let worker = MemoryLifecycleWorker::new(
        Arc::new(persistence.clone()),
        http.clone(),
        live.provisioner.credential_id(),
    );
    for _ in 0..40 {
        if !worker.pass(&CancellationToken::new()).await.unwrap() {
            return;
        }
    }
    panic!("the reconciler never ran out of due rows");
}

async fn deliver(f: &Fixture, http: &Arc<MemoryServiceHttp>) {
    let worker = MemoryDeliveryWorker::new(Arc::new(f.persistence.clone()), http.clone());
    for _ in 0..20 {
        if !worker.pass(&CancellationToken::new()).await.unwrap() {
            return;
        }
    }
    panic!("the delivery worker never ran out of due intents");
}

/// The ready namespace of `kind` and its opened client credential.
async fn ready(f: &Fixture, kind: &str) -> (Uuid, ClientCredential) {
    let (namespace, id, envelope): (Uuid, Uuid, Option<String>) = sqlx::query_as(
        "SELECT id, credential_id, credential_envelope FROM memory_namespaces \
         WHERE company_id = $1 AND scope_kind = $2 AND state = 'ready'",
    )
    .bind(f.company)
    .bind(kind)
    .fetch_one(f.pool())
    .await
    .unwrap_or_else(|_| panic!("a ready {kind} namespace"));
    let token = f
        .persistence
        .credential_cipher()
        .unwrap()
        .open_envelope(
            &CredentialContext::memory_client_credential(f.company, namespace),
            &envelope.unwrap(),
        )
        .unwrap();
    (
        namespace,
        ClientCredential {
            tenant: TenantExternalId(f.company),
            id,
            token,
        },
    )
}

async fn delivery(f: &Fixture, key: &str) -> (String, Option<Value>) {
    let (state, outcome, result): (String, Option<String>, Option<Value>) = sqlx::query_as(
        "SELECT state, outcome_code, result FROM memory_deliveries \
         WHERE company_id = $1 AND idempotency_key = $2",
    )
    .bind(f.company)
    .bind(key)
    .fetch_one(f.pool())
    .await
    .unwrap();
    // The outcome travels with the state, so a failed intent says why.
    (
        outcome.map_or(state.clone(), |outcome| format!("{state}({outcome})")),
        result,
    )
}

pub(crate) async fn eventually<T>(what: &str, mut probe: impl AsyncFnMut() -> Option<T>) -> T {
    for _ in 0..240 {
        if let Some(found) = probe().await {
            return found;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    panic!("never: {what}");
}

/// The service's own API, for what OpenBot does not call (operator settings, generation status).
pub(crate) async fn call(live: &Live, path: &str, bearer: &str, body: Value) -> Value {
    let answer = reqwest::Client::new()
        .post(format!("{}/{path}", live.url.trim_end_matches('/')))
        .bearer_auth(bearer)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert!(answer.status().is_success(), "{path}: {}", answer.status());
    answer.json().await.unwrap()
}

/// E1, E2, E4, E7, E8, E9 end to end: enable provisions the tenant and namespaces with real
/// client credentials; a frozen save is delivered once and is then found by search, fetch and
/// pack, and validates; after the operator raises the tenant's generation cap, the documented
/// disable/enable refreshes OpenBot's cached cap; a capture is delivered as a generation request,
/// the service's generation worker calls the Messages API and the generated memory is labelled at
/// the producing ceiling; disabling memory disables the tenant, so reads are refused and new
/// freezes are not enabled.
#[tokio::test]
#[ignore = "needs the live memory service: scripts/memory-integration.sh"]
async fn memory_integration_live_provisioning_save_recall_capture_and_disable() {
    let live = live();
    let http = client(&live);
    let f = Fixture::new().await.expect("TEST_DATABASE_URL");
    let owner = Fixture::actor(f.owner);
    let provisioner = live.provisioner.credential_id();
    f.persistence
        .enable_memory(f.scope(), owner, 0, provisioner)
        .await
        .unwrap();
    for owner in [NamespaceOwner::Company, NamespaceOwner::Agent(f.agent)] {
        assert_eq!(
            f.persistence
                .ensure_namespace(f.scope(), owner)
                .await
                .unwrap(),
            NamespaceReadiness::NotReady
        );
    }
    reconcile(&f, &live, &http).await;
    let (company_ns, company) = ready(&f, "company").await;
    let (agent_ns, agent) = ready(&f, "agent").await;

    // A frozen save, delivered once and replayed as the same receipt.
    let facts =
        vec!["LIVE-SAVED renewal is due in March.\nThe customer asked for net 30.".to_owned()];
    let intent = SaveIntent {
        company: f.scope(),
        run: RunId::new(Uuid::new_v4()),
        execution: ExecutionId::new(Uuid::new_v4()),
        invocation: None,
        wait: None,
        scope: SaveScope::Company,
        key: "ob.s.live-save",
        facts: &facts,
        external_sources: Vec::new(),
    };
    let levels = LockedRun {
        ceiling: AccessLevel::L6,
        content: AccessLevel::L2,
        content_exact: true,
    };
    let mut tx = f.pool().begin().await.unwrap();
    assert!(matches!(
        freeze_save_on(&mut tx, &intent, levels).await.unwrap(),
        SaveEnqueue::Enqueued { .. }
    ));
    tx.commit().await.unwrap();
    deliver(&f, &http).await;
    let (state, result) = delivery(&f, "ob.s.live-save").await;
    assert_eq!(state, "delivered");
    let saved: Uuid =
        serde_json::from_value(result.unwrap()["observations"][0]["observation_id"].clone())
            .unwrap();

    // Recall surfaces: search (once indexed), fetch, pack and validate under the ceiling.
    let hits = eventually("the saved memory is searchable", async || {
        let found = http
            .search(
                &company,
                &MemorySearch {
                    namespace: NamespaceExternalId(company_ns),
                    read_ceiling: 6,
                    query: "renewal net 30".into(),
                    limit: 5,
                },
            )
            .await
            .unwrap();
        found
            .items
            .iter()
            .any(|hit| hit.observation_id == saved)
            .then_some(found)
    })
    .await;
    let hit = hits
        .items
        .iter()
        .find(|hit| hit.observation_id == saved)
        .unwrap();
    assert_eq!(
        hit.access_level, 6,
        "company scope: authored input is inexact there (U2), so consumed is unknown and the \
         save is labelled at the producing ceiling"
    );
    let fetched = http
        .fetch(&company, NamespaceExternalId(company_ns), 6, &[saved])
        .await
        .unwrap();
    assert!(fetched.items[0].body.contains("LIVE-SAVED"));
    let below = http
        .fetch(&company, NamespaceExternalId(company_ns), 1, &[saved])
        .await
        .unwrap();
    assert!(
        below.items.is_empty(),
        "above a lower ceiling the item is absent"
    );
    let packs = http
        .context_pack(&[ScopedPack {
            credential: &company,
            pack: MemoryPackRequest {
                namespace: NamespaceExternalId(company_ns),
                read_ceiling: 6,
                query: Some("renewal".into()),
                max_tokens: 4096,
                max_bytes: 65_536,
            },
        }])
        .await
        .unwrap();
    let item = packs[0]
        .items
        .iter()
        .find(|item| item.observation_id == saved)
        .expect("packed");
    let statuses = http
        .context_validate(
            &company,
            NamespaceExternalId(company_ns),
            6,
            &[PackedRevision {
                observation_id: item.observation_id,
                revision: item.revision,
                sources_sha256: item.sources_sha256.clone(),
            }],
        )
        .await
        .unwrap();
    assert_eq!(statuses, vec![PackItemStatus::Valid]);

    // The operator raises the cap; the documented disable/enable refreshes the cached cap.
    let tenant = f.company.hyphenated().to_string();
    call(
        &live,
        "admin/v1/tenants/settings",
        &live.operator,
        json!({"tenant": tenant, "generation_daily_token_cap": 100_000}),
    )
    .await;
    f.persistence
        .disable_memory(f.scope(), owner, f.revision().await)
        .await
        .unwrap();
    f.persistence
        .enable_memory(f.scope(), owner, f.revision().await, provisioner)
        .await
        .unwrap();
    reconcile(&f, &live, &http).await;
    let cap: Option<i64> =
        sqlx::query_scalar("SELECT generation_cap FROM memory_tenants WHERE company_id = $1")
            .bind(f.company)
            .fetch_one(f.pool())
            .await
            .unwrap();
    assert_eq!(cap, Some(100_000));

    // A capture of the agent's thread window, delivered as a generation request.
    f.persistence
        .set_memory_capture(
            f.scope(),
            owner,
            &MemoryCaptureChange {
                agent: f.agent,
                kind: MemoryCaptureKind::Threads,
                enabled: true,
                limits: CaptureLimits::DEFAULT,
                reason: "Live capture".into(),
                expected_revision: 0,
            },
        )
        .await
        .unwrap();
    let exchange = Uuid::new_v4();
    let window = CaptureIntent {
        company: f.scope(),
        run: RunId::new(Uuid::new_v4()),
        execution: ExecutionId::new(Uuid::new_v4()),
        exchange,
        agent: f.agent,
        kind: MemoryCaptureKind::Threads,
        scope: SaveScope::Agent(f.agent),
        input: r#"{"message":"Please renew the contract on net 30 terms."}"#,
        output: r#"{"body":"Renewal booked for March on net 30."}"#,
    };
    let mut tx = f.pool().begin().await.unwrap();
    let captured = freeze_capture_on(
        &mut tx,
        &window,
        LockedRun {
            ceiling: AccessLevel::L6,
            content: AccessLevel::L0,
            content_exact: false,
        },
    )
    .await
    .unwrap();
    assert!(
        matches!(captured, CaptureEnqueue::Enqueued { .. }),
        "{captured:?}"
    );
    tx.commit().await.unwrap();
    deliver(&f, &http).await;
    let key = format!("ob.g.{}", exchange.simple());
    let (state, result) = delivery(&f, &key).await;
    assert_eq!(state, "delivered");
    assert!(result.unwrap()["operation_id"].as_str().is_some());
    // The service's generation worker called the Messages API stand-in; its memory appears in the
    // agent's namespace (found through search: OpenBot's credentials are not granted status).
    let generated = eventually("the generated memory is searchable", async || {
        let found = http
            .search(
                &agent,
                &MemorySearch {
                    namespace: NamespaceExternalId(agent_ns),
                    read_ceiling: 6,
                    query: "LIVE-GENERATED summarized".into(),
                    limit: 5,
                },
            )
            .await
            .unwrap();
        (!found.items.is_empty()).then_some(found)
    })
    .await;
    let ids: Vec<Uuid> = generated
        .items
        .iter()
        .map(|hit| hit.observation_id)
        .collect();
    let details = http
        .fetch(&agent, NamespaceExternalId(agent_ns), 6, &ids)
        .await
        .unwrap();
    assert!(!details.items.is_empty());
    assert!(
        details
            .items
            .iter()
            .all(|detail| detail.body.contains("LIVE-GENERATED")),
        "{details:?}"
    );
    assert!(
        details.items.iter().all(|detail| detail.access_level == 6),
        "unknown consumed: labelled at the producing ceiling"
    );
    // 06B.2 provenance (review B3 F2a): each generated memory cites both captured parts, input
    // and output, as recorded source revisions. OpenBot's `MemoryDetail` does not carry sources,
    // so the service's own API is read with the same credential.
    let token = secrecy::ExposeSecret::expose_secret(&agent.token).to_owned();
    let raw = call(
        &live,
        "v1/observations/fetch",
        &token,
        json!({"namespaces": [agent_ns.hyphenated().to_string()], "read_ceiling": 6, "ids": ids}),
    )
    .await;
    let items = raw["items"].as_array().unwrap();
    assert_eq!(items.len(), ids.len());
    for item in items {
        let sources = item["sources"].as_array().unwrap();
        let distinct: std::collections::HashSet<&str> = sources
            .iter()
            .map(|source| source["source_id"].as_str().unwrap())
            .collect();
        assert_eq!(distinct.len(), 2, "both captured parts are cited: {item}");
        assert!(
            sources
                .iter()
                .all(|source| source["kind"] == "source" && source["revision"] == "1"),
            "{item}"
        );
    }

    // Disable: the tenant is disabled remotely, reads are refused and nothing new freezes.
    f.persistence
        .disable_memory(f.scope(), owner, f.revision().await)
        .await
        .unwrap();
    reconcile(&f, &live, &http).await;
    let refused = http
        .fetch(&company, NamespaceExternalId(company_ns), 6, &[saved])
        .await
        .unwrap_err();
    assert!(
        matches!(refused, MemoryServiceError::Forbidden { ref code } if code == "tenant_disabled"),
        "{refused:?}"
    );
    let mut tx = f.pool().begin().await.unwrap();
    let again = SaveIntent {
        key: "ob.s.live-after-disable",
        ..intent
    };
    assert_eq!(
        freeze_save_on(&mut tx, &again, levels).await.unwrap(),
        SaveEnqueue::NotEnabled
    );
    tx.rollback().await.unwrap();
}

/// E8 with competing claimants against the real service: two delivery workers race for the same
/// company's intents; every intent is delivered exactly once and a replay keeps its receipt.
#[tokio::test]
#[ignore = "needs the live memory service: scripts/memory-integration.sh"]
async fn memory_integration_live_competing_delivery_workers_deliver_each_intent_once() {
    let live = live();
    let http = client(&live);
    let f = Fixture::new().await.expect("TEST_DATABASE_URL");
    f.persistence
        .enable_memory(
            f.scope(),
            Fixture::actor(f.owner),
            0,
            live.provisioner.credential_id(),
        )
        .await
        .unwrap();
    assert_eq!(
        f.persistence
            .ensure_namespace(f.scope(), NamespaceOwner::Company)
            .await
            .unwrap(),
        NamespaceReadiness::NotReady
    );
    reconcile(&f, &live, &http).await;
    let keys: Vec<String> = (0..4)
        .map(|index| format!("ob.s.live-race-{index}"))
        .collect();
    for key in &keys {
        let facts = vec![format!("LIVE-RACE fact {key}")];
        let mut tx = f.pool().begin().await.unwrap();
        let intent = SaveIntent {
            company: f.scope(),
            run: RunId::new(Uuid::new_v4()),
            execution: ExecutionId::new(Uuid::new_v4()),
            invocation: None,
            wait: None,
            scope: SaveScope::Company,
            key,
            facts: &facts,
            external_sources: Vec::new(),
        };
        let levels = LockedRun {
            ceiling: AccessLevel::L3,
            content: AccessLevel::L1,
            content_exact: true,
        };
        assert!(matches!(
            freeze_save_on(&mut tx, &intent, levels).await.unwrap(),
            SaveEnqueue::Enqueued { .. }
        ));
        tx.commit().await.unwrap();
    }
    let workers = [
        Arc::new(MemoryDeliveryWorker::new(
            Arc::new(f.persistence.clone()),
            http.clone(),
        )),
        Arc::new(MemoryDeliveryWorker::new(
            Arc::new(f.persistence.clone()),
            http.clone(),
        )),
    ];
    let racing: Vec<_> = workers
        .iter()
        .map(|worker| {
            let worker = worker.clone();
            tokio::spawn(async move {
                let mut passes = 0;
                while worker.pass(&CancellationToken::new()).await.unwrap() {
                    passes += 1;
                    assert!(passes < 20, "never ran out of due intents");
                }
            })
        })
        .collect();
    for handle in racing {
        handle.await.unwrap();
    }
    let (_, company) = ready(&f, "company").await;
    let mut observations = Vec::new();
    for key in &keys {
        let (state, result) = delivery(&f, key).await;
        assert_eq!(state, "delivered", "{key}");
        let result = result.unwrap();
        assert_eq!(result["replayed"], json!(false), "{key}: sent once");
        observations.push(
            serde_json::from_value::<Uuid>(result["observations"][0]["observation_id"].clone())
                .unwrap(),
        );
    }
    let (namespace, _) = ready(&f, "company").await;
    let fetched = http
        .fetch(&company, NamespaceExternalId(namespace), 3, &observations)
        .await
        .unwrap();
    assert_eq!(
        fetched.items.len(),
        keys.len(),
        "one observation per intent"
    );
}

#[path = "e2e_tests.rs"]
mod e2e;
