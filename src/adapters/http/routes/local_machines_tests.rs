use super::*;
use crate::{
    adapters::{
        http::{
            auth::require_auth,
            auth_admission::{self, AuthAdmission},
            security::browser_security,
            session::SessionAuthority,
        },
        persistence::local_agents::test_support::{Candidate, Fixture},
    },
    infra::config::AppConfig,
    use_cases::user::UserPersistence,
};
use axum::{
    body::{Body, to_bytes},
    http::{Method, Request},
    middleware,
};
use tower::ServiceExt;

fn public(f: &Fixture) -> Router {
    public_router()
        .with_state(f.cases.clone())
        .layer(middleware::from_fn_with_state(
            Arc::new(AuthAdmission::new(Vec::new())),
            auth_admission::admit,
        ))
}

fn request(method: Method, path: &str, body: String) -> Request<Body> {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::HOST, "localhost")
        .body(Body::from(body))
        .unwrap();
    request.extensions_mut().insert(axum::extract::ConnectInfo(
        "127.0.0.1:12345".parse::<std::net::SocketAddr>().unwrap(),
    ));
    request
}

async fn json_body(response: Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 64 * 1024).await.unwrap()).unwrap()
}

fn verifier(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn claim_body(candidate: &Candidate) -> Value {
    json!({"code":candidate.claim.code.expose_secret(),"installation_id":candidate.claim.installation_id,
        "name":candidate.claim.name,"claim_verifier":verifier(candidate.claim.claim_verifier.as_bytes()),
        "credential_verifier":verifier(candidate.claim.credential_verifier.as_bytes())})
}

fn protected(f: &Fixture) -> (Router, Arc<SessionAuthority>) {
    let config = Arc::new(AppConfig::for_test());
    let authority = Arc::new(SessionAuthority::new(&config, f.persistence.clone()));
    let app = protected_router()
        .route_layer(middleware::from_fn_with_state(
            authority.clone(),
            require_auth,
        ))
        .with_state(f.cases.clone())
        .layer(middleware::from_fn_with_state(config, browser_security))
        .layer(middleware::from_fn_with_state(
            Arc::new(AuthAdmission::new(Vec::new())),
            auth_admission::admit,
        ));
    (app, authority)
}

struct SessionCookie(HeaderValue);

struct ForeignMutation {
    method: Method,
    path: String,
    body: Value,
}

async fn assert_missing_mutation(
    app: &Router,
    owner: &SessionCookie,
    mutation: ForeignMutation,
    resource: Uuid,
) {
    let missing = mutation
        .path
        .replace(&resource.to_string(), &Uuid::new_v4().to_string());
    let foreign_response = app
        .clone()
        .oneshot(authenticated(
            mutation.method.clone(),
            &mutation.path,
            owner,
            mutation.body.clone(),
        ))
        .await
        .unwrap();
    let missing_response = app
        .clone()
        .oneshot(authenticated(
            mutation.method,
            &missing,
            owner,
            mutation.body,
        ))
        .await
        .unwrap();
    assert_eq!(foreign_response.status(), StatusCode::NOT_FOUND);
    assert_eq!(missing_response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        to_bytes(foreign_response.into_body(), 4096).await.unwrap(),
        to_bytes(missing_response.into_body(), 4096).await.unwrap()
    );
}

async fn assert_foreign_machine_routes(f: &Fixture, app: &Router, owner: &SessionCookie) {
    let path = format!("/api/companies/{}/local-machines", f.company);
    let paired = f.pair(f.company).await;
    let mut machine_auth = request(Method::GET, &path, String::new());
    machine_auth.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {}", paired.candidate.credential.expose_secret())
            .parse()
            .unwrap(),
    );
    assert_eq!(
        app.clone().oneshot(machine_auth).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    let wrong = format!(
        "/api/companies/{}/local-machines/{}",
        f.foreign, paired.receipt.machine.id
    );
    assert_eq!(
        app.clone()
            .oneshot(authenticated(Method::GET, &wrong, owner, json!({})))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.clone()
            .oneshot(authenticated(
                Method::POST,
                &format!("{wrong}/state"),
                owner,
                json!({"action":"revoke"})
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        f.cases
            .machine(f.owner, f.company, paired.receipt.machine.id)
            .await
            .unwrap()
            .state,
        crate::domain::local_agents::MachineState::Enabled
    );
}

#[tokio::test]
async fn cloudroom_pairing_http_response_loss_and_dedicated_header_recovery() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let issued = f
        .cases
        .create_pairing(f.owner, f.company, None)
        .await
        .unwrap();
    let candidate = Candidate::new(issued.code);
    let app = public(&f);
    let paired = app
        .clone()
        .oneshot(request(
            Method::POST,
            "/v1/local-machines/pair",
            claim_body(&candidate).to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(paired.status(), StatusCode::OK);
    assert_eq!(paired.headers()[header::CACHE_CONTROL], "no-store");
    let paired = json_body(paired).await;
    assert!(
        !paired
            .to_string()
            .contains(candidate.credential.expose_secret())
    );
    let missing = app
        .clone()
        .oneshot(request(
            Method::GET,
            "/v1/local-machines/pair-status?claim=not-accepted",
            String::new(),
        ))
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
    let mut status = request(Method::GET, "/v1/local-machines/pair-status", String::new());
    status.headers_mut().insert(
        "x-openbot-pair-claim",
        candidate.secret.expose_secret().parse().unwrap(),
    );
    status.headers_mut().insert(
        "x-openbot-installation-id",
        candidate.claim.installation_id.to_string().parse().unwrap(),
    );
    let result = app.clone().oneshot(status).await.unwrap();
    assert_eq!(result.status(), StatusCode::OK);
    assert_eq!(result.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        json_body(result).await["machine"]["id"],
        paired["machine"]["id"]
    );
    let mut duplicate = request(Method::GET, "/v1/local-machines/pair-status", String::new());
    duplicate.headers_mut().append(
        "x-openbot-pair-claim",
        candidate.secret.expose_secret().parse().unwrap(),
    );
    duplicate.headers_mut().append(
        "x-openbot-pair-claim",
        candidate.secret.expose_secret().parse().unwrap(),
    );
    duplicate.headers_mut().insert(
        "x-openbot-installation-id",
        candidate.claim.installation_id.to_string().parse().unwrap(),
    );
    assert_eq!(
        app.oneshot(duplicate).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn cloudroom_pairing_http_bounds_and_closed_payloads_reject_before_enrollment() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let app = public(&f);
    let oversized = app
        .clone()
        .oneshot(request(
            Method::POST,
            "/v1/local-machines/pair",
            "x".repeat(4097),
        ))
        .await
        .unwrap();
    assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let issued = f
        .cases
        .create_pairing(f.owner, f.company, None)
        .await
        .unwrap();
    let candidate = Candidate::new(issued.code);
    let mut unknown = claim_body(&candidate);
    unknown["company_id"] = json!(f.foreign);
    let response = app
        .clone()
        .oneshot(request(
            Method::POST,
            "/v1/local-machines/pair",
            unknown.to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let mut malformed = claim_body(&candidate);
    malformed["credential_verifier"] = json!("A".repeat(64));
    assert_eq!(
        app.oneshot(request(
            Method::POST,
            "/v1/local-machines/pair",
            malformed.to_string()
        ))
        .await
        .unwrap()
        .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM local_machines")
        .fetch_one(&f.database.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

async fn cookie(f: &Fixture, authority: &SessionAuthority, user: Uuid) -> SessionCookie {
    let user = UserPersistence::get_by_id(f.persistence.as_ref(), user)
        .await
        .unwrap()
        .unwrap();
    let cookie = authority.cookie(&user).await.unwrap();
    SessionCookie(
        format!("{}={}", cookie.name(), cookie.value())
            .parse()
            .unwrap(),
    )
}

fn authenticated(method: Method, path: &str, cookie: &SessionCookie, body: Value) -> Request<Body> {
    let mut result = request(method, path, body.to_string());
    result
        .headers_mut()
        .insert(header::COOKIE, cookie.0.clone());
    result
        .headers_mut()
        .insert(header::ORIGIN, HeaderValue::from_static("http://localhost"));
    result
}

#[tokio::test]
async fn cloudroom_company_http_session_csrf_and_foreign_object_guards() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (app, authority) = protected(&f);
    let owner = cookie(&f, &authority, f.owner).await;
    let path = format!("/api/companies/{}/local-machine-pairings", f.company);
    assert_eq!(
        app.clone()
            .oneshot(request(Method::GET, &path, String::new()))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let mut csrf = authenticated(Method::POST, &path, &owner, json!({}));
    csrf.headers_mut().remove(header::ORIGIN);
    assert_eq!(
        app.clone().oneshot(csrf).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let created = app
        .clone()
        .oneshot(authenticated(Method::POST, &path, &owner, json!({})))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    assert_eq!(created.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(json_body(created).await["code"].as_str().unwrap().len(), 32);
    let member = cookie(&f, &authority, f.member).await;
    assert_eq!(
        app.clone()
            .oneshot(authenticated(Method::POST, &path, &member, json!({})))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let stranger = cookie(&f, &authority, f.stranger).await;
    assert_eq!(
        app.clone()
            .oneshot(authenticated(Method::POST, &path, &stranger, json!({})))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_foreign_machine_routes(&f, &app, &owner).await;
}

#[tokio::test]
async fn cloudroom_company_http_foreign_mutations_preserve_pairing_projects_and_machine() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (app, authority) = protected(&f);
    let owner = cookie(&f, &authority, f.owner).await;
    let paired = f.pair(f.foreign).await;
    let catalog = f.install_catalog(&paired).await;
    let machine = paired.receipt.machine.id;
    f.cases
        .set_projects(f.owner, f.foreign, machine, &catalog.catalog.projects)
        .await
        .unwrap();
    let issued = f
        .cases
        .create_pairing(f.owner, f.foreign, None)
        .await
        .unwrap();
    let pairings_before = f
        .cases
        .list_pairings(f.owner, f.company, None)
        .await
        .unwrap();
    let base = format!("/api/companies/{}", f.company);
    assert_missing_mutation(
        &app,
        &owner,
        ForeignMutation {
            method: Method::DELETE,
            path: format!("{base}/local-machine-pairings/{}", issued.attempt.id),
            body: json!({}),
        },
        issued.attempt.id.as_uuid(),
    )
    .await;
    assert_missing_mutation(
        &app,
        &owner,
        ForeignMutation {
            method: Method::PUT,
            path: format!("{base}/local-machines/{machine}/projects"),
            body: json!({"projects":[]}),
        },
        machine.as_uuid(),
    )
    .await;
    assert_missing_mutation(
        &app,
        &owner,
        ForeignMutation {
            method: Method::POST,
            path: format!("{base}/local-machines/{machine}/replacement-pairing"),
            body: json!({}),
        },
        machine.as_uuid(),
    )
    .await;
    assert_foreign_mutation_targets_preserved(&f, machine, issued, pairings_before.len()).await;
}

async fn assert_foreign_mutation_targets_preserved(
    f: &Fixture,
    machine: MachineId,
    issued: CreatedPairing,
    pairings_before: usize,
) {
    assert_eq!(
        f.cases
            .machine(f.owner, f.foreign, machine)
            .await
            .unwrap()
            .state,
        crate::domain::local_agents::MachineState::Enabled
    );
    let projects: Vec<String> = sqlx::query_scalar("SELECT project_id FROM local_machine_projects WHERE company_id=$1 AND machine_id=$2 ORDER BY project_id")
        .bind(f.foreign).bind(machine.as_uuid()).fetch_all(&f.database.pool).await.unwrap();
    assert_eq!(projects, vec!["proj_test"]);
    assert_eq!(
        f.cases
            .list_pairings(f.owner, f.company, None)
            .await
            .unwrap()
            .len(),
        pairings_before
    );
    // Prove the foreign pairing was neither cancelled nor consumed by the wrong route.
    let candidate = Candidate::new(issued.code);
    assert_eq!(
        f.cases
            .pair("127.0.0.1".parse().unwrap(), candidate.claim)
            .await
            .unwrap()
            .company_id,
        f.foreign
    );
    assert_eq!(
        f.cases
            .list_pairings(f.owner, f.foreign, None)
            .await
            .unwrap()
            .len(),
        2
    );
}
