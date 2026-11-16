//! The actual password handlers, account use cases and bridge, with isolated PostgreSQL.
use super::*;
use crate::{
    adapters::{
        http::{
            attribution::Bridge,
            auth,
            auth_admission::{self, AuthAdmission},
            security,
        },
        persistence::{
            PostgresPersistence,
            test_support::{insert_company_with_owner, own_database},
        },
    },
    application::use_cases::user::{UserCredentialsHasher, UserPersistence},
    infra::{attribution::AttributionConfig, config::AppConfig},
};
use axum::{
    Router,
    extract::FromRef,
    middleware,
    routing::{get, post},
};
use base64::Engine;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
#[derive(Clone)]
struct BrowserState {
    users: Arc<UserUseCases>,
    sessions: Arc<SessionAuthority>,
    companies: Arc<CompanyUseCases>,
    invites: Arc<CompanyInviteUseCases>,
    bridge: Option<Arc<Bridge>>,
}
macro_rules! state_ref {
    ($target:ty,$field:ident) => {
        impl FromRef<BrowserState> for $target {
            fn from_ref(state: &BrowserState) -> Self {
                state.$field.clone()
            }
        }
    };
}
state_ref!(Arc<UserUseCases>, users);
state_ref!(Arc<SessionAuthority>, sessions);
state_ref!(Arc<CompanyUseCases>, companies);
state_ref!(Arc<CompanyInviteUseCases>, invites);
state_ref!(Option<Arc<Bridge>>, bridge);

#[tokio::test]
#[ignore = "requires built standalone attribution binary and pinned Playwright; required integration gate"]
async fn attribution_sso_browser_real_openbot_password_and_existing_session() {
    let database = own_database()
        .await
        .expect("browser integration requires PostgreSQL");
    let database_name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&database.pool)
        .await
        .unwrap();
    assert!(
        database_name.starts_with("open_bots_own_")
            && database_name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_')
    );
    sqlx::query(&format!(
        "REVOKE CONNECT ON DATABASE {database_name} FROM PUBLIC"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    let store = Arc::new(PostgresPersistence::new(database.pool.clone()));
    let mut config = AppConfig::for_test();
    config.secure_cookies = false; // Existing Openbot loopback development policy.
    let config = Arc::new(config);
    let sessions = Arc::new(SessionAuthority::new(&config, store.clone()));
    let username = format!("as-browser-{}", Uuid::new_v4().simple());
    let password = "attribution-browser-test-password";
    let hasher = crate::infra::argon2_password_hasher();
    let hash = hasher.hash_password(password).unwrap();
    let user = store
        .create_user(&username, &format!("{username}@example.test"), &hash)
        .await
        .unwrap();
    let company = Uuid::new_v4();
    insert_company_with_owner(
        &database.pool,
        company,
        "Browser fixture",
        &company.simple().to_string(),
        user.id,
    )
    .await;
    let openbot_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let openbot_origin = format!(
        "http://localhost:{}",
        openbot_listener.local_addr().unwrap().port()
    );
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let service_address = reservation.local_addr().unwrap();
    let service_origin = format!("http://{service_address}");
    let client = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([31u8; 32]);
    let lifecycle = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([32u8; 32]);
    let bridge_config = AttributionConfig::from_lookup(|name| match name {
        "OPENBOT_ATTRIBUTION_ORIGIN" => Some(service_origin.clone()),
        "OPENBOT_ATTRIBUTION_CLIENT_SECRET" => Some(client.clone()),
        "OPENBOT_ATTRIBUTION_LIFECYCLE_SECRET" => Some(lifecycle.clone()),
        _ => None,
    })
    .unwrap()
    .unwrap();
    let bridge = Arc::new(
        Bridge::new(
            Arc::new(bridge_config),
            store.clone(),
            store.clone(),
            sessions.clone(),
        )
        .unwrap(),
    );
    let state = BrowserState {
        users: Arc::new(UserUseCases::new(
            Arc::new(hasher),
            store.clone(),
            config.auth_attempt_secret.clone(),
        )),
        sessions: sessions.clone(),
        companies: Arc::new(CompanyUseCases::new(store.clone())),
        invites: Arc::new(CompanyInviteUseCases::new(store.clone(), store.clone())),
        bridge: Some(bridge.clone()),
    };
    let protected = Router::new()
        .route("/", get(index))
        .route("/logout", post(logout))
        .route_layer(middleware::from_fn_with_state(sessions, auth::require_auth));
    let app = Router::new()
        .merge(protected)
        .route("/login", get(login_page).post(login_form))
        .route("/api/user/login", post(login_form))
        .merge(super::super::assets::router())
        .merge(crate::adapters::http::attribution::router(Some(bridge)))
        .with_state(state)
        .layer(middleware::from_fn_with_state(
            Arc::new(AuthAdmission::from_env().unwrap()),
            auth_admission::admit,
        ))
        .layer(middleware::from_fn_with_state(
            config,
            security::browser_security,
        ));
    let shutdown = CancellationToken::new();
    let stop = shutdown.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            openbot_listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(stop.cancelled_owned())
        .await
    });
    drop(reservation);
    let binary = std::env::var("ATTRIBUTION_TEST_BINARY")
        .expect("build service and set ATTRIBUTION_TEST_BINARY");
    let output = tokio::process::Command::new("python3")
        .arg("services/attribution/scripts/browser-sso.py")
        .env("ATTRIBUTION_TEST_BINARY", binary)
        .env("ATTRIBUTION_FIXTURE_OPENBOT_ORIGIN", openbot_origin)
        .env("ATTRIBUTION_PUBLIC_ORIGIN", service_origin)
        .env("ATTRIBUTION_BIND_ADDR", service_address.to_string())
        .env("ATTRIBUTION_FIXTURE_USERNAME", username)
        .env("ATTRIBUTION_FIXTURE_PASSWORD", password)
        .env("ATTRIBUTION_FIXTURE_USER_ID", user.id.to_string())
        .env("ATTRIBUTION_FIXTURE_OPENBOT_DATABASE", database_name)
        .env("ATTRIBUTION_CLIENT_SECRET", client)
        .env("ATTRIBUTION_LIFECYCLE_SECRET", lifecycle)
        .kill_on_drop(true)
        .output()
        .await
        .unwrap();
    shutdown.cancel();
    server.await.unwrap().unwrap();
    assert!(
        output.status.success(),
        "browser fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let proof: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(proof["result"], "PASS");
    assert_eq!(proof["checks"].as_array().unwrap().len(), 8);
    println!("{}", String::from_utf8_lossy(&output.stdout));
    let redeemed:i64=sqlx::query_scalar("SELECT count(*) FROM attribution_identity_codes WHERE user_id = $1 AND consumed_at IS NOT NULL")
        .bind(user.id).fetch_one(&database.pool).await.unwrap();
    assert_eq!(redeemed, 2);
}
