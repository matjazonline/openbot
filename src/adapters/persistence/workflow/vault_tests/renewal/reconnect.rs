use super::*;
use crate::{
    adapters::{
        integrations::oauth_state::SignedConnectState,
        persistence::{
            integration_connect_authority::PostgresConnectAuthority,
            integrations::connect_tests::verified,
        },
    },
    application::{
        integrations::connect::ConnectService,
        session::{SessionId, SessionPersistence, SessionRecord, VerifiedSession},
    },
    infra::config::AppConfig,
};
struct Flow {
    service: Arc<ConnectService>,
    owner: VerifiedOwner,
    application: String,
}
impl Flow {
    async fn new(f: &RenewalSetup, registry: Arc<CodeOwnedRegistry>) -> Self {
        let service = f.separate_service().await;
        let persistence = service.vault.persistence.clone();
        let user = f.setup.fixture.target.actor.user_id();
        let revision: i64 = sqlx::query_scalar("SELECT auth_revision FROM users WHERE id=$1")
            .bind(user)
            .fetch_one(persistence.pool())
            .await
            .unwrap();
        let record = SessionRecord {
            id: SessionId(Uuid::new_v4()),
            user_id: user,
            auth_revision: revision,
            issued_at: Utc::now(),
            expires_at: Utc::now() + ChronoDuration::days(1),
        };
        persistence.create_session(&record).await.unwrap();
        let owner = verified(VerifiedSession {
            user_id: user,
            session_id: record.id,
        });
        let vault = PostgresVault::with_registry(persistence.clone(), registry.clone());
        let connect = Arc::new(ConnectService {
            registry,
            store: Arc::new(vault),
            codec: Arc::new(SignedConnectState::new(&AppConfig::for_test().jwt_secret)),
            authority: Arc::new(PostgresConnectAuthority { persistence }),
        });
        Self {
            service: connect,
            owner,
            application: service.application,
        }
    }
    async fn begin(&self, account: &ConnectedAccount) -> ConnectStart {
        self.service
            .begin_connect(
                &self.owner,
                BeginConnect {
                    app: AppId::parse("gmail").unwrap(),
                    config: AuthConfigId::parse("google-personal").unwrap(),
                    requested_capabilities: RequestedCapabilities::try_from(vec![
                        Capability::parse("gmail.read").unwrap(),
                        Capability::parse("gmail.send").unwrap(),
                    ])
                    .unwrap(),
                    return_target: SafeReturnTarget::parse("/ui/profile/vault".into()).unwrap(),
                    reconnect: Some(ReconnectTarget {
                        account: account.id,
                        authorization: account.authorization_revision,
                    }),
                },
            )
            .await
            .unwrap()
    }
    fn callback(start: &ConnectStart) -> ConnectCallback {
        let url = url::Url::parse(start.authorization_url.as_str()).unwrap();
        let state = url
            .query_pairs()
            .find(|(key, _)| key == "state")
            .unwrap()
            .1
            .into_owned();
        ConnectCallback {
            provider: ProviderId::parse("google").unwrap(),
            state,
            code: Some(SecretString::from("reconnect-code-sentinel")),
            error: None,
        }
    }
    fn work() -> ConnectWork {
        ConnectWork {
            deadline: Utc::now() + ChronoDuration::seconds(20),
            cancellation: CancellationToken::new(),
        }
    }
}
#[tokio::test]
async fn google_connection_lifecycle_reconnect_ignores_routine_cache_revision_but_stales_old_grants()
 {
    let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
    let flow = Flow::new(&f, f.vault.registry.clone()).await;
    let start = flow.begin(&f.account).await;
    let callback = Flow::callback(&start);
    let (i, c) = prepared(&f.setup).await;
    let old = i.clone();
    let old_context = c.clone();
    let held = f.server.state.gate.clone().acquire_owned().await.unwrap();
    let service = f.separate_service().await;
    let refresh = tokio::spawn(async move { service.vault.resolve(&i, &c).await });
    f.server.wait_calls(1).await;
    let name = flow.application.clone();
    let callback = tokio::spawn(async move {
        flow.service
            .finish_connect(&flow.owner, callback, Flow::work())
            .await
    });
    f.wait_blocked(&name).await;
    drop(held);
    let _ = refresh.await.unwrap();
    assert_eq!(
        callback.await.unwrap().unwrap().status,
        ConnectStatus::Connected
    );
    let current = f.vault.get(&f.account.owner, f.account.id).await.unwrap();
    assert_eq!(current.authorization_revision.get(), 2);
    assert_eq!(f.credential().await.versions.generation.get(), 3);
    assert!(f.vault.resolve(&old, &old_context).await.is_err());
    assert_eq!(f.server.calls(), 2);
    assert_eq!(f.server.state.identities.load(Ordering::SeqCst), 1);
    let grants = f
        .vault
        .grants(&f.account.owner, f.account.id, None)
        .await
        .unwrap();
    assert!(grants.grants.iter().all(|grant| !grant.enabled));
}
#[tokio::test]
async fn google_connection_lifecycle_stale_invalid_grant_after_crash_cannot_invalidate_new_consent()
{
    let f = Box::pin(RenewalSetup::new(Utc::now() - ChronoDuration::seconds(1))).await;
    let consent = TokenServer::new().await;
    *consent.state.reply.lock().unwrap() = Reply::Json(
        json!({"access_token":"reconnected-access","refresh_token":"reconnected-refresh","expires_in":3600,"token_type":"Bearer","scope":"openid email https://www.googleapis.com/auth/gmail.readonly https://www.googleapis.com/auth/gmail.send"}),
    );
    let flow = Flow::new(&f, google_registry(&consent)).await;
    let start = flow.begin(&f.account).await;
    let (i, c) = prepared(&f.setup).await;
    let old = i.clone();
    let old_context = c.clone();
    let held = f.server.state.gate.clone().acquire_owned().await.unwrap();
    let service = f.separate_service().await;
    let application = service.application.clone();
    let refresh = tokio::spawn(async move { service.vault.resolve(&i, &c).await });
    f.server.wait_calls(1).await;
    terminate(&f.vault, &application).await;
    let outcome = flow
        .service
        .finish_connect(&flow.owner, Flow::callback(&start), Flow::work())
        .await
        .unwrap();
    assert_eq!(outcome.status, ConnectStatus::Connected);
    *f.server.state.status.lock().unwrap() = axum::http::StatusCode::BAD_REQUEST;
    *f.server.state.reply.lock().unwrap() = Reply::Json(json!({"error":"invalid_grant"}));
    drop(held);
    assert!(refresh.await.unwrap().is_err());
    let current = f.credential().await;
    assert_eq!(current.account.status, AccountStatus::Ready);
    assert_eq!(current.versions.authorization.get(), 2);
    let payload: Value = serde_json::from_str(current.payload.expose_secret()).unwrap();
    assert_eq!(payload["refresh_token"], "reconnected-refresh");
    assert!(f.vault.resolve(&old, &old_context).await.is_err());
    assert_eq!(f.server.calls(), 1);
    assert_eq!(consent.calls(), 1);
}
async fn terminate(vault: &PostgresVault, application: &str) {
    let pids:Vec<i32>=sqlx::query_scalar("SELECT pid FROM pg_stat_activity WHERE datname=current_database() AND application_name=$1 AND state='idle in transaction'")
        .bind(application).fetch_all(vault.persistence.pool()).await.unwrap();
    assert_eq!(pids.len(), 1);
    let done: bool = sqlx::query_scalar("SELECT pg_terminate_backend($1)")
        .bind(pids[0])
        .fetch_one(vault.persistence.pool())
        .await
        .unwrap();
    assert!(done);
}
