//! A memory-service restore as OpenBot reconciles it (p06 gate A4-4, A4-8, A4-9): the restore
//! SQL in docs/deploy.md (run verbatim), the credential check of a re-driven present step, and a
//! `401` that re-verifies a credential without failing the intent waiting on it.
use super::*;
use crate::adapters::persistence::memory::deliveries::{
    LockedRun, SaveIntent, SaveScope, freeze_save_on,
};
use crate::domain::workflow::{ExecutionId, RunId};
use crate::entities::access_level::AccessLevel;

/// The documented restore SQL with `backup_time` filled in.
fn restore_sql(backup_time: &str) -> String {
    let doc = include_str!("../../../../docs/deploy.md");
    let marked = &doc[doc
        .find("<!-- tested: memory-restore-sql -->")
        .expect("the restore SQL is marked in docs/deploy.md")..];
    let start = marked.find("```sql\n").unwrap() + "```sql\n".len();
    let end = start + marked[start..].find("```").unwrap();
    let sql = &marked[start..end];
    assert!(sql.contains(":'backup_time'"));
    sql.replace(":'backup_time'", &format!("'{backup_time}'"))
}

async fn now(f: &Fixture) -> String {
    sqlx::query_scalar("SELECT clock_timestamp()::text")
        .fetch_one(f.pool())
        .await
        .unwrap()
}

/// The delivery worker as composed in production: every call through the re-verifying client.
fn deliveries(f: &Fixture, fake: &ContractFake) -> MemoryDeliveryWorker {
    let client: Arc<dyn MemoryClient> = Arc::new(ReverifyingClient::new(
        http(fake),
        Arc::new(f.persistence.clone()),
    ));
    MemoryDeliveryWorker::new(Arc::new(f.persistence.clone()), client)
}

/// A pending company-scope save intent under `key`.
async fn save(f: &Fixture, key: &str) {
    let facts = vec!["Renewal is due in March.".to_owned()];
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
    let mut tx = f.pool().begin().await.unwrap();
    freeze_save_on(
        &mut tx,
        &intent,
        LockedRun {
            ceiling: AccessLevel::L6,
            content: AccessLevel::L0,
            content_exact: true,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

async fn intent(f: &Fixture, key: &str) -> (String, Option<String>, i32, i32, bool) {
    sqlx::query_as(
        "SELECT state, outcome_code, attempts, transient_retries, request IS NOT NULL \
         FROM memory_deliveries WHERE company_id = $1 AND idempotency_key = $2",
    )
    .bind(f.company)
    .bind(key)
    .fetch_one(f.pool())
    .await
    .unwrap()
}

async fn deliveries_due(f: &Fixture) {
    sqlx::query(
        "UPDATE memory_deliveries SET available_at = clock_timestamp() WHERE state = 'pending'",
    )
    .execute(f.pool())
    .await
    .unwrap();
}

async fn namespace_state(f: &Fixture, namespace: Uuid) -> String {
    sqlx::query_scalar("SELECT state FROM memory_namespaces WHERE id = $1")
        .bind(namespace)
        .fetch_one(f.pool())
        .await
        .unwrap()
}

async fn generation_of(f: &Fixture, namespace: Uuid) -> i64 {
    sqlx::query_scalar("SELECT generation FROM memory_lifecycles WHERE namespace_id = $1")
        .bind(namespace)
        .fetch_one(f.pool())
        .await
        .unwrap()
}

/// The company namespace, provisioned through the lifecycle worker.
async fn provisioned(f: &Fixture, worker: &MemoryLifecycleWorker) -> (Uuid, Uuid) {
    f.ensure(NamespaceOwner::Company).await;
    drain(worker).await;
    let (namespace, _) = f.namespaces("company").await[0].clone();
    let (credential, _) = stored(f, namespace).await.unwrap();
    (namespace, credential)
}

/// A4-8 and F4: a namespace and credential created after the backup are unknown to the restored
/// service. The documented SQL re-drives the row; its present step re-creates the namespace and
/// swaps in a new credential while the namespace stays `ready`. Open captures are cancelled, and
/// the open save is then delivered with the new credential.
#[tokio::test]
async fn memory_operations_a_restored_service_without_the_stored_credential_gets_a_new_one() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    let worker = worker(&f, &fake, PROVISIONER);
    f.enable().await.unwrap();
    drain(&worker).await;
    let backup_time = now(&f).await;
    let (namespace, old) = provisioned(&f, &worker).await;
    save(&f, "ob.s.after-backup").await;
    sqlx::query(
        "INSERT INTO memory_deliveries (company_id, id, namespace_id, kind, run_id, \
           exchange_id, idempotency_key, expires_at, request, disable_epoch, state, available_at) \
         SELECT $1, gen_random_uuid(), $2, 'generation', gen_random_uuid(), gen_random_uuid(), \
                'ob.g.after-backup', now() + interval '1 day', \
                '{\"version\": \"1\", \"body\": {}}'::jsonb, tenant.disable_epoch, 'pending', \
                now() + interval '1 hour' \
           FROM memory_tenants AS tenant WHERE tenant.company_id = $1",
    )
    .bind(f.company)
    .bind(namespace)
    .execute(f.pool())
    .await
    .unwrap();
    // The restore: the service never had this namespace or its credential.
    fake.forget_credential(old);
    fake.state
        .lock()
        .unwrap()
        .namespaces
        .remove(&(tenant_key(&f), namespace.hyphenated().to_string()));

    sqlx::raw_sql(&restore_sql(&backup_time))
        .execute(f.pool())
        .await
        .unwrap();
    assert_eq!(
        row(&f, Some(namespace)).await,
        expect("present", "present", "pending", 0, Some("service_restored"))
    );
    assert_eq!(
        row(&f, None).await,
        expect("present", "present", "settled", 0, None),
        "the tenant row predates the backup and is left alone"
    );
    assert_eq!(
        intent(&f, "ob.g.after-backup").await,
        (
            "cancelled".into(),
            Some("service_restored".into()),
            0,
            0,
            false
        ),
        "an open capture is cancelled, never sent twice"
    );
    assert_eq!(intent(&f, "ob.s.after-backup").await.0, "pending");
    assert_eq!(namespace_state(&f, namespace).await, "ready");

    drain(&worker).await;
    assert_eq!(
        row(&f, Some(namespace)).await,
        expect("present", "present", "settled", 0, None)
    );
    assert_eq!(namespace_state(&f, namespace).await, "ready");
    let (new, _) = stored(&f, namespace).await.unwrap();
    assert_ne!(new, old);
    assert_eq!(
        credentials(&fake, &f),
        vec![(
            new,
            client_credential_label(NamespaceExternalId(namespace)),
            false
        )]
    );
    let deliveries = deliveries(&f, &fake);
    deliveries_due(&f).await;
    assert!(deliveries.pass(&CancellationToken::new()).await.unwrap());
    assert_eq!(intent(&f, "ob.s.after-backup").await.0, "delivered");
}

/// A4-9 and F4: a save pending while its credential is refused retries without an attempt,
/// queues the re-verification, and is delivered after the new credential is swapped in. The
/// namespace never leaves `ready`, so the claim never fails the intent.
#[tokio::test]
async fn memory_operations_a_delivery_pending_through_a_reissue_is_delivered() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    let worker = worker(&f, &fake, PROVISIONER);
    f.enable().await.unwrap();
    drain(&worker).await;
    let (namespace, old) = provisioned(&f, &worker).await;
    save(&f, "ob.s.through-reissue").await;
    fake.forget_credential(old);
    let deliveries = deliveries(&f, &fake);
    deliveries_due(&f).await;
    assert!(deliveries.pass(&CancellationToken::new()).await.unwrap());
    assert_eq!(
        intent(&f, "ob.s.through-reissue").await,
        ("pending".into(), None, 0, 1, true),
        "transient: no attempt spent, the request kept"
    );
    assert_eq!(
        row(&f, Some(namespace)).await,
        expect(
            "present",
            "present",
            "pending",
            0,
            Some("credential_unverified")
        )
    );
    assert_eq!(namespace_state(&f, namespace).await, "ready");
    drain(&worker).await;
    let (new, _) = stored(&f, namespace).await.unwrap();
    assert_ne!(new, old);
    assert_eq!(namespace_state(&f, namespace).await, "ready");
    // The old id no longer moves anything: the namespace stores the new one.
    assert!(
        !f.persistence
            .credential_refused(TenantExternalId(f.company), old)
            .await
            .unwrap()
    );
    deliveries_due(&f).await;
    assert!(deliveries.pass(&CancellationToken::new()).await.unwrap());
    assert_eq!(intent(&f, "ob.s.through-reissue").await.0, "delivered");
}

/// A4-9: concurrent `401`s for one credential re-pend its lifecycle once; a `403` (a grant or
/// scope refusal) re-pends nothing; the call itself still fails.
#[tokio::test]
async fn memory_operations_a_delivery_401_reverifies_the_namespace_credential() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    let worker = worker(&f, &fake, PROVISIONER);
    f.enable().await.unwrap();
    drain(&worker).await;
    let (namespace, id) = provisioned(&f, &worker).await;
    let (_, envelope) = stored(&f, namespace).await.unwrap();
    let token = f
        .persistence
        .credential_cipher()
        .unwrap()
        .open_envelope(
            &CredentialContext::memory_client_credential(f.company, namespace),
            &envelope,
        )
        .unwrap();
    let credential = ClientCredential {
        tenant: TenantExternalId(f.company),
        id,
        token,
    };
    let client = Arc::new(ReverifyingClient::new(
        http(&fake),
        Arc::new(f.persistence.clone()),
    ));
    let search = MemorySearch {
        namespace: NamespaceExternalId(namespace),
        read_ceiling: 6,
        query: "renewal".into(),
        limit: 5,
    };
    fake.fault("observations/search", &[403]);
    assert!(matches!(
        client.search(&credential, &search).await,
        Err(MemoryServiceError::Forbidden { .. })
    ));
    let before = generation_of(&f, namespace).await;
    assert_eq!(
        row(&f, Some(namespace)).await.state,
        "settled",
        "a 403 re-pends nothing"
    );
    fake.forget_credential(id);
    let calls: Vec<_> = (0..3)
        .map(|_| {
            let client = client.clone();
            let credential = ClientCredential {
                tenant: credential.tenant,
                id: credential.id,
                token: credential.token.clone(),
            };
            let search = search.clone();
            tokio::spawn(async move { client.search(&credential, &search).await })
        })
        .collect();
    for call in calls {
        assert!(matches!(
            call.await.unwrap(),
            Err(MemoryServiceError::Unauthorized)
        ));
    }
    assert_eq!(generation_of(&f, namespace).await, before + 1, "moved once");
    assert_eq!(
        row(&f, Some(namespace)).await,
        expect(
            "present",
            "present",
            "pending",
            0,
            Some("credential_unverified")
        )
    );
}

/// A4-4: a deletion made after the backup comes back with the restore; the documented SQL
/// re-drives its settled `absent` row, which deletes the namespace again and revokes its
/// credential again.
#[tokio::test]
async fn memory_operations_a_redriven_absent_lifecycle_deletes_a_restored_namespace_again() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let fake = ContractFake::start(ADMIN).await;
    let worker = worker(&f, &fake, PROVISIONER);
    f.enable().await.unwrap();
    drain(&worker).await;
    f.ensure(NamespaceOwner::Agent(f.agent)).await;
    drain(&worker).await;
    let (namespace, _) = f.namespaces("agent").await[0].clone();
    let (credential, _) = stored(&f, namespace).await.unwrap();
    let backup_time = now(&f).await;
    AgentPersistence::delete(&f.persistence, f.agent)
        .await
        .unwrap();
    drain(&worker).await;
    assert_eq!(
        row(&f, Some(namespace)).await,
        expect("absent", "absent", "settled", 0, None)
    );
    let key = (tenant_key(&f), namespace.hyphenated().to_string());
    // The restore: the service holds the namespace and its live credential again.
    {
        let mut state = fake.state.lock().unwrap();
        state.namespaces.insert(key.clone(), true);
        state.credentials.get_mut(&credential).unwrap().2 = false;
    }
    sqlx::raw_sql(&restore_sql(&backup_time))
        .execute(f.pool())
        .await
        .unwrap();
    assert_eq!(
        row(&f, Some(namespace)).await,
        expect("absent", "absent", "pending", 0, Some("service_restored"))
    );
    drain(&worker).await;
    assert_eq!(
        row(&f, Some(namespace)).await,
        expect("absent", "absent", "settled", 0, None)
    );
    let state = fake.state.lock().unwrap();
    assert_eq!(state.namespaces.get(&key), Some(&false), "deleted again");
    assert!(state.credentials[&credential].2, "revoked again");
}
