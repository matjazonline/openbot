use super::*;
use crate::adapters::persistence::{
    PostgresPersistence,
    credentials::CredentialCipher,
    test_support::{OwnDatabase, own_database},
};
use crate::application::{
    services::mcp_client::{McpCallResult, McpClient, McpSession},
    session::{SessionId, VerifiedSession},
};
use crate::entities::mcp::{McpDiscoveredTool, McpEndpoint, McpToolName};
use crate::infra::workflow::WorkflowEngine;
use crate::use_cases::{
    company::{CompanyPersistence, CompanyWrite},
    user::UserPersistence,
};
use async_trait::async_trait;
use axum::{body::Body, http::Request};
use serde_json::{Value, json};
use std::sync::Mutex;
use tower::ServiceExt;

#[derive(Clone)]
pub(super) struct State {
    pub(super) mcp: Arc<crate::use_cases::mcp::McpUseCases>,
    pub(super) engine: Arc<WorkflowServices>,
    pub(super) sessions: Arc<crate::adapters::http::session::SessionAuthority>,
}
impl FromRef<State> for Arc<crate::adapters::http::session::SessionAuthority> {
    fn from_ref(state: &State) -> Self {
        state.sessions.clone()
    }
}
impl FromRef<State> for Arc<crate::use_cases::mcp::McpUseCases> {
    fn from_ref(state: &State) -> Self {
        state.mcp.clone()
    }
}
impl FromRef<State> for Arc<WorkflowServices> {
    fn from_ref(state: &State) -> Self {
        state.engine.clone()
    }
}

/// Discovery-only stand-in. HTTP transport/endpoint checks have separate real-server tests.
pub(super) struct Discovery {
    pub(super) tools: Arc<Mutex<Vec<McpDiscoveredTool>>>,
}
#[async_trait]
impl McpClient for Discovery {
    async fn connect_prepared(
        &self,
        _: &McpEndpoint,
        _: SecretString,
        _: crate::services::mcp_client::McpPreparation,
    ) -> crate::services::mcp_client::McpPreparationResult<Box<dyn McpSession>> {
        // This legacy-connection fixture does not implement integration discovery.
        Err(crate::services::mcp_client::McpPreparationFailure::Unavailable)
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
        Ok(Box::new(Discovery {
            tools: self.tools.clone(),
        }))
    }
}
#[async_trait]
impl McpSession for Discovery {
    async fn discover(&self) -> AppResult<Vec<McpDiscoveredTool>> {
        Ok(self.tools.lock().unwrap().clone())
    }
    async fn call(&self, _: &McpToolName, _: Value) -> AppResult<McpCallResult> {
        Err(AppError::Internal(
            "Operation review must not call an external tool".into(),
        ))
    }
    async fn close(self: Box<Self>) -> AppResult<()> {
        Ok(())
    }
}

pub(super) struct Fixture {
    _db: OwnDatabase,
    pub(super) persistence: PostgresPersistence,
    pub(super) state: State,
    pub(super) tools: Arc<Mutex<Vec<McpDiscoveredTool>>>,
    pub(super) user: Uuid,
    pub(super) company: Uuid,
    pub(super) other: Uuid,
    pub(super) left: CompanyMcpConnection,
    pub(super) right: CompanyMcpConnection,
}

impl Fixture {
    pub(super) async fn new(names: &[&str]) -> Self {
        let db = own_database().await.expect("owned PostgreSQL required");
        let persistence = PostgresPersistence::with_credential_cipher(
            db.pool.clone(),
            CredentialCipher::for_test(),
        );
        let suffix = Uuid::new_v4().simple().to_string();
        let user = persistence
            .create_user(&suffix, &format!("{suffix}@example.test"), "hash")
            .await
            .unwrap()
            .id;
        let mut companies = Vec::new();
        for index in 0..2 {
            companies.push(
                CompanyPersistence::create(
                    &persistence,
                    user,
                    CompanyWrite {
                        name: "Operation review".into(),
                        slug: format!("{suffix}-{index}"),
                        ..Default::default()
                    },
                )
                .await
                .unwrap()
                .id,
            );
        }
        let tools = Arc::new(Mutex::new(catalog(names)));
        let mcp = Arc::new(crate::use_cases::mcp::McpUseCases::new(
            Arc::new(persistence.clone()),
            Arc::new(persistence.clone()),
            Arc::new(persistence.clone()),
            Arc::new(Discovery {
                tools: tools.clone(),
            }),
            Arc::new(crate::use_cases::mcp::UnavailableMcpDiscovery),
        ));
        let discovered = tools.lock().unwrap().clone();
        let left = connection(&mcp, user, companies[0], &discovered, "left-key").await;
        let right = connection(&mcp, user, companies[1], &discovered, "right-key").await;
        let engine = WorkflowEngine::for_tests(persistence.clone()).services();
        Self {
            _db: db,
            persistence: persistence.clone(),
            state: State {
                mcp,
                engine,
                sessions: Arc::new(crate::adapters::http::session::SessionAuthority::new(
                    &crate::infra::config::AppConfig::for_test(),
                    Arc::new(persistence.clone()),
                )),
            },
            tools,
            user,
            company: companies[0],
            other: companies[1],
            left,
            right,
        }
    }

    pub(super) fn app(&self) -> Router {
        let mut config = crate::infra::config::AppConfig::for_test();
        config.secure_cookies = false;
        super::super::super::router()
            .with_state(self.state.clone())
            .layer(axum::middleware::from_fn_with_state(
                Arc::new(config),
                crate::adapters::http::security::browser_security,
            ))
    }

    pub(super) fn request(
        &self,
        method: &str,
        company: Uuid,
        connection: Uuid,
        suffix: &str,
        body: Value,
    ) -> Request<Body> {
        let mut request = Request::builder()
            .method(method)
            .uri(format!(
                "/api/companies/{company}/mcp-connections/{connection}/operations{suffix}"
            ))
            .header("host", "app.test")
            .header("origin", "http://app.test")
            .header("cookie", "fixture=present")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        request.extensions_mut().insert(VerifiedSession {
            user_id: self.user,
            session_id: SessionId(Uuid::new_v4()),
        });
        request
    }

    pub(super) fn connection_request(
        &self,
        method: &str,
        company: Uuid,
        connection: Uuid,
        suffix: &str,
        body: Value,
    ) -> Request<Body> {
        let mut request = self.request(method, company, connection, "", body);
        *request.uri_mut() =
            format!("/api/companies/{company}/mcp-connections/{connection}{suffix}")
                .parse()
                .unwrap();
        request
    }

    pub(super) async fn send(&self, request: Request<Body>) -> Response {
        let response = self.app().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        Response {
            status,
            body: serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| json!({"error":String::from_utf8_lossy(&bytes)})),
        }
    }
}

pub(super) struct Response {
    pub(super) status: StatusCode,
    pub(super) body: Value,
}

pub(super) fn policy(revision: i64, version: Option<Uuid>) -> Value {
    json!({"expected_revision":revision,"expected_operation_version":version,"effect":"write","approval_required":true})
}

pub(super) async fn connection(
    service: &crate::use_cases::mcp::McpUseCases,
    user: Uuid,
    company: Uuid,
    tools: &[McpDiscoveredTool],
    token: &str,
) -> CompanyMcpConnection {
    let current = service
        .create(
            user,
            company,
            crate::use_cases::mcp::McpConnectionWrite {
                slug: "doop".into(),
                endpoint: "https://doop.fixture.test/mcp"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                enabled: true,
                auth: McpAuth::Bearer,
                discovered_tools: tools.to_vec(),
                tool_grants: tools.iter().map(|tool| tool.name.clone()).collect(),
            },
        )
        .await
        .unwrap();
    service
        .replace_token(user, &current, Some(SecretString::from(token)))
        .await
        .unwrap();
    service.get(user, company, current.id).await.unwrap()
}

fn catalog(names: &[&str]) -> Vec<McpDiscoveredTool> {
    let captured: Value =
        serde_json::from_str(include_str!("../../../mcp/fixtures/doop/tools.json")).unwrap();
    names
        .iter()
        .map(|name| {
            let tool = captured["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["name"] == *name)
                .unwrap();
            McpDiscoveredTool {
                name: name.to_string().try_into().unwrap(),
                description: tool["description"].as_str().unwrap_or("").into(),
                input_schema: tool["inputSchema"].clone(),
                output_schema: tool.get("outputSchema").cloned(),
            }
        })
        .collect()
}
