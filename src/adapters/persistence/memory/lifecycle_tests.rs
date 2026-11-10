//! The lifecycle reconciler against the real store and the contract fake (E2, E9). Each test has
//! its own database, because a claim sweeps the whole table.
use super::tests::{Fixture, PROVISIONER, company};
use super::*;
use crate::adapters::memory_service::MemoryServiceHttp;
use crate::adapters::memory_service::contract_fake::ContractFake;
use crate::adapters::persistence::credentials::envelope::CredentialContext;
use crate::application::memory::*;
use crate::application::workflow::CompanyId;
use crate::infra::memory_config::{MemoryServiceConfig, ProvisionerToken};
use crate::use_cases::agent::AgentPersistence;
use crate::use_cases::company::CompanyPersistence;
use secrecy::{ExposeSecret, SecretString};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// The provisioner token whose credential id is [`PROVISIONER`].
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

fn worker(f: &Fixture, fake: &ContractFake, credential_id: Uuid) -> MemoryLifecycleWorker {
    MemoryLifecycleWorker::new(Arc::new(f.persistence.clone()), http(fake), credential_id)
}

/// A rotated provisioner token, with the credential id it carries (p06 A3-4).
const ROTATED: &str =
    "msk_adm_0190f1a2b3c44d5e8f9a0b1c2d3e4f99_BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";
const ROTATED_ID: Uuid = Uuid::from_u128(0x0190f1a2_b3c4_4d5e_8f9a_0b1c2d3e4f99);

/// A worker configured with the rotated token, as a restarted OpenBot would be.
fn rotated(f: &Fixture, fake: &ContractFake) -> MemoryLifecycleWorker {
    fake.accept_admin(ROTATED);
    let http = Arc::new(
        MemoryServiceHttp::new(&MemoryServiceConfig {
            base_url: fake.base_url.clone(),
            provisioner: ProvisionerToken::parse(SecretString::from(ROTATED)).unwrap(),
            request_timeout: Duration::from_secs(5),
            recall_deadline: Duration::from_secs(1),
            interactive_in_flight: 2,
            background_in_flight: 2,
        })
        .unwrap(),
    );
    MemoryLifecycleWorker::new(Arc::new(f.persistence.clone()), http, ROTATED_ID)
}

/// Calls other than the management probe (a listing and a create).
fn destructive(fake: &ContractFake) -> Vec<String> {
    fake.calls()
        .into_iter()
        .filter(|call| !call.ends_with("credentials/list") && !call.ends_with("tenants/create"))
        .collect()
}

async fn recorded(f: &Fixture) -> Vec<Uuid> {
    sqlx::query_scalar(
        "SELECT DISTINCT provisioner_credential_id FROM memory_lifecycles WHERE company_id = $1 \
         UNION SELECT provisioner_credential_id FROM memory_tenants WHERE company_id = $1",
    )
    .bind(f.company)
    .fetch_all(f.pool())
    .await
    .unwrap()
}

async fn pass(worker: &MemoryLifecycleWorker) -> bool {
    worker.pass(&CancellationToken::new()).await.unwrap()
}

/// Every due row, until none is: bounded, so a hot loop fails the test instead of hanging it.
async fn drain(worker: &MemoryLifecycleWorker) {
    for _ in 0..20 {
        if !pass(worker).await {
            return;
        }
    }
    panic!("the reconciler never ran out of due rows");
}

/// Make deferred rows due now instead of waiting out their backoff.
async fn due(f: &Fixture) {
    sqlx::query(
        "UPDATE memory_lifecycles SET available_at = clock_timestamp() WHERE state = 'pending'",
    )
    .execute(f.pool())
    .await
    .unwrap();
}

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct Row {
    desired: String,
    observed: String,
    state: String,
    attempts: i32,
    outcome_code: Option<String>,
}

async fn row(f: &Fixture, namespace: Option<Uuid>) -> Row {
    sqlx::query_as(
        "SELECT desired, observed, state, attempts, outcome_code FROM memory_lifecycles \
         WHERE company_id = $1 AND namespace_id IS NOT DISTINCT FROM $2",
    )
    .bind(f.company)
    .bind(namespace)
    .fetch_one(f.pool())
    .await
    .unwrap()
}

fn expect(desired: &str, observed: &str, state: &str, attempts: i32, outcome: Option<&str>) -> Row {
    Row {
        desired: desired.into(),
        observed: observed.into(),
        state: state.into(),
        attempts,
        outcome_code: outcome.map(str::to_owned),
    }
}

fn tenant_key(f: &Fixture) -> String {
    f.company.hyphenated().to_string()
}

/// (credential id, label, revoked) the fake holds for this company.
fn credentials(fake: &ContractFake, f: &Fixture) -> Vec<(Uuid, String, bool)> {
    let tenant = tenant_key(f);
    let mut listed: Vec<_> = fake
        .state
        .lock()
        .unwrap()
        .credentials
        .iter()
        .filter(|(_, (owner, _, _))| *owner == tenant)
        .map(|(id, (_, label, revoked))| (*id, label.clone(), *revoked))
        .collect();
    listed.sort();
    listed
}

async fn stored(f: &Fixture, namespace: Uuid) -> Option<(Uuid, String)> {
    sqlx::query_as(
        "SELECT credential_id, credential_envelope FROM memory_namespaces \
         WHERE id = $1 AND credential_id IS NOT NULL",
    )
    .bind(namespace)
    .fetch_optional(f.pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn memory_integration_reconciler_provisions_the_tenant_and_a_namespace_credential() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    let worker = worker(&f, &fake, PROVISIONER);
    f.enable().await.unwrap();
    drain(&worker).await;
    assert_eq!(
        row(&f, None).await,
        expect("present", "present", "settled", 0, None)
    );
    // The operator cap is cached; that refresh bumps neither the revision nor the epoch.
    let (cap, revision, epoch): (Option<i64>, i64, i64) = sqlx::query_as(
        "SELECT generation_cap, revision, disable_epoch FROM memory_tenants WHERE company_id = $1",
    )
    .bind(f.company)
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert_eq!(
        (cap, revision, epoch),
        (Some(0), 1, 0),
        "the cap refresh keeps the form current"
    );

    assert_eq!(
        f.ensure(NamespaceOwner::Agent(f.agent)).await,
        NamespaceReadiness::NotReady
    );
    drain(&worker).await;
    let (namespace, state) = f.namespaces("agent").await[0].clone();
    assert_eq!(state, "ready");
    assert_eq!(
        f.ensure(NamespaceOwner::Agent(f.agent)).await,
        NamespaceReadiness::Ready(NamespaceExternalId(namespace))
    );
    let (credential, envelope) = stored(&f, namespace).await.unwrap();
    let token = f
        .persistence
        .credential_cipher()
        .unwrap()
        .open_envelope(
            &CredentialContext::memory_client_credential(f.company, namespace),
            &envelope,
        )
        .unwrap();
    assert!(token.expose_secret().starts_with("msk_cli_"));
    assert_eq!(
        credentials(&fake, &f),
        [(
            credential,
            client_credential_label(NamespaceExternalId(namespace)),
            false
        )]
    );
    assert_eq!(
        fake.calls(),
        [
            "tenants/create",
            "namespaces/create",
            "credentials/list",
            "credentials/issue"
        ]
    );
}

#[tokio::test]
async fn memory_scope_reconciler_disable_and_enable_follow_the_desired_state() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    let worker = worker(&f, &fake, PROVISIONER);
    f.enable().await.unwrap();
    drain(&worker).await;
    f.disable().await.unwrap();
    drain(&worker).await;
    assert_eq!(
        row(&f, None).await,
        expect("disabled", "disabled", "settled", 0, None)
    );
    let remote = |fake: &ContractFake| fake.state.lock().unwrap().tenants[&tenant_key(&f)].0;
    assert_eq!(
        remote(&fake),
        crate::adapters::memory_service::contract_fake::FakeTenantState::Disabled
    );
    f.enable().await.unwrap();
    drain(&worker).await;
    assert_eq!(
        row(&f, None).await,
        expect("present", "present", "settled", 0, None)
    );
    assert_eq!(
        remote(&fake),
        crate::adapters::memory_service::contract_fake::FakeTenantState::Active
    );
}

#[tokio::test]
async fn memory_scope_reconciler_company_delete_creates_then_deletes_and_only_a_200_completes() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    let worker = worker(&f, &fake, PROVISIONER);
    // Deleted before any pass: the reconciler still creates first, making the state definite.
    f.enable().await.unwrap();
    CompanyPersistence::delete(&f.persistence, f.company)
        .await
        .unwrap();
    fake.fault("tenants/delete", &[503]);
    assert!(pass(&worker).await);
    assert_eq!(
        row(&f, None).await,
        expect("absent", "none", "pending", 0, Some("service_unavailable"))
    );
    due(&f).await;
    drain(&worker).await;
    assert_eq!(
        row(&f, None).await,
        expect("absent", "absent", "settled", 0, None)
    );
    assert_eq!(
        fake.calls(),
        [
            "tenants/create",
            "tenants/delete",
            "tenants/create",
            "tenants/delete"
        ]
    );
    assert_eq!(
        fake.state.lock().unwrap().tenants[&tenant_key(&f)].0,
        crate::adapters::memory_service::contract_fake::FakeTenantState::Deleted
    );
}

#[tokio::test]
async fn memory_scope_reconciler_foreign_managed_tenant_never_completes_deletion() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    let worker = worker(&f, &fake, PROVISIONER);
    fake.foreign_tenant(f.company);
    f.enable().await.unwrap();
    drain(&worker).await;
    assert_eq!(
        row(&f, None).await,
        expect("present", "none", "failed", 0, Some("tenant_conflict"))
    );
    CompanyPersistence::delete(&f.persistence, f.company)
        .await
        .unwrap();
    drain(&worker).await;
    // A 404 from delete after a refused create is not ours to complete: it waits, visible,
    // never settled (p06 B1 F1; a reassignment back to us would let it complete).
    assert_eq!(
        row(&f, None).await,
        expect("absent", "none", "pending", 0, Some("provisioner_mismatch"))
    );
    // A delete 404 right after a create that succeeded is definite: the tenant was ours.
    {
        let mut state = fake.state.lock().unwrap();
        state.tenants.remove(&tenant_key(&f));
        state.faults.insert("tenants/delete".into(), vec![404]);
    }
    due(&f).await;
    assert!(pass(&worker).await);
    assert_eq!(
        row(&f, None).await,
        expect("absent", "none", "failed", 0, Some("tenant_unmanageable"))
    );
}

/// p06 A3-4 and the A3-7 rotation order: a rotated credential the service does not (yet) confirm
/// postpones every row without an attempt and with no destructive call; after the operator's
/// reassignment the rows adopt it and their steps complete, and the old credential is refused.
#[tokio::test]
async fn memory_scope_reconciler_refuses_a_changed_provisioner_credential_id() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    let old = worker(&f, &fake, PROVISIONER);
    f.enable().await.unwrap();
    drain(&old).await;
    f.ensure(NamespaceOwner::Agent(f.agent)).await;
    let (namespace, _) = f.namespaces("agent").await[0].clone();
    let new = rotated(&f, &fake);
    assert!(pass(&new).await);
    assert_eq!(
        row(&f, Some(namespace)).await,
        expect(
            "present",
            "none",
            "pending",
            0,
            Some("provisioner_mismatch")
        )
    );
    assert!(!pass(&new).await, "postponed out of the due set");
    assert!(destructive(&fake).is_empty(), "{:?}", fake.calls());
    assert_eq!(recorded(&f).await, vec![PROVISIONER]);
    // The operator reassigns; the next pass adopts and provisions.
    assert_eq!(fake.reassign(ADMIN, ROTATED), 1);
    due(&f).await;
    drain(&new).await;
    assert_eq!(
        row(&f, Some(namespace)).await,
        expect("present", "present", "settled", 0, None)
    );
    assert_eq!(recorded(&f).await, vec![ROTATED_ID]);
    // The old credential no longer manages anything: refused, nothing changed.
    f.ensure(NamespaceOwner::Company).await;
    let calls = fake.calls().len();
    assert!(pass(&old).await);
    assert!(
        fake.calls()[calls..]
            .iter()
            .all(|call| call.ends_with("credentials/list") || call.ends_with("tenants/create")),
        "{:?}",
        fake.calls()
    );
}

/// p06 A3 F5: rotating while a deleted company's absent rows are still pending adopts them
/// through the tombstone (a listing answers for it) and completes them.
#[tokio::test]
async fn memory_operations_rotation_mid_deletion_adopts_and_completes_pending_absent_rows() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    let old = worker(&f, &fake, PROVISIONER);
    f.enable().await.unwrap();
    f.ensure(NamespaceOwner::Agent(f.agent)).await;
    drain(&old).await;
    let (namespace, _) = f.namespaces("agent").await[0].clone();
    CompanyPersistence::delete(&f.persistence, f.company)
        .await
        .unwrap();
    // The tenant's deletion already landed under the old credential (its tombstone), while the
    // company's rows are still pending.
    fake.state
        .lock()
        .unwrap()
        .tenants
        .get_mut(&tenant_key(&f))
        .unwrap()
        .0 = crate::adapters::memory_service::contract_fake::FakeTenantState::Deleted;
    let new = rotated(&f, &fake);
    assert_eq!(fake.reassign(ADMIN, ROTATED), 1);
    due(&f).await;
    drain(&new).await;
    assert_eq!(
        row(&f, None).await,
        expect("absent", "absent", "settled", 0, None)
    );
    // The namespace row settles under the tenant's tombstone; nothing stays postponed.
    let namespace_row = row(&f, Some(namespace)).await;
    assert_eq!(
        (namespace_row.desired.as_str(), namespace_row.state.as_str()),
        ("absent", "settled"),
        "{namespace_row:?}"
    );
    assert_ne!(
        namespace_row.outcome_code.as_deref(),
        Some("provisioner_mismatch")
    );
    assert_eq!(recorded(&f).await, vec![ROTATED_ID]);
}

/// p06 A3-5: an absent step never completes on a 404 that can mean "not ours" (the tenant was
/// reassigned elsewhere), and with the tenant disabled a delete 404 waits instead of completing.
#[tokio::test]
async fn memory_operations_absent_steps_never_complete_on_a_404_that_may_mean_not_ours() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    let worker = worker(&f, &fake, PROVISIONER);
    f.enable().await.unwrap();
    f.ensure(NamespaceOwner::Agent(f.agent)).await;
    drain(&worker).await;
    let (namespace, _) = f.namespaces("agent").await[0].clone();
    // Disabled tenant, and a namespace whose create never landed: the delete answers 404.
    {
        let mut state = fake.state.lock().unwrap();
        state.tenants.get_mut(&tenant_key(&f)).unwrap().0 =
            crate::adapters::memory_service::contract_fake::FakeTenantState::Disabled;
        state
            .namespaces
            .remove(&(tenant_key(&f), namespace.hyphenated().to_string()));
    }
    AgentPersistence::delete(&f.persistence, f.agent)
        .await
        .unwrap();
    assert!(pass(&worker).await);
    assert_eq!(
        row(&f, Some(namespace)).await,
        expect("absent", "present", "pending", 0, Some("tenant_disabled"))
    );
    // Reassigned to someone else: the create's 404 is probed and postponed, never settled.
    assert_eq!(fake.reassign(ADMIN, "someone-else"), 1);
    due(&f).await;
    assert!(pass(&worker).await);
    assert_eq!(
        row(&f, Some(namespace)).await,
        expect(
            "absent",
            "present",
            "pending",
            0,
            Some("provisioner_mismatch")
        )
    );
    // Back with us (and enabled): create + delete are definite, and the row completes.
    fake.reassign("someone-else", ADMIN);
    fake.state
        .lock()
        .unwrap()
        .tenants
        .get_mut(&tenant_key(&f))
        .unwrap()
        .0 = crate::adapters::memory_service::contract_fake::FakeTenantState::Active;
    due(&f).await;
    drain(&worker).await;
    assert_eq!(
        row(&f, Some(namespace)).await,
        expect("absent", "absent", "settled", 0, None)
    );
}

#[tokio::test]
async fn memory_scope_reconciler_fences_stolen_and_superseded_claims() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let store: &dyn MemoryLifecycleStore = &f.persistence;
    f.enable().await.unwrap();
    let first = store
        .claim_lifecycle(Uuid::new_v4(), Duration::from_secs(60))
        .await
        .unwrap()
        .unwrap();
    // Its lease expires; the next claim fences it out and is charged an attempt.
    sqlx::query(
        "UPDATE memory_lifecycles SET lease_until = clock_timestamp() - interval '1 second'",
    )
    .execute(f.pool())
    .await
    .unwrap();
    let second = store
        .claim_lifecycle(Uuid::new_v4(), Duration::from_secs(60))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (second.id, second.fence, second.attempts),
        (first.id, first.fence + 1, 1)
    );
    assert_eq!(
        store
            .settle_lifecycle(&first, LifecycleObserved::Present, Some(0))
            .await
            .unwrap(),
        LifecycleCompletion::Lost
    );
    assert_eq!(
        store
            .settle_lifecycle(&second, LifecycleObserved::Present, Some(0))
            .await
            .unwrap(),
        LifecycleCompletion::Applied
    );

    // A provisioning claim superseded by the agent's deletion stores nothing.
    f.ensure(NamespaceOwner::Agent(f.agent)).await;
    let (namespace, _) = f.namespaces("agent").await[0].clone();
    let claim = store
        .claim_lifecycle(Uuid::new_v4(), Duration::from_secs(60))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        claim.resource,
        LifecycleResource::Namespace(NamespaceExternalId(namespace))
    );
    AgentPersistence::delete(&f.persistence, f.agent)
        .await
        .unwrap();
    let issued = IssuedClientCredential {
        credential_id: Uuid::new_v4(),
        token: SecretString::from("msk_cli_late"),
    };
    assert_eq!(
        store.store_credential(&claim, issued, None).await.unwrap(),
        LifecycleCompletion::Superseded
    );
    assert_eq!(
        row(&f, Some(namespace)).await,
        expect("absent", "none", "pending", 0, None)
    );
    assert!(stored(&f, namespace).await.is_none());
}

#[tokio::test]
async fn memory_scope_reconciler_revokes_strays_and_removes_an_absent_namespace() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    let worker = worker(&f, &fake, PROVISIONER);
    f.enable().await.unwrap();
    drain(&worker).await;
    f.ensure(NamespaceOwner::Agent(f.agent)).await;
    let (namespace, _) = f.namespaces("agent").await[0].clone();
    let label = client_credential_label(NamespaceExternalId(namespace));
    // A stray labelled for this namespace but not stored (an earlier ambiguous issue).
    let stray = Uuid::new_v4();
    fake.state
        .lock()
        .unwrap()
        .credentials
        .insert(stray, (tenant_key(&f), label.clone(), false));
    drain(&worker).await;
    let (credential, _) = stored(&f, namespace).await.unwrap();
    let mut expected = vec![
        (credential, label.clone(), false),
        (stray, label.clone(), true),
    ];
    expected.sort();
    assert_eq!(credentials(&fake, &f), expected);

    AgentPersistence::delete(&f.persistence, f.agent)
        .await
        .unwrap();
    drain(&worker).await;
    assert_eq!(
        row(&f, Some(namespace)).await,
        expect("absent", "absent", "settled", 0, None)
    );
    assert!(
        f.namespaces("agent").await.is_empty(),
        "the removed row is gone"
    );
    assert!(
        credentials(&fake, &f)
            .iter()
            .all(|(_, _, revoked)| *revoked)
    );
    assert!(
        !fake.state.lock().unwrap().namespaces
            [&(tenant_key(&f), namespace.hyphenated().to_string())],
        "the remote namespace is a tombstone"
    );
    assert_eq!(
        f.ensure(NamespaceOwner::Agent(f.agent)).await,
        NamespaceReadiness::OwnerUnavailable
    );
}

#[tokio::test]
async fn memory_integration_reconciler_ambiguous_issue_leaves_a_stray_the_next_pass_revokes() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    let worker = worker(&f, &fake, PROVISIONER);
    f.enable().await.unwrap();
    drain(&worker).await;
    f.ensure(NamespaceOwner::Company).await;
    let (namespace, _) = f.namespaces("company").await[0].clone();
    fake.drop_after_write("credentials/issue");
    assert!(pass(&worker).await);
    assert_eq!(
        row(&f, Some(namespace)).await,
        expect("present", "none", "pending", 0, Some("outcome_unknown"))
    );
    assert!(stored(&f, namespace).await.is_none());
    due(&f).await;
    drain(&worker).await;
    let (credential, _) = stored(&f, namespace).await.unwrap();
    let listed = credentials(&fake, &f);
    assert_eq!(listed.len(), 2);
    assert_eq!(
        listed
            .iter()
            .filter(|(_, _, revoked)| !revoked)
            .map(|(id, _, _)| *id)
            .collect::<Vec<_>>(),
        [credential],
        "only the stored credential stays live"
    );
}

#[tokio::test]
async fn memory_scope_reconciler_turns_present_into_absent_when_the_owner_is_gone() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    let worker = worker(&f, &fake, PROVISIONER);
    f.enable().await.unwrap();
    drain(&worker).await;
    f.ensure(NamespaceOwner::Agent(f.agent)).await;
    let (namespace, _) = f.namespaces("agent").await[0].clone();
    // The agent goes without the memory hook.
    sqlx::query("UPDATE principals SET kind = 'external', agent_id = NULL WHERE agent_id = $1")
        .bind(f.agent)
        .execute(f.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM agents WHERE id = $1")
        .bind(f.agent)
        .execute(f.pool())
        .await
        .unwrap();
    drain(&worker).await;
    assert_eq!(
        row(&f, Some(namespace)).await,
        expect("absent", "absent", "settled", 0, None)
    );
    assert!(f.namespaces("agent").await.is_empty());
    assert!(credentials(&fake, &f).is_empty(), "never issued for nobody");
}

/// A full batch of failures leaves the due set: two passes without advancing time never reclaim
/// the same rows (src/application AGENTS, poison batch).
#[tokio::test]
async fn memory_integration_reconciler_poison_batch_leaves_the_due_set() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    let worker = worker(&f, &fake, PROVISIONER);
    f.enable().await.unwrap();
    for _ in 0..2 {
        let other = company(&f.persistence, f.owner).await;
        f.persistence
            .enable_memory(
                CompanyId::new(other),
                Fixture::actor(f.owner),
                0,
                PROVISIONER,
            )
            .await
            .unwrap();
    }
    fake.fault("tenants/create", &[503, 503, 503, 503]);
    for _ in 0..3 {
        assert!(pass(&worker).await);
    }
    assert!(
        !pass(&worker).await,
        "every failed row moved out of the due set"
    );
    let attempts: Vec<(i32, Option<String>)> =
        sqlx::query_as("SELECT attempts, outcome_code FROM memory_lifecycles ORDER BY company_id")
            .fetch_all(f.pool())
            .await
            .unwrap();
    assert_eq!(
        attempts,
        // An outage is charged to the transient count, never to the bounded attempts.
        vec![(0, Some("service_unavailable".to_owned())); 3]
    );
    assert_eq!(fake.calls().len(), 3);
}

#[tokio::test]
async fn memory_integration_reconciler_bounds_attempts_and_expired_leases() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    let worker = worker(&f, &fake, PROVISIONER);
    f.enable().await.unwrap();
    // The eighth attempt fails terminally instead of backing off again.
    sqlx::query("UPDATE memory_lifecycles SET attempts = 7")
        .execute(f.pool())
        .await
        .unwrap();
    // A definite protocol error (400) is bounded; an outage is not (see the outage test).
    fake.fault("tenants/create", &[400]);
    assert!(pass(&worker).await);
    assert_eq!(
        row(&f, None).await,
        expect("present", "none", "failed", 8, Some("protocol_error"))
    );
    // Failed rows are named at startup (the docs/deploy.md re-drive), with or without the URL.
    let (failed, more) = super::pending_lifecycles(f.pool(), &["failed"], 20)
        .await
        .unwrap();
    assert_eq!((failed.len(), more), (1, false));
    assert_eq!(failed[0].outcome_code.as_deref(), Some("protocol_error"));
    // A lease expiring on the last attempt is terminal at the next claim, not reclaimed.
    sqlx::query(
        "UPDATE memory_lifecycles SET state = 'claimed', attempts = 7, lease_owner = gen_random_uuid(), \
         lease_until = clock_timestamp() - interval '1 second', outcome_code = NULL",
    )
    .execute(f.pool())
    .await
    .unwrap();
    assert!(!pass(&worker).await);
    assert_eq!(
        row(&f, None).await,
        expect("present", "none", "failed", 8, Some("lease_expired"))
    );
}

#[tokio::test]
async fn memory_integration_reconciler_stops_on_shutdown() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    let worker = Arc::new(worker(&f, &fake, PROVISIONER));
    let stop = CancellationToken::new();
    let running = {
        let (worker, stop) = (worker.clone(), stop.clone());
        tokio::spawn(async move { worker.run(&stop).await })
    };
    f.enable().await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while row(&f, None).await.state != "settled" {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the supervised loop reconciles");
    stop.cancel();
    tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .expect("the loop joins promptly on shutdown")
        .unwrap();
}

/// A revoked provisioner credential: every admin call is 401, each pass backs off and the row
/// stays unsettled and visible; client credentials are untouched (E1 compromise steps).
#[tokio::test]
async fn memory_integration_reconciler_revoked_provisioner_backs_off_visibly() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(
        "msk_adm_0190f1a2b3c44d5e8f9a0b1c2d3e4f59_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    )
    .await;
    let worker = worker(&f, &fake, PROVISIONER);
    f.enable().await.unwrap();
    assert!(pass(&worker).await);
    assert_eq!(
        row(&f, None).await,
        expect(
            "present",
            "none",
            "pending",
            0,
            Some("provisioner_unauthorized")
        )
    );
    assert!(!pass(&worker).await, "backed off, not hot-looping");
}

async fn transient(f: &Fixture, namespace: Option<Uuid>) -> i32 {
    sqlx::query_scalar(
        "SELECT transient_retries FROM memory_lifecycles \
         WHERE company_id = $1 AND namespace_id IS NOT DISTINCT FROM $2",
    )
    .bind(f.company)
    .bind(namespace)
    .fetch_one(f.pool())
    .await
    .unwrap()
}

/// Review p7b r1 M1: a namespace waits for a disabled tenant without spending attempts, and a
/// failed provisioning is re-driven by enable or by the next lazy provisioning of that scope.
#[tokio::test]
async fn memory_scope_reconciler_failed_provisioning_is_redriven_not_permanent() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    let worker = worker(&f, &fake, PROVISIONER);
    f.enable().await.unwrap();
    drain(&worker).await;
    f.ensure(NamespaceOwner::Agent(f.agent)).await;
    let (namespace, _) = f.namespaces("agent").await[0].clone();
    f.disable().await.unwrap();
    drain(&worker).await;
    assert_eq!(
        row(&f, Some(namespace)).await,
        expect("present", "none", "pending", 0, Some("tenant_not_ready")),
        "waiting for the tenant spends no attempt"
    );
    // Exhausted anyway (any bounded failure): enable re-drives it with a fresh budget.
    let fail = "UPDATE memory_lifecycles SET state = 'failed', attempts = 8, \
                outcome_code = 'protocol_error' WHERE namespace_id = $1";
    sqlx::query(fail)
        .bind(namespace)
        .execute(f.pool())
        .await
        .unwrap();
    f.enable().await.unwrap();
    assert_eq!(
        row(&f, Some(namespace)).await,
        expect("present", "none", "pending", 0, None)
    );
    drain(&worker).await;
    due(&f).await;
    drain(&worker).await;
    assert_eq!(
        f.ensure(NamespaceOwner::Agent(f.agent)).await,
        NamespaceReadiness::Ready(NamespaceExternalId(namespace))
    );
    // A retryable failure of a still-provisioning scope is re-driven by `ensure` itself once
    // its cool-down has passed (r2 m1).
    f.ensure(NamespaceOwner::Company).await;
    let (company_namespace, _) = f.namespaces("company").await[0].clone();
    sqlx::query(
        "UPDATE memory_lifecycles SET state = 'failed', attempts = 8, outcome_code = 'lease_expired', \
         updated_at = clock_timestamp() - interval '1 hour' WHERE namespace_id = $1",
    )
    .bind(company_namespace)
    .execute(f.pool())
    .await
    .unwrap();
    assert_eq!(
        f.ensure(NamespaceOwner::Company).await,
        NamespaceReadiness::NotReady
    );
    assert_eq!(
        row(&f, Some(company_namespace)).await,
        expect("present", "none", "pending", 0, None)
    );
    drain(&worker).await;
    assert!(matches!(
        f.ensure(NamespaceOwner::Company).await,
        NamespaceReadiness::Ready(_)
    ));
}

/// Review p7b r1 M2: a deletion outlives an outage of any length; the transient count drives the
/// backoff and the bounded attempts stay untouched.
#[tokio::test]
async fn memory_scope_reconciler_deletion_survives_an_outage() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    let worker = worker(&f, &fake, PROVISIONER);
    f.enable().await.unwrap();
    drain(&worker).await;
    CompanyPersistence::delete(&f.persistence, f.company)
        .await
        .unwrap();
    fake.fault("tenants/delete", &[503; 12]);
    for _ in 0..12 {
        assert!(pass(&worker).await);
        due(&f).await;
    }
    assert_eq!(
        row(&f, None).await,
        expect(
            "absent",
            "present",
            "pending",
            0,
            Some("service_unavailable")
        )
    );
    assert_eq!(transient(&f, None).await, 12);
    // Stuck past the threshold: named at startup beside the failed rows (r2 m2).
    let (stuck, _) = super::pending_lifecycles(f.pool(), &["failed"], 20)
        .await
        .unwrap();
    assert_eq!(stuck.len(), 1);
    assert_eq!(stuck[0].state, "pending");
    drain(&worker).await;
    assert_eq!(
        row(&f, None).await,
        expect("absent", "absent", "settled", 0, None)
    );
}

/// Review p7b r1 M3: a superseding desired state starts a fresh budget, on the crash path (the
/// lease of an attempt-7 claim expires after the owner went) and on the completion path.
#[tokio::test]
async fn memory_scope_reconciler_superseded_generation_starts_a_fresh_attempt_budget() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let store: &dyn MemoryLifecycleStore = &f.persistence;
    let lease = Duration::from_secs(60);
    f.enable().await.unwrap();
    // Completion path: a tenant claim at attempt 7 is superseded by disable.
    sqlx::query("UPDATE memory_lifecycles SET attempts = 7 WHERE company_id = $1")
        .bind(f.company)
        .execute(f.pool())
        .await
        .unwrap();
    let claim = store
        .claim_lifecycle(Uuid::new_v4(), lease)
        .await
        .unwrap()
        .unwrap();
    f.disable().await.unwrap();
    assert_eq!(
        store
            .retry_lifecycle(&claim, "protocol_error", None, RetryBudget::Bounded)
            .await
            .unwrap(),
        LifecycleCompletion::Superseded
    );
    assert_eq!(
        row(&f, None).await,
        expect("disabled", "none", "pending", 0, None)
    );

    // Crash path: the reviewer's attempt-7 → absent → lease-expired case. The tenant row is put
    // out of reach so the claim takes the namespace.
    f.enable().await.unwrap();
    sqlx::query("UPDATE memory_lifecycles SET available_at = 'infinity' WHERE company_id = $1")
        .bind(f.company)
        .execute(f.pool())
        .await
        .unwrap();
    f.ensure(NamespaceOwner::Agent(f.agent)).await;
    let (namespace, _) = f.namespaces("agent").await[0].clone();
    sqlx::query("UPDATE memory_lifecycles SET attempts = 7 WHERE namespace_id = $1")
        .bind(namespace)
        .execute(f.pool())
        .await
        .unwrap();
    let claim = store
        .claim_lifecycle(Uuid::new_v4(), lease)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        claim.resource,
        LifecycleResource::Namespace(NamespaceExternalId(namespace))
    );
    AgentPersistence::delete(&f.persistence, f.agent)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE memory_lifecycles SET lease_until = clock_timestamp() - interval '1 second' \
         WHERE namespace_id = $1",
    )
    .bind(namespace)
    .execute(f.pool())
    .await
    .unwrap();
    let reclaimed = store
        .claim_lifecycle(Uuid::new_v4(), lease)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (reclaimed.id, reclaimed.desired, reclaimed.attempts),
        (claim.id, LifecycleDesired::Absent, 1),
        "only the crashed execution's own attempt is charged"
    );
}

/// m1: two concurrent claimants never own the same row; the expiry sweep skips a locked row.
#[tokio::test]
async fn memory_scope_reconciler_competing_claimants_get_distinct_rows() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.enable().await.unwrap();
    for owner in [
        NamespaceOwner::Company,
        NamespaceOwner::Agent(f.agent),
        NamespaceOwner::User(f.member),
    ] {
        f.ensure(owner).await;
    }
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let claimant = |persistence: crate::adapters::persistence::PostgresPersistence,
                    barrier: Arc<tokio::sync::Barrier>| {
        tokio::spawn(async move {
            barrier.wait().await;
            let mut claimed = Vec::new();
            while let Some(claim) = persistence
                .claim_lifecycle(Uuid::new_v4(), Duration::from_secs(60))
                .await
                .unwrap()
            {
                claimed.push(claim.id);
            }
            claimed
        })
    };
    let first = claimant(f.persistence.clone(), barrier.clone());
    let second = claimant(f.persistence.clone(), barrier);
    let mut all = first.await.unwrap();
    all.extend(second.await.unwrap());
    let total = all.len();
    all.sort();
    all.dedup();
    assert_eq!(
        (total, all.len()),
        (4, 4),
        "four rows, each claimed exactly once"
    );

    // An expired claim locked by another transaction (a completion in flight) is skipped by the
    // sweep, not waited on until `lock_timeout`.
    sqlx::query(
        "UPDATE memory_lifecycles SET attempts = 7, \
         lease_until = clock_timestamp() - interval '1 second'",
    )
    .execute(f.pool())
    .await
    .unwrap();
    let mut held = f.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM memory_lifecycles WHERE id = $1 FOR UPDATE")
        .bind(all[0])
        .execute(&mut *held)
        .await
        .unwrap();
    let started = std::time::Instant::now();
    f.persistence
        .claim_lifecycle(Uuid::new_v4(), Duration::from_secs(60))
        .await
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(1));
    let held_state: String =
        sqlx::query_scalar("SELECT state FROM memory_lifecycles WHERE id = $1")
            .bind(all[0])
            .fetch_one(f.pool())
            .await
            .unwrap();
    assert_eq!(held_state, "claimed", "the locked row was skipped");
    held.rollback().await.unwrap();
}

/// m1: a fenced completion blocked behind company deletion's namespace lock ends `Lost`.
#[tokio::test]
async fn memory_scope_reconciler_completion_racing_company_delete_is_lost() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    f.enable().await.unwrap();
    f.ensure(NamespaceOwner::Agent(f.agent)).await;
    sqlx::query("UPDATE memory_lifecycles SET available_at = 'infinity' WHERE resource = 'tenant'")
        .execute(f.pool())
        .await
        .unwrap();
    let claim = f
        .persistence
        .claim_lifecycle(Uuid::new_v4(), Duration::from_secs(60))
        .await
        .unwrap()
        .unwrap();
    let mut delete = f.pool().begin().await.unwrap();
    let pid = super::tests::backend_pid(&mut delete).await;
    sqlx::query("SELECT id FROM companies WHERE id = $1 FOR UPDATE")
        .bind(f.company)
        .execute(&mut *delete)
        .await
        .unwrap();
    retire_company_on(&mut delete, f.company).await.unwrap();
    let persistence = f.persistence.clone();
    let completion = tokio::spawn(async move {
        let credential = IssuedClientCredential {
            credential_id: Uuid::new_v4(),
            token: SecretString::from("msk_cli_racing"),
        };
        persistence.store_credential(&claim, credential, None).await
    });
    super::tests::blocked_by(f.pool(), pid).await;
    sqlx::query("DELETE FROM companies WHERE id = $1")
        .bind(f.company)
        .execute(&mut *delete)
        .await
        .unwrap();
    delete.commit().await.unwrap();
    assert_eq!(
        completion.await.unwrap().unwrap(),
        LifecycleCompletion::Lost
    );
}

/// m2: a mismatched provisioner id is refused on the deletion paths too, without contact and
/// without an attempt (the namespace-absent 404 settle must never run for a foreign manager).
#[tokio::test]
async fn memory_scope_reconciler_mismatch_refuses_tenant_and_namespace_deletion() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    // A rotated credential the service does not confirm: the fake already holds this tenant
    // under the original token.
    fake.state.lock().unwrap().tenants.insert(
        tenant_key(&f),
        (
            crate::adapters::memory_service::contract_fake::FakeTenantState::Active,
            ADMIN.to_owned(),
        ),
    );
    let other = rotated(&f, &fake);
    f.enable().await.unwrap();
    f.ensure(NamespaceOwner::Agent(f.agent)).await;
    let (namespace, _) = f.namespaces("agent").await[0].clone();
    AgentPersistence::delete(&f.persistence, f.agent)
        .await
        .unwrap();
    drain(&other).await;
    assert_eq!(
        row(&f, Some(namespace)).await,
        expect("absent", "none", "pending", 0, Some("provisioner_mismatch"))
    );
    CompanyPersistence::delete(&f.persistence, f.company)
        .await
        .unwrap();
    due(&f).await;
    drain(&other).await;
    assert_eq!(
        row(&f, None).await,
        expect("absent", "none", "pending", 0, Some("provisioner_mismatch"))
    );
    assert!(
        destructive(&fake).is_empty(),
        "only the management probe under a foreign id: {:?}",
        fake.calls()
    );
}

/// m3: shutdown during an in-flight request: the pass is interrupted, charged to the transient
/// count, its lease released, and the loop joins promptly.
#[tokio::test]
async fn memory_integration_reconciler_shutdown_during_active_work_releases_the_lease() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    fake.state
        .lock()
        .unwrap()
        .delays
        .insert("tenants/create".into(), Duration::from_secs(30));
    let worker = Arc::new(worker(&f, &fake, PROVISIONER));
    let stop = CancellationToken::new();
    f.enable().await.unwrap();
    let running = {
        let (worker, stop) = (worker.clone(), stop.clone());
        tokio::spawn(async move { worker.run(&stop).await })
    };
    tokio::time::timeout(Duration::from_secs(10), async {
        while !fake.state.lock().unwrap().delays.is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the request is in flight");
    stop.cancel();
    tokio::time::timeout(Duration::from_secs(2), running)
        .await
        .expect("the loop joins promptly while a request is in flight")
        .unwrap();
    assert_eq!(
        row(&f, None).await,
        expect("present", "none", "pending", 0, Some("interrupted"))
    );
    assert_eq!(transient(&f, None).await, 1);
    let lease: Option<Uuid> =
        sqlx::query_scalar("SELECT lease_owner FROM memory_lifecycles WHERE company_id = $1")
            .bind(f.company)
            .fetch_one(f.pool())
            .await
            .unwrap();
    assert!(lease.is_none());
}

/// r2 n2: a batch whose revocations all answer 404 while the listing still shows them is not
/// progress, so it spends an attempt instead of looping with zero delay.
#[tokio::test]
async fn memory_integration_reconciler_revocation_without_progress_spends_an_attempt() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    let worker = worker(&f, &fake, PROVISIONER);
    f.enable().await.unwrap();
    drain(&worker).await;
    f.ensure(NamespaceOwner::Agent(f.agent)).await;
    let (namespace, _) = f.namespaces("agent").await[0].clone();
    let label = client_credential_label(NamespaceExternalId(namespace));
    for _ in 0..17 {
        fake.state
            .lock()
            .unwrap()
            .credentials
            .insert(Uuid::new_v4(), (tenant_key(&f), label.clone(), false));
    }
    fake.fault("credentials/revoke", &[404; 16]);
    assert!(pass(&worker).await);
    assert_eq!(
        row(&f, Some(namespace)).await,
        expect(
            "present",
            "none",
            "pending",
            1,
            Some("stray_revocation_stuck")
        )
    );
    assert!(!pass(&worker).await, "backed off, not looping");
}

/// Review p7b r3 m1: a tenant deleted service-side answers 409 `conflict` to namespace create.
/// That is the tenant's failure, never our namespace's tombstone: repeated `ensure` calls (even
/// past the cool-down) mint no new namespace ids, and settings show it at tenant level.
#[tokio::test]
async fn memory_scope_reconciler_deleted_tenant_conflict_mints_no_new_namespaces() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    let worker = worker(&f, &fake, PROVISIONER);
    f.enable().await.unwrap();
    drain(&worker).await;
    fake.state
        .lock()
        .unwrap()
        .tenants
        .get_mut(&tenant_key(&f))
        .unwrap()
        .0 = crate::adapters::memory_service::contract_fake::FakeTenantState::Deleted;
    f.ensure(NamespaceOwner::Agent(f.agent)).await;
    let (namespace, _) = f.namespaces("agent").await[0].clone();
    drain(&worker).await;
    assert_eq!(
        row(&f, Some(namespace)).await,
        expect("present", "none", "failed", 0, Some("tenant_conflict"))
    );
    sqlx::query(
        "UPDATE memory_lifecycles SET updated_at = clock_timestamp() - interval '1 hour' \
         WHERE namespace_id = $1",
    )
    .bind(namespace)
    .execute(f.pool())
    .await
    .unwrap();
    for _ in 0..3 {
        assert_eq!(
            f.ensure(NamespaceOwner::Agent(f.agent)).await,
            NamespaceReadiness::NotReady
        );
        drain(&worker).await;
    }
    assert_eq!(
        f.namespaces("agent").await,
        [(namespace, "provisioning".into())]
    );
    let lifecycles: i64 =
        sqlx::query_scalar("SELECT count(*) FROM memory_lifecycles WHERE resource = 'namespace'")
            .fetch_one(f.pool())
            .await
            .unwrap();
    assert_eq!(lifecycles, 1, "no stray lifecycle rows");
    assert_eq!(
        row(&f, Some(namespace)).await.outcome_code.as_deref(),
        Some("tenant_conflict")
    );
    let view = MemorySettings::new(&f.persistence, Some(PROVISIONER))
        .view(f.scope(), Fixture::actor(f.owner))
        .await
        .unwrap();
    let html = crate::adapters::http::pages::company_memory::section(f.company, &view, None);
    assert!(
        html.contains("tenant as deleted or managed elsewhere"),
        "{html}"
    );
}

#[path = "restore_tests.rs"]
mod restore;
