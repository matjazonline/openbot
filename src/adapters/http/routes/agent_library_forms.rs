//! Native library writes share the same parser and application validation as agent settings.
use super::*;
use crate::adapters::http::routes::{agent::AgentForm, ui_agents::SubmittedAgent};
use axum::response::{IntoResponse, Redirect, Response};

pub(super) async fn create(
    State(agents): State<Arc<AgentUseCases>>,
    State(users): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Form(form): Form<AgentForm>,
) -> AppResult<Response> {
    require_operator(&user, &users, &config).await?;
    let submitted = SubmittedAgent::new(form);
    let result = match submitted.agent_write() {
        Ok(write) => agents.create_library_agent(write).await,
        Err(message) => Err(AppError::BadRequest(message)),
    };
    Ok(form_result(result, &submitted, None))
}

pub(super) async fn update(
    State(agents): State<Arc<AgentUseCases>>,
    State(users): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path(id): Path<Uuid>,
    Form(form): Form<AgentForm>,
) -> AppResult<Response> {
    require_operator(&user, &users, &config).await?;
    let submitted = SubmittedAgent::new(form);
    let result = match submitted.agent_write() {
        Ok(write) => agents.update_library_agent(id, write).await,
        Err(message) => Err(AppError::BadRequest(message)),
    };
    Ok(form_result(result, &submitted, Some(id)))
}

fn form_result(result: AppResult<Agent>, submitted: &SubmittedAgent, id: Option<Uuid>) -> Response {
    match result {
        Ok(_) => Redirect::to("/ui/agent-library").into_response(),
        Err(error) => {
            let action = id
                .map(|id| format!("/ui/agent-library/{id}/save"))
                .unwrap_or_else(|| "/ui/agent-library/create".into());
            let body = format!(
                r#"{}<form method="post" action="{action}" data-submit="busy-once">{}<button type="submit" class="btn btn-primary">Save</button></form>"#,
                pages::error_alert(&error.to_string()),
                pages::library_agent_fields(&submitted.draft(), id)
            );
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Html(pages::agent_library_form_page(&body)),
            )
                .into_response()
        }
    }
}
