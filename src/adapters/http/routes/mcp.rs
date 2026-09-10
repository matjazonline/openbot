//! One company catalog and a separate IDs-only agent selection operation.
use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser},
    app_error::{AppError, AppResult},
    entities::{mcp::*, value_objects::McpConnectionSlug},
    use_cases::mcp::McpConnectionWrite,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::StatusCode,
    routing::{get, post, put},
};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/companies/{company}/mcp-connections",
            get(list).post(create),
        )
        .route(
            "/api/companies/{company}/mcp-connections/{id}",
            get(detail).put(update).delete(delete),
        )
        .route(
            "/api/companies/{company}/mcp-connections/{id}/credential",
            put(credential),
        )
        .route(
            "/api/companies/{company}/mcp-connections/{id}/refresh",
            post(refresh),
        )
        .route(
            "/api/companies/{company}/agents/{agent}/mcp-selection",
            get(selection).put(select),
        )
        .layer(DefaultBodyLimit::max(128 * 1024))
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub slug: McpConnectionSlug,
    pub endpoint_url: McpEndpoint,
    pub transport: Transport,
    pub enabled: bool,
    pub auth: Authentication,
    #[serde(default)]
    pub tool_grants: Vec<Grant>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    StreamableHttp,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authentication {
    pub r#type: McpAuth,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub name: McpToolName,
}
impl Definition {
    pub fn write(self, current: Option<&CompanyMcpConnection>) -> McpConnectionWrite {
        let tools = current
            .filter(|c| c.endpoint == self.endpoint_url)
            .map(|c| c.discovered_tools.clone())
            .unwrap_or_default();
        McpConnectionWrite {
            slug: self.slug,
            endpoint: self.endpoint_url,
            enabled: self.enabled,
            auth: self.auth.r#type,
            discovered_tools: tools,
            tool_grants: self.tool_grants.into_iter().map(|g| g.name).collect(),
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    pub expected_revision: i64,
    pub definition: Definition,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revision {
    pub expected_revision: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credential {
    pub expected_revision: i64,
    pub token: Option<SecretString>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub expected_revision: i64,
    pub mcp_connection_ids: Option<Vec<Uuid>>,
}

pub fn revision(current: &CompanyMcpConnection, expected: i64) -> AppResult<()> {
    if current.revision != expected {
        return Err(AppError::Conflict(
            "MCP configuration changed; reload before saving".into(),
        ));
    }
    Ok(())
}
pub fn projection(c: &CompanyMcpConnection) -> serde_json::Value {
    serde_json::json!({"id":c.id,"slug":c.slug,"endpoint_url":c.endpoint,"transport":"streamable_http","enabled":c.enabled,
        "auth":{"type":c.auth,"secret_set":c.secret_set},"revision":c.revision,
        "tool_grants":c.tool_grants.iter().map(|name| Grant{name:name.clone()}).collect::<Vec<_>>(),
        "discovered_tools":c.discovered_tools.iter().map(|tool| serde_json::json!({"name":tool.name,"description":tool.description,"input_schema":tool.input_schema,"schema_fingerprint":crate::services::mcp_runtime::fingerprint(&tool.input_schema)})).collect::<Vec<_>>()})
}
async fn list(
    State(s): State<std::sync::Arc<crate::use_cases::mcp::McpUseCases>>,
    user: AuthenticatedUser,
    Path(company): Path<Uuid>,
) -> AppResult<Json<serde_json::Value>> {
    let connections = s.list(user.id, company).await?;
    Ok(Json(
        serde_json::json!({"mcp_connections":connections.iter().map(projection).collect::<Vec<_>>()}),
    ))
}
async fn detail(
    State(s): State<std::sync::Arc<crate::use_cases::mcp::McpUseCases>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<serde_json::Value>> {
    let current = s.get(user.id, company, id).await?;
    Ok(Json(
        serde_json::json!({"connection":projection(&current),"selected_by":s.selecting_agents(user.id,company,id).await?}),
    ))
}
async fn create(
    State(s): State<std::sync::Arc<crate::use_cases::mcp::McpUseCases>>,
    user: AuthenticatedUser,
    Path(company): Path<Uuid>,
    Json(body): Json<Definition>,
) -> AppResult<(StatusCode, Json<serde_json::Value>)> {
    Ok((
        StatusCode::CREATED,
        Json(projection(
            &s.create(user.id, company, body.write(None)).await?,
        )),
    ))
}
async fn update(
    State(s): State<std::sync::Arc<crate::use_cases::mcp::McpUseCases>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, Uuid)>,
    Json(body): Json<Update>,
) -> AppResult<Json<serde_json::Value>> {
    let current = s.get(user.id, company, id).await?;
    revision(&current, body.expected_revision)?;
    Ok(Json(projection(
        &s.update(user.id, &current, body.definition.write(Some(&current)))
            .await?,
    )))
}
async fn delete(
    State(s): State<std::sync::Arc<crate::use_cases::mcp::McpUseCases>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, Uuid)>,
    Json(body): Json<Revision>,
) -> AppResult<StatusCode> {
    let current = s.get(user.id, company, id).await?;
    revision(&current, body.expected_revision)?;
    s.delete(user.id, &current).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn credential(
    State(s): State<std::sync::Arc<crate::use_cases::mcp::McpUseCases>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, Uuid)>,
    Json(body): Json<Credential>,
) -> AppResult<Json<serde_json::Value>> {
    let current = s.get(user.id, company, id).await?;
    revision(&current, body.expected_revision)?;
    let revision = s.replace_token(user.id, &current, body.token).await?;
    Ok(Json(serde_json::json!({"revision":revision})))
}
async fn refresh(
    State(s): State<std::sync::Arc<crate::use_cases::mcp::McpUseCases>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, Uuid)>,
    Json(body): Json<Revision>,
) -> AppResult<Json<serde_json::Value>> {
    let current = s.get(user.id, company, id).await?;
    revision(&current, body.expected_revision)?;
    Ok(Json(projection(&s.refresh(user.id, &current).await?)))
}
async fn selection(
    State(s): State<std::sync::Arc<crate::use_cases::mcp::McpUseCases>>,
    user: AuthenticatedUser,
    Path((company, agent)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<AgentMcpSelection>> {
    Ok(Json(s.selection(user.id, company, agent).await?))
}
async fn select(
    State(s): State<std::sync::Arc<crate::use_cases::mcp::McpUseCases>>,
    user: AuthenticatedUser,
    Path((company, agent)): Path<(Uuid, Uuid)>,
    Json(body): Json<Selection>,
) -> AppResult<Json<AgentMcpSelection>> {
    let mut current = s.selection(user.id, company, agent).await?;
    current.revision = body.expected_revision;
    Ok(Json(
        s.select(user.id, current, body.mcp_connection_ids).await?,
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_writes_cannot_supply_discovery_or_inline_selection_authority() {
        let mut definition = serde_json::json!({"slug":"crm","endpoint_url":"https://example.com/mcp","transport":"streamable_http","enabled":true,"auth":{"type":"none"}});
        assert!(serde_json::from_value::<Definition>(definition.clone()).is_ok());
        definition["discovered_tools"] = serde_json::json!([]);
        assert!(serde_json::from_value::<Definition>(definition).is_err());
        for field in ["endpoint_url", "token", "tool_grants", "requires_approval"] {
            let mut selection = serde_json::json!({"expected_revision":1,"mcp_connection_ids":[]});
            selection[field] = serde_json::json!("forbidden");
            assert!(serde_json::from_value::<Selection>(selection).is_err());
        }
        assert!(
            serde_json::from_value::<Selection>(serde_json::json!({"expected_revision":1}))
                .unwrap()
                .mcp_connection_ids
                .is_none()
        );
        assert_eq!(
            serde_json::from_value::<Selection>(
                serde_json::json!({"expected_revision":1,"mcp_connection_ids":[]})
            )
            .unwrap()
            .mcp_connection_ids,
            Some(vec![])
        );
    }
}

#[cfg(test)]
#[path = "mcp_api_tests.rs"]
mod api_tests;
