use super::*;
use crate::{
    adapters::{
        http::{auth, security, session::SessionAuthority},
        integrations::owner::owner_for_user,
        persistence::{
            PostgresPersistence,
            credentials::CredentialCipher,
            integration_bindings::PostgresIntegrationBindings,
            integrations::PostgresVault,
            test_support::{OwnDatabase, insert_company_with_owner, own_database},
        },
    },
    application::{
        integrations::management::{AccountRepository, AccountWrite},
        workflow::{
            CompanyId, WorkflowActor,
            http::{HttpConnectionWrite, HttpConnections, HttpOrigin},
        },
    },
    infra::config::AppConfig,
    use_cases::{company::CompanyPersistence, user::UserPersistence},
};
use axum::{
    body::{Body, to_bytes},
    extract::FromRef,
    http::{Request, StatusCode},
    middleware,
};
use secrecy::SecretString;
use tower::ServiceExt;
#[derive(Clone)]
struct BrowserState {
    gateway: Arc<dyn IntegrationGateway>,
    bindings: Arc<dyn IntegrationBindings>,
}
impl FromRef<BrowserState> for Arc<dyn IntegrationGateway> {
    fn from_ref(s: &BrowserState) -> Self {
        s.gateway.clone()
    }
}
impl FromRef<BrowserState> for Arc<dyn IntegrationBindings> {
    fn from_ref(s: &BrowserState) -> Self {
        s.bindings.clone()
    }
}
struct Fixture {
    _database: OwnDatabase,
    p: PostgresPersistence,
    app: Router,
    cookie: String,
    user: Uuid,
    company: Uuid,
    connection: Uuid,
    proposal: Uuid,
    account: ConnectedAccount,
    vault: PostgresVault,
}
impl Fixture {
    async fn new() -> Self {
        let database = own_database().await.expect("owned database required");
        let p = PostgresPersistence::with_credential_cipher(
            database.pool.clone(),
            CredentialCipher::for_test(),
        );
        let config = Arc::new(AppConfig::for_test());
        let BrowserIdentity {
            user,
            company,
            cookie,
            sessions,
        } = browser_identity(&p, &config).await;
        let connection = p
            .create_http(
                CompanyId::new(company),
                WorkflowActor::authenticated(user).unwrap(),
                HttpConnectionWrite {
                    origin: HttpOrigin::parse("https://api.example.test").unwrap(),
                    bearer: false,
                },
            )
            .await
            .unwrap();
        let vault = PostgresVault::new(p.clone());
        let account = vault
            .create(
                &owner_for_user(user),
                AccountWrite {
                    key: AccountKey::parse("browser-private").unwrap(),
                    provider: ProviderId::parse("static-bearer").unwrap(),
                    config: AuthConfigId::parse("static-bearer").unwrap(),
                    method: AuthMethod::StaticSecret,
                    schema: CredentialSchema::parse("static-v1").unwrap(),
                    identity: None,
                    metadata: AccountMetadata::new("<private label>".into(), None).unwrap(),
                    payload: SecretString::from("sentinel-never-in-browser"),
                },
            )
            .await
            .unwrap();
        let bindings = Arc::new(PostgresIntegrationBindings {
            persistence: p.clone(),
            vault: vault.clone(),
        });
        let proposal = bindings
            .propose(
                user,
                ProposeBinding {
                    company,
                    connection: connection.id,
                    kind: BindingKind::Http,
                    app: AppId::parse("static-bearer").unwrap(),
                    expected_revision: connection.revision,
                },
            )
            .await
            .unwrap();
        let state = BrowserState {
            gateway: vault.gateway(
                bindings.clone(),
                Arc::new(crate::application::integrations::unavailable::UnavailableIntegrations),
                Arc::new(crate::application::integrations::unavailable::UnavailableIntegrations),
            ),
            bindings,
        };
        let app = router()
            .with_state(state)
            .layer(middleware::from_fn_with_state(sessions, auth::require_auth))
            .layer(middleware::from_fn_with_state(
                config,
                security::browser_security,
            ));
        Self {
            _database: database,
            p,
            app,
            cookie,
            user,
            company,
            connection: connection.id,
            proposal: proposal.id,
            account,
            vault,
        }
    }
    fn path(&self) -> String {
        format!("/ui/profile/vault/binding-proposals/{}", self.proposal)
    }
    fn request(
        &self,
        method: &str,
        path: &str,
        body: String,
        origin: Option<&str>,
    ) -> Request<Body> {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("host", "localhost:3000")
            .header("cookie", &self.cookie)
            .header("content-type", "application/x-www-form-urlencoded");
        if let Some(origin) = origin {
            request = request.header("origin", origin);
        }
        request.body(Body::from(body)).unwrap()
    }
}
#[path = "vault_authoring_route_tests.rs"]
mod authoring;
#[tokio::test]
async fn vault_binding_browser_consent_is_authenticated_csrf_protected_bounded_and_one_shot() {
    let f = Fixture::new().await;
    let value = format!("account_revision={}%3A1", f.account.id.as_uuid());
    rejected_requests(&f, &value).await;
    private_view(&f).await;
    confirm_and_unbind(&f, &value).await;
}

struct BrowserIdentity {
    user: Uuid,
    company: Uuid,
    cookie: String,
    sessions: Arc<SessionAuthority>,
}
async fn browser_identity(p: &PostgresPersistence, config: &Arc<AppConfig>) -> BrowserIdentity {
    let marker = Uuid::new_v4().simple().to_string();
    let user = p
        .create_user(
            &format!("binding-{marker}"),
            &format!("binding-{marker}@example.test"),
            "fixture-hash",
        )
        .await
        .unwrap();
    let company = Uuid::new_v4();
    insert_company_with_owner(
        p.pool(),
        company,
        "<company label>",
        &format!("binding-{marker}"),
        user.id,
    )
    .await;
    let mut principal_tx = p.pool().begin().await.unwrap();
    crate::adapters::persistence::participant::create_person_principal_on(
        &mut principal_tx,
        company,
        user.id,
        &format!("binding-{marker}"),
        &format!("binding-{marker}@example.test"),
    )
    .await
    .unwrap();
    principal_tx.commit().await.unwrap();
    let sessions = Arc::new(SessionAuthority::new(config, Arc::new(p.clone())));
    let cookie = sessions.cookie(&user).await.unwrap();
    let cookie = format!("{}={}", cookie.name(), cookie.value());
    BrowserIdentity {
        user: user.id,
        company,
        cookie,
        sessions,
    }
}

async fn rejected_requests(f: &Fixture, value: &str) {
    let mut no_cookie = f.request(
        "POST",
        &f.path(),
        value.to_owned(),
        Some("http://localhost:3000"),
    );
    no_cookie.headers_mut().remove("cookie");
    let anonymous = f.app.clone().oneshot(no_cookie).await.unwrap();
    assert!(!anonymous.status().is_success());
    assert!(
        f.vault
            .grants(&owner_for_user(f.user), f.account.id, None)
            .await
            .unwrap()
            .grants
            .is_empty()
    );
    let denied = f
        .app
        .clone()
        .oneshot(f.request(
            "POST",
            &f.path(),
            value.to_owned(),
            Some("https://foreign.example"),
        ))
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let huge = f
        .app
        .clone()
        .oneshot(f.request(
            "POST",
            &f.path(),
            format!("account_revision={}", "x".repeat(9000)),
            Some("http://localhost:3000"),
        ))
        .await
        .unwrap();
    assert_eq!(huge.status(), StatusCode::PAYLOAD_TOO_LARGE);
}
async fn private_view(f: &Fixture) {
    let view = f
        .app
        .clone()
        .oneshot(f.request("GET", &f.path(), String::new(), None))
        .await
        .unwrap();
    assert_eq!(view.status(), StatusCode::OK);
    let html = String::from_utf8(
        to_bytes(view.into_body(), 128 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains("&lt;company label&gt;") && html.contains("&lt;private label&gt;"));
    assert!(!html.contains("sentinel-never-in-browser") && !html.contains("enc:v2"));
    assert!(html.contains("hx-sync=\"this:drop\"") && !html.contains("data-submit=\"busy-once\""));
}
async fn confirm_and_unbind(f: &Fixture, value: &str) {
    let foreign = f
        .app
        .clone()
        .oneshot(f.request(
            "POST",
            &f.path(),
            format!("account_revision={}%3A1", Uuid::new_v4()),
            Some("http://localhost:3000"),
        ))
        .await
        .unwrap();
    assert!(!foreign.status().is_success());
    let saved = f
        .app
        .clone()
        .oneshot(f.request(
            "POST",
            &f.path(),
            value.to_owned(),
            Some("http://localhost:3000"),
        ))
        .await
        .unwrap();
    assert_eq!(saved.status(), StatusCode::SEE_OTHER);
    assert!(saved.headers().contains_key("hx-redirect"));
    let replay = f
        .app
        .clone()
        .oneshot(f.request(
            "POST",
            &f.path(),
            value.to_owned(),
            Some("http://localhost:3000"),
        ))
        .await
        .unwrap();
    assert!(!replay.status().is_success());
    let grants = f
        .vault
        .grants(&owner_for_user(f.user), f.account.id, None)
        .await
        .unwrap();
    assert_eq!(grants.grants.len(), 1);
    f.p.delete(f.company).await.unwrap();
    let grant = &grants.grants[0];
    let unbound = f
        .app
        .clone()
        .oneshot(f.request(
            "POST",
            &format!("/ui/profile/vault/grants/{}/revoke", grant.id.as_uuid()),
            format!("expected_revision={}", grant.revision.get()),
            Some("http://localhost:3000"),
        ))
        .await
        .unwrap();
    assert_eq!(unbound.status(), StatusCode::SEE_OTHER);
    assert!(!f.vault.grant_metadata(grant.id).await.unwrap().enabled);
    assert!(
        f.vault
            .get(&owner_for_user(f.user), f.account.id)
            .await
            .is_ok()
    );
}
