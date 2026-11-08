use super::*;
use crate::{
    adapters::{
        http::session::SessionAuthority,
        integrations::{
            google_authorization::registration, google_http::GoogleHttp,
            oauth_state::SignedConnectState, owner::owner_for_user,
        },
        persistence::{
            credentials::CredentialCipher,
            integration_connect_authority::PostgresConnectAuthority,
            test_support::{OwnDatabase, own_database},
        },
    },
    application::{
        integrations::{
            connect::*, management::AccountRepository, private::CredentialStore,
            registry::static_registry,
        },
        session::VerifiedSession,
    },
    infra::config::AppConfig,
    use_cases::user::UserPersistence,
};
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use chrono::Utc;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
mod bounds;
mod control_wire;
mod lifecycle;
mod portability;
mod provider;
mod temporal;
mod workspace;
pub(crate) struct FormObservation {
    pub challenge: String,
    pub code_digest: [u8; 32],
    pub client_valid: bool,
    pub grant_type: String,
}
pub(crate) struct Script {
    pub token: Mutex<Value>,
    pub profile: Mutex<Value>,
    pub status: Mutex<u16>,
    pub calls: AtomicUsize,
    pub identities: AtomicUsize,
    pub entered: tokio::sync::Notify,
    pub gate: Arc<tokio::sync::Semaphore>,
    pub observed: Mutex<Option<FormObservation>>,
}
impl Script {
    pub(crate) async fn wait_calls(&self, expected: usize) {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let event = self.entered.notified();
                if self.calls.load(Ordering::SeqCst) >= expected {
                    break;
                }
                event.await;
            }
        })
        .await
        .unwrap();
    }
    fn new() -> Self {
        Self {
            token: Mutex::new(
                json!({"access_token":"access-sentinel","refresh_token":"refresh-sentinel","token_type":"Bearer","expires_in":3600,"scope":"openid email https://www.googleapis.com/auth/gmail.readonly https://www.googleapis.com/auth/gmail.send"}),
            ),
            profile: Mutex::new(
                json!({"sub":"provider-subject","email":"different-mailbox@gmail.com","email_verified":true}),
            ),
            status: Mutex::new(200),
            calls: AtomicUsize::new(0),
            identities: AtomicUsize::new(0),
            entered: tokio::sync::Notify::new(),
            gate: Arc::new(tokio::sync::Semaphore::new(1)),
            observed: Mutex::new(None),
        }
    }
}
async fn token(
    State(script): State<Arc<Script>>,
    axum::Form(form): axum::Form<std::collections::BTreeMap<String, String>>,
) -> impl IntoResponse {
    use base64::Engine;
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(
        form.get("code_verifier")
            .map(String::as_bytes)
            .unwrap_or_default(),
    ));
    *script.observed.lock().unwrap() = Some(FormObservation {
        challenge,
        code_digest: Sha256::digest(form.get("code").map(String::as_bytes).unwrap_or_default())
            .into(),
        client_valid: form
            .get("client_id")
            .is_some_and(|v| v == "google-client-fixture")
            && form
                .get("client_secret")
                .is_some_and(|v| v == "client-secret-sentinel"),
        grant_type: form.get("grant_type").cloned().unwrap_or_default(),
    });
    script.calls.fetch_add(1, Ordering::SeqCst);
    script.entered.notify_one();
    let permit = script.gate.acquire().await.unwrap();
    drop(permit);
    let status = StatusCode::from_u16(*script.status.lock().unwrap()).unwrap();
    (status, Json(script.token.lock().unwrap().clone()))
}
async fn identity(State(script): State<Arc<Script>>) -> Json<Value> {
    script.identities.fetch_add(1, Ordering::SeqCst);
    Json(script.profile.lock().unwrap().clone())
}
pub(crate) struct Fixture {
    pub vault: PostgresVault,
    pub service: Arc<ConnectService>,
    pub owner: VerifiedOwner,
    pub sessions: Arc<SessionAuthority>,
    pub session: VerifiedSession,
    pub cookie: String,
    pub script: Arc<Script>,
    server: tokio::task::JoinHandle<()>,
    _database: OwnDatabase,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}
impl Fixture {
    pub fn new() -> impl std::future::Future<Output = Self> {
        Self::with_lifetime(time::Duration::hours(24))
    }
    pub async fn with_lifetime(lifetime: time::Duration) -> Self {
        let database = own_database().await.expect("owned connect database");
        let p = PostgresPersistence::with_credential_cipher(
            database.pool.clone(),
            CredentialCipher::for_test_with_keys(&[(1, [7; 32])], 1),
        );
        let id = Uuid::new_v4().simple().to_string();
        let user = p
            .create_user(&id, &format!("{id}@login.example"), "hash")
            .await
            .unwrap();
        let config = AppConfig::for_test();
        // Production configuration keeps its minimum lifetime; seed a valid near-expiry record for temporal tests.
        let sessions = Arc::new(SessionAuthority::new(&config, Arc::new(p.clone())));
        let cookie = sessions.cookie(&user).await.unwrap();
        let mut session = sessions.verify(cookie.value()).await.unwrap().unwrap();
        if lifetime < time::Duration::days(1) {
            use crate::application::session::{SessionId, SessionPersistence, SessionRecord};
            let record = SessionRecord {
                id: SessionId(Uuid::new_v4()),
                user_id: user.id,
                auth_revision: user.auth_revision,
                issued_at: Utc::now() - chrono::Duration::hours(24),
                expires_at: Utc::now() + chrono::Duration::seconds(lifetime.whole_seconds()),
            };
            p.create_session(&record).await.unwrap();
            session = VerifiedSession {
                user_id: user.id,
                session_id: record.id,
            };
        }
        let script = Arc::new(Script::new());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/token", post(token))
            .route("/identity", get(identity))
            .with_state(script.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut registry = static_registry();
        registry
            .register(
                registration(
                    ExternalSubject::parse("google-client-fixture").unwrap(),
                    SecretString::from("client-secret-sentinel"),
                    url::Url::parse("https://openbot.example/auth/integrations/google/callback")
                        .unwrap(),
                    Arc::new(GoogleHttp::scripted(
                        format!("{base}/token"),
                        format!("{base}/identity"),
                    )),
                )
                .unwrap(),
            )
            .unwrap();
        let registry = Arc::new(registry);
        let vault = PostgresVault::with_registry(p.clone(), registry.clone());
        let service = Arc::new(ConnectService {
            registry,
            store: Arc::new(vault.clone()),
            codec: Arc::new(SignedConnectState::new(&AppConfig::for_test().jwt_secret)),
            authority: Arc::new(PostgresConnectAuthority { persistence: p }),
        });
        Self {
            vault,
            service,
            owner: verified(session),
            sessions,
            session,
            cookie: cookie.value().to_owned(),
            script,
            server,
            _database: database,
        }
    }
    pub fn gateway(&self) -> Arc<dyn IntegrationGateway> {
        self.vault.gateway(
            Arc::new(
                crate::adapters::persistence::integration_bindings::PostgresIntegrationBindings {
                    persistence: self.vault.persistence.clone(),
                    vault: self.vault.clone(),
                },
            ),
            self.service.clone(),
            Arc::new(crate::application::integrations::unavailable::UnavailableIntegrations),
        )
    }
    pub async fn new_cookie(&self) -> String {
        let user = self
            .vault
            .persistence
            .get_by_id(self.session.user_id)
            .await
            .unwrap()
            .unwrap();
        self.sessions
            .cookie(&user)
            .await
            .unwrap()
            .value()
            .to_owned()
    }
    pub async fn other_cookie(&self) -> String {
        let id = Uuid::new_v4().simple().to_string();
        let user = self
            .vault
            .persistence
            .create_user(&id, &format!("{id}@other.example"), "hash")
            .await
            .unwrap();
        self.sessions
            .cookie(&user)
            .await
            .unwrap()
            .value()
            .to_owned()
    }
    fn service_on(&self, vault: PostgresVault) -> Arc<ConnectService> {
        Arc::new(ConnectService {
            registry: vault.registry.clone(),
            codec: self.service.codec.clone(),
            authority: Arc::new(PostgresConnectAuthority {
                persistence: vault.persistence.clone(),
            }),
            store: Arc::new(vault),
        })
    }
    fn spawn_finish(
        &self,
        start: &ConnectStart,
        cancellation: CancellationToken,
    ) -> tokio::task::JoinHandle<IntegrationResult<ConnectAttempt>> {
        let service = self.service.clone();
        let owner = self.owner.clone();
        let callback = self.callback(start);
        tokio::spawn(async move {
            service
                .finish_connect(&owner, callback, work(cancellation))
                .await
        })
    }
    pub async fn begin(&self, reconnect: Option<ReconnectTarget>) -> ConnectStart {
        self.service
            .begin_connect(&self.owner, command(reconnect))
            .await
            .unwrap()
    }
    pub fn callback(&self, start: &ConnectStart) -> ConnectCallback {
        let url = url::Url::parse(start.authorization_url.as_str()).unwrap();
        let state = url
            .query_pairs()
            .find(|(k, _)| k == "state")
            .unwrap()
            .1
            .into_owned();
        ConnectCallback {
            provider: ProviderId::parse("google").unwrap(),
            state,
            code: Some(SecretString::from("authorization-code-sentinel")),
            error: None,
        }
    }
    pub async fn finish(&self, start: &ConnectStart) -> ConnectAttempt {
        self.service
            .finish_connect(
                &self.owner,
                self.callback(start),
                work(CancellationToken::new()),
            )
            .await
            .unwrap()
    }
    pub fn pool(&self) -> &sqlx::PgPool {
        self.vault.persistence.pool()
    }
    pub(crate) async fn attempt_count(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM integration_connect_attempts WHERE owner_namespace=$1 AND owner_subject=$2")
            .bind(self.owner.owner().namespace.as_str()).bind(self.owner.owner().subject.as_str())
            .fetch_one(self.pool()).await.unwrap()
    }
}
pub(crate) fn verified(session: VerifiedSession) -> VerifiedOwner {
    let reference = session.session_id.0.to_string();
    VerifiedOwner::for_browser(
        owner_for_user(session.user_id),
        ConnectBrowser {
            session: ConnectSessionReference::parse(reference.clone()).unwrap(),
            binding: BrowserBinding::parse(format!(
                "{:x}",
                Sha256::digest(format!("openbot/vault/{reference}").as_bytes())
            ))
            .unwrap(),
        },
    )
}
fn command(reconnect: Option<ReconnectTarget>) -> BeginConnect {
    BeginConnect {
        app: AppId::parse("gmail").unwrap(),
        config: AuthConfigId::parse("google-personal").unwrap(),
        requested_capabilities: RequestedCapabilities::try_from(vec![
            Capability::parse("gmail.read").unwrap(),
            Capability::parse("gmail.send").unwrap(),
        ])
        .unwrap(),
        return_target: SafeReturnTarget::parse("/ui/profile/vault".into()).unwrap(),
        reconnect,
    }
}
fn work(cancellation: CancellationToken) -> ConnectWork {
    ConnectWork {
        deadline: Utc::now() + chrono::Duration::seconds(20),
        cancellation,
    }
}
