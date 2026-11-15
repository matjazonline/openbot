use super::*;
use crate::{
    adapters::{
        http::{auth::require_auth, security::browser_security, session::SessionAuthority},
        persistence::local_agents::test_support::Fixture,
    },
    entities::{
        skill_bundle::{MAX_SKILL_ASSET_BYTES, MAX_SKILL_FILES, SkillFile},
        value_objects::EmailAddress,
    },
    use_cases::user::UserPersistence,
};
use axum::{
    body::{Body, to_bytes},
    extract::FromRef,
    http::{HeaderValue, Request, header},
    middleware,
};
use serde_json::{Value, json};
use tower::ServiceExt;

#[derive(Clone)]
struct TestState {
    skills: Arc<SkillUseCases>,
    users: Arc<UserUseCases>,
    config: Arc<AppConfig>,
}
impl FromRef<TestState> for Arc<SkillUseCases> {
    fn from_ref(s: &TestState) -> Self {
        s.skills.clone()
    }
}
impl FromRef<TestState> for Arc<UserUseCases> {
    fn from_ref(s: &TestState) -> Self {
        s.users.clone()
    }
}
impl FromRef<TestState> for Arc<AppConfig> {
    fn from_ref(s: &TestState) -> Self {
        s.config.clone()
    }
}

async fn app(f: &Fixture) -> (Router, Arc<SessionAuthority>) {
    sqlx::query("UPDATE users SET email_verified_at=CURRENT_TIMESTAMP WHERE id=$1")
        .bind(f.owner)
        .execute(&f.database.pool)
        .await
        .unwrap();
    let owner = f.persistence.get_by_id(f.owner).await.unwrap().unwrap();
    let mut config = AppConfig::for_test();
    config.operator_emails = vec![EmailAddress::from(owner.email.as_str())];
    let config = Arc::new(config);
    let authority = Arc::new(SessionAuthority::new(&config, f.persistence.clone()));
    let state = TestState {
        skills: Arc::new(SkillUseCases::new(
            f.persistence.clone(),
            f.persistence.clone(),
            f.persistence.clone(),
        )),
        users: Arc::new(UserUseCases::new(
            Arc::new(crate::infra::argon2_password_hasher()),
            f.persistence.clone(),
            "test-skill-attempt-key".into(),
        )),
        config: config.clone(),
    };
    (
        router()
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
    let user = f.persistence.get_by_id(actor).await.unwrap().unwrap();
    let cookie = authority.cookie(&user).await.unwrap();
    format!("{}={}", cookie.name(), cookie.value())
        .parse()
        .unwrap()
}
fn request(
    method: &str,
    path: &str,
    cookie: &HeaderValue,
    bytes: Vec<u8>,
    content_type: &str,
) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "localhost")
        .header(header::ORIGIN, "http://localhost")
        .header(header::COOKIE, cookie)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(bytes))
        .unwrap()
}
async fn value(response: Response) -> Value {
    serde_json::from_slice(
        &to_bytes(response.into_body(), SKILL_JSON_BODY_LIMIT)
            .await
            .unwrap(),
    )
    .unwrap()
}
fn payload(slug: &str) -> Value {
    json!({"slug":slug,"name":"Directory skill","description":"Use its files.","instructions":"Use scripts/check.py.\n","files":[]})
}

#[tokio::test]
async fn skill_bundle_http_maximum_json_uses_the_real_company_and_operator_routes() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (app, authority) = app(&f).await;
    let owner = cookie(&f, &authority, f.owner).await;
    let files: Vec<_> = (0..MAX_SKILL_FILES)
        .map(|n| {
            SkillFile::from_bytes(
                format!("assets/file-{n:03}.bin"),
                &vec![n as u8; MAX_SKILL_ASSET_BYTES / MAX_SKILL_FILES],
                n == 0,
            )
            .unwrap()
        })
        .collect();
    let mut input = payload("maximum-directory");
    input["files"] = serde_json::to_value(&files).unwrap();
    let bytes = serde_json::to_vec(&input).unwrap();
    assert!(bytes.len() > SKILL_FORM_BODY_LIMIT);
    assert!(bytes.len() < SKILL_JSON_BODY_LIMIT * 3 / 4);
    for uri in [
        format!("/api/companies/{}/skills", f.company),
        "/api/skill-library".into(),
    ] {
        let response = app
            .clone()
            .oneshot(request(
                "POST",
                &uri,
                &owner,
                bytes.clone(),
                "application/json",
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let created: Skill = serde_json::from_value(value(response).await).unwrap();
        assert_eq!(created.files, files);
        assert_eq!(created.instructions, "Use scripts/check.py.\n");
        let read = app
            .clone()
            .oneshot(request(
                "GET",
                &format!("{uri}/{}", created.id),
                &owner,
                vec![],
                "application/json",
            ))
            .await
            .unwrap();
        assert_eq!(read.status(), StatusCode::OK);
        assert_eq!(
            serde_json::from_value::<Skill>(value(read).await)
                .unwrap()
                .files,
            files
        );
        let listed = app
            .clone()
            .oneshot(request("GET", &uri, &owner, vec![], "application/json"))
            .await
            .unwrap();
        assert_eq!(listed.status(), StatusCode::OK);
        let bytes = to_bytes(listed.into_body(), SKILL_JSON_BODY_LIMIT)
            .await
            .unwrap();
        assert!(
            bytes.len() < 4096,
            "Lists must not multiply asset bodies by the page size"
        );
        let list: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(list["items"][0]["file_count"], MAX_SKILL_FILES);
        assert!(list["items"][0].get("files").is_none());
    }
}

#[tokio::test]
async fn skill_bundle_http_json_body_boundary_rejects_before_writing() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (app, authority) = app(&f).await;
    let owner = cookie(&f, &authority, f.owner).await;
    for (suffix, length, status) in [
        ("at-limit", SKILL_JSON_BODY_LIMIT, StatusCode::CREATED),
        (
            "over-limit",
            SKILL_JSON_BODY_LIMIT + 1,
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
    ] {
        let mut bytes = serde_json::to_vec(&payload(suffix)).unwrap();
        bytes.resize(length, b' ');
        for uri in [
            format!("/api/companies/{}/skills", f.company),
            "/api/skill-library".into(),
        ] {
            let response = app
                .clone()
                .oneshot(request(
                    "POST",
                    &uri,
                    &owner,
                    bytes.clone(),
                    "application/json",
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), status, "{uri} length {length}");
        }
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM skills WHERE (company_id=$1 OR company_id IS NULL) AND slug='over-limit'")
        .bind(f.company).fetch_one(&f.database.pool).await.unwrap();
    assert_eq!(count, 0);
}

fn multipart(bytes: &[u8]) -> Vec<u8> {
    let mut body = b"--skill-boundary\r\nContent-Disposition: form-data; name=\"bundle\"; filename=\"skill.zip\"\r\nContent-Type: application/zip\r\n\r\n".to_vec();
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n--skill-boundary--\r\n");
    body
}

#[path = "http_bundle_tests.rs"]
mod bundles;

#[test]
#[ignore = "explicit browser fixture generator"]
fn dump_skill_bundle_browser_fixtures() {
    let directory = std::path::PathBuf::from(
        std::env::var_os("SKILL_BUNDLE_BROWSER_FIXTURES")
            .expect("set SKILL_BUNDLE_BROWSER_FIXTURES"),
    );
    std::fs::create_dir_all(&directory).unwrap();
    let now = chrono::Utc::now();
    let skill = Skill {
        id: Uuid::from_u128(901),
        company_id: Some(Uuid::from_u128(902)),
        slug: crate::entities::value_objects::SkillSlug::parse("script-directory").unwrap(),
        name: "Script directory".into(),
        description: "Use its bundled script.".into(),
        instructions: "Run scripts/check.py with assets/data.bin.\n".into(),
        files: vec![
            SkillFile::from_bytes(
                "scripts/check.py".into(),
                b"#!/usr/bin/env python3\nprint('native-skill-script-ok')\n",
                true,
            )
            .unwrap(),
            SkillFile::from_bytes("assets/data.bin".into(), b"\0\xff\r\n", false).unwrap(),
        ],
        created_by: crate::entities::creation::CreationProvenance::system(),
        created_at: now,
        updated_at: now,
    };
    let draft = pages::SkillDraft::stored(&skill);
    let summaries = [crate::entities::skill::SkillSummary::from(&skill)];
    let render = |error, notice| {
        pages::company_skills_section(&pages::SkillSection {
            company_id: skill.company_id,
            skills: &summaries,
            next_url: None,
            previous_url: None,
            draft: Some(&draft),
            error,
            notice,
            editable: true,
        })
    };
    std::fs::write(
        directory.join("page.html"),
        pages::base_layout("Skill directories", &render(None, None)),
    )
    .unwrap();
    std::fs::write(
        directory.join("success.html"),
        render(None, Some("Skill directory imported.")),
    )
    .unwrap();
    std::fs::write(
        directory.join("failure.html"),
        render(Some("Invalid skill ZIP. Choose another file."), None),
    )
    .unwrap();
    std::fs::write(
        directory.join("files.html"),
        pages::skill_bundle_files(&skill),
    )
    .unwrap();
    std::fs::write(
        directory.join("skill.zip"),
        archive::encode(&skill).unwrap(),
    )
    .unwrap();
    std::fs::write(directory.join("app.js"), pages::application_javascript()).unwrap();
    std::fs::write(
        directory.join("theme-init.js"),
        pages::theme_init_javascript(),
    )
    .unwrap();
}
