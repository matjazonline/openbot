use super::*;
use crate::use_cases::{
    company::CompanyUseCases,
    thread::{InboundIngestPorts, ThreadStores, ThreadUseCases},
    user::UserUseCases,
};
use crate::{
    adapters::{
        http::{auth::require_auth, security::browser_security, session::SessionAuthority},
        persistence::local_agents::test_support::Fixture,
    },
    infra::config::AppConfig,
    use_cases::user::UserPersistence,
};
use axum::{
    body::{Body, to_bytes},
    extract::FromRef,
    http::{HeaderValue, Method, Request, header},
};
use serde_json::{Value, json};
use tower::ServiceExt;

#[path = "ui_local_agents_tests.rs"]
mod ui;

#[derive(Clone)]
struct State {
    machines: Arc<LocalMachineUseCases>,
    management: Arc<LocalAgentManagement>,
    companies: Arc<CompanyUseCases>,
    threads: Arc<ThreadUseCases>,
    users: Arc<UserUseCases>,
}
impl FromRef<State> for Arc<LocalMachineUseCases> {
    fn from_ref(state: &State) -> Self {
        state.machines.clone()
    }
}
impl FromRef<State> for Arc<LocalAgentManagement> {
    fn from_ref(state: &State) -> Self {
        state.management.clone()
    }
}
impl FromRef<State> for Arc<CompanyUseCases> {
    fn from_ref(state: &State) -> Self {
        state.companies.clone()
    }
}
impl FromRef<State> for Arc<ThreadUseCases> {
    fn from_ref(state: &State) -> Self {
        state.threads.clone()
    }
}
impl FromRef<State> for Arc<UserUseCases> {
    fn from_ref(state: &State) -> Self {
        state.users.clone()
    }
}

fn app(f: &Fixture) -> (Router, Arc<SessionAuthority>) {
    let config = Arc::new(AppConfig::for_test());
    let authority = Arc::new(SessionAuthority::new(&config, f.persistence.clone()));
    let state = State {
        machines: f.cases.clone(),
        management: Arc::new(LocalAgentManagement::new(
            f.persistence.clone(),
            f.persistence.clone(),
        )),
        companies: Arc::new(CompanyUseCases::new(f.persistence.clone())),
        users: Arc::new(UserUseCases::new(
            Arc::new(crate::infra::argon2_password_hasher()),
            f.persistence.clone(),
            "test-attempt-secret".into(),
        )),
        threads: Arc::new(ThreadUseCases::new(
            ThreadStores {
                threads: f.persistence.clone(),
                channels: f.persistence.clone(),
                companies: f.persistence.clone(),
                participants: f.persistence.clone(),
            },
            InboundIngestPorts {
                committer: f.persistence.clone(),
                correlation: f.persistence.clone(),
                bindings: f.persistence.clone(),
                standalone_deliveries: f.persistence.clone(),
            },
            Arc::new(crate::application::transport::TransportRenderers::new()),
            Arc::new(crate::application::settings::ThreadSettings {
                mail_domain: "agents.test".into(),
                max_spam_score: 5.0,
            }),
        )),
    };
    (
        router()
            .merge(super::super::ui_local_agents::router())
            .route_layer(middleware::from_fn_with_state(
                authority.clone(),
                require_auth,
            ))
            .with_state(state)
            .layer(middleware::from_fn_with_state(config, browser_security)),
        authority,
    )
}

async fn cookie(f: &Fixture, authority: &SessionAuthority, actor: Uuid) -> HeaderValue {
    let user = UserPersistence::get_by_id(f.persistence.as_ref(), actor)
        .await
        .unwrap()
        .unwrap();
    let cookie = authority.cookie(&user).await.unwrap();
    format!("{}={}", cookie.name(), cookie.value())
        .parse()
        .unwrap()
}

fn request(method: Method, path: &str, cookie: &HeaderValue, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "localhost")
        .header(header::COOKIE, cookie)
        .header(header::ORIGIN, "http://localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn body(response: Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap()).unwrap()
}

#[tokio::test]
async fn cloudroom_management_http_auth_csrf_typed_requests_and_frozen_session_roundtrip() {
    let f = Fixture::new().await.unwrap();
    let paired = f.pair(f.company).await;
    f.install_catalog(&paired).await;
    let profile = f.profile(paired.receipt.machine.id);
    f.cases
        .set_projects(
            f.owner,
            f.company,
            paired.receipt.machine.id,
            std::slice::from_ref(&profile.selection.project_id),
        )
        .await
        .unwrap();
    let (app, authority) = app(&f);
    let cookie = cookie(&f, &authority, f.owner).await;
    let profiles = format!("/api/companies/{}/local-agent-profiles", f.company);
    let input = json!({"name":"Local <test>","selection":profile.selection});
    let mut anonymous = request(Method::GET, &profiles, &cookie, json!({}));
    anonymous.headers_mut().remove(header::COOKIE);
    assert_eq!(
        app.clone().oneshot(anonymous).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    let mut csrf = request(Method::POST, &profiles, &cookie, input.clone());
    csrf.headers_mut().remove(header::ORIGIN);
    assert_eq!(
        app.clone().oneshot(csrf).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let response = app
        .clone()
        .oneshot(request(Method::POST, &profiles, &cookie, input))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let saved = body(response).await;
    let conversation = f.conversation(f.company).await;
    let sessions = format!("/api/companies/{}/local-agent-sessions", f.company);
    let operation = Uuid::now_v7();
    let input = json!({"request_id":operation,"profile_id":saved["id"],"conversation_id":conversation,"prompt":"explicit HTTP task"});
    let response = app
        .clone()
        .oneshot(request(Method::POST, &sessions, &cookie, input.clone()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let accepted = body(response).await;
    assert_eq!(accepted["origin"], "direct");
    assert_eq!(
        accepted["operation"]["command"]["prompt"],
        "explicit HTTP task"
    );
    let replay = app
        .clone()
        .oneshot(request(Method::POST, &sessions, &cookie, input.clone()))
        .await
        .unwrap();
    assert_eq!(body(replay).await, accepted);
    let details = app
        .clone()
        .oneshot(request(
            Method::GET,
            &format!("{sessions}/{operation}"),
            &cookie,
            json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(details.status(), StatusCode::OK);
    assert_eq!(body(details).await["session"]["profile_id"], saved["id"]);
    let mut forged = input;
    forged["origin"] = json!("workflow");
    assert_eq!(
        app.clone()
            .oneshot(request(Method::POST, &sessions, &cookie, forged))
            .await
            .unwrap()
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        app.oneshot(request(Method::GET, &sessions, &cookie, json!({})))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn cloudroom_management_http_foreign_ids_cannot_read_or_mutate_resources() {
    let f = Fixture::new().await.unwrap();
    let paired = f.pair(f.company).await;
    f.install_catalog(&paired).await;
    let profile = f.profile(paired.receipt.machine.id);
    f.cases
        .set_projects(
            f.owner,
            f.company,
            paired.receipt.machine.id,
            std::slice::from_ref(&profile.selection.project_id),
        )
        .await
        .unwrap();
    f.cases.save_profile(f.owner, &profile).await.unwrap();
    let (app, authority) = app(&f);
    let cookie = cookie(&f, &authority, f.owner).await;
    let id = OperationId::new(Uuid::now_v7()).unwrap();
    let conversation = f.conversation(f.company).await;
    let accepted=app.clone().oneshot(request(Method::POST,&format!("/api/companies/{}/local-agent-sessions",f.company),&cookie,
        json!({"request_id":id,"profile_id":profile.id,"conversation_id":conversation,"prompt":"private task"}))).await.unwrap();
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    let base = format!("/api/companies/{}", f.foreign);
    let paths = [
        (
            Method::GET,
            format!("{base}/local-agent-profiles/{}", profile.id),
            json!({}),
        ),
        (
            Method::PUT,
            format!("{base}/local-agent-profiles/{}", profile.id),
            json!({"name":"forged","selection":profile.selection}),
        ),
        (
            Method::DELETE,
            format!("{base}/local-agent-profiles/{}", profile.id),
            json!({}),
        ),
        (
            Method::GET,
            format!("{base}/local-agent-sessions/{id}"),
            json!({}),
        ),
        (
            Method::GET,
            format!("{base}/local-agent-sessions?conversation_id={conversation}"),
            json!({}),
        ),
        (
            Method::GET,
            format!("{base}/local-agent-operations/{id}"),
            json!({}),
        ),
        (
            Method::GET,
            format!(
                "{base}/local-machines/{}/runtime",
                paired.receipt.machine.id
            ),
            json!({}),
        ),
        (
            Method::POST,
            format!(
                "{base}/local-machines/{}/test-connection",
                paired.receipt.machine.id
            ),
            json!({"request_id":Uuid::now_v7()}),
        ),
        (
            Method::POST,
            format!("{base}/local-agent-sessions/{id}/commands"),
            json!({"request_id":Uuid::now_v7(),"command":{"kind":"inspect"}}),
        ),
        (
            Method::POST,
            format!("{base}/local-agent-profiles"),
            json!({"name":"forged","selection":profile.selection}),
        ),
        (
            Method::POST,
            format!("{base}/local-agent-sessions"),
            json!({"request_id":Uuid::now_v7(),"profile_id":profile.id,"conversation_id":conversation,"prompt":"forged"}),
        ),
    ];
    for (method, path, input) in paths {
        let response = app
            .clone()
            .oneshot(request(method, &path, &cookie, input))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM local_agent_operations")
        .fetch_one(&f.database.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}
