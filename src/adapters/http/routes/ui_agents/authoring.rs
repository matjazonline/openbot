//! Agent authoring transports share the application ports; no route resolves a tool or grants access.
use super::*;
use crate::application::agent::*;
use crate::application::workflow::WorkflowActor;
use crate::domain::workflow::CompanyId;
use crate::entities::agent_capability::*;
use axum::Json;
use serde::Serialize;

mod catalog;
mod publication;
#[cfg(test)]
mod test_support;

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .merge(catalog::router())
        .merge(publication::router())
        .route(
            "/api/companies/{company_id}/agents/{agent_id}/authoring",
            get(state_json),
        )
        .route("/ui/agents/{agent_id}/publication", get(panel))
}

pub(super) fn scope(user: Uuid, company: Uuid, agent: Uuid) -> AppResult<AgentPublicationScope> {
    Ok(AgentPublicationScope {
        company: CompanyId::new(company),
        actor: WorkflowActor::authenticated(user)?,
        agent: AgentId::new(agent),
    })
}

async fn fresh_json<T: Serialize>(
    workspace: &Workspace,
    scope: AgentPublicationScope,
    value: T,
) -> AppResult<Json<T>> {
    workspace
        .workflow
        .agent_authoring_reads()
        .authorize_authoring(scope)
        .await?;
    Ok(Json(value))
}

async fn state_json(
    workspace: Workspace,
    Path((company, agent)): Path<(Uuid, Uuid)>,
) -> AppResult<impl IntoResponse> {
    let scope = scope(workspace.user_id, company, agent)?;
    let state = workspace
        .workflow
        .agent_authoring_reads()
        .authoring_state(scope)
        .await?;
    fresh_json(&workspace, scope, state).await
}

async fn panel(
    workspace: Workspace,
    Path(agent): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
) -> AppResult<Html<String>> {
    panel_response(
        &workspace,
        scope(workspace.user_id, query.company_id, agent)?,
        None,
        None,
    )
    .await
}

async fn panel_response(
    workspace: &Workspace,
    scope: AgentPublicationScope,
    retry: Option<&PublishAgent>,
    message: Option<&str>,
) -> AppResult<Html<String>> {
    let state = workspace
        .workflow
        .agent_authoring_reads()
        .authoring_state(scope)
        .await?;
    let html = pages::agent_publication_panel(scope.company.as_uuid(), &state, retry, message);
    workspace
        .workflow
        .agent_authoring_reads()
        .authorize_authoring(scope)
        .await?;
    Ok(Html(html))
}

fn editable_failure(error: AppError) -> AppResult<String> {
    match error {
        AppError::BadRequest(_) | AppError::Conflict(_) | AppError::ServiceUnavailable(_) => {
            Ok(error.to_string())
        }
        error => Err(error),
    }
}

#[cfg(test)]
mod browser_fixtures;

#[cfg(test)]
mod security_tests;
