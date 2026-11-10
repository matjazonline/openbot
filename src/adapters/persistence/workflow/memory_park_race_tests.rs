//! Phase 05 E6 on the production park entry (F4): `memory.save`'s park against an in-flight
//! classification rise of the same run, with real competing transactions in both orders. The
//! save is agent-scoped, so an exact run sends its exact level and the race discriminates the
//! frozen value (a company-scope save always sends `unknown` under U2).
use super::*;
use crate::adapters::memory_service::MemoryServiceHttp;
use crate::adapters::memory_service::contract_fake::ContractFake;
use crate::adapters::persistence::credentials::CredentialContext;
use crate::adapters::persistence::workflow::knowledge;
use crate::application::memory::MemoryNamespaces;
use crate::application::memory::{MemorySettingsStore, NamespaceOwner, NamespaceReadiness};
use crate::application::workflow::activation::ActivationRequest;
use crate::application::workflow::memory::MemoryStepRuntime;
use crate::entities::access_level::AccessLevel;
use crate::infra::memory_config::{MemoryServiceConfig, ProvisionerToken};
use secrecy::SecretString;

const ADMIN: &str =
    "msk_adm_0190f1a2b3c44d5e8f9a0b1c2d3e4f50_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const PROVISIONER: Uuid = Uuid::from_u128(0x0190f1a2_b3c4_4d5e_8f9a_0b1c2d3e4f50);

/// `start` saves to the assistant's memory, then the agent step that makes it the workflow's.
async fn agent_save_setup() -> AgentSetup {
    Box::pin(agent_setup_source(
        "http://127.0.0.1:1",
        None,
        false,
        |source, _| {
            source["steps"]["run"] = source["steps"]["start"].clone();
            source["steps"]["start"] = json!({"type":"memory.save","with":{
                "scope":{"literal":{"kind":"agent","agent":"assistant"}},
                "facts":{"literal":["The customer prefers concise replies."]}},
                "routes":{"success":"run"}});
        },
    ))
    .await
}

/// Memory enabled and the assistant's namespace ready under a token the fake grants.
async fn ready(setup: &AgentSetup) -> (ContractFake, Arc<MemoryServiceHttp>) {
    let p = &setup.fixture.persistence;
    let company = setup.fixture.target.company;
    let fake = ContractFake::start(ADMIN).await;
    p.enable_memory(company, setup.fixture.target.actor, 0, PROVISIONER)
        .await
        .unwrap();
    assert_eq!(
        p.ensure_namespace(company, NamespaceOwner::Agent(setup.agent))
            .await
            .unwrap(),
        NamespaceReadiness::NotReady
    );
    let namespace: Uuid = sqlx::query_scalar(
        "SELECT id FROM memory_namespaces WHERE company_id=$1 AND scope_kind='agent' AND agent_id=$2",
    )
    .bind(company.as_uuid())
    .bind(setup.agent)
    .fetch_one(p.pool())
    .await
    .unwrap();
    let token = fake.client_token(company.as_uuid(), namespace);
    let envelope = p
        .credential_cipher()
        .unwrap()
        .seal_envelope(
            &CredentialContext::memory_client_credential(company.as_uuid(), namespace),
            &SecretString::from(token),
        )
        .unwrap();
    sqlx::query("UPDATE memory_namespaces SET state='ready',credential_id=gen_random_uuid(),credential_envelope=$3 WHERE company_id=$1 AND id=$2")
        .bind(company.as_uuid()).bind(namespace).bind(envelope).execute(p.pool()).await.unwrap();
    let client = Arc::new(
        MemoryServiceHttp::new(&MemoryServiceConfig {
            base_url: fake.base_url.clone(),
            provisioner: ProvisionerToken::parse(SecretString::from(ADMIN)).unwrap(),
            request_timeout: Duration::from_secs(5),
            recall_deadline: Duration::from_secs(1),
            interactive_in_flight: 2,
            background_in_flight: 2,
        })
        .unwrap(),
    );
    (fake, client)
}

/// The backend waiting on `blocker`'s locks, once one is.
pub(super) async fn waiter_of(pool: &sqlx::PgPool, blocker: i32) -> i32 {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: Option<i32> = sqlx::query_scalar(
                "SELECT pid FROM pg_stat_activity \
                 WHERE datname = current_database() AND $1 = ANY(pg_blocking_pids(pid)) LIMIT 1",
            )
            .bind(blocker)
            .fetch_optional(pool)
            .await
            .unwrap();
            if let Some(pid) = waiting {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("a backend waits on the holder")
}

pub(super) async fn raise(
    pool: &sqlx::PgPool,
    scope: ActivationRequest,
    level: AccessLevel,
) -> sqlx::Transaction<'static, sqlx::Postgres> {
    let mut tx = pool.begin().await.unwrap();
    knowledge::raise_content_level_on(&mut tx, scope.company, scope.run, level)
        .await
        .unwrap();
    tx
}

pub(super) async fn pid(tx: &mut sqlx::Transaction<'static, sqlx::Postgres>) -> i32 {
    sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut **tx)
        .await
        .unwrap()
}

/// F4 (E6): rise first — the park (`park_on`: run facts FOR UPDATE, fence, freeze) waits on the
/// in-flight rise 4 → 6 and freezes 6. Park first — held after it took the run lock (it waits
/// on the tenant row) — the rise waits for the park, the intent keeps 4, and the run moves to 6
/// afterwards. The inexact run freezes `unknown` in both orders.
#[tokio::test]
async fn memory_access_level_competing_rise_and_parked_save_freeze_the_committed_level() {
    for exact in [true, false] {
        for rise_first in [true, false] {
            let setup = agent_save_setup().await;
            let (fake, client) = ready(&setup).await;
            let p = setup.fixture.persistence.clone();
            let pool = p.pool().clone();
            let scope = setup.claim.fence.scope;
            if exact {
                knowledge::exactness_fixture::mark_exact(&pool, scope.company, scope.run).await;
            } else {
                let mut tx = pool.begin().await.unwrap();
                knowledge::lower_content_exact_on(&mut tx, scope.company, scope.run)
                    .await
                    .unwrap();
                tx.commit().await.unwrap();
            }
            raise(&pool, scope, AccessLevel::L4)
                .await
                .commit()
                .await
                .unwrap();
            let runtime = MemoryStepRuntime::new(p.clone(), client.clone());
            let claim = setup.claim.clone();
            let spawn_park = move || {
                tokio::spawn(async move {
                    runtime
                        .execute(&WorkflowStepKind("memory.save".into()), &claim)
                        .await
                })
            };
            let (park, expected) = if rise_first {
                let mut rise = raise(&pool, scope, AccessLevel::L6).await;
                let holder = pid(&mut rise).await;
                let park = spawn_park();
                waiter_of(&pool, holder).await;
                rise.commit().await.unwrap();
                (park, 6)
            } else {
                let mut tenant = pool.begin().await.unwrap();
                sqlx::query("SELECT 1 FROM memory_tenants WHERE company_id=$1 FOR UPDATE")
                    .bind(scope.company.as_uuid())
                    .execute(&mut *tenant)
                    .await
                    .unwrap();
                let tenant_holder = pid(&mut tenant).await;
                let park = spawn_park();
                let parker = waiter_of(&pool, tenant_holder).await;
                let rising = tokio::spawn({
                    let pool = pool.clone();
                    async move {
                        raise(&pool, scope, AccessLevel::L6)
                            .await
                            .commit()
                            .await
                            .unwrap()
                    }
                });
                // The rise waits on the run row the park holds.
                waiter_of(&pool, parker).await;
                tenant.commit().await.unwrap();
                rising.await.unwrap();
                (park, 4)
            };
            let WorkflowHandlerOutcome::Suspended(parked) = park.await.unwrap() else {
                panic!("memory.save parks");
            };
            let consumed: Value = sqlx::query_scalar("SELECT request->'body'->'context'->'consumed' FROM memory_deliveries WHERE company_id=$1 AND wait_id=$2")
                .bind(scope.company.as_uuid()).bind(parked.wait.as_uuid()).fetch_one(&pool).await.unwrap();
            let want = if exact {
                json!({"kind":"exact","level":expected})
            } else {
                json!({"kind":"unknown"})
            };
            assert_eq!(consumed, want, "exact={exact} rise_first={rise_first}");
            let level: i16 = sqlx::query_scalar(
                "SELECT content_level::smallint FROM workflow_runs WHERE company_id=$1 AND id=$2",
            )
            .bind(scope.company.as_uuid())
            .bind(scope.run.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(level, 6, "the rise committed in both orders");
            assert!(fake.calls().iter().all(|call| call != "observations/save"));
        }
    }
}
