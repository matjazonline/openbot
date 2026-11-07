use super::*;
#[tokio::test]
async fn google_vault_callback_competing_same_attempt_exchanges_once() {
    let f = Box::pin(Fixture::new()).await;
    let start = f.begin(None).await;
    let permit = f.script.gate.clone().acquire_owned().await.unwrap();
    let worker = f.spawn_finish(&start, CancellationToken::new());
    f.script.wait_calls(1).await;
    assert!(
        f.service
            .finish_connect(&f.owner, f.callback(&start), work(CancellationToken::new()))
            .await
            .is_err()
    );
    drop(permit);
    assert_eq!(
        worker.await.unwrap().unwrap().status,
        ConnectStatus::Connected
    );
    assert_eq!(f.script.calls.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn google_vault_callback_competing_distinct_attempts_never_overwrite_authorization() {
    let f = Box::pin(Fixture::new()).await;
    let a = f.begin(None).await;
    let b = f.begin(None).await;
    let (a, b) = tokio::join!(f.finish(&a), f.finish(&b));
    let statuses = [a.status, b.status];
    assert!(
        statuses.contains(&ConnectStatus::Connected) && statuses.contains(&ConnectStatus::Conflict)
    );
    assert_eq!(
        f.vault
            .list(f.owner.owner(), PageRequest::new(None, 50).unwrap())
            .await
            .unwrap()
            .accounts
            .len(),
        1
    );
}
#[tokio::test]
async fn google_vault_callback_reconnect_preserves_missing_refresh_only_for_intended_identity() {
    let f = Box::pin(Fixture::new()).await;
    let done = f.finish(&f.begin(None).await).await;
    let id = done.account.unwrap();
    let before = f.vault.get(f.owner.owner(), id).await.unwrap();
    f.script
        .token
        .lock()
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("refresh_token");
    let reconnect = f
        .begin(Some(ReconnectTarget {
            account: id,
            authorization: before.authorization_revision,
        }))
        .await;
    let done = f.finish(&reconnect).await;
    assert_eq!(done.account, Some(id));
    assert_eq!(done.status, ConnectStatus::Connected);
    let after = f.vault.get(f.owner.owner(), id).await.unwrap();
    assert_eq!(
        after.authorization_revision.get(),
        before.authorization_revision.get() + 1
    );
    let payload = f.vault.read_usable(f.owner.owner(), id).await.unwrap();
    assert!(payload.payload.expose_secret().contains("refresh-sentinel"));
    assert_eq!(payload.versions.generation.get(), 2);
    f.script.profile.lock().unwrap()["sub"] = json!("different-subject");
    let start = f
        .begin(Some(ReconnectTarget {
            account: id,
            authorization: after.authorization_revision,
        }))
        .await;
    assert_eq!(
        f.finish(&start).await.status,
        ConnectStatus::IdentityMismatch
    );
    assert_eq!(
        f.vault
            .get(f.owner.owner(), id)
            .await
            .unwrap()
            .authorization_revision,
        after.authorization_revision
    );
}
#[derive(Clone, Copy)]
enum Management {
    Disconnect,
    Delete,
    Logout,
    HostDelete,
}
async fn delayed_callback(change: Management) {
    use crate::application::{
        ports::account_deletion::HostAccountDeletion, session::SessionPersistence,
    };
    let f = Box::pin(Fixture::new()).await;
    let initial = f.finish(&f.begin(None).await).await;
    let account = f
        .vault
        .get(f.owner.owner(), initial.account.unwrap())
        .await
        .unwrap();
    let start = f
        .begin(Some(ReconnectTarget {
            account: account.id,
            authorization: account.authorization_revision,
        }))
        .await;
    let permit = f.script.gate.clone().acquire_owned().await.unwrap();
    let worker = f.spawn_finish(&start, CancellationToken::new());
    f.script.wait_calls(2).await;
    match change {
        Management::Disconnect => {
            tokio::time::timeout(
                std::time::Duration::from_secs(1),
                f.vault
                    .disconnect(f.owner.owner(), account.id, account.authorization_revision),
            )
            .await
            .unwrap()
            .unwrap();
        }
        Management::Delete => {
            tokio::time::timeout(
                std::time::Duration::from_secs(1),
                f.vault
                    .delete(f.owner.owner(), account.id, account.authorization_revision),
            )
            .await
            .unwrap()
            .unwrap();
        }
        Management::HostDelete => f
            .vault
            .persistence
            .delete_user_account(f.session.user_id)
            .await
            .unwrap(),
        Management::Logout => f.vault.persistence.revoke_session(f.session).await.unwrap(),
    }
    drop(permit);
    assert!(worker.await.unwrap().is_err());
    match change {
        Management::Disconnect => assert_eq!(
            f.vault
                .get(f.owner.owner(), account.id)
                .await
                .unwrap()
                .status,
            AccountStatus::Disconnected
        ),
        Management::Delete => {
            assert!(f.vault.get(f.owner.owner(), account.id).await.is_err())
        }
        Management::HostDelete => {
            assert_eq!(
                f.vault
                    .get(f.owner.owner(), account.id)
                    .await
                    .unwrap()
                    .status,
                AccountStatus::Disconnected
            );
            assert!(
                f.vault
                    .read_usable(f.owner.owner(), account.id)
                    .await
                    .is_err()
            );
            assert!(
                f.vault
                    .persistence
                    .get_by_id(f.session.user_id)
                    .await
                    .unwrap()
                    .is_none()
            );
        }
        Management::Logout => assert_eq!(
            f.vault
                .get(f.owner.owner(), account.id)
                .await
                .unwrap()
                .authorization_revision,
            account.authorization_revision
        ),
    }
}
#[tokio::test]
async fn google_vault_callback_disconnect_wins_delayed_reconnect() {
    Box::pin(delayed_callback(Management::Disconnect)).await;
}
#[tokio::test]
async fn google_vault_callback_deletion_wins_delayed_reconnect() {
    Box::pin(delayed_callback(Management::Delete)).await;
}
#[tokio::test]
async fn google_vault_callback_logout_wins_delayed_reconnect() {
    Box::pin(delayed_callback(Management::Logout)).await;
}
#[tokio::test]
async fn google_vault_callback_owner_retirement_erases_pending_verifiers() {
    let f = Box::pin(Fixture::new()).await;
    let start = f.begin(None).await;
    let mut tx = f.pool().begin().await.unwrap();
    super::super::lifecycle::retire_owner_on(&mut tx, f.owner.owner())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM integration_connect_attempts WHERE owner_namespace=$1 AND owner_subject=$2").bind(f.owner.owner().namespace.as_str()).bind(f.owner.owner().subject.as_str()).fetch_one(f.pool()).await.unwrap();
    assert_eq!(count, 0);
    assert!(
        f.service
            .finish_connect(&f.owner, f.callback(&start), work(CancellationToken::new()))
            .await
            .is_err()
    );
    assert_eq!(f.script.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn google_vault_callback_user_deletion_wins_delayed_reconnect() {
    Box::pin(delayed_callback(Management::HostDelete)).await;
}

fn read_and_full_registry(f: &Fixture) -> Arc<CodeOwnedRegistry> {
    use crate::application::integrations::registry::Registration;
    let mut registry = static_registry();
    let full = f
        .vault
        .registry
        .get(&AppId::parse("gmail").unwrap())
        .unwrap();
    for id in ["gmail", "gmail-read"] {
        let mut app = full.app.clone();
        app.id = AppId::parse(id).unwrap();
        if id == "gmail-read" {
            app.capabilities = vec![Capability::parse("gmail.read").unwrap()];
        }
        registry
            .register(Registration {
                provider: full.provider.clone(),
                config: full.config.clone(),
                app,
                adapter: full.adapter.clone(),
                authorization: full.authorization.clone(),
                renewal: full.renewal.clone(),
            })
            .unwrap();
    }
    Arc::new(registry)
}
#[tokio::test]
async fn google_vault_callback_broader_consent_requires_reconnect_and_invalidates_old_grants() {
    use crate::{
        application::integrations::grants::GrantWriter, domain::integrations::authority::*,
    };
    let f = Box::pin(Fixture::new()).await;
    let vault =
        PostgresVault::with_registry(f.vault.persistence.clone(), read_and_full_registry(&f));
    let service = f.service_on(vault.clone());
    f.script.token.lock().unwrap()["scope"] =
        json!("openid email https://www.googleapis.com/auth/gmail.readonly");
    let mut read = command(None);
    read.app = AppId::parse("gmail-read").unwrap();
    read.requested_capabilities =
        RequestedCapabilities::try_from(vec![Capability::parse("gmail.read").unwrap()]).unwrap();
    let start = service.begin_connect(&f.owner, read).await.unwrap();
    let done = service
        .finish_connect(&f.owner, f.callback(&start), work(CancellationToken::new()))
        .await
        .unwrap();
    let id = done.account.unwrap();
    let before = vault.get(f.owner.owner(), id).await.unwrap();
    let payload = vault.read_usable(f.owner.owner(), id).await.unwrap();
    let full = vault.registry.get(&AppId::parse("gmail").unwrap()).unwrap();
    assert!(
        full.adapter
            .validate_use(&payload.payload, &full.app)
            .is_err()
    );
    let company = Uuid::new_v4();
    let confirmation = OwnerConfirmation {
        owner: f.owner.clone(),
        account: id,
        host_scope: HostBinding::parse(format!("company/{company}")).unwrap(),
        binding: HostBinding::parse(format!("http/{company}/{}", Uuid::new_v4())).unwrap(),
        app: AppId::parse("gmail-read").unwrap(),
        destination: ApprovedDestination::parse("https://gmail.googleapis.com").unwrap(),
        purpose: InvocationKind::DeterministicHttp,
        expected_authorization: before.authorization_revision,
        host_authority: HostAuthorityReference::parse(Uuid::new_v4().to_string()).unwrap(),
    };
    // Module storage fixture supplies trusted host consent facts; actual host enforcement is covered by03.
    let grant = vault
        .create_confirmed(
            &confirmation,
            GrantAuthorityFacts {
                owner_is_member: true,
                administrator_can_edit: true,
                owner_confirmed: true,
            },
        )
        .await
        .unwrap();
    f.script.token.lock().unwrap()["scope"] = json!(
        "openid email https://www.googleapis.com/auth/gmail.readonly https://www.googleapis.com/auth/gmail.send"
    );
    let start = service
        .begin_connect(
            &f.owner,
            command(Some(ReconnectTarget {
                account: id,
                authorization: before.authorization_revision,
            })),
        )
        .await
        .unwrap();
    let done = service
        .finish_connect(&f.owner, f.callback(&start), work(CancellationToken::new()))
        .await
        .unwrap();
    assert_eq!(done.status, ConnectStatus::Connected);
    let after = vault.get(f.owner.owner(), id).await.unwrap();
    assert_eq!(
        after.authorization_revision.get(),
        before.authorization_revision.get() + 1
    );
    let inspection = vault.grants(f.owner.owner(), id, None).await.unwrap();
    let stale = inspection.grants.iter().find(|g| g.id == grant.id).unwrap();
    assert!(!stale.enabled);
    assert_ne!(stale.account_authorization, after.authorization_revision);
    let payload = vault.read_usable(f.owner.owner(), id).await.unwrap();
    assert!(
        full.adapter
            .validate_use(&payload.payload, &full.app)
            .is_ok()
    );
}
