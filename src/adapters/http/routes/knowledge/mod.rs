//! One scoped library service for authenticated browser and JSON entrypoints.
mod upload;
use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser, pages},
    app_error::{AppError, AppResult},
    application::knowledge::*,
    domain::workflow::CompanyId,
    entities::knowledge::*,
    infra::config::AppConfig,
    use_cases::{company::CompanyUseCases, user::UserUseCases},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, FromRef, Path, Query, Request, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
struct Services {
    library: Arc<KnowledgeLibrary>,
    companies: Arc<CompanyUseCases>,
    users: Arc<UserUseCases>,
    config: Arc<AppConfig>,
}
impl FromRef<AppState> for Services {
    fn from_ref(state: &AppState) -> Self {
        Self {
            library: state.knowledge.clone(),
            companies: state.company_use_cases.clone(),
            users: state.user_use_cases.clone(),
            config: state.config.clone(),
        }
    }
}
pub(super) fn router() -> Router<AppState> {
    routes()
}
fn routes<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    Services: FromRef<S>,
{
    Router::new()
        .route(
            "/api/companies/{company}/knowledge/documents",
            get(api_list).post(api_upload),
        )
        .route(
            "/api/companies/{company}/knowledge/documents/{id}",
            axum::routing::delete(api_delete).layer(DefaultBodyLimit::max(4096)),
        )
        .route("/ui/knowledge", get(ui_list))
        .route("/ui/knowledge/upload", post(ui_upload))
        .route(
            "/ui/knowledge/{id}/delete",
            post(ui_delete).layer(DefaultBodyLimit::max(4096)),
        )
        .layer(DefaultBodyLimit::max(upload::BODY_LIMIT))
}
fn scope(user: &AuthenticatedUser, company: Uuid) -> KnowledgeScope {
    KnowledgeScope {
        company: CompanyId::new(company),
        actor_id: user.id,
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompanyQuery {
    company_id: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LayerPath {
    company: Uuid,
    id: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteDraft {
    expected_revision: i64,
}
impl DeleteDraft {
    fn command(self, scope: KnowledgeScope, id: Uuid) -> AppResult<DeleteKnowledgeLayer> {
        if self.expected_revision < 1 {
            return Err(AppError::BadRequest(
                "The file revision must be positive".into(),
            ));
        }
        Ok(DeleteKnowledgeLayer {
            scope,
            document_id: id,
            expected_revision: self.expected_revision,
        })
    }
}
async fn api_list(
    State(s): State<Services>,
    user: AuthenticatedUser,
    Path(company): Path<Uuid>,
) -> AppResult<Json<Vec<KnowledgeDocumentLayer>>> {
    Ok(Json(
        s.library
            .documents()
            .list(scope(&user, company))
            .await?
            .layers,
    ))
}
async fn api_upload(
    State(s): State<Services>,
    user: AuthenticatedUser,
    Path(company): Path<Uuid>,
    request: Request,
) -> Response {
    match upload::receive(&s, scope(&user, company), request).await {
        Ok(layers) => (StatusCode::CREATED, Json(layers)).into_response(),
        Err(response) => response,
    }
}
async fn api_delete(
    State(s): State<Services>,
    user: AuthenticatedUser,
    Path(path): Path<LayerPath>,
    Json(draft): Json<DeleteDraft>,
) -> Response {
    let result = async {
        s.library
            .documents()
            .delete(draft.command(scope(&user, path.company), path.id)?)
            .await?;
        Ok::<_, AppError>(StatusCode::NO_CONTENT)
    }
    .await;
    match result {
        Ok(status) => status.into_response(),
        Err(error) => mutation_error(error),
    }
}
// Central HTTP errors intentionally hide service messages. A human write must still require a
// refresh when its acknowledgment can be unknown; apply this conservatively to unavailable writes.
fn mutation_error(error: AppError) -> Response {
    match error {
        AppError::ServiceUnavailable(_) | AppError::Timeout(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "The library change could not be confirmed. Refresh the library before trying again.",
        )
            .into_response(),
        other => other.into_response(),
    }
}

async fn ui_list(
    State(s): State<Services>,
    user: AuthenticatedUser,
    Query(query): Query<CompanyQuery>,
) -> AppResult<Response> {
    render(&s, &user, query.company_id, None).await
}
async fn ui_upload(
    State(s): State<Services>,
    user: AuthenticatedUser,
    Query(query): Query<CompanyQuery>,
    request: Request,
) -> AppResult<Response> {
    match upload::receive(&s, scope(&user, query.company_id), request).await {
        Ok(_) => Ok(
            Redirect::to(&format!("/ui/knowledge?company_id={}", query.company_id)).into_response(),
        ),
        Err(response) => render_failure(&s, &user, query.company_id, response).await,
    }
}
async fn ui_delete(
    State(s): State<Services>,
    user: AuthenticatedUser,
    Query(query): Query<CompanyQuery>,
    Path(id): Path<Uuid>,
    axum_extra::extract::Form(draft): axum_extra::extract::Form<DeleteDraft>,
) -> AppResult<Response> {
    let command = draft.command(scope(&user, query.company_id), id)?;
    match s.library.documents().delete(command).await {
        Ok(()) => Ok(
            Redirect::to(&format!("/ui/knowledge?company_id={}", query.company_id)).into_response(),
        ),
        Err(error) => render_failure(&s, &user, query.company_id, mutation_error(error)).await,
    }
}
struct Feedback {
    status: StatusCode,
    message: String,
}
async fn render_failure(
    s: &Services,
    user: &AuthenticatedUser,
    company: Uuid,
    response: Response,
) -> AppResult<Response> {
    let status = response.status();
    let message = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_else(|_| {
            "The request could not be completed. Refresh the library and try again.".into()
        });
    // Reload both authority and visible rows after any failed mutation, including a demotion.
    render(s, user, company, Some(Feedback { status, message })).await
}
async fn render(
    s: &Services,
    user: &AuthenticatedUser,
    company: Uuid,
    feedback: Option<Feedback>,
) -> AppResult<Response> {
    let view = s.library.documents().list(scope(user, company)).await?;
    let company = s
        .companies
        .readable_company(user.id, company)
        .await?
        .company;
    let account = super::ui::load_account(&s.users, user.id).await?;
    let email = account.email.as_str().into();
    let reader = super::ui::workspace_user(&account, &email, &s.config)
        .with_company_membership(view.actor.membership);
    let content = pages::knowledge_library(
        company.id,
        &view,
        feedback.as_ref().map(|feedback| feedback.message.as_str()),
    );
    let html = pages::ui_shell(&pages::UiShell {
        title: "Knowledge",
        user: &reader,
        company: Some(&company),
        section: pages::UiSection::Knowledge,
        content: &content,
    });
    Ok((
        feedback.map_or(StatusCode::OK, |feedback| feedback.status),
        Html(html),
    )
        .into_response())
}
#[cfg(test)]
mod tests;
