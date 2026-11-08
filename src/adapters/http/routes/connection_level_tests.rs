use super::*;
use crate::adapters::persistence::{
    PostgresPersistence,
    test_support::{OwnDatabase, own_database},
};
use crate::application::{
    services::mcp_client::{McpClient, McpSession},
    workflow::http::{HttpConnectionWrite, HttpConnections, HttpOrigin},
};
use crate::entities::mcp::{CompanyMcpConnection, McpAuth, McpEndpoint};
use crate::use_cases::{
    company::{CompanyPersistence, CompanyUseCases, CompanyWrite},
    mcp::{McpConnectionWrite, McpPersistence, McpUseCases},
    user::UserPersistence,
};
use secrecy::SecretString;
use serde_json::{Value, json};
use std::sync::Arc;
struct NoNetwork;
#[path = "http_authoring_tests.rs"]
mod authoring_tests;
#[async_trait::async_trait]
impl McpClient for NoNetwork {
    async fn connect_prepared(
        &self,
        _: &McpEndpoint,
        _: secrecy::SecretString,
        _: crate::application::services::mcp_client::McpPreparation,
    ) -> crate::application::services::mcp_client::McpPreparationResult<
        Box<dyn crate::application::services::mcp_client::McpSession>,
    > {
        Err(crate::application::services::mcp_client::McpPreparationFailure::Unavailable)
    }
    async fn shutdown(&self) -> AppResult<()> {
        Ok(())
    }
    fn validate_endpoint(&self, _: &McpEndpoint) -> AppResult<()> {
        Ok(())
    }
    async fn connect(
        &self,
        _: &McpEndpoint,
        _: Option<SecretString>,
    ) -> AppResult<Box<dyn McpSession>> {
        panic!("level management must not connect to a provider")
    }
}
struct Fixture {
    _db: OwnDatabase,
    p: PostgresPersistence,
    s: ConnectionServices,
    owner: Uuid,
    company: Uuid,
    foreign: Uuid,
    http: HttpConnection,
    mcp: CompanyMcpConnection,
    foreign_http: HttpConnection,
    foreign_mcp: CompanyMcpConnection,
}
struct CompanyOwner {
    owner: Uuid,
    company: Uuid,
}
async fn company(p: &PostgresPersistence) -> CompanyOwner {
    let key = Uuid::new_v4().simple().to_string();
    let user = p
        .create_user(&key, &format!("{key}@example.test"), "hash")
        .await
        .unwrap();
    let company = CompanyPersistence::create(
        p,
        user.id,
        CompanyWrite {
            name: key.clone(),
            slug: key,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    CompanyOwner {
        owner: user.id,
        company: company.id,
    }
}
async fn connections(
    p: &PostgresPersistence,
    owner: Uuid,
    company: Uuid,
) -> (HttpConnection, CompanyMcpConnection) {
    let http = p
        .create_http(
            CompanyId::new(company),
            WorkflowActor::authenticated(owner).unwrap(),
            HttpConnectionWrite {
                origin: HttpOrigin::parse("https://example.test").unwrap(),
                bearer: false,
            },
        )
        .await
        .unwrap();
    let mcp = p
        .create_mcp_connection(
            company,
            McpConnectionWrite {
                slug: "server".into(),
                endpoint: "https://example.test/mcp".to_string().try_into().unwrap(),
                enabled: true,
                auth: McpAuth::None,
                discovered_tools: vec![],
                tool_grants: vec![],
            },
        )
        .await
        .unwrap();
    (http, mcp)
}
impl Fixture {
    async fn new() -> Self {
        let db = own_database().await.expect("database required");
        let p = PostgresPersistence::new(db.pool.clone());
        let CompanyOwner { owner, company } = company(&p).await;
        let CompanyOwner {
            owner: outsider,
            company: foreign,
        } = self::company(&p).await;
        let (http, mcp) = connections(&p, owner, company).await;
        let (foreign_http, foreign_mcp) = connections(&p, outsider, foreign).await;
        let shared = Arc::new(p.clone());
        let s = ConnectionServices {
            workflow: crate::infra::workflow::WorkflowEngine::for_tests(p.clone()).services(),
            company_use_cases: Arc::new(CompanyUseCases::new(shared.clone())),
            mcp_use_cases: Arc::new(McpUseCases::new(
                shared.clone(),
                shared.clone(),
                shared.clone(),
                Arc::new(NoNetwork),
                Arc::new(crate::application::use_cases::mcp::UnavailableMcpDiscovery),
            )),
        };
        Self {
            _db: db,
            p,
            s,
            owner,
            company,
            foreign,
            http,
            mcp,
            foreign_http,
            foreign_mcp,
        }
    }
    fn path(&self, id: Uuid) -> Path<ConnectionPath> {
        Path(ConnectionPath {
            company: self.company,
            id,
        })
    }
    fn user(&self) -> AuthenticatedUser {
        AuthenticatedUser { id: self.owner }
    }
    async fn snapshot(&self) -> Value {
        sqlx::query_scalar("SELECT jsonb_build_object('http',(SELECT jsonb_agg(to_jsonb(connection) ORDER BY id) FROM workflow_http_connections AS connection),'mcp',(SELECT jsonb_agg(to_jsonb(connection) ORDER BY id) FROM company_mcp_connections AS connection),'credentials',(SELECT count(*) FROM company_mcp_credentials)+(SELECT count(*) FROM workflow_http_credentials),'grants',(SELECT count(*) FROM company_mcp_tool_grants))").fetch_one(self.p.pool()).await.unwrap()
    }
    async fn admin(&self, level: i16) -> Uuid {
        let key = Uuid::new_v4().simple().to_string();
        let user = self
            .p
            .create_user(&key, &format!("{key}@example.test"), "hash")
            .await
            .unwrap();
        sqlx::query("INSERT INTO company_members(id,company_id,user_id,role,access_level) VALUES($1,$2,$3,'admin',$4::smallint)").bind(Uuid::new_v4()).bind(self.company).bind(user.id).bind(level).execute(self.p.pool()).await.unwrap();
        sqlx::query("INSERT INTO principals(id,company_id,kind,user_id,display_label) VALUES($1,$2,'person',$3,'admin')").bind(Uuid::new_v4()).bind(self.company).bind(user.id).execute(self.p.pool()).await.unwrap();
        user.id
    }
}
fn draft(revision: i64, level: i16) -> LevelDraft {
    LevelDraft {
        expected_revision: revision,
        access_level: level,
    }
}
#[tokio::test]
async fn knowledge_run_level_http_new_read_routes_hide_valid_foreign_company_and_resource_ids() {
    let f = Fixture::new().await;
    let before = f.snapshot().await;
    assert!(
        http_list(State(f.s.clone()), f.user(), Path(f.foreign))
            .await
            .is_err()
    );
    assert!(
        http_catalog(State(f.s.clone()), f.user(), Path(f.foreign))
            .await
            .is_err()
    );
    assert!(
        http_get(State(f.s.clone()), f.user(), f.path(f.foreign_http.id))
            .await
            .is_err()
    );
    assert!(
        http_detail(State(f.s.clone()), f.user(), f.path(f.foreign_http.id))
            .await
            .is_err()
    );
    assert_eq!(f.snapshot().await, before);
    assert_eq!(
        http_get(State(f.s.clone()), f.user(), f.path(f.http.id))
            .await
            .unwrap()
            .0
            .access_level,
        AccessLevel::PUBLIC
    );
    assert!(
        http_detail(State(f.s.clone()), f.user(), f.path(f.http.id))
            .await
            .unwrap()
            .0
            .contains("Save access level")
    );
}
#[tokio::test]
async fn knowledge_run_level_all_four_connection_level_write_routes_scope_valid_foreign_resources()
{
    let f = Fixture::new().await;
    let before = f.snapshot().await;
    assert!(
        http_set(
            State(f.s.clone()),
            f.user(),
            f.path(f.foreign_http.id),
            Json(draft(1, 5))
        )
        .await
        .is_err()
    );
    assert!(
        http_save(
            State(f.s.clone()),
            f.user(),
            f.path(f.foreign_http.id),
            Form(draft(1, 5))
        )
        .await
        .is_err()
    );
    assert!(
        mcp_set(
            State(f.s.clone()),
            f.user(),
            f.path(f.foreign_mcp.id),
            Json(draft(1, 5))
        )
        .await
        .is_err()
    );
    assert!(
        mcp_save(
            State(f.s.clone()),
            f.user(),
            f.path(f.foreign_mcp.id),
            Form(draft(1, 5))
        )
        .await
        .is_err()
    );
    assert_eq!(f.snapshot().await, before);
}
fn only_classification_changed(mut before: Value, mut after: Value) {
    for value in [&mut before, &mut after] {
        for table in ["http", "mcp"] {
            for row in value[table].as_array_mut().unwrap() {
                let object = row.as_object_mut().unwrap();
                object.remove("access_level");
                object.remove("revision");
                object.remove("updated_at");
            }
        }
    }
    assert_eq!(after, before);
}
async fn owner_raises_both_connections(f: &Fixture) {
    let html = http_save(
        State(f.s.clone()),
        f.user(),
        f.path(f.http.id),
        Form(draft(2, 8)),
    )
    .await
    .unwrap()
    .0;
    assert!(html.contains("Access level saved"));
    let html = mcp_save(
        State(f.s.clone()),
        f.user(),
        f.path(f.mcp.id),
        Form(draft(2, 8)),
    )
    .await
    .unwrap()
    .0;
    assert!(html.contains("Access level saved"));
}

#[tokio::test]
async fn knowledge_run_level_connection_commands_enforce_caps_and_preserve_other_settings() {
    let f = Fixture::new().await;
    let admin = f.admin(3).await;
    let before = f.snapshot().await;
    let http = http_set(
        State(f.s.clone()),
        AuthenticatedUser { id: admin },
        f.path(f.http.id),
        Json(draft(1, 2)),
    )
    .await
    .unwrap()
    .0;
    let mcp = mcp_set(
        State(f.s.clone()),
        AuthenticatedUser { id: admin },
        f.path(f.mcp.id),
        Json(draft(1, 2)),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(http.access_level, AccessLevel::L2);
    assert_eq!(mcp.access_level, AccessLevel::L2);
    assert_eq!(http.revision, 2);
    assert_eq!(mcp.revision, 2);
    only_classification_changed(before, f.snapshot().await);
    let before = f.snapshot().await;
    assert!(
        http_set(
            State(f.s.clone()),
            AuthenticatedUser { id: admin },
            f.path(f.http.id),
            Json(draft(2, 4))
        )
        .await
        .is_err()
    );
    assert!(
        mcp_set(
            State(f.s.clone()),
            AuthenticatedUser { id: admin },
            f.path(f.mcp.id),
            Json(draft(2, 4))
        )
        .await
        .is_err()
    );
    assert_eq!(f.snapshot().await, before);
    owner_raises_both_connections(&f).await;
    let before = f.snapshot().await;
    assert!(
        http_set(
            State(f.s.clone()),
            AuthenticatedUser { id: admin },
            f.path(f.http.id),
            Json(draft(3, 1))
        )
        .await
        .is_err()
    );
    assert!(
        mcp_set(
            State(f.s.clone()),
            AuthenticatedUser { id: admin },
            f.path(f.mcp.id),
            Json(draft(3, 1))
        )
        .await
        .is_err()
    );
    assert_eq!(f.snapshot().await, before);
    let pane = http_detail(
        State(f.s.clone()),
        AuthenticatedUser { id: admin },
        f.path(f.http.id),
    )
    .await
    .unwrap()
    .0;
    assert!(!pane.contains("Save access level"));
}
#[tokio::test]
async fn knowledge_run_level_race_connection_revision_writers_have_one_winner() {
    let f = Fixture::new().await;
    let command = draft(1, 2).command(f.owner, f.company, f.http.id).unwrap();
    let (first, second) = tokio::join!(
        f.p.set_http_level(command),
        f.p.set_http_level(ConnectionLevelCommand {
            access_level: AccessLevel::L3,
            ..command
        })
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    let command = draft(1, 2).command(f.owner, f.company, f.mcp.id).unwrap();
    let (first, second) = tokio::join!(
        f.p.set_mcp_level(command),
        f.p.set_mcp_level(ConnectionLevelCommand {
            access_level: AccessLevel::L3,
            ..command
        })
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
}
#[test]
fn knowledge_run_level_connection_payloads_are_strict_bounded_and_forms_keep_progress() {
    for value in [
        json!({"expected_revision":1,"access_level":5,"role":"owner"}),
        json!({"expected_revision":1,"access_level":5,"credential":"secret"}),
    ] {
        assert!(serde_json::from_value::<LevelDraft>(value).is_err());
    }
    let invalid =
        serde_json::from_value::<LevelDraft>(json!({"expected_revision":1,"access_level":10}))
            .unwrap();
    assert!(
        invalid
            .command(Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4())
            .is_err()
    );
    let actor = LevelActor {
        membership: crate::entities::company_member::CompanyMembership::Admin,
        access_level: AccessLevel::L3,
    };
    let html =
        pages::connection_levels::level_form("/safe", 1, AccessLevel::PUBLIC, actor, Some(2));
    assert!(html.contains("data-submit=\"busy-once\""));
    assert!(html.contains("value=\"2\" selected"));
    assert!(!html.contains("value=\"4\""));
    let failed =
        pages::connection_levels::level_form("/safe", 1, AccessLevel::PUBLIC, actor, Some(4));
    assert!(failed.contains("Requested access level: 4"));
    assert!(!failed.contains("<option value=\"4\""));
}

#[tokio::test]
async fn knowledge_run_level_race_live_admin_demotion_precedes_connection_level_write() {
    let f = Fixture::new().await;
    let admin = f.admin(5).await;
    let mut demotion = f.p.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(f.company)
        .execute(&mut *demotion)
        .await
        .unwrap();
    sqlx::query("UPDATE company_members SET access_level=1 WHERE company_id=$1 AND user_id=$2")
        .bind(f.company)
        .bind(admin)
        .execute(&mut *demotion)
        .await
        .unwrap();
    let command = draft(1, 3).command(admin, f.company, f.http.id).unwrap();
    let p = f.p.clone();
    let writer = tokio::spawn(async move { p.set_http_level(command).await });
    tokio::task::yield_now().await;
    demotion.commit().await.unwrap();
    assert!(matches!(
        writer.await.unwrap(),
        Err(AppError::BadRequest(_))
    ));
    let current =
        f.p.list_http(
            CompanyId::new(f.company),
            WorkflowActor::authenticated(f.owner).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(current[0].access_level, AccessLevel::PUBLIC);
    assert_eq!(current[0].revision, 1);
}

#[tokio::test]
async fn knowledge_run_level_connection_form_parses_integer_strings_and_rejects_injected_fields() {
    use axum::extract::FromRequest;
    for body in [
        "expected_revision=1&access_level=2",
        "expected_revision=1&access_level=2&role=owner",
    ] {
        let request = axum::http::Request::post("/test")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(axum::body::Body::from(body))
            .unwrap();
        let result = Form::<LevelDraft>::from_request(request, &()).await;
        assert_eq!(result.is_ok(), !body.contains("role="));
    }
}
