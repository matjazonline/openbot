use super::*;
#[tokio::test]
async fn vault_dispatch_authority_real_persisted_fence_and_exact_invocation_are_required() {
    let setup = Box::pin(setup("https://api.example.test", Entrance::Http)).await;
    let (vault, account) = account(
        &setup,
        setup.fixture.target.actor.user_id(),
        "dispatch",
        "secret",
    )
    .await;
    attach(&setup, &vault, &account).await;
    let (invocation, context) = prepared(&setup).await;
    vault
        .final_dispatch_guard(&invocation, &context)
        .await
        .unwrap();
    for edit in 0..5 {
        let mut forged = invocation.clone();
        match edit {
            0 => {
                forged.host_authority =
                    integration::HostAuthorityReference::parse("forged").unwrap()
            }
            1 => forged.request_digest[0] ^= 1,
            2 => {
                forged.authorization.binding =
                    integration::HostBinding::parse("foreign/connection").unwrap()
            }
            3 => {
                forged.authorization.client = integration::OwnerNamespace::parse("foreign").unwrap()
            }
            _ => forged.arguments = OperationArguments::new(json!({"path":"/changed"})).unwrap(),
        }
        assert!(vault.final_dispatch_guard(&forged, &context).await.is_err());
    }
    sqlx::query("UPDATE background_tasks SET worker_id=$2 WHERE id=$1")
        .bind(setup.claim.fence.scope.job.0)
        .bind(Uuid::new_v4())
        .execute(setup.fixture.persistence.pool())
        .await
        .unwrap();
    assert!(
        vault
            .final_dispatch_guard(&invocation, &context)
            .await
            .is_err()
    );
}
#[tokio::test]
async fn vault_dispatch_authority_revoke_competes_before_the_final_check_and_sends_nothing() {
    Box::pin(authority_change_before_final_check(
        AuthorityMutation::Revoke,
    ))
    .await;
}
#[tokio::test]
async fn vault_dispatch_authority_replacement_competes_before_the_final_check_and_sends_nothing() {
    Box::pin(authority_change_before_final_check(
        AuthorityMutation::Replace,
    ))
    .await;
}
enum AuthorityMutation {
    Revoke,
    Replace,
}
async fn authority_change_before_final_check(mutation: AuthorityMutation) {
    let setup = Box::pin(setup("https://api.example.test", Entrance::Http)).await;
    let (vault, account) = account(
        &setup,
        setup.fixture.target.actor.user_id(),
        "revocation",
        "secret",
    )
    .await;
    let grant = attach(&setup, &vault, &account).await;
    let (invocation, context) = prepared(&setup).await;
    let ready = Arc::new(tokio::sync::Notify::new());
    let resume = Arc::new(tokio::sync::Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let (worker_vault, worker_ready, worker_resume, worker_calls) =
        (vault.clone(), ready.clone(), resume.clone(), calls.clone());
    let worker = tokio::spawn(async move {
        worker_ready.notify_one();
        worker_resume.notified().await;
        let result = worker_vault
            .final_dispatch_guard(&invocation, &context)
            .await;
        if result.is_ok() {
            worker_calls.fetch_add(1, Ordering::SeqCst);
        }
        result
    });
    ready.notified().await;
    if matches!(mutation, AuthorityMutation::Replace) {
        vault
            .replace(
                &account.owner,
                account.id,
                account.revision,
                write("revocation", "new-secret"),
            )
            .await
            .unwrap();
    } else {
        vault
            .revoke(&account.owner, grant.id, grant.revision)
            .await
            .unwrap();
    }
    resume.notify_one();
    assert!(worker.await.unwrap().is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn vault_dispatch_authority_replacement_and_rebind_cannot_reuse_frozen_action() {
    let setup = Box::pin(setup("https://api.example.test", Entrance::Http)).await;
    let (vault, account) = account(
        &setup,
        setup.fixture.target.actor.user_id(),
        "replacement",
        "secret",
    )
    .await;
    attach(&setup, &vault, &account).await;
    let (invocation, context) = prepared(&setup).await;
    let changed = vault
        .replace(
            &account.owner,
            account.id,
            account.revision,
            write("replacement", "new-secret"),
        )
        .await
        .unwrap();
    assert!(
        vault
            .final_dispatch_guard(&invocation, &context)
            .await
            .is_err()
    );
    attach(&setup, &vault, &changed).await;
    assert!(
        setup
            .fixture
            .persistence
            .http_step_request(&setup.claim, &setup.kind)
            .await
            .is_err()
    );
}
#[tokio::test]
async fn vault_dispatch_authority_host_cannot_skip_or_substitute_the_module_callback() {
    struct Skip;
    #[async_trait]
    impl DispatchAuthority for Skip {
        async fn check(
            &self,
            _: &AuthorizedInvocation,
            _: &dyn CredentialDispatchGate,
        ) -> integration::IntegrationResult<()> {
            Ok(())
        }
    }
    let setup = Box::pin(setup("https://api.example.test", Entrance::Http)).await;
    let (vault, account) = account(
        &setup,
        setup.fixture.target.actor.user_id(),
        "callback",
        "secret",
    )
    .await;
    attach(&setup, &vault, &account).await;
    let (invocation, mut context) = prepared(&setup).await;
    context.authority = Arc::new(Skip);
    assert!(
        vault
            .final_dispatch_guard(&invocation, &context)
            .await
            .is_err()
    );
}
