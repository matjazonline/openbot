use super::*;
#[derive(Clone, Copy)]
enum Action {
    Disconnect,
    Delete,
}
async fn race(action: Action) {
    let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
    if matches!(action, Action::Disconnect) {
        *f.server.state.status.lock().unwrap() = axum::http::StatusCode::SERVICE_UNAVAILABLE;
        *f.server.state.reply.lock().unwrap() =
            Reply::Json(json!({"error":"temporarily_unavailable"}));
    }
    let (i, c) = prepared(&f.setup).await;
    let old = i.clone();
    let old_context = c.clone();
    let held = f.server.state.gate.clone().acquire_owned().await.unwrap();
    let service = f.separate_service().await;
    let refresh = tokio::spawn(async move { service.vault.resolve(&i, &c).await });
    f.server.wait_calls(1).await;
    let service = f.separate_service().await;
    let app = service.application.clone();
    let account = f.account.clone();
    let removal = tokio::spawn(async move {
        match action {
            Action::Disconnect => service
                .vault
                .disconnect(&account.owner, account.id, account.authorization_revision)
                .await
                .map(|_| ()),
            Action::Delete => {
                service
                    .vault
                    .delete(&account.owner, account.id, account.authorization_revision)
                    .await
            }
        }
    });
    f.wait_blocked(&app).await;
    drop(held);
    removal.await.unwrap().unwrap();
    let _ = refresh.await.unwrap();
    assert!(f.vault.resolve(&old, &old_context).await.is_err());
    assert_eq!(f.server.calls(), 1);
    match action {
        Action::Disconnect => {
            let current = f.vault.get(&f.account.owner, f.account.id).await.unwrap();
            assert_eq!(current.status, AccountStatus::Disconnected);
            assert_eq!(
                current.authorization_revision.get(),
                f.account.authorization_revision.get() + 1
            );
            assert!(
                f.vault
                    .read_usable(&f.account.owner, f.account.id)
                    .await
                    .is_err()
            );
        }
        Action::Delete => assert_eq!(
            f.vault
                .get(&f.account.owner, f.account.id)
                .await
                .unwrap_err(),
            IntegrationError::NotFound
        ),
    }
}
#[tokio::test]
async fn google_connection_lifecycle_disconnect_competes_with_refresh_and_provider_failure() {
    Box::pin(race(Action::Disconnect)).await;
}
#[tokio::test]
async fn google_connection_lifecycle_delete_competes_with_refresh_without_restoring_old_authority()
{
    Box::pin(race(Action::Delete)).await;
}
#[tokio::test]
async fn google_refresh_worker_connection_crash_releases_locks_and_cannot_apply_a_stale_error() {
    let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
    let (i, c) = prepared(&f.setup).await;
    let held = f.server.state.gate.clone().acquire_owned().await.unwrap();
    let service = f.separate_service().await;
    let application = service.application.clone();
    let worker = tokio::spawn(async move { service.vault.resolve(&i, &c).await });
    f.server.wait_calls(1).await;
    let pids:Vec<i32>=sqlx::query_scalar("SELECT pid FROM pg_stat_activity WHERE datname=current_database() AND application_name=$1 AND state='idle in transaction'").bind(application).fetch_all(f.vault.persistence.pool()).await.unwrap();
    assert_eq!(pids.len(), 1);
    let terminated: bool = sqlx::query_scalar("SELECT pg_terminate_backend($1)")
        .bind(pids[0])
        .fetch_one(f.vault.persistence.pool())
        .await
        .unwrap();
    assert!(terminated);
    f.vault
        .disconnect(
            &f.account.owner,
            f.account.id,
            f.account.authorization_revision,
        )
        .await
        .unwrap();
    *f.server.state.status.lock().unwrap() = axum::http::StatusCode::BAD_REQUEST;
    *f.server.state.reply.lock().unwrap() = Reply::Json(json!({"error":"invalid_grant"}));
    drop(held);
    assert!(worker.await.unwrap().is_err());
    assert_eq!(
        f.vault
            .get(&f.account.owner, f.account.id)
            .await
            .unwrap()
            .status,
        AccountStatus::Disconnected
    );
    assert_eq!(f.server.calls(), 1);
}
