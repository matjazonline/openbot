//! One scoped library service for authenticated browser and JSON entrypoints.
mod manual;
mod upload;
use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser, pages},
    app_error::{AppError, AppResult},
    application::knowledge::*,
    domain::workflow::CompanyId,
    entities::{department::DepartmentId, knowledge::*},
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
        .route(
            "/api/companies/{company}/knowledge/search",
            post(manual::api_search).layer(DefaultBodyLimit::max(16384)),
        )
        .route(
            "/api/companies/{company}/knowledge/documents/{id}/move",
            post(manual::api_move).layer(DefaultBodyLimit::max(4096)),
        )
        .route("/ui/knowledge", get(ui_list))
        .route("/ui/knowledge/upload", post(ui_upload))
        .route(
            "/ui/knowledge/search",
            post(manual::ui_search).layer(DefaultBodyLimit::max(16384)),
        )
        .route(
            "/ui/knowledge/{id}/move",
            get(manual::ui_move_form)
                .post(manual::ui_move)
                .layer(DefaultBodyLimit::max(4096)),
        )
        .route(
            "/ui/knowledge/{id}/move/destinations",
            get(manual::ui_destinations),
        )
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
    unit_id: Option<DepartmentId>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveQuery {
    company_id: Uuid,
    unit_id: Option<DepartmentId>,
    target_unit_id: Option<DepartmentId>,
    after: Option<DepartmentId>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UnitFilter {
    unit_id: Option<DepartmentId>,
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
    Query(filter): Query<UnitFilter>,
) -> AppResult<Json<Vec<KnowledgeDocumentLayer>>> {
    Ok(Json(
        s.library
            .documents()
            .list(scope(&user, company), filter.unit_id)
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
        Err(response) => *response,
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
    render(&s, &user, query.company_id, query.unit_id, None, None).await
}
async fn ui_upload(
    State(s): State<Services>,
    user: AuthenticatedUser,
    Query(query): Query<CompanyQuery>,
    request: Request,
) -> AppResult<Response> {
    match upload::receive(&s, scope(&user, query.company_id), request).await {
        Ok(_) => Ok(Redirect::to(&library_url(query.company_id, query.unit_id)).into_response()),
        Err(response) => {
            render_failure(&s, &user, query.company_id, query.unit_id, *response).await
        }
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
        Ok(()) => Ok(Redirect::to(&library_url(query.company_id, query.unit_id)).into_response()),
        Err(error) => {
            render_failure(
                &s,
                &user,
                query.company_id,
                query.unit_id,
                mutation_error(error),
            )
            .await
        }
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
    unit: Option<DepartmentId>,
    response: Response,
) -> AppResult<Response> {
    let feedback = response_feedback(response).await;
    // Reload both authority and visible rows after any failed mutation, including a demotion.
    render(s, user, company, unit, Some(feedback), None).await
}

async fn response_feedback(response: Response) -> Feedback {
    let status = response.status();
    let message = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_else(|_| {
            "The request could not be completed. Refresh the library and try again.".into()
        });
    Feedback { status, message }
}
async fn render(
    s: &Services,
    user: &AuthenticatedUser,
    company: Uuid,
    unit: Option<DepartmentId>,
    feedback: Option<Feedback>,
    search: Option<pages::KnowledgeSearchView>,
) -> AppResult<Response> {
    let view = s
        .library
        .documents()
        .list(scope(user, company), unit)
        .await?;
    let content = pages::knowledge_library(
        company,
        &view,
        feedback.as_ref().map(|feedback| feedback.message.as_str()),
        unit,
        search.as_ref(),
    );
    render_shell(
        s,
        user,
        company,
        &view,
        feedback.map_or(StatusCode::OK, |feedback| feedback.status),
        content,
    )
    .await
}

async fn render_move(
    s: &Services,
    user: &AuthenticatedUser,
    query: &MoveQuery,
    document: Uuid,
    feedback: Option<&Feedback>,
) -> AppResult<Response> {
    let loaded = move_view(s, user, query, document).await?;
    let content = pages::knowledge_move(
        query.company_id,
        &loaded.anchor,
        &loaded.view,
        pages::KnowledgeMoveState {
            return_unit: query.unit_id,
            target: query.target_unit_id,
            next: loaded.next,
            error: feedback.map(|feedback| feedback.message.as_str()),
        },
    );
    render_shell(
        s,
        user,
        query.company_id,
        &loaded.view,
        feedback.map_or(StatusCode::OK, |feedback| feedback.status),
        content,
    )
    .await
}

struct MoveView {
    view: KnowledgeLibraryView,
    anchor: KnowledgeDocumentLayer,
    next: Option<DepartmentId>,
}

async fn move_view(
    s: &Services,
    user: &AuthenticatedUser,
    query: &MoveQuery,
    document: Uuid,
) -> AppResult<MoveView> {
    let scope = scope(user, query.company_id);
    if query.unit_id.is_some() {
        s.library
            .documents()
            .preflight_search(scope, query.unit_id)
            .await?;
    }
    // The return filter and selected destination do not change the source document's identity.
    let mut view = s.library.documents().list(scope, None).await?;
    let anchor = view
        .layers
        .iter()
        .find(|layer| layer.id == document && view.writable_units.contains(&layer.unit_id))
        .cloned()
        .ok_or_else(|| AppError::NotFound("Writable file not found".into()))?;
    let choices = s
        .library
        .documents()
        .unit_choices(
            scope,
            KnowledgeUnitQuery {
                selected: query.target_unit_id,
                after: query.after,
            },
        )
        .await?;
    view.upload_units = choices.units;
    view.units_truncated = choices.next.is_some();
    Ok(MoveView {
        view,
        anchor,
        next: choices.next,
    })
}

async fn render_destinations(
    s: &Services,
    user: &AuthenticatedUser,
    query: &MoveQuery,
    document: Uuid,
) -> AppResult<Response> {
    let loaded = move_view(s, user, query, document).await?;
    let content = pages::knowledge_move_destinations(
        query.company_id,
        &loaded.anchor,
        &loaded.view,
        loaded.next,
        query.unit_id,
    );
    render_shell(
        s,
        user,
        query.company_id,
        &loaded.view,
        StatusCode::OK,
        content,
    )
    .await
}

async fn render_shell(
    s: &Services,
    user: &AuthenticatedUser,
    company: Uuid,
    view: &KnowledgeLibraryView,
    status: StatusCode,
    content: String,
) -> AppResult<Response> {
    let company = s
        .companies
        .readable_company(user.id, company)
        .await?
        .company;
    let account = super::ui::load_account(&s.users, user.id).await?;
    let email = account.email.as_str().into();
    let reader = super::ui::workspace_user(&account, &email, &s.config)
        .with_company_membership(view.actor.membership);
    let html = pages::ui_shell(&pages::UiShell {
        title: "Knowledge",
        user: &reader,
        company: Some(&company),
        section: pages::UiSection::Knowledge,
        content: &content,
    });
    Ok((status, Html(html)).into_response())
}

fn library_url(company: Uuid, unit: Option<DepartmentId>) -> String {
    let mut url = format!("/ui/knowledge?company_id={company}");
    if let Some(unit) = unit {
        url.push_str(&format!("&unit_id={}", unit.as_uuid()));
    }
    url
}
#[cfg(test)]
mod tests;
