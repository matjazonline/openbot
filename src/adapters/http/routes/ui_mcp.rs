use super::mcp::{Credential, Revision, revision};
use crate::{
    adapters::http::{
        app_state::AppState,
        auth::AuthenticatedUser,
        pages::mcp::{self, DefinitionDraft},
    },
    app_error::{AppError, AppResult},
    entities::mcp::*,
    use_cases::mcp::McpConnectionWrite,
};
use axum::{
    Router,
    extract::{DefaultBodyLimit, Path, State},
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
};
use axum_extra::extract::Form;
use serde::Deserialize;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/ui/companies/{company}/mcp-connections",
            get(catalog).post(create),
        )
        .route(
            "/ui/companies/{company}/mcp-connections/{id}",
            get(detail).post(update),
        )
        .route(
            "/ui/companies/{company}/mcp-connections/{id}/credential",
            axum::routing::post(credential),
        )
        .route(
            "/ui/companies/{company}/mcp-connections/{id}/refresh",
            axum::routing::post(refresh),
        )
        .route(
            "/ui/companies/{company}/mcp-connections/{id}/delete",
            axum::routing::post(delete),
        )
        .route(
            "/ui/companies/{company}/agents/{agent}/mcp-selection",
            get(selection).post(select),
        )
        .layer(DefaultBodyLimit::max(128 * 1024))
}
fn write(
    draft: &DefinitionDraft,
    current: Option<&CompanyMcpConnection>,
) -> AppResult<McpConnectionWrite> {
    let endpoint: McpEndpoint = draft
        .endpoint_url
        .clone()
        .try_into()
        .map_err(AppError::BadRequest)?;
    let auth = match draft.auth_type.as_str() {
        "none" => McpAuth::None,
        "bearer" => McpAuth::Bearer,
        _ => {
            return Err(AppError::BadRequest(
                "Unsupported MCP authentication".into(),
            ));
        }
    };
    Ok(McpConnectionWrite {
        slug: draft.slug.clone().into(),
        endpoint: endpoint.clone(),
        enabled: draft.enabled.is_some(),
        auth,
        discovered_tools: current
            .filter(|c| c.endpoint == endpoint)
            .map(|c| c.discovered_tools.clone())
            .unwrap_or_default(),
        tool_grants: draft
            .grants
            .iter()
            .cloned()
            .map(McpToolName::try_from)
            .collect::<Result<_, _>>()
            .map_err(AppError::BadRequest)?,
    })
}
fn feedback(error: &AppError) -> String {
    match error {
        AppError::BadRequest(message) | AppError::Conflict(message) => message.clone(),
        _ => "MCP operation failed. Reload and try again.".into(),
    }
}
async fn catalog(
    State(s): State<std::sync::Arc<crate::use_cases::mcp::McpUseCases>>,
    user: AuthenticatedUser,
    Path(company): Path<Uuid>,
) -> AppResult<Html<String>> {
    let connections = s.list(user.id, company).await?;
    Ok(Html(mcp::shell(
        "MCP servers",
        company,
        &mcp::catalog(company, &connections, &DefinitionDraft::default()),
        None,
    )))
}
async fn detail_page(
    s: &crate::use_cases::mcp::McpUseCases,
    user: Uuid,
    company: Uuid,
    id: Uuid,
    draft: Option<&DefinitionDraft>,
    message: Option<&str>,
) -> AppResult<Html<String>> {
    let current = s.get(user, company, id).await?;
    let usage = s.selecting_agents(user, company, id).await?;
    Ok(Html(mcp::shell(
        "MCP server",
        company,
        &mcp::detail(
            company,
            &current,
            &usage,
            draft.unwrap_or(&DefinitionDraft::stored(&current)),
        ),
        message,
    )))
}
async fn detail(
    State(s): State<std::sync::Arc<crate::use_cases::mcp::McpUseCases>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, Uuid)>,
) -> AppResult<Html<String>> {
    detail_page(&s, user.id, company, id, None, None).await
}
async fn create(
    State(s): State<std::sync::Arc<crate::use_cases::mcp::McpUseCases>>,
    user: AuthenticatedUser,
    Path(company): Path<Uuid>,
    Form(draft): Form<DefinitionDraft>,
) -> AppResult<Response> {
    // Authorize even a malformed draft before rendering any company data.
    let connections = s.list(user.id, company).await?;
    let result = async { s.create(user.id, company, write(&draft, None)?).await }.await;
    match result {
        Ok(c) => Ok(
            Redirect::to(&format!("/ui/companies/{company}/mcp-connections/{}", c.id))
                .into_response(),
        ),
        Err(e) => Ok(Html(mcp::shell(
            "MCP servers",
            company,
            &mcp::catalog(company, &connections, &draft),
            Some(&feedback(&e)),
        ))
        .into_response()),
    }
}
async fn update(
    State(s): State<std::sync::Arc<crate::use_cases::mcp::McpUseCases>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, Uuid)>,
    Form(draft): Form<DefinitionDraft>,
) -> AppResult<Html<String>> {
    let current = s.get(user.id, company, id).await?;
    let result = async {
        revision(&current, draft.expected_revision)?;
        s.update(user.id, &current, write(&draft, Some(&current))?)
            .await
    }
    .await;
    match result {
        Ok(_) => {
            detail_page(
                &s,
                user.id,
                company,
                id,
                None,
                Some("Definition saved. Use Test connection to verify the server."),
            )
            .await
        }
        Err(e) => detail_page(&s, user.id, company, id, Some(&draft), Some(&feedback(&e))).await,
    }
}
async fn credential(
    State(s): State<std::sync::Arc<crate::use_cases::mcp::McpUseCases>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, Uuid)>,
    Form(body): Form<Credential>,
) -> AppResult<Html<String>> {
    use secrecy::ExposeSecret;
    let current = s.get(user.id, company, id).await?;
    let result = async {
        revision(&current, body.expected_revision)?;
        s.replace_token(
            user.id,
            &current,
            body.token.filter(|s| !s.expose_secret().is_empty()),
        )
        .await
    }
    .await;
    let message = result
        .map(|_| "Credential updated".into())
        .unwrap_or_else(|e| feedback(&e));
    detail_page(&s, user.id, company, id, None, Some(&message)).await
}
async fn refresh(
    State(s): State<std::sync::Arc<crate::use_cases::mcp::McpUseCases>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, Uuid)>,
    Form(body): Form<Revision>,
) -> AppResult<Html<String>> {
    let current = s.get(user.id, company, id).await?;
    let result = async {
        revision(&current, body.expected_revision)?;
        s.refresh(user.id, &current).await
    }
    .await;
    let message = result
        .map(|_| "Connection tested; tools refreshed. Review grants before use.".into())
        .unwrap_or_else(|e| feedback(&e));
    detail_page(&s, user.id, company, id, None, Some(&message)).await
}
async fn delete(
    State(s): State<std::sync::Arc<crate::use_cases::mcp::McpUseCases>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, Uuid)>,
    Form(body): Form<Revision>,
) -> AppResult<Response> {
    let current = s.get(user.id, company, id).await?;
    let result = async {
        revision(&current, body.expected_revision)?;
        s.delete(user.id, &current).await
    }
    .await;
    match result {
        Ok(()) => {
            Ok(Redirect::to(&format!("/ui/companies/{company}/mcp-connections")).into_response())
        }
        Err(e) => Ok(
            detail_page(&s, user.id, company, id, None, Some(&feedback(&e)))
                .await?
                .into_response(),
        ),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectionForm {
    expected_revision: i64,
    #[serde(default)]
    mcp_connection_ids: Vec<Uuid>,
}
async fn selection(
    State(s): State<std::sync::Arc<crate::use_cases::mcp::McpUseCases>>,
    user: AuthenticatedUser,
    Path((company, agent)): Path<(Uuid, Uuid)>,
) -> AppResult<Html<String>> {
    let selection = s.selection(user.id, company, agent).await?;
    let catalog = s.list(user.id, company).await?;
    Ok(Html(mcp::shell(
        "Agent MCP servers",
        company,
        &mcp::selection(company, &selection, &catalog, None),
        None,
    )))
}
async fn select(
    State(s): State<std::sync::Arc<crate::use_cases::mcp::McpUseCases>>,
    user: AuthenticatedUser,
    Path((company, agent)): Path<(Uuid, Uuid)>,
    Form(body): Form<SelectionForm>,
) -> AppResult<Html<String>> {
    let mut current = s.selection(user.id, company, agent).await?;
    let catalog = s.list(user.id, company).await?;
    current.revision = body.expected_revision;
    let result = s
        .select(
            user.id,
            current.clone(),
            Some(body.mcp_connection_ids.clone()),
        )
        .await;
    let (selection, message) = match result {
        Ok(v) => (v, "Selections saved".into()),
        Err(e) => (current, feedback(&e)),
    };
    Ok(Html(mcp::shell(
        "Agent MCP servers",
        company,
        &mcp::selection(
            company,
            &selection,
            &catalog,
            Some(&body.mcp_connection_ids),
        ),
        Some(&message),
    )))
}
