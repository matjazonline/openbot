//! `memory.save` and `memory.load` through their real handler, persistence, delivery worker and
//! effect wait (phase 05 E3, E4, E6, E8), against the contract fake. The fixture's database is
//! its own, so the delivery worker's unscoped claim sees only these intents.
use super::*;
use crate::adapters::memory_service::MemoryServiceHttp;
use crate::adapters::memory_service::contract_fake::ContractFake;
use crate::adapters::persistence::credentials::{CredentialCipher, CredentialContext};
use crate::application::memory::{
    ClientCredential, MemoryClient, MemoryDeliveryWorker, MemoryNamespaces, MemoryServiceError,
    MemorySettingsStore, NamespaceOwner, NamespaceReadiness,
};
use crate::application::workflow::{
    lease::WorkflowHandlerOutcome,
    memory::MemoryStepRuntime,
    polling::WorkflowStepKind,
    waits::{WaitProgress, WorkflowWaits},
    worker::WorkflowHandler,
};
use crate::entities::access_level::AccessLevel;
use crate::infra::memory_config::{MemoryServiceConfig, ProvisionerToken};
use secrecy::SecretString;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

const ADMIN: &str =
    "msk_adm_0190f1a2b3c44d5e8f9a0b1c2d3e4f50_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const PROVISIONER: Uuid = Uuid::from_u128(0x0190f1a2_b3c4_4d5e_8f9a_0b1c2d3e4f50);

fn source() -> Value {
    let mut source: Value =
        serde_json::from_str(&registry::example("memory.save").unwrap().source).unwrap();
    source["steps"]["start"]["with"]["facts"] =
        json!({"literal":["The customer prefers concise replies.\nAsked twice."]});
    source["steps"]["start"]["routes"] = json!({"success":"load"});
    source["steps"]["load"] = json!({"type":"memory.load","with":{
        "scope":{"literal":{"kind":"company"}},"query":{"literal":"concise replies"},
        "limit":{"literal":5}},"routes":{"success":"$end"}});
    source
}

pub(in crate::adapters::persistence::workflow::tests) struct Memory {
    pub(in crate::adapters::persistence::workflow::tests) persistence: PostgresPersistence,
    fake: ContractFake,
    pub(in crate::adapters::persistence::workflow::tests) client: Arc<MemoryServiceHttp>,
}

fn memory(
    f: &AdmissionFixture,
    scope: ActivationRequest,
) -> impl std::future::Future<Output = Memory> + '_ {
    memory_for(f.persistence(), scope.company, f.binding.target.actor)
}

pub(in crate::adapters::persistence::workflow::tests) async fn memory_for(
    p: &PostgresPersistence,
    company: CompanyId,
    actor: WorkflowActor,
) -> Memory {
    let persistence = PostgresPersistence::with_credential_cipher(
        p.pool().clone(),
        CredentialCipher::for_test_with_keys(&[(1, [7_u8; 32])], 1),
    );
    let fake = ContractFake::start(ADMIN).await;
    persistence
        .enable_memory(company, actor, 0, PROVISIONER)
        .await
        .unwrap();
    assert_eq!(
        persistence
            .ensure_namespace(company, NamespaceOwner::Company)
            .await
            .unwrap(),
        NamespaceReadiness::NotReady
    );
    let namespace: Uuid = sqlx::query_scalar(
        "SELECT id FROM memory_namespaces WHERE company_id=$1 AND scope_kind='company'",
    )
    .bind(company.as_uuid())
    .fetch_one(persistence.pool())
    .await
    .unwrap();
    let token = fake.client_token(company.as_uuid(), namespace);
    let envelope = persistence
        .credential_cipher()
        .unwrap()
        .seal_envelope(
            &CredentialContext::memory_client_credential(company.as_uuid(), namespace),
            &SecretString::from(token),
        )
        .unwrap();
    sqlx::query("UPDATE memory_namespaces SET state='ready',credential_id=gen_random_uuid(),credential_envelope=$3 WHERE company_id=$1 AND id=$2")
        .bind(company.as_uuid()).bind(namespace).bind(envelope).execute(persistence.pool()).await.unwrap();
    let client = Arc::new(
        MemoryServiceHttp::new(&MemoryServiceConfig {
            base_url: fake.base_url.clone(),
            provisioner: ProvisionerToken::parse(SecretString::from(ADMIN)).unwrap(),
            request_timeout: std::time::Duration::from_secs(5),
            recall_deadline: std::time::Duration::from_secs(1),
            interactive_in_flight: 2,
            background_in_flight: 2,
        })
        .unwrap(),
    );
    Memory {
        persistence,
        fake,
        client,
    }
}

async fn level(p: &PostgresPersistence, scope: ActivationRequest) -> i16 {
    sqlx::query_scalar(
        "SELECT content_level::smallint FROM workflow_runs WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap()
}

/// Reports every fetch as `exposed_level` 0, whatever its items carry (F11).
struct UnderReported(Arc<MemoryServiceHttp>);

#[async_trait::async_trait]
impl MemoryClient for UnderReported {
    async fn request_generation(
        &self,
        credential: &ClientCredential,
        request: &crate::application::memory::MemoryGenerationRequest,
    ) -> Result<crate::application::memory::MemoryGenerationReceipt, MemoryServiceError> {
        self.0.request_generation(credential, request).await
    }
    async fn save(
        &self,
        credential: &ClientCredential,
        request: &crate::application::memory::MemorySaveRequest,
    ) -> Result<crate::application::memory::MemorySaveReceipt, MemoryServiceError> {
        self.0.save(credential, request).await
    }
    async fn search(
        &self,
        credential: &ClientCredential,
        search: &crate::application::memory::MemorySearch,
    ) -> Result<crate::application::memory::MemorySearchResult, MemoryServiceError> {
        self.0.search(credential, search).await
    }
    async fn context_pack(
        &self,
        packs: &[crate::application::memory::ScopedPack<'_>],
    ) -> Result<Vec<crate::application::memory::MemoryPack>, MemoryServiceError> {
        self.0.context_pack(packs).await
    }
    async fn context_validate(
        &self,
        credential: &ClientCredential,
        namespace: crate::application::memory::NamespaceExternalId,
        read_ceiling: u8,
        items: &[crate::application::memory::PackedRevision],
    ) -> Result<Vec<crate::application::memory::PackItemStatus>, MemoryServiceError> {
        self.0
            .context_validate(credential, namespace, read_ceiling, items)
            .await
    }
    async fn search_scopes(
        &self,
        searches: &[crate::application::memory::ScopedSearch<'_>],
    ) -> Result<Vec<crate::application::memory::MemorySearchResult>, MemoryServiceError> {
        self.0.search_scopes(searches).await
    }
    async fn timeline(
        &self,
        credential: &ClientCredential,
        timeline: &crate::application::memory::MemoryTimeline,
    ) -> Result<crate::application::memory::MemorySearchResult, MemoryServiceError> {
        self.0.timeline(credential, timeline).await
    }
    async fn fetch(
        &self,
        credential: &ClientCredential,
        namespace: crate::application::memory::NamespaceExternalId,
        read_ceiling: u8,
        ids: &[Uuid],
    ) -> Result<crate::application::memory::MemoryFetchResult, MemoryServiceError> {
        let mut fetched = self
            .0
            .fetch(credential, namespace, read_ceiling, ids)
            .await?;
        fetched.exposed_level = 0;
        Ok(fetched)
    }
}

/// A save parks on its committed intent, is delivered once, and resumes with the service's
/// label; the following load reads it at the run's ceiling, raises the run's level before its
/// output is returned, and records the consumption. The load's service under-reports its label
/// (`exposed_level` 0), so the output, the run and the consumption all carry the item's level 9
/// (F11).
#[tokio::test]
async fn memory_integration_save_and_load_steps_round_trip_through_the_intent() {
    let (f, scope) = fixture_source(source()).await;
    let m = memory(&f, scope).await;
    let p = &m.persistence;
    let runtime = MemoryStepRuntime::new(p.clone(), m.client.clone());
    let claim = p
        .settled()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    let WorkflowHandlerOutcome::Suspended(parked) = runtime
        .execute(&WorkflowStepKind("memory.save".into()), &claim)
        .await
    else {
        panic!("memory.save parks on its intent");
    };
    assert!(
        p.confirm_suspension(claim.fence, parked.wait)
            .await
            .unwrap()
    );
    let key: String = sqlx::query_scalar(
        "SELECT idempotency_key FROM memory_deliveries WHERE company_id=$1 AND wait_id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(parked.wait.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(key, format!("ob.s.{}", scope.execution.as_uuid().simple()));
    assert_eq!(p.resume_wait(scope).await.unwrap(), WaitProgress::Pending);
    let delivery = MemoryDeliveryWorker::new(Arc::new(p.clone()), m.client.clone());
    assert!(delivery.pass(&CancellationToken::new()).await.unwrap());
    let WaitProgress::Completed(saved) = p.resume_wait(scope).await.unwrap() else {
        panic!("the delivered intent completes the step");
    };
    // A manual run is conversational input: the company-scope save falls back to the ceiling.
    assert_eq!(saved.output["access_level"], 9);
    assert_eq!(saved.output["replayed"], false);
    assert_eq!(saved.output["saved"].as_array().unwrap().len(), 1);
    let next = saved.successor.expect("the load follows");
    assert_eq!(level(p, next).await, 0);
    let runtime = MemoryStepRuntime::new(p.clone(), Arc::new(UnderReported(m.client.clone())));
    let claim = p
        .settled()
        .claim_io(next, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    let WorkflowHandlerOutcome::Completed(Ok(loaded)) = runtime
        .execute(&WorkflowStepKind("memory.load".into()), &claim)
        .await
    else {
        panic!("memory.load completes");
    };
    assert_eq!(loaded["items"].as_array().unwrap().len(), 1);
    assert_eq!(loaded["items"][0]["access_level"], 9);
    assert_eq!(loaded["items"][0]["scope"], "company");
    assert_eq!(loaded["exposed_level"], 9);
    assert_eq!(
        level(p, next).await,
        9,
        "raised before the output is returned"
    );
    let consumed: (i16, i16) = sqlx::query_as("SELECT read_ceiling::smallint, exposed_level::smallint FROM workflow_memory_consumptions WHERE company_id=$1 AND execution_id=$2")
        .bind(scope.company.as_uuid()).bind(next.execution.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(consumed, (9, 9));
    let saves = m.fake.state.lock().unwrap().saves.len();
    assert_eq!(saves, 1);
}

/// Disabled after publication: a save fails terminally as `memory.not_enabled` before any
/// intent, and a load likewise; nothing reaches the service.
#[tokio::test]
async fn memory_integration_steps_fail_terminally_once_memory_is_disabled() {
    let (f, scope) = fixture_source(source()).await;
    let m = memory(&f, scope).await;
    let p = &m.persistence;
    p.disable_memory(scope.company, f.binding.target.actor, 1)
        .await
        .unwrap();
    let runtime = MemoryStepRuntime::new(p.clone(), m.client.clone());
    let claim = p
        .settled()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    let WorkflowHandlerOutcome::Completed(Err(failure)) = runtime
        .execute(&WorkflowStepKind("memory.save".into()), &claim)
        .await
    else {
        panic!("a disabled save fails");
    };
    assert_eq!(failure.failure.code().as_str(), "memory.not_enabled");
    assert_eq!(failure.failure.class(), FailureClass::Terminal);
    let intents: i64 = sqlx::query_scalar("SELECT count(*) FROM memory_deliveries")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(intents, 0);
    assert!(m.fake.calls().is_empty());
}

/// E9: cancelling the run cancels its parked save's intent (run → intent, by trigger), so
/// nothing is delivered for a cancelled run.
#[tokio::test]
async fn memory_integration_run_cancellation_cancels_the_parked_intent() {
    use crate::application::workflow::{WorkflowInspection, WorkflowRunTransitions};
    let (f, scope) = fixture_source(source()).await;
    let m = memory(&f, scope).await;
    let p = &m.persistence;
    let runtime = MemoryStepRuntime::new(p.clone(), m.client.clone());
    let claim = p
        .settled()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        runtime
            .execute(&WorkflowStepKind("memory.save".into()), &claim)
            .await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    let revision = p
        .head(scope.company, scope.run)
        .await
        .unwrap()
        .unwrap()
        .revision;
    p.cancel(crate::application::workflow::CancelCommand {
        company_id: scope.company,
        actor: f.binding.target.actor,
        run_id: scope.run,
        command_key: IdempotencyKey::parse("cancel-memory").unwrap(),
        expected_revision: revision,
    })
    .await
    .unwrap();
    let (state, outcome): (String, Option<String>) = sqlx::query_as(
        "SELECT state, outcome_code FROM memory_deliveries WHERE company_id=$1 AND execution_id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.execution.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(
        (state.as_str(), outcome.as_deref()),
        ("cancelled", Some("run_cancelled"))
    );
    let delivery = MemoryDeliveryWorker::new(Arc::new(p.clone()), m.client.clone());
    assert!(!delivery.pass(&CancellationToken::new()).await.unwrap());
    assert!(m.fake.calls().is_empty());
}

/// p06 A3-1, review F1: a settled save intent stays while its run is live or still retryable (a
/// control retry can reopen a terminal run, with the same execution and key, until its
/// deadline), and goes once the run is terminal past its deadline.
#[tokio::test]
async fn memory_operations_settled_save_intents_outlive_every_run_that_can_replay_them() {
    use crate::application::memory::MemoryMaintenanceStore;
    use crate::application::workflow::{WorkflowInspection, WorkflowRunTransitions};
    let (f, scope) = fixture_source(source()).await;
    let m = memory(&f, scope).await;
    let p = &m.persistence;
    let runtime = MemoryStepRuntime::new(p.clone(), m.client.clone());
    let claim = p
        .settled()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        runtime
            .execute(&WorkflowStepKind("memory.save".into()), &claim)
            .await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    let age = |sql: &'static str| {
        let pool = p.pool().clone();
        let company = scope.company.as_uuid();
        let run = scope.run.as_uuid();
        async move {
            sqlx::query(sql)
                .bind(company)
                .bind(run)
                .execute(&pool)
                .await
                .unwrap();
        }
    };
    // Settled while the run is live, eight days ago: the parked step still reads its receipt.
    age(
        "UPDATE memory_deliveries SET state = 'delivered', result = '{}'::jsonb, request = NULL, \
           lease_owner = NULL, lease_until = NULL, created_at = now() - interval '8 days', \
           expires_at = now() - interval '2 days' WHERE company_id = $1 AND run_id = $2",
    )
    .await;
    assert_eq!(
        p.sweep_settled_deliveries(500).await.unwrap(),
        0,
        "live run"
    );
    let revision = p
        .head(scope.company, scope.run)
        .await
        .unwrap()
        .unwrap()
        .revision;
    p.cancel(crate::application::workflow::CancelCommand {
        company_id: scope.company,
        actor: f.binding.target.actor,
        run_id: scope.run,
        command_key: IdempotencyKey::parse("cancel-retention").unwrap(),
        expected_revision: revision,
    })
    .await
    .unwrap();
    // Terminal, but before its deadline a control retry could reopen it.
    assert_eq!(
        p.sweep_settled_deliveries(500).await.unwrap(),
        0,
        "terminal run before its deadline"
    );
    // The deadline must follow the run's creation (a schema check): a millisecond after it is
    // already past.
    age(
        "UPDATE workflow_runs SET deadline = created_at + interval '1 millisecond' \
           WHERE company_id = $1 AND id = $2",
    )
    .await;
    assert_eq!(p.sweep_settled_deliveries(500).await.unwrap(), 1);
    let left: i64 =
        sqlx::query_scalar("SELECT count(*) FROM memory_deliveries WHERE company_id = $1")
            .bind(scope.company.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(left, 0);
}

/// An agent-scope namespace, ready with a sealed token the fake grants (agent ids are plain ids
/// on namespaces, so the freeze needs no agent row).
async fn agent_namespace(m: &Memory, scope: ActivationRequest) -> Uuid {
    let agent = Uuid::new_v4();
    let namespace = Uuid::new_v4();
    let token = m.fake.client_token(scope.company.as_uuid(), namespace);
    let envelope = m
        .persistence
        .credential_cipher()
        .unwrap()
        .seal_envelope(
            &CredentialContext::memory_client_credential(scope.company.as_uuid(), namespace),
            &SecretString::from(token),
        )
        .unwrap();
    sqlx::query("INSERT INTO memory_namespaces (company_id,id,scope_kind,agent_id,state,credential_id,credential_envelope) VALUES ($1,$2,'agent',$3,'ready',gen_random_uuid(),$4)")
        .bind(scope.company.as_uuid()).bind(namespace).bind(agent).bind(envelope)
        .execute(m.persistence.pool()).await.unwrap();
    agent
}

async fn freeze_on(pool: &sqlx::PgPool, scope: ActivationRequest, agent: Uuid, key: &str) -> Value {
    use crate::adapters::persistence::memory::deliveries::{
        SaveEnqueue, SaveIntent, SaveScope, enqueue_save_on,
    };
    let facts = vec!["A remembered fact".to_owned()];
    let mut tx = pool.begin().await.unwrap();
    let saved = enqueue_save_on(
        &mut tx,
        &SaveIntent {
            company: scope.company,
            run: scope.run,
            execution: scope.execution,
            invocation: None,
            wait: None,
            scope: SaveScope::Agent(agent),
            key,
            facts: &facts,
            external_sources: Vec::new(),
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        saved,
        SaveEnqueue::Enqueued {
            replayed: false,
            ..
        }
    ));
    tx.commit().await.unwrap();
    sqlx::query_scalar("SELECT request->'body'->'context' FROM memory_deliveries WHERE company_id=$1 AND idempotency_key=$2")
        .bind(scope.company.as_uuid()).bind(key).fetch_one(pool).await.unwrap()
}

/// Until some backend waits on `blocker`'s locks: a real lock wait, not a sleep.
async fn blocked_by(pool: &sqlx::PgPool, blocker: i32) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let waiting: Option<i32> = sqlx::query_scalar(
                "SELECT pid FROM pg_stat_activity \
                 WHERE datname = current_database() AND $1 = ANY(pg_blocking_pids(pid)) LIMIT 1",
            )
            .bind(blocker)
            .fetch_optional(pool)
            .await
            .unwrap();
            if waiting.is_some() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the freeze must wait on the in-flight observation's run lock");
}

async fn raise(pool: &sqlx::PgPool, scope: ActivationRequest, level: AccessLevel) {
    let mut tx = pool.begin().await.unwrap();
    knowledge::raise_content_level_on(&mut tx, scope.company, scope.run, level)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

/// E6: a recall's classification rise and a save's freeze serialize on the run row. If the rise
/// 4 → 6 is in flight, the freeze waits for it (a real lock wait) and freezes 6; if the freeze
/// commits first it freezes 4, and the next save after the rise freezes 6. The inexact
/// (fallback) run freezes `unknown` at its ceiling in both orders.
#[tokio::test]
async fn memory_access_level_competing_recall_and_save_freeze_preserve_the_watermark() {
    for exact in [true, false] {
        for observation_first in [true, false] {
            let (f, scope) = fixture_source(source()).await;
            let m = memory(&f, scope).await;
            let pool = m.persistence.pool().clone();
            let agent = agent_namespace(&m, scope).await;
            if exact {
                knowledge::exactness_fixture::mark_exact(&pool, scope.company, scope.run).await;
            }
            raise(&pool, scope, AccessLevel::L4).await;
            let expect = |level: u8| {
                if exact {
                    json!({"kind":"exact","level":level})
                } else {
                    json!({"kind":"unknown"})
                }
            };
            if observation_first {
                let mut observation = pool.begin().await.unwrap();
                knowledge::raise_content_level_on(
                    &mut observation,
                    scope.company,
                    scope.run,
                    AccessLevel::L6,
                )
                .await
                .unwrap();
                let holder: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                    .fetch_one(&mut *observation)
                    .await
                    .unwrap();
                let freezing = tokio::spawn({
                    let pool = pool.clone();
                    async move { freeze_on(&pool, scope, agent, "ob.s.race-held").await }
                });
                blocked_by(&pool, holder).await;
                observation.commit().await.unwrap();
                let frozen = freezing.await.unwrap();
                assert_eq!(
                    frozen["consumed"],
                    expect(6),
                    "exact={exact}: waited and froze 6"
                );
            } else {
                let first = freeze_on(&pool, scope, agent, "ob.s.race-first").await;
                assert_eq!(first["consumed"], expect(4), "exact={exact}");
                raise(&pool, scope, AccessLevel::L6).await;
                let next = freeze_on(&pool, scope, agent, "ob.s.race-next").await;
                assert_eq!(next["consumed"], expect(6), "exact={exact}");
            }
        }
    }
}

/// E6: a load's output never precedes its committed observation. With the consumption insert
/// failing, the step fails, nothing is returned and the run's level is unchanged.
#[tokio::test]
async fn memory_access_level_load_output_never_precedes_its_committed_observation() {
    let (f, scope) = fixture_source(source()).await;
    let m = memory(&f, scope).await;
    let p = &m.persistence;
    let runtime = MemoryStepRuntime::new(p.clone(), m.client.clone());
    let claim = p
        .settled()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        runtime
            .execute(&WorkflowStepKind("memory.save".into()), &claim)
            .await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    let delivery = MemoryDeliveryWorker::new(Arc::new(p.clone()), m.client.clone());
    assert!(delivery.pass(&CancellationToken::new()).await.unwrap());
    let WaitProgress::Completed(saved) = p.resume_wait(scope).await.unwrap() else {
        panic!("saved");
    };
    let next = saved.successor.unwrap();
    sqlx::raw_sql("CREATE FUNCTION test_memory_consumption_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected memory consumption failure' USING ERRCODE='23514'; END $$; CREATE TRIGGER test_memory_consumption_fault BEFORE INSERT ON workflow_memory_consumptions FOR EACH ROW EXECUTE FUNCTION test_memory_consumption_fault();")
        .execute(p.pool()).await.unwrap();
    let claim = p
        .settled()
        .claim_io(next, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    let outcome = runtime
        .execute(&WorkflowStepKind("memory.load".into()), &claim)
        .await;
    assert!(
        matches!(outcome, WorkflowHandlerOutcome::Completed(Err(_))),
        "no output without its observation: {outcome:?}"
    );
    assert_eq!(level(p, next).await, 0);
    let searched = m
        .fake
        .calls()
        .iter()
        .filter(|call| *call == "observations/fetch")
        .count();
    assert_eq!(searched, 1, "the body was fetched, but never returned");
}

/// F1 (E2, E8, E9): a parked save whose intent ends disabled, expired or refused is listed by the
/// poller only once terminal, and its wake fails the step: the run fails through the final-error
/// route, the wait completes and the audit records the failed delivery.
#[tokio::test]
async fn memory_integration_terminal_intent_wakes_and_fails_the_parked_save() {
    use crate::application::workflow::polling::{PollWork, WorkflowPolling};
    for case in ["disabled", "expired", "refused"] {
        let (f, scope) = fixture_source(source()).await;
        let m = memory(&f, scope).await;
        let p = &m.persistence;
        let runtime = MemoryStepRuntime::new(p.clone(), m.client.clone());
        let claim = p
            .settled()
            .claim_io(scope, worker(), policy())
            .await
            .unwrap()
            .unwrap();
        let WorkflowHandlerOutcome::Suspended(parked) = runtime
            .execute(&WorkflowStepKind("memory.save".into()), &claim)
            .await
        else {
            panic!("{case}: memory.save parks");
        };
        let listed = |page: &crate::application::workflow::polling::PollPage| {
            page.candidates
                .iter()
                .filter(|candidate| candidate.scope.run == scope.run)
                .map(|candidate| candidate.work)
                .collect::<Vec<_>>()
        };
        assert!(
            listed(&p.poll_work(None, 128).await.unwrap()).is_empty(),
            "{case}: open intent"
        );
        let delivery = MemoryDeliveryWorker::new(Arc::new(p.clone()), m.client.clone());
        let expected = match case {
            "disabled" => {
                p.disable_memory(scope.company, f.binding.target.actor, 1)
                    .await
                    .unwrap();
                ("failed", Some("disabled"))
            }
            "expired" => {
                sqlx::query("UPDATE memory_deliveries SET expires_at = created_at + interval '1 microsecond' WHERE company_id=$1 AND wait_id=$2")
                    .bind(scope.company.as_uuid()).bind(parked.wait.as_uuid()).execute(p.pool()).await.unwrap();
                assert!(delivery.pass(&CancellationToken::new()).await.unwrap());
                ("expired", Some("expired"))
            }
            _ => {
                m.fake.fault("observations/save", &[403]);
                assert!(delivery.pass(&CancellationToken::new()).await.unwrap());
                ("failed", Some("authority"))
            }
        };
        let intent: (String, Option<String>) = sqlx::query_as(
            "SELECT state, outcome_code FROM memory_deliveries WHERE company_id=$1 AND wait_id=$2",
        )
        .bind(scope.company.as_uuid())
        .bind(parked.wait.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
        assert_eq!((intent.0.as_str(), intent.1.as_deref()), expected, "{case}");
        assert_eq!(
            listed(&p.poll_work(None, 128).await.unwrap()),
            [PollWork::Wait],
            "{case}"
        );
        let progress = p.resume_wait(scope).await.unwrap();
        assert!(
            matches!(&progress, WaitProgress::Completed(step) if step.route == crate::domain::workflow::RouteSelection::FinalError),
            "{case}: {progress:?}"
        );
        let (run, wait, audit): (String, String, bool) = sqlx::query_as("SELECT (SELECT state FROM workflow_runs WHERE company_id=$1 AND id=$2), (SELECT state FROM workflow_waits WHERE company_id=$1 AND id=$3), EXISTS(SELECT 1 FROM workflow_run_events WHERE company_id=$1 AND run_id=$2 AND event_kind='memory_delivery_failed')")
            .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(parked.wait.as_uuid()).fetch_one(p.pool()).await.unwrap();
        assert_eq!(
            (run.as_str(), wait.as_str(), audit),
            ("failed", "completed", true),
            "{case}"
        );
    }
}

/// F2 (E4): memory.save is marker-owned. A claimed attempt that dies before its park committed
/// an intent sent nothing, so lease retirement makes it retryable instead of reconciliation;
/// after the park, the attempt is closed and the run waits on its intent.
#[tokio::test]
async fn memory_integration_interrupted_save_without_an_intent_retries() {
    use crate::adapters::persistence::workflow::lease::Retirement;
    let (f, scope) = fixture_source(source()).await;
    let m = memory(&f, scope).await;
    let p = &m.persistence;
    let claim = p
        .settled()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    // The worker died between claim and park: no intent exists when its lease expires.
    let mut tx = p.pool().begin().await.unwrap();
    sqlx::query("UPDATE background_tasks SET locked_at=clock_timestamp()-interval '10 seconds',lock_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1")
        .bind(scope.job.0).execute(&mut *tx).await.unwrap();
    crate::adapters::persistence::workflow::recovery::retire_on(
        &mut tx,
        claim.fence,
        Retirement::Expired,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let (safety, run, job): (String, String, String) = sqlx::query_as("SELECT (SELECT workflow_retry_safety FROM task_attempts WHERE task_id=$3 AND attempt_number=$4), (SELECT state FROM workflow_runs WHERE company_id=$1 AND id=$2), (SELECT status FROM background_tasks WHERE id=$3)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.job.0).bind(claim.fence.attempt.0).fetch_one(p.pool()).await.unwrap();
    assert_eq!(
        (safety.as_str(), run.as_str(), job.as_str()),
        ("safe", "running", "pending")
    );
    // A retry parks normally, exactly once.
    sqlx::query(
        "UPDATE background_tasks SET run_at=clock_timestamp() WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.job.0)
    .execute(p.pool())
    .await
    .unwrap();
    let again = p
        .settled()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    let runtime = MemoryStepRuntime::new(p.clone(), m.client.clone());
    assert!(matches!(
        runtime
            .execute(&WorkflowStepKind("memory.save".into()), &again)
            .await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    let (run, intents): (String, i64) = sqlx::query_as("SELECT (SELECT waiting_reason FROM workflow_runs WHERE company_id=$1 AND id=$2), (SELECT count(*) FROM memory_deliveries WHERE company_id=$1 AND run_id=$2)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!((run.as_str(), intents), ("effect", 1));
}

/// B3 ordering (E9): memory disable and run cancellation both settle every open intent of the
/// run, in id order, so racing them on several intents never deadlocks and leaves each intent
/// terminal exactly once, by whichever committed first.
#[tokio::test]
async fn memory_integration_competing_disable_and_run_cancel_settle_every_intent() {
    use crate::application::workflow::WorkflowInspection;
    for round in 0..4 {
        let (f, scope) = fixture_source(source()).await;
        let m = memory(&f, scope).await;
        let p = m.persistence.clone();
        for index in 0..6 {
            let agent = agent_namespace(&m, scope).await;
            freeze_on(p.pool(), scope, agent, &format!("ob.s.order-{index}")).await;
        }
        let revision = p
            .head(scope.company, scope.run)
            .await
            .unwrap()
            .unwrap()
            .revision;
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let disable = tokio::spawn({
            let (p, barrier, actor) = (p.clone(), barrier.clone(), f.binding.target.actor);
            async move {
                barrier.wait().await;
                p.disable_memory(scope.company, actor, 1).await
            }
        });
        let cancel = tokio::spawn({
            let (p, barrier, actor) = (p.clone(), barrier.clone(), f.binding.target.actor);
            async move {
                barrier.wait().await;
                crate::application::workflow::WorkflowRunTransitions::cancel(
                    &p,
                    crate::application::workflow::CancelCommand {
                        company_id: scope.company,
                        actor,
                        run_id: scope.run,
                        command_key: IdempotencyKey::parse("cancel-order").unwrap(),
                        expected_revision: revision,
                    },
                )
                .await
            }
        });
        let (disabled, cancelled) = (disable.await.unwrap(), cancel.await.unwrap());
        assert!(disabled.is_ok(), "round {round}: disable: {disabled:?}");
        assert!(cancelled.is_ok(), "round {round}: cancel: {cancelled:?}");
        let states: Vec<(String, Option<String>, bool)> = sqlx::query_as(
            "SELECT state, outcome_code, request IS NULL FROM memory_deliveries WHERE company_id=$1 AND run_id=$2",
        )
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .fetch_all(p.pool())
        .await
        .unwrap();
        assert_eq!(states.len(), 6);
        assert!(
            states.iter().all(|(state, outcome, cleared)| *cleared
                && matches!(
                    (state.as_str(), outcome.as_deref()),
                    ("failed", Some("disabled")) | ("cancelled", Some("run_cancelled"))
                )),
            "round {round}: {states:?}"
        );
    }
}

/// B3 ordering, forced (E9): a third transaction holds the middle intent, so the run-cancel
/// trigger locks the intents below it and waits. Disable starts while that wait lasts. In id
/// order, disable waits behind the trigger's first row, and both finish once the holder lets go.
/// Locked in any other order, disable would take rows above the held one, and the two settlers
/// would each hold a row the other needs: a deadlock (`40P01`) fails one of them.
/// Stand-ins: the holder is the pause between the settlers' row locks. The cancel is the bare
/// run → `cancelled` update that fires the trigger. A user cancel and disable both take the
/// company row (`authorize_company`, FOR NO KEY UPDATE), so they never overlap on intents. The
/// unserialized writer is a guard failure's child cancellation (`cancel_children_on`). The
/// stand-in rolls back, so disable settles every intent.
#[tokio::test]
async fn memory_integration_disable_and_run_cancel_overlapping_in_id_order_never_deadlock() {
    let (f, scope) = fixture_source(source()).await;
    let m = memory(&f, scope).await;
    let p = m.persistence.clone();
    for index in 0..6 {
        let agent = agent_namespace(&m, scope).await;
        freeze_on(p.pool(), scope, agent, &format!("ob.s.forced-{index}")).await;
    }
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM memory_deliveries WHERE company_id=$1 AND run_id=$2 ORDER BY id",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_all(p.pool())
    .await
    .unwrap();
    assert_eq!(ids.len(), 6);
    let mut holder = p.pool().begin().await.unwrap();
    let holder_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *holder)
        .await
        .unwrap();
    sqlx::query("SELECT 1 FROM memory_deliveries WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(scope.company.as_uuid())
        .bind(ids[3])
        .execute(&mut *holder)
        .await
        .unwrap();
    // Backends waiting on the holder, directly or behind another waiter.
    let waiting = || async {
        sqlx::query_scalar::<_, i64>(
            "WITH RECURSIVE blocked(pid) AS ( \
               SELECT activity.pid FROM pg_stat_activity AS activity \
               WHERE $1 = ANY(pg_blocking_pids(activity.pid)) \
               UNION SELECT activity.pid FROM pg_stat_activity AS activity \
               JOIN blocked ON blocked.pid = ANY(pg_blocking_pids(activity.pid))) \
             SELECT count(*) FROM blocked",
        )
        .bind(holder_pid)
        .fetch_one(p.pool())
        .await
        .unwrap()
    };
    let until = |count: i64| async move {
        let started = std::time::Instant::now();
        while waiting().await < count {
            assert!(
                started.elapsed() < std::time::Duration::from_secs(10),
                "{count} settlers never waited on the held intent"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    };
    let cancel = tokio::spawn({
        let p = p.clone();
        async move {
            let mut tx = p.pool().begin().await?;
            sqlx::query(
                "UPDATE workflow_runs SET state='cancelled',waiting_reason=NULL \
                 WHERE company_id=$1 AND id=$2",
            )
            .bind(scope.company.as_uuid())
            .bind(scope.run.as_uuid())
            .execute(&mut *tx)
            .await?;
            tx.rollback().await
        }
    });
    until(1).await;
    let disable = tokio::spawn({
        let (p, actor) = (p.clone(), f.binding.target.actor);
        async move { p.disable_memory(scope.company, actor, 1).await }
    });
    until(2).await;
    holder.rollback().await.unwrap();
    let (cancelled, disabled) = (cancel.await.unwrap(), disable.await.unwrap());
    assert!(cancelled.is_ok(), "cancel: {cancelled:?}");
    assert!(disabled.is_ok(), "disable: {disabled:?}");
    let states: Vec<(String, Option<String>, bool)> = sqlx::query_as(
        "SELECT state, outcome_code, request IS NULL FROM memory_deliveries \
         WHERE company_id=$1 AND run_id=$2 ORDER BY id",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_all(p.pool())
    .await
    .unwrap();
    assert_eq!(
        states,
        vec![("failed".to_owned(), Some("disabled".to_owned()), true); 6]
    );
}

/// A company member at `level` joins the thread as a reader.
async fn thread_reader(
    f: &AdmissionFixture,
    channel: Uuid,
    thread: Uuid,
    level: Option<AccessLevel>,
) {
    let pool = f.persistence().pool();
    let company = f.binding.target.company.as_uuid();
    let user = match level {
        None => f.binding.target.actor.user_id(),
        Some(level) => {
            let user = Uuid::new_v4();
            let email = format!("{}@example.test", user.simple());
            sqlx::query("INSERT INTO users (id, username, email, password_hash) VALUES ($1, $2, $3, 'test')")
                .bind(user).bind(user.simple().to_string()).bind(&email).execute(pool).await.unwrap();
            sqlx::query("INSERT INTO company_members (id, company_id, user_id, role, access_level) VALUES ($1, $2, $3, 'member', $4)")
                .bind(Uuid::new_v4()).bind(company).bind(user).bind(level.as_i16()).execute(pool).await.unwrap();
            let mut connection = pool.acquire().await.unwrap();
            crate::adapters::persistence::participant::create_person_principal_on(
                &mut connection,
                company,
                user,
                "Reader",
                &email,
            )
            .await
            .unwrap();
            user
        }
    };
    sqlx::query("INSERT INTO thread_principals(company_id,channel_id,thread_id,principal_id,role) SELECT $1,$2,$3,id,'participant' FROM principals WHERE company_id=$1 AND user_id=$4")
        .bind(company).bind(channel).bind(thread).bind(user).execute(pool).await.unwrap();
}

/// F5 (E3, 05A): a thread run with readers 6/8/9 is admitted at ceiling 6 and its load reads
/// levels 0–6 only; a reader at 3 joining afterwards lowers the read to 3; and once the run
/// holds level-4 content, that drop refuses the load before any service call.
#[tokio::test]
async fn memory_access_level_load_reads_at_the_thread_audience_and_refuses_a_lower_destination() {
    use super::super::admission_history_tests::Conversation;
    for case in ["audience", "drop", "refused"] {
        let mut load: Value =
            serde_json::from_str(&registry::example("memory.load").unwrap().source).unwrap();
        load["steps"]["start"]["with"] = json!({"scope":{"literal":{"kind":"company"}},
            "query":{"literal":"concise replies"},"limit":{"literal":20}});
        let f = AdmissionFixture::with_binding(BindingFixture::from_source(load).await).await;
        let c = Conversation::new(&f).await;
        for level in [None, Some(AccessLevel::L8), Some(AccessLevel::L6)] {
            thread_reader(&f, c.channel, c.thread, level).await;
        }
        let command = f.prepare(c.request(&f, case, c.message)).await;
        f.persistence().admit(&command).await.unwrap();
        let job =
            sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
                .bind(command.first_execution_id().as_uuid())
                .fetch_one(f.persistence().pool())
                .await
                .unwrap();
        let scope = ActivationRequest {
            company: command.company_id(),
            run: command.proposed_run_id(),
            execution: command.first_execution_id(),
            job: WorkflowJobId(job),
        };
        let ceiling: i16 = sqlx::query_scalar(
            "SELECT knowledge_ceiling::smallint FROM workflow_runs WHERE company_id=$1 AND id=$2",
        )
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
        assert_eq!(ceiling, 6, "{case}: the lowest reader");
        let m = memory(&f, scope).await;
        let p = &m.persistence;
        let namespace: Uuid = sqlx::query_scalar(
            "SELECT id FROM memory_namespaces WHERE company_id=$1 AND scope_kind='company'",
        )
        .bind(scope.company.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
        {
            let mut state = m.fake.state.lock().unwrap();
            for level in 0..=9_u8 {
                state.observations.push((namespace.hyphenated().to_string(), json!({
                    "observation_id":Uuid::new_v4(),"kind":"fact","title":format!("Concise replies {level}"),
                    "body":"The customer prefers concise replies.","occurred_at":"2026-11-01T00:00:00Z",
                    "access_level":level,"revision":1,"content_sha256":"0".repeat(64),"sources":[],
                    "index_state":"pending"})));
            }
        }
        if case != "audience" {
            thread_reader(&f, c.channel, c.thread, Some(AccessLevel::L3)).await;
        }
        if case == "refused" {
            raise(p.pool(), scope, AccessLevel::L4).await;
        }
        let claim = p
            .settled()
            .claim_io(scope, worker(), policy())
            .await
            .unwrap()
            .unwrap();
        let runtime = MemoryStepRuntime::new(p.clone(), m.client.clone());
        let outcome = runtime
            .execute(&WorkflowStepKind("memory.load".into()), &claim)
            .await;
        if case == "refused" {
            let WorkflowHandlerOutcome::Completed(Err(failure)) = outcome else {
                panic!("the drop refuses the load: {outcome:?}");
            };
            assert_eq!(
                failure.failure.code().as_str(),
                crate::app_error::KNOWLEDGE_AUDIENCE_BELOW_CONTENT
            );
            assert!(
                m.fake
                    .calls()
                    .iter()
                    .all(|call| !call.starts_with("observations/"))
            );
            assert_eq!(level(p, scope).await, 4);
            continue;
        }
        let WorkflowHandlerOutcome::Completed(Ok(loaded)) = outcome else {
            panic!("{case}: memory.load completes: {outcome:?}");
        };
        let expected: u8 = if case == "audience" { 6 } else { 3 };
        let mut levels: Vec<u64> = loaded["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["access_level"].as_u64().unwrap())
            .collect();
        levels.sort_unstable();
        assert_eq!(
            levels,
            (0..=u64::from(expected)).collect::<Vec<_>>(),
            "{case}"
        );
        assert_eq!(loaded["exposed_level"], expected, "{case}");
        assert_eq!(level(p, scope).await, i16::from(expected), "{case}");
        let read: i16 = sqlx::query_scalar("SELECT read_ceiling::smallint FROM workflow_memory_consumptions WHERE company_id=$1 AND execution_id=$2")
            .bind(scope.company.as_uuid()).bind(scope.execution.as_uuid()).fetch_one(p.pool()).await.unwrap();
        assert_eq!(read, i16::from(expected), "{case}");
    }
}

/// F5 (E9): cancelling the run while its parked save is in flight cancels the claimed intent at
/// once (run → intent, by trigger); the worker's completion is fenced out and the cancelled run
/// is never resumed by it.
#[tokio::test]
async fn memory_integration_run_cancel_during_an_in_flight_delivery_loses_its_completion() {
    use crate::application::workflow::{WorkflowInspection, WorkflowRunTransitions};
    let (f, scope) = fixture_source(source()).await;
    let m = memory(&f, scope).await;
    let p = &m.persistence;
    let runtime = MemoryStepRuntime::new(p.clone(), m.client.clone());
    let claim = p
        .settled()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        runtime
            .execute(&WorkflowStepKind("memory.save".into()), &claim)
            .await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    m.fake.state.lock().unwrap().delays.insert(
        "observations/save".into(),
        std::time::Duration::from_millis(700),
    );
    let delivery = MemoryDeliveryWorker::new(Arc::new(p.clone()), m.client.clone());
    let sending = tokio::spawn(async move { delivery.pass(&CancellationToken::new()).await });
    let intent = || async {
        sqlx::query_as::<_, (String, Option<String>, bool)>(
            "SELECT state, outcome_code, result IS NULL FROM memory_deliveries WHERE company_id=$1 AND run_id=$2",
        )
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap()
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while intent().await.0 != "claimed" {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the worker claims the intent");
    let revision = p
        .head(scope.company, scope.run)
        .await
        .unwrap()
        .unwrap()
        .revision;
    p.cancel(crate::application::workflow::CancelCommand {
        company_id: scope.company,
        actor: f.binding.target.actor,
        run_id: scope.run,
        command_key: IdempotencyKey::parse("cancel-in-flight").unwrap(),
        expected_revision: revision,
    })
    .await
    .unwrap();
    assert!(sending.await.unwrap().unwrap());
    let (state, outcome, no_receipt) = intent().await;
    assert_eq!(
        (state.as_str(), outcome.as_deref(), no_receipt),
        ("cancelled", Some("run_cancelled"), true)
    );
    let run: String =
        sqlx::query_scalar("SELECT state FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(scope.company.as_uuid())
            .bind(scope.run.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(run, "cancelled");
}

#[path = "memory_steps_e2e_tests.rs"]
mod e2e;
