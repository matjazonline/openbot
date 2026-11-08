use super::*;
#[derive(Clone, Copy)]
enum OwnerCase {
    SameOwner,
    DifferentUser,
}
async fn second_setup(f: &RenewalSetup, kind: OwnerCase) -> Setup {
    let suffix = format!("-{}", Uuid::new_v4().simple());
    let mut fixture = match kind {
        OwnerCase::DifferentUser => {
            Box::pin(Fixture::in_database(f.setup.fixture._db.clone(), &suffix)).await
        }
        OwnerCase::SameOwner => {
            let company = crate::use_cases::company::CompanyPersistence::create(
                &f.setup.fixture.persistence,
                f.setup.fixture.target.actor.user_id(),
                CompanyWrite {
                    name: "Parallel account company".into(),
                    slug: format!("parallel{suffix}"),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            Fixture {
                _db: f.setup.fixture._db.clone(),
                persistence: f.setup.fixture.persistence.clone(),
                target: DefinitionTarget {
                    company: CompanyId::new(company.id),
                    ..f.setup.fixture.target
                },
            }
        }
    };
    fixture.persistence = PostgresPersistence::with_credential_cipher(
        fixture.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    let connection = Box::pin(configured(
        &fixture,
        "https://api.example.test",
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
    Setup {
        version,
        fixture,
        connection,
        claim,
        kind: WorkflowStepKind("http.request".into()),
    }
}
async fn parallel(kind: OwnerCase) {
    let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
    let second = Box::pin(second_setup(&f, kind)).await;
    let account = managed(
        &f.vault,
        second.fixture.target.actor.user_id(),
        "independent-account",
        Utc::now() - ChronoDuration::seconds(1),
    )
    .await;
    assert_ne!(f.account.id, account.id);
    match kind {
        OwnerCase::SameOwner => assert_eq!(f.account.owner, account.owner),
        OwnerCase::DifferentUser => assert_ne!(f.account.owner, account.owner),
    }
    attach_app(&second, &f.vault, &account, AppId::parse("gmail").unwrap()).await;
    let a = prepared(&f.setup).await;
    let b = prepared(&second).await;
    let held = f.server.state.gate.clone().acquire_owned().await.unwrap();
    let service = independent_service(&f.vault).await;
    let first = tokio::spawn(async move { service.vault.resolve(&a.0, &a.1).await });
    f.server.wait_calls(1).await;
    let service = independent_service(&f.vault).await;
    let second = tokio::spawn(async move { service.vault.resolve(&b.0, &b.1).await });
    f.server.wait_calls(2).await;
    drop(held);
    assert_eq!(first.await.unwrap().unwrap().generation.get(), 2);
    assert_eq!(second.await.unwrap().unwrap().generation.get(), 2);
    assert_eq!(f.server.calls(), 2);
}
#[tokio::test]
async fn google_refresh_race_an_independent_user_is_not_blocked_by_another_account() {
    Box::pin(parallel(OwnerCase::DifferentUser)).await;
}
#[tokio::test]
async fn google_refresh_race_same_owner_different_accounts_reach_provider_independently() {
    Box::pin(parallel(OwnerCase::SameOwner)).await;
}
