use super::*;
use crate::application::integrations::lifecycle::TeardownDrain;
use crate::use_cases::company::{CompanyPersistence, CompanyWrite};

#[tokio::test]
async fn vault_google_e2e_all_vault_kinds_survive_key_rotation_retirement_and_native_renewal() {
    for case in [Case::Bearer, Case::ApiKey, Case::Google, Case::Workspace] {
        Box::pin(rotate(case)).await;
    }
}
async fn rotate(case: Case) {
    let f = Box::pin(Journey::new(case)).await;
    let before = f
        .vault
        .read_usable(&f.account.owner, f.account.id)
        .await
        .unwrap();
    let rotating = PostgresPersistence::with_credential_cipher(
        f.vault.persistence.pool().clone(),
        CredentialCipher::for_test_with_keys(&[(1, [42; 32]), (2, [23; 32])], 2),
    );
    assert!(rotating.rotate_credentials().await.unwrap().complete);
    let retired = PostgresPersistence::with_credential_cipher(
        f.vault.persistence.pool().clone(),
        CredentialCipher::for_test_with_keys(&[(2, [23; 32])], 2),
    );
    assert!(
        retired
            .credential_status()
            .await
            .unwrap()
            .satisfies_required_version(2)
    );
    let vault = PostgresVault::with_registry(retired, f.vault.registry.clone());
    let after = vault
        .read_usable(&f.account.owner, f.account.id)
        .await
        .unwrap();
    assert_eq!(
        after.payload.expose_secret(),
        before.payload.expose_secret()
    );
    assert_eq!(after.versions, before.versions);
    let runtime = HttpRuntime::with_integrations(
        vault.persistence.clone(),
        HttpDestinationPolicy::default(),
        lease(),
        None,
        Arc::new(AuthenticatedExecution {
            registry: vault.registry.clone(),
            credentials: Arc::new(vault.clone()),
            transports: f.peer.transports(case.destination()),
        }),
    );
    Box::pin(runtime.run(&f.setup.kind, &f.setup.claim))
        .await
        .completed_result()
        .unwrap();
    assert_eq!(f.peer.calls(), 1);
    assert_eq!(
        f.token.calls(),
        usize::from(matches!(case, Case::Google | Case::Workspace))
    );
    assert_eq!(
        vault
            .get(&f.account.owner, f.account.id)
            .await
            .unwrap()
            .authorization_revision,
        f.account.authorization_revision
    );
    assert!(
        vault
            .persistence
            .credential_status()
            .await
            .unwrap()
            .satisfies_required_version(2)
    );
    f.peer.close().await;
}
#[tokio::test]
async fn vault_google_e2e_stale_key_rotation_cas_cannot_overwrite_native_google_refresh() {
    use crate::adapters::persistence::{credentials::CredentialFormat, integrations::inventory};
    let f = Box::pin(Journey::new(Case::Google)).await;
    let mut connection = f.vault.persistence.pool().acquire().await.unwrap();
    let old = inventory::page(&mut connection, None, 100)
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.id == f.account.id.as_uuid())
        .unwrap();
    drop(connection);
    let cipher = CredentialCipher::for_test_with_keys(&[(1, [42; 32]), (2, [23; 32])], 2);
    let rotated = cipher
        .rotate_to_active(
            &CredentialFormat::Envelope(old.context.clone()),
            &old.envelope,
        )
        .unwrap();
    let refresh = Box::pin(renewal_support::independent_service(&f.vault)).await;
    let active = PostgresVault::with_registry(
        PostgresPersistence::with_credential_cipher(
            refresh.vault.persistence.pool().clone(),
            CredentialCipher::for_test_with_keys(&[(1, [42; 32]), (2, [23; 32])], 2),
        ),
        f.vault.registry.clone(),
    );
    let runtime = HttpRuntime::with_integrations(
        active.persistence.clone(),
        HttpDestinationPolicy::default(),
        lease(),
        None,
        Arc::new(AuthenticatedExecution {
            registry: active.registry.clone(),
            credentials: Arc::new(active.clone()),
            transports: f.peer.transports(Case::Google.destination()),
        }),
    );
    let rotation = Box::pin(renewal_support::independent_service(&f.vault)).await;
    let held = f.token.state.gate.clone().acquire_owned().await.unwrap();
    let (result, rows) = tokio::join!(
        Box::pin(runtime.run(&f.setup.kind, &f.setup.claim)),
        async {
            f.token.wait_calls(1).await;
            let (rows, ()) = tokio::join!(
                async {
                    let mut tx = rotation.vault.persistence.pool().begin().await.unwrap();
                    let rows = inventory::rewrap(&mut tx, old.id, &old.envelope, &rotated)
                        .await
                        .unwrap()
                        .rows_affected();
                    tx.commit().await.unwrap();
                    rows
                },
                async {
                    renewal_support::wait_blocked(&f.vault, &rotation.application).await;
                    drop(held);
                }
            );
            rows
        }
    );
    result.completed_result().unwrap();
    assert_eq!(
        rows, 0,
        "stale envelope CAS must lose to the refreshed generation"
    );
    assert_eq!(f.peer.calls(), 1);
    assert_eq!(f.token.calls(), 1);
    assert_retired_refresh(&f, &active).await;
    refresh.vault.persistence.pool().close().await;
    rotation.vault.persistence.pool().close().await;
    f.peer.close().await;
}
async fn surviving_company(f: &Journey) -> Setup {
    let p = &f.setup.fixture.persistence;
    let company = CompanyPersistence::create(
        p,
        f.setup.fixture.target.actor.user_id(),
        CompanyWrite {
            name: "Surviving company".into(),
            slug: "surviving-company".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let fixture = Fixture {
        _db: f.setup.fixture._db.clone(),
        persistence: p.clone(),
        target: DefinitionTarget {
            company: CompanyId::new(company.id),
            actor: f.setup.fixture.target.actor,
            workflow: WorkflowId::new(Uuid::from_u128(1)),
        },
    };
    let connection = Box::pin(configured(
        &fixture,
        Case::Bearer.destination().as_str(),
        Entrance::Http,
    ))
    .await;
    let version = Box::pin(published(&fixture, &connection, Entrance::Http, None)).await;
    let target = Box::pin(bound(
        &fixture,
        &connection,
        version,
        Entrance::Http,
        RelatedAssociation::Company,
    ))
    .await;
    let claim = Box::pin(claimed(&fixture, target, RelatedAssociation::Company)).await;
    let setup = Setup {
        fixture,
        connection,
        version,
        claim,
        kind: WorkflowStepKind("http.request".into()),
    };
    Box::pin(attach_source(
        &setup,
        &f.vault,
        &f.account,
        Case::Bearer.app(),
    ))
    .await;
    setup
}
#[tokio::test]
async fn vault_google_e2e_company_teardown_restart_preserves_other_company_then_account_delete_denies_new_run()
 {
    let f = Box::pin(Journey::new(Case::Bearer)).await;
    let other_user = member(&f.setup).await;
    let other_owner = owner_for_user(other_user);
    let unrelated = f
        .vault
        .create(&other_owner, write("unrelated-private", "unrelated-secret"))
        .await
        .unwrap();
    let survivor = Box::pin(surviving_company(&f)).await;
    CompanyPersistence::delete(
        &f.setup.fixture.persistence,
        f.setup.fixture.target.company.as_uuid(),
    )
    .await
    .unwrap();
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM integration_teardown_intents WHERE state='pending'",
    )
    .fetch_one(f.vault.persistence.pool())
    .await
    .unwrap();
    assert_eq!(
        pending, 1,
        "host denial commits before asynchronous module cleanup"
    );
    assert!(
        Box::pin(f.runtime.run(&f.setup.kind, &f.setup.claim))
            .await
            .completed_result()
            .is_err()
    );
    assert_eq!(f.peer.calls(), 0);
    let restarted =
        PostgresVault::with_registry(f.vault.persistence.clone(), f.vault.registry.clone());
    Box::pin(f.runtime.run(&survivor.kind, &survivor.claim))
        .await
        .completed_result()
        .unwrap();
    assert_eq!(
        f.peer.calls(),
        1,
        "other company's authorized grant survives pending cleanup"
    );
    assert_eq!(
        restarted
            .drain(16, &CancellationToken::new())
            .await
            .unwrap(),
        1
    );
    let grants = restarted
        .grants(&f.account.owner, f.account.id, None)
        .await
        .unwrap();
    assert_eq!(grants.grants.iter().filter(|g| g.enabled).count(), 1);
    assert!(
        restarted
            .read_usable(&other_owner, unrelated.id)
            .await
            .is_ok()
    );
    Box::pin(delete_shared_account(
        &f,
        &survivor,
        &restarted,
        &other_owner,
        unrelated.id,
    ))
    .await;
    f.peer.close().await;
}

async fn delete_shared_account(
    f: &Journey,
    survivor: &Setup,
    restarted: &PostgresVault,
    other_owner: &AccountOwner,
    unrelated_id: AccountId,
) {
    let target = Box::pin(bound(
        &survivor.fixture,
        &survivor.connection,
        survivor.version,
        Entrance::Http,
        RelatedAssociation::Company,
    ))
    .await;
    let claim = Box::pin(
        crate::adapters::persistence::workflow::tests::http_tests::support::claimed_with_key(
            &survivor.fixture,
            target,
            RelatedAssociation::Company,
            "after-account-delete",
        ),
    )
    .await;
    restarted
        .delete(
            &f.account.owner,
            f.account.id,
            f.account.authorization_revision,
        )
        .await
        .unwrap();
    assert!(
        Box::pin(f.runtime.run(&survivor.kind, &claim))
            .await
            .completed_result()
            .is_err()
    );
    assert_eq!(f.peer.calls(), 1);
    assert!(
        restarted
            .read_usable(other_owner, unrelated_id)
            .await
            .is_ok()
    );
}

async fn assert_retired_refresh(f: &Journey, active: &PostgresVault) {
    assert!(
        active
            .persistence
            .rotate_credentials()
            .await
            .unwrap()
            .complete
    );
    let retired = PostgresVault::with_registry(
        PostgresPersistence::with_credential_cipher(
            f.vault.persistence.pool().clone(),
            CredentialCipher::for_test_with_keys(&[(2, [23; 32])], 2),
        ),
        f.vault.registry.clone(),
    );
    let stored = retired
        .read_usable(&f.account.owner, f.account.id)
        .await
        .unwrap();
    assert_eq!(stored.versions.generation.get(), 2);
    assert_eq!(
        stored.versions.authorization,
        f.account.authorization_revision
    );
    let payload: Value = serde_json::from_str(stored.payload.expose_secret()).unwrap();
    assert_eq!(payload["access_token"], Case::Google.access());
    assert!(
        retired
            .persistence
            .credential_status()
            .await
            .unwrap()
            .satisfies_required_version(2)
    );
}
