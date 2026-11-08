//! Level-only endpoints keep trust classification separate from credentials and catalog edits.
use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser, pages},
    app_error::{AppError, AppResult},
    application::workflow::connection_level::ConnectionLevelCommand,
    application::workflow::{WorkflowActor, http::HttpConnection},
    domain::workflow::CompanyId,
    entities::access_level::{AccessLevel, LevelActor},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    response::Html,
    routing::{get, post, put},
};
use axum_extra::extract::Form;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Clone)]
struct ConnectionServices {
    workflow: std::sync::Arc<crate::infra::workflow::WorkflowServices>,
    company_use_cases: std::sync::Arc<crate::use_cases::company::CompanyUseCases>,
    mcp_use_cases: std::sync::Arc<crate::use_cases::mcp::McpUseCases>,
}
impl axum::extract::FromRef<AppState> for ConnectionServices {
    fn from_ref(state: &AppState) -> Self {
        Self {
            workflow: state.workflow.clone(),
            company_use_cases: state.company_use_cases.clone(),
            mcp_use_cases: state.mcp_use_cases.clone(),
        }
    }
}

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/companies/{company}/http-connections",
            get(http_list)
                .post(authoring::create)
                .layer(DefaultBodyLimit::max(65_536)),
        )
        .route(
            "/api/companies/{company}/http-connections/{id}/operations",
            post(authoring::register).layer(DefaultBodyLimit::max(65_536)),
        )
        .route(
            "/api/companies/{company}/http-connections/{id}",
            get(http_get),
        )
        .route(
            "/api/companies/{company}/http-connections/{id}/access-level",
            put(http_set),
        )
        .route(
            "/api/companies/{company}/mcp-connections/{id}/access-level",
            put(mcp_set),
        )
        .route(
            "/ui/companies/{company}/http-connections",
            get(http_catalog),
        )
        .route(
            "/ui/companies/{company}/http-connections/{id}",
            get(http_detail),
        )
        .route(
            "/ui/companies/{company}/http-connections/{id}/access-level",
            post(http_save),
        )
        .route(
            "/ui/companies/{company}/mcp-connections/{id}/access-level",
            post(mcp_save),
        )
        .layer(DefaultBodyLimit::max(4096))
}
mod authoring;
#[derive(Deserialize)]
struct ConnectionPath {
    company: Uuid,
    id: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LevelDraft {
    expected_revision: i64,
    access_level: i16,
}
impl LevelDraft {
    fn command(&self, user: Uuid, company: Uuid, id: Uuid) -> AppResult<ConnectionLevelCommand> {
        Ok(ConnectionLevelCommand {
            company: CompanyId::new(company),
            actor: WorkflowActor::authenticated(user)?,
            connection: id,
            expected_revision: self.expected_revision,
            access_level: AccessLevel::try_from(self.access_level)
                .map_err(|e| AppError::BadRequest(e.into()))?,
        })
    }
}
fn missing() -> AppError {
    AppError::NotFound("Company connection".into())
}
fn feedback(result: AppResult<()>) -> String {
    match result {
        Ok(()) => "Access level saved.".into(),
        Err(AppError::BadRequest(message) | AppError::Conflict(message)) => message,
        Err(_) => "Connection could not be updated. Reload and try again.".into(),
    }
}
async fn http_list(
    State(s): State<ConnectionServices>,
    user: AuthenticatedUser,
    Path(company): Path<Uuid>,
) -> AppResult<Json<Vec<HttpConnection>>> {
    Ok(Json(
        s.workflow
            .http_connections()
            .list_http(
                CompanyId::new(company),
                WorkflowActor::authenticated(user.id)?,
            )
            .await?,
    ))
}
async fn http_get(
    State(s): State<ConnectionServices>,
    user: AuthenticatedUser,
    Path(ConnectionPath { company, id }): Path<ConnectionPath>,
) -> AppResult<Json<HttpConnection>> {
    let rows = s
        .workflow
        .http_connections()
        .list_http(
            CompanyId::new(company),
            WorkflowActor::authenticated(user.id)?,
        )
        .await?;
    Ok(Json(
        rows.into_iter().find(|c| c.id == id).ok_or_else(missing)?,
    ))
}
async fn http_set(
    State(s): State<ConnectionServices>,
    user: AuthenticatedUser,
    Path(ConnectionPath { company, id }): Path<ConnectionPath>,
    Json(draft): Json<LevelDraft>,
) -> AppResult<Json<HttpConnection>> {
    Ok(Json(
        s.workflow
            .http_connections()
            .set_http_level(draft.command(user.id, company, id)?)
            .await?,
    ))
}
async fn mcp_set(
    State(s): State<ConnectionServices>,
    user: AuthenticatedUser,
    Path(ConnectionPath { company, id }): Path<ConnectionPath>,
    Json(draft): Json<LevelDraft>,
) -> AppResult<Json<crate::entities::mcp::CompanyMcpConnection>> {
    Ok(Json(
        s.mcp_use_cases
            .set_level(draft.command(user.id, company, id)?)
            .await?,
    ))
}
async fn http_catalog(
    State(s): State<ConnectionServices>,
    user: AuthenticatedUser,
    Path(company): Path<Uuid>,
) -> AppResult<Html<String>> {
    let rows = s
        .workflow
        .http_connections()
        .list_http(
            CompanyId::new(company),
            WorkflowActor::authenticated(user.id)?,
        )
        .await?;
    Ok(Html(pages::mcp::shell(
        "HTTP connections",
        company,
        &pages::connection_levels::http_catalog(company, &rows),
        None,
    )))
}
async fn http_detail(
    State(s): State<ConnectionServices>,
    user: AuthenticatedUser,
    Path(ConnectionPath { company, id }): Path<ConnectionPath>,
) -> AppResult<Html<String>> {
    let access = s
        .company_use_cases
        .managed_company(user.id, company)
        .await?;
    let actor = LevelActor {
        membership: access.membership,
        access_level: access.access_level,
    };
    let rows = s
        .workflow
        .http_connections()
        .list_http(
            CompanyId::new(company),
            WorkflowActor::authenticated(user.id)?,
        )
        .await?;
    let current = rows.into_iter().find(|c| c.id == id).ok_or_else(missing)?;
    Ok(Html(pages::mcp::shell(
        "HTTP connection",
        company,
        &pages::connection_levels::http_detail(&current, actor, None),
        None,
    )))
}
async fn http_save(
    State(s): State<ConnectionServices>,
    user: AuthenticatedUser,
    Path(ConnectionPath { company, id }): Path<ConnectionPath>,
    Form(draft): Form<LevelDraft>,
) -> AppResult<Html<String>> {
    // Scope the real resource before even rendering a malformed/failed submission.
    let rows = s
        .workflow
        .http_connections()
        .list_http(
            CompanyId::new(company),
            WorkflowActor::authenticated(user.id)?,
        )
        .await?;
    if !rows.iter().any(|c| c.id == id) {
        return Err(missing());
    }
    let result = async {
        s.workflow
            .http_connections()
            .set_http_level(draft.command(user.id, company, id)?)
            .await
            .map(|_| ())
    }
    .await;
    let message = feedback(result);
    let access = s
        .company_use_cases
        .managed_company(user.id, company)
        .await?;
    let actor = LevelActor {
        membership: access.membership,
        access_level: access.access_level,
    };
    let rows = s
        .workflow
        .http_connections()
        .list_http(
            CompanyId::new(company),
            WorkflowActor::authenticated(user.id)?,
        )
        .await?;
    let current = rows.into_iter().find(|c| c.id == id).ok_or_else(missing)?;
    Ok(Html(pages::mcp::shell(
        "HTTP connection",
        company,
        &pages::connection_levels::http_detail(&current, actor, Some(draft.access_level)),
        Some(&message),
    )))
}
async fn mcp_save(
    State(s): State<ConnectionServices>,
    user: AuthenticatedUser,
    Path(ConnectionPath { company, id }): Path<ConnectionPath>,
    Form(draft): Form<LevelDraft>,
) -> AppResult<Html<String>> {
    s.mcp_use_cases.get(user.id, company, id).await?;
    let result = async {
        s.mcp_use_cases
            .set_level(draft.command(user.id, company, id)?)
            .await
            .map(|_| ())
    }
    .await;
    let message = feedback(result);
    let current = s.mcp_use_cases.get(user.id, company, id).await?;
    let actor = s.mcp_use_cases.level_actor(user.id, company).await?;
    let form = pages::connection_levels::level_form(
        &format!("/ui/companies/{company}/mcp-connections/{id}/access-level"),
        current.revision,
        current.access_level,
        actor,
        Some(draft.access_level),
    );
    let body = format!(
        "{}{}",
        pages::mcp::detail(
            company,
            &current,
            &pages::mcp::DefinitionDraft::stored(&current)
        ),
        form
    );
    Ok(Html(pages::mcp::shell(
        "MCP server",
        company,
        &body,
        Some(&message),
    )))
}

#[cfg(test)]
#[path = "connection_level_tests.rs"]
mod tests;
