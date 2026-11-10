use super::contract_fake::ContractFake;
use super::*;
use crate::infra::memory_config::ProvisionerToken;

/// Every credential of the tenant in one page.
const EVERY: CredentialQuery<'static> = CredentialQuery {
    label: None,
    active_only: false,
    after: None,
    limit: 1000,
};

const ADMIN: &str =
    "msk_adm_0190f1a2b3c44d5e8f9a0b1c2d3e4f50_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

fn client(base_url: &str, token: &str) -> MemoryServiceHttp {
    MemoryServiceHttp::new(&MemoryServiceConfig {
        base_url: base_url.to_owned(),
        provisioner: ProvisionerToken::parse(SecretString::from(token)).unwrap(),
        request_timeout: Duration::from_secs(5),
        recall_deadline: Duration::from_secs(1),
        interactive_in_flight: 2,
        background_in_flight: 2,
    })
    .unwrap()
}

#[tokio::test]
async fn memory_integration_adapter_admin_round_trip_matches_the_published_contract() {
    let fake = ContractFake::start(ADMIN).await;
    let http = client(&fake.base_url, ADMIN);
    let tenant = TenantExternalId(Uuid::new_v4());
    let namespace = NamespaceExternalId(Uuid::new_v4());

    let created = http.create_tenant(tenant).await.unwrap();
    assert_eq!(created.state, RemoteTenantState::Active);
    assert_eq!(created.generation_daily_token_cap, 0);
    // Idempotent for the managing admin.
    assert_eq!(
        http.create_tenant(tenant).await.unwrap().state,
        RemoteTenantState::Active
    );
    assert_eq!(
        http.create_namespace(tenant, namespace).await.unwrap(),
        RemoteNamespaceState::Active
    );
    let label = client_credential_label(namespace);
    let issued = http
        .issue_client_credential(tenant, namespace, &label)
        .await
        .unwrap();
    assert!(issued.token.expose_secret().starts_with("msk_cli_"));
    let listed = http
        .list_client_credentials(tenant, EVERY)
        .await
        .unwrap()
        .credentials;
    assert_eq!(listed.len(), 1);
    assert_eq!(
        (
            listed[0].credential_id,
            listed[0].label.as_str(),
            listed[0].revoked
        ),
        (issued.credential_id, label.as_str(), false)
    );
    http.revoke_client_credential(tenant, issued.credential_id)
        .await
        .unwrap();
    assert!(
        http.list_client_credentials(tenant, EVERY)
            .await
            .unwrap()
            .credentials[0]
            .revoked
    );
    assert_eq!(
        http.disable_tenant(tenant).await.unwrap().state,
        RemoteTenantState::Disabled
    );
    assert_eq!(
        http.enable_tenant(tenant).await.unwrap().state,
        RemoteTenantState::Active
    );
    assert_eq!(
        http.delete_namespace(tenant, namespace).await.unwrap(),
        RemoteNamespaceState::Deleted
    );
    assert_eq!(
        http.delete_tenant(tenant).await.unwrap().state,
        RemoteTenantState::Deleted
    );
    // Deleting our own tombstone answers 200 (the reconciler's completion signal, E2).
    assert_eq!(
        http.delete_tenant(tenant).await.unwrap().state,
        RemoteTenantState::Deleted
    );
    // A tombstone is never recreated.
    assert!(matches!(
        http.create_tenant(tenant).await,
        Err(MemoryServiceError::Conflict { .. })
    ));
    assert_eq!(fake.calls().len(), 13);
}

#[tokio::test]
async fn memory_scope_foreign_managed_tenant_is_conflict_on_create_and_not_found_on_delete() {
    let fake = ContractFake::start(ADMIN).await;
    let http = client(&fake.base_url, ADMIN);
    let tenant = Uuid::new_v4();
    fake.foreign_tenant(tenant);
    assert!(matches!(
        http.create_tenant(TenantExternalId(tenant)).await,
        Err(MemoryServiceError::Conflict { .. })
    ));
    // Never "done": a 404 is the reconciler's terminal tenant_unmanageable.
    assert_eq!(
        http.delete_tenant(TenantExternalId(tenant))
            .await
            .unwrap_err(),
        MemoryServiceError::NotFound
    );
}

#[tokio::test]
async fn memory_integration_adapter_revoked_provisioner_is_unauthorized() {
    let fake = ContractFake::start(ADMIN).await;
    let other =
        "msk_adm_0190f1a2b3c44d5e8f9a0b1c2d3e4f51_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let http = client(&fake.base_url, other);
    assert_eq!(
        http.create_tenant(TenantExternalId(Uuid::new_v4()))
            .await
            .unwrap_err(),
        MemoryServiceError::Unauthorized
    );
}

#[tokio::test]
async fn memory_integration_adapter_classifies_statuses_and_unreachable_service() {
    let fake = ContractFake::start(ADMIN).await;
    let http = client(&fake.base_url, ADMIN);
    fake.fault("tenants/create", &[503, 504, 429, 403, 500]);
    let tenant = TenantExternalId(Uuid::new_v4());
    assert_eq!(
        http.create_tenant(tenant).await.unwrap_err(),
        MemoryServiceError::Unavailable
    );
    assert_eq!(
        http.create_tenant(tenant).await.unwrap_err(),
        MemoryServiceError::Ambiguous
    );
    assert!(matches!(
        http.create_tenant(tenant).await,
        Err(MemoryServiceError::Capacity { .. })
    ));
    assert_eq!(
        http.create_tenant(tenant).await.unwrap_err(),
        MemoryServiceError::Forbidden {
            code: "injected".into()
        }
    );
    // A 500 follows a request the service received: it may have run.
    assert_eq!(
        http.create_tenant(tenant).await.unwrap_err(),
        MemoryServiceError::Ambiguous
    );
    // A closed port: nothing was sent.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let closed = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    assert_eq!(
        client(&closed, ADMIN)
            .create_tenant(tenant)
            .await
            .unwrap_err(),
        MemoryServiceError::Unavailable
    );
}

#[test]
fn memory_integration_adapter_error_taxonomy() {
    let body = |code: &str, details: Value| {
        serde_json::to_vec(&json!({"error":{"code":code,"message":"m","retryable":false,"details":details},"request_id":"r"})).unwrap()
    };
    assert_eq!(classify(401, b"", None), MemoryServiceError::Unauthorized);
    assert_eq!(classify(404, b"", None), MemoryServiceError::NotFound);
    assert_eq!(classify(410, b"", None), MemoryServiceError::Expired);
    assert_eq!(
        classify(409, &body("idempotency_conflict", Value::Null), None),
        MemoryServiceError::Conflict {
            code: "idempotency_conflict".into()
        }
    );
    assert_eq!(
        classify(
            422,
            &body(
                "classification_refused",
                json!({"reason":"source_above_ceiling"})
            ),
            None
        ),
        MemoryServiceError::Refused {
            reason: "source_above_ceiling".into()
        }
    );
    assert_eq!(
        classify(429, b"", Some(3)),
        MemoryServiceError::Capacity {
            retry_after_secs: Some(3)
        }
    );
    assert_eq!(
        classify(429, &body("spend_exhausted", Value::Null), Some(3)),
        MemoryServiceError::Refused {
            reason: "spend_exhausted".into()
        }
    );
    assert_eq!(
        classify(503, &body("unavailable", Value::Null), None),
        MemoryServiceError::Unavailable
    );
    // Only the service's own envelope makes a 503 definite; a proxy's may follow an executed request.
    for status in [500, 502, 503, 504, 418] {
        assert_eq!(
            classify(status, b"<html>bad gateway</html>", None),
            MemoryServiceError::Ambiguous,
            "{status}"
        );
    }
    assert_eq!(classify(400, b"", None), MemoryServiceError::Protocol);
    assert_eq!(classify(413, b"", None), MemoryServiceError::Protocol);
}

#[tokio::test]
async fn memory_integration_adapter_connection_dropped_after_the_write_is_ambiguous() {
    let fake = ContractFake::start(ADMIN).await;
    let http = client(&fake.base_url, ADMIN);
    let tenant = TenantExternalId(Uuid::new_v4());
    fake.drop_after_write("tenants/create");
    assert_eq!(
        http.create_tenant(tenant).await.unwrap_err(),
        MemoryServiceError::Ambiguous
    );
    // The write landed although no answer arrived.
    assert!(
        fake.state
            .lock()
            .unwrap()
            .tenants
            .contains_key(&tenant.0.hyphenated().to_string())
    );
    assert_eq!(
        http.create_tenant(tenant).await.unwrap().state,
        RemoteTenantState::Active
    );
}

/// The fake answers as the service does where the reconciler depends on it (review p7b1 M2).
#[tokio::test]
async fn memory_integration_contract_fake_matches_the_service_namespace_and_tenant_answers() {
    let fake = ContractFake::start(ADMIN).await;
    let http = client(&fake.base_url, ADMIN);
    let tenant = TenantExternalId(Uuid::new_v4());
    let namespace = NamespaceExternalId(Uuid::new_v4());
    http.create_tenant(tenant).await.unwrap();
    // A namespace whose create never landed: 404 and no tombstone, so a later create succeeds.
    assert_eq!(
        http.delete_namespace(tenant, namespace).await.unwrap_err(),
        MemoryServiceError::NotFound
    );
    let label = client_credential_label(namespace);
    // Issue naming a namespace that is not active is the client's error.
    assert_eq!(
        http.issue_client_credential(tenant, namespace, &label)
            .await
            .err(),
        Some(MemoryServiceError::Protocol)
    );
    http.create_namespace(tenant, namespace).await.unwrap();
    let issued = http
        .issue_client_credential(tenant, namespace, &label)
        .await
        .unwrap();
    http.delete_namespace(tenant, namespace).await.unwrap();
    assert_eq!(
        http.create_namespace(tenant, namespace).await.unwrap_err(),
        MemoryServiceError::Conflict {
            code: "namespace_deleted".into()
        }
    );
    http.disable_tenant(tenant).await.unwrap();
    let other = NamespaceExternalId(Uuid::new_v4());
    assert_eq!(
        http.create_namespace(tenant, other).await.unwrap_err(),
        MemoryServiceError::Forbidden {
            code: "tenant_disabled".into()
        }
    );
    http.delete_tenant(tenant).await.unwrap();
    // A deleted tenant is a uniform conflict for namespace and credential work.
    assert!(matches!(
        http.create_namespace(tenant, other).await,
        Err(MemoryServiceError::Conflict { .. })
    ));
    assert!(matches!(
        http.issue_client_credential(tenant, other, &label).await,
        Err(MemoryServiceError::Conflict { .. })
    ));
    // Tenant deletion leaves credentials unrevoked; the tombstone makes them unusable.
    let listed = http
        .list_client_credentials(tenant, EVERY)
        .await
        .unwrap()
        .credentials;
    assert_eq!(listed.len(), 1);
    assert_eq!(
        (listed[0].credential_id, listed[0].revoked),
        (issued.credential_id, false)
    );
}

#[tokio::test]
async fn memory_integration_contract_fake_reports_violations_on_the_test_thread() {
    let fake = ContractFake::start(ADMIN).await;
    let http = client(&fake.base_url, ADMIN);
    // An id outside the contract's pattern is a request violation, answered 500 and recorded.
    let body = json!({"external_id": "not allowed!"});
    let error = http
        .admin::<_, Value>(Uuid::new_v4(), "tenants/create", &body)
        .await
        .unwrap_err();
    assert_eq!(error, MemoryServiceError::Ambiguous);
    let violations = fake.state.lock().unwrap().violations.clone();
    assert_eq!(violations.len(), 1, "{violations:?}");
    let reported = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fake.assert_clean()));
    assert!(reported.is_err(), "a violation fails the test");
    fake.state.lock().unwrap().violations.clear();
}

#[tokio::test]
async fn memory_integration_adapter_per_company_share_and_global_pool() {
    let pools = MemoryPools::new(2, 2);
    let company = Uuid::new_v4();
    let soon = || Instant::now() + Duration::from_millis(50);
    let first = pools
        .acquire(MemoryPool::Background, company, soon())
        .await
        .unwrap();
    // The background share per company is one operation.
    assert_eq!(
        pools
            .acquire(MemoryPool::Background, company, soon())
            .await
            .err(),
        Some(MemoryServiceError::Unavailable)
    );
    // Another company still gets a slot; interactive work is independent.
    let other = pools
        .acquire(MemoryPool::Background, Uuid::new_v4(), soon())
        .await
        .unwrap();
    let interactive = pools
        .acquire(MemoryPool::Interactive, company, soon())
        .await
        .unwrap();
    // The global background pool (2) is now full.
    assert!(
        pools
            .acquire(MemoryPool::Background, Uuid::new_v4(), soon())
            .await
            .is_err()
    );
    drop((first, other, interactive));
    assert!(
        pools
            .acquire(MemoryPool::Background, company, soon())
            .await
            .is_ok()
    );
}

/// E1: a tool's multi-scope search is one interactive operation. Its three requests are held by
/// the service at the same time under one company slot, so another operation of the same company
/// still gets the second slot of its share (2) meanwhile. With a permit per request at most two
/// could ever be held together, so the wait for three would time out.
#[tokio::test]
async fn memory_integration_a_multi_scope_search_is_one_interactive_operation() {
    let fake = ContractFake::start(ADMIN).await;
    let http = std::sync::Arc::new(client(&fake.base_url, ADMIN));
    let tenant = Uuid::new_v4();
    let namespaces: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
    let credentials: Vec<ClientCredential> = namespaces
        .iter()
        .map(|namespace| ClientCredential {
            tenant: TenantExternalId(tenant),
            id: Uuid::nil(),
            token: SecretString::from(fake.client_token(tenant, *namespace)),
        })
        .collect();
    fake.state
        .lock()
        .unwrap()
        .holds
        .insert("observations/search".into(), Duration::from_millis(1500));
    let search = tokio::spawn({
        let http = http.clone();
        let credentials = credentials
            .iter()
            .map(|credential| ClientCredential {
                tenant: credential.tenant,
                id: credential.id,
                token: credential.token.clone(),
            })
            .collect::<Vec<_>>();
        let namespaces = namespaces.clone();
        async move {
            let searches: Vec<ScopedSearch<'_>> = credentials
                .iter()
                .zip(&namespaces)
                .map(|(credential, namespace)| ScopedSearch {
                    credential,
                    search: MemorySearch {
                        namespace: NamespaceExternalId(*namespace),
                        read_ceiling: 9,
                        query: "anything".into(),
                        limit: 5,
                    },
                })
                .collect();
            http.search_scopes(&searches).await
        }
    });
    let held = |map: &str| {
        let state = fake.state.lock().unwrap();
        let counts = if map == "now" {
            &state.held_now
        } else {
            &state.held_peak
        };
        counts.get("observations/search").copied().unwrap_or(0)
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        while held("now") < 3 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("all three requests held by the service at the same time");
    assert_eq!(
        http.interactive_in_use(tenant),
        1,
        "one slot for the operation"
    );
    let fetched = http
        .fetch(
            &credentials[0],
            NamespaceExternalId(namespaces[0]),
            9,
            &[Uuid::new_v4()],
        )
        .await
        .unwrap();
    assert!(fetched.items.is_empty());
    assert!(
        !search.is_finished(),
        "the other operation did not wait for it"
    );
    assert_eq!(search.await.unwrap().unwrap().len(), 3);
    assert_eq!(held("peak"), 3);
    assert_eq!(http.interactive_in_use(tenant), 0);
}

/// E7 adapter round trip against the contract-checked fake: compact packs within the ceiling and
/// budget, in request order, and validation statuses for frozen items.
#[tokio::test]
async fn memory_preparation_adapter_packs_and_validates_through_the_contract() {
    let fake = ContractFake::start(ADMIN).await;
    let http = client(&fake.base_url, ADMIN);
    let tenant = Uuid::new_v4();
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let credential = |namespace| ClientCredential {
        tenant: TenantExternalId(tenant),
        id: Uuid::nil(),
        token: SecretString::from(fake.client_token(tenant, namespace)),
    };
    let (ca, cb) = (credential(a), credential(b));
    let seed = |namespace: Uuid, level: u8, minute: u8, body: &str| {
        let id = Uuid::new_v4();
        fake.state.lock().unwrap().observations.push((
            namespace.hyphenated().to_string(),
            json!({"observation_id":id,"kind":"fact","title":"Note","body":body,
                "occurred_at":format!("2026-11-01T00:{minute:02}:00Z"),"access_level":level,
                "revision":1,"content_sha256":"0".repeat(64),"sources":[],"index_state":"pending"}),
        ));
        id
    };
    let old = seed(a, 2, 1, "concise replies preferred");
    let new = seed(a, 4, 2, "concise replies again");
    seed(a, 7, 3, "concise replies secret");
    let other = seed(b, 1, 4, "concise replies agent");
    let request = |namespace, max_tokens| MemoryPackRequest {
        namespace: NamespaceExternalId(namespace),
        read_ceiling: 5,
        query: Some("concise replies".into()),
        max_tokens,
        max_bytes: 65_536,
    };
    let packs = http
        .context_pack(&[
            ScopedPack {
                credential: &ca,
                pack: request(a, 2048),
            },
            ScopedPack {
                credential: &cb,
                pack: request(b, 2048),
            },
        ])
        .await
        .unwrap();
    let ids: Vec<Vec<Uuid>> = packs
        .iter()
        .map(|pack| pack.items.iter().map(|item| item.observation_id).collect())
        .collect();
    assert_eq!(
        ids,
        vec![vec![new, old], vec![other]],
        "newest first, ceiling 5, request order"
    );
    assert_eq!((packs[0].exposed_level, packs[1].exposed_level), (4, 1));
    assert_eq!(packs[0].pack_digest.len(), 64);
    // A tight budget keeps only whole items that fit.
    let tight = http
        .context_pack(&[ScopedPack {
            credential: &ca,
            pack: request(a, 64 + 2 * 700),
        }])
        .await
        .unwrap();
    assert_eq!(tight[0].items.len(), 1);
    let frozen: Vec<PackedRevision> = packs[0]
        .items
        .iter()
        .map(|item| PackedRevision {
            observation_id: item.observation_id,
            revision: item.revision,
            sources_sha256: item.sources_sha256.clone(),
        })
        .collect();
    assert_eq!(
        http.context_validate(&ca, NamespaceExternalId(a), 5, &frozen)
            .await
            .unwrap(),
        vec![PackItemStatus::Valid, PackItemStatus::Valid]
    );
    {
        let mut state = fake.state.lock().unwrap();
        for (_, detail) in state.observations.iter_mut() {
            if detail["observation_id"] == json!(new) {
                detail["revision"] = json!(2);
            }
            if detail["observation_id"] == json!(old) {
                detail["expired"] = json!(true);
            }
        }
    }
    assert_eq!(
        http.context_validate(&ca, NamespaceExternalId(a), 5, &frozen)
            .await
            .unwrap(),
        vec![PackItemStatus::Changed, PackItemStatus::Expired]
    );
    assert_eq!(
        http.context_validate(&ca, NamespaceExternalId(a), 1, &frozen)
            .await
            .unwrap(),
        vec![PackItemStatus::Unavailable, PackItemStatus::Unavailable],
        "above the ceiling is unavailable"
    );
}

/// E1/E11 (review MEM-P05-B4-F3): two concurrent three-scope recalls in one company both
/// succeed. Each recall is one interactive operation (one of the company's two slots), so all six
/// pack requests are held by the service at the same time while exactly two slots are in use.
/// With a permit per request at most two could be held together and the rest would wait past the
/// 1 s recall deadline.
#[tokio::test]
async fn memory_preparation_two_concurrent_recalls_in_one_company_both_succeed() {
    let fake = ContractFake::start(ADMIN).await;
    let http = std::sync::Arc::new(client(&fake.base_url, ADMIN));
    let tenant = Uuid::new_v4();
    fake.state
        .lock()
        .unwrap()
        .holds
        .insert("context/pack".into(), Duration::from_millis(700));
    let recall = |http: std::sync::Arc<MemoryServiceHttp>| {
        let namespaces = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
        let credentials: Vec<ClientCredential> = namespaces
            .iter()
            .map(|namespace| ClientCredential {
                tenant: TenantExternalId(tenant),
                id: Uuid::nil(),
                token: SecretString::from(fake.client_token(tenant, *namespace)),
            })
            .collect();
        tokio::spawn(async move {
            let packs: Vec<ScopedPack<'_>> = credentials
                .iter()
                .zip(namespaces)
                .map(|(credential, namespace)| ScopedPack {
                    credential,
                    pack: MemoryPackRequest {
                        namespace: NamespaceExternalId(namespace),
                        read_ceiling: 9,
                        query: None,
                        max_tokens: 2048,
                        max_bytes: 65_536,
                    },
                })
                .collect();
            http.context_pack(&packs).await
        })
    };
    let (first, second) = (recall(http.clone()), recall(http.clone()));
    let held = |map: &str| {
        let state = fake.state.lock().unwrap();
        let counts = if map == "now" {
            &state.held_now
        } else {
            &state.held_peak
        };
        counts.get("context/pack").copied().unwrap_or(0)
    };
    tokio::time::timeout(Duration::from_millis(650), async {
        while held("now") < 6 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("both recalls' six requests held by the service at the same time");
    assert_eq!(
        http.interactive_in_use(tenant),
        2,
        "one slot per recall, the company's share"
    );
    let (first, second) = (first.await.unwrap(), second.await.unwrap());
    assert_eq!(first.unwrap().len(), 3);
    assert_eq!(second.unwrap().len(), 3);
    assert_eq!(held("peak"), 6);
    assert_eq!(http.interactive_in_use(tenant), 0);
}
