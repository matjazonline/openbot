use crate::{
    adapters::http::auth::AuthenticatedUser,
    app_error::{AppError, AppResult},
    application::local_agents::{LocalMachineUseCases, management::*},
    domain::local_agents::*,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, FromRef, Path, Query, State},
    http::StatusCode,
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

#[cfg(test)]
#[path = "local_agents_tests.rs"]
mod tests;

pub fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    Arc<LocalAgentManagement>: FromRef<S>,
    Arc<LocalMachineUseCases>: FromRef<S>,
{
    Router::new()
        .route(
            "/api/companies/{company}/local-agent-profiles",
            get(profiles).post(create_profile),
        )
        .route(
            "/api/companies/{company}/local-agent-profiles/{profile}",
            get(profile).put(update_profile).delete(delete_profile),
        )
        .route(
            "/api/companies/{company}/local-agent-sessions",
            get(sessions).post(start),
        )
        .route(
            "/api/companies/{company}/local-agent-sessions/{session}",
            get(session),
        )
        .route(
            "/api/companies/{company}/local-agent-sessions/{session}/commands",
            post(control),
        )
        .route(
            "/api/companies/{company}/local-agent-operations/{operation}",
            get(operation),
        )
        .route(
            "/api/companies/{company}/local-machines/{machine}/runtime",
            get(runtime),
        )
        .route(
            "/api/companies/{company}/local-machines/{machine}/test-connection",
            post(test_connection),
        )
        // JSON escaping can use six bytes per prompt byte, plus the fixed request fields.
        .layer(DefaultBodyLimit::max(MAX_PROMPT_BYTES * 6 + 4096))
        .layer(middleware::map_response(
            super::local_machines::private_response,
        ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileInput {
    agent_id: Option<crate::entities::agent_capability::AgentId>,
    name: String,
    selection: ExecutionSelection,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartInput {
    request_id: OperationId,
    profile_id: ProfileId,
    conversation_id: Uuid,
    prompt: LocalPrompt,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ControlInput {
    request_id: OperationId,
    command: SessionCommand,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TestInput {
    request_id: OperationId,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileQuery {
    after: Option<ProfileId>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionQuery {
    conversation_id: Option<Uuid>,
    run_id: Option<Uuid>,
}

async fn profiles(
    State(cases): State<Arc<LocalAgentManagement>>,
    user: AuthenticatedUser,
    Path(company): Path<Uuid>,
    Query(query): Query<ProfileQuery>,
) -> AppResult<Json<RecordPage<LocalAgentProfile>>> {
    Ok(Json(cases.profiles(user.id, company, query.after).await?))
}
async fn profile(
    State(cases): State<Arc<LocalAgentManagement>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, ProfileId)>,
) -> AppResult<Json<LocalAgentProfile>> {
    Ok(Json(
        cases
            .profile(user.id, company, id)
            .await?
            .ok_or_else(missing)?,
    ))
}
async fn create_profile(
    State(cases): State<Arc<LocalMachineUseCases>>,
    user: AuthenticatedUser,
    Path(company): Path<Uuid>,
    Json(input): Json<ProfileInput>,
) -> AppResult<impl IntoResponse> {
    let profile = LocalAgentProfile {
        id: ProfileId::new(Uuid::new_v4()).map_err(|_| missing())?,
        company_id: company,
        name: input.name,
        agent_id: input.agent_id,
        selection: input.selection,
    };
    cases.save_profile(user.id, &profile).await?;
    Ok((StatusCode::CREATED, Json(profile)))
}
async fn update_profile(
    State(cases): State<Arc<LocalMachineUseCases>>,
    State(management): State<Arc<LocalAgentManagement>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, ProfileId)>,
    Json(input): Json<ProfileInput>,
) -> AppResult<Json<LocalAgentProfile>> {
    management
        .profile(user.id, company, id)
        .await?
        .ok_or_else(missing)?;
    let profile = LocalAgentProfile {
        id,
        company_id: company,
        name: input.name,
        agent_id: input.agent_id,
        selection: input.selection,
    };
    cases.save_profile(user.id, &profile).await?;
    Ok(Json(profile))
}
async fn delete_profile(
    State(cases): State<Arc<LocalAgentManagement>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, ProfileId)>,
) -> AppResult<StatusCode> {
    cases.delete_profile(user.id, company, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn sessions(
    State(cases): State<Arc<LocalAgentManagement>>,
    user: AuthenticatedUser,
    Path(company): Path<Uuid>,
    Query(query): Query<SessionQuery>,
) -> AppResult<Json<RecordPage<SessionSummary>>> {
    let association = match (query.conversation_id, query.run_id) {
        (Some(conversation_id), None) => SessionAssociation::Conversation { conversation_id },
        (None, Some(run_id)) => SessionAssociation::Workflow { run_id },
        _ => {
            return Err(AppError::BadRequest(
                "Select exactly one conversation or workflow run".into(),
            ));
        }
    };
    Ok(Json(cases.sessions(user.id, company, &association).await?))
}
async fn session(
    State(cases): State<Arc<LocalAgentManagement>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, SessionId)>,
) -> AppResult<Json<SessionDetails>> {
    Ok(Json(cases.session(user.id, company, id).await?))
}
async fn start(
    State(cases): State<Arc<LocalAgentManagement>>,
    user: AuthenticatedUser,
    Path(company): Path<Uuid>,
    Json(input): Json<StartInput>,
) -> AppResult<Response> {
    Ok(operation_response(
        cases
            .start(StartSession {
                actor: user.id,
                company,
                request_id: input.request_id,
                profile_id: input.profile_id,
                conversation_id: input.conversation_id,
                prompt: input.prompt,
            })
            .await?,
    ))
}
async fn control(
    State(cases): State<Arc<LocalAgentManagement>>,
    user: AuthenticatedUser,
    Path((company, session_id)): Path<(Uuid, SessionId)>,
    Json(input): Json<ControlInput>,
) -> AppResult<Response> {
    Ok(operation_response(
        cases
            .control(ControlSession {
                actor: user.id,
                company,
                session_id,
                request_id: input.request_id,
                command: input.command,
            })
            .await?,
    ))
}
async fn operation(
    State(cases): State<Arc<LocalAgentManagement>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, OperationId)>,
) -> AppResult<Json<LocalOperationView>> {
    Ok(Json(cases.operation(user.id, company, id).await?))
}
async fn runtime(
    State(cases): State<Arc<LocalAgentManagement>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, MachineId)>,
) -> AppResult<Json<MachineRuntime>> {
    Ok(Json(cases.runtime(user.id, company, id).await?))
}
async fn test_connection(
    State(cases): State<Arc<LocalAgentManagement>>,
    user: AuthenticatedUser,
    Path((company, id)): Path<(Uuid, MachineId)>,
    Json(input): Json<TestInput>,
) -> AppResult<impl IntoResponse> {
    Ok((
        StatusCode::ACCEPTED,
        Json(
            cases
                .test_connection(user.id, company, id, input.request_id)
                .await?,
        ),
    ))
}
fn operation_response(view: LocalOperationView) -> Response {
    (
        if view.operation.outcome().is_some() {
            StatusCode::OK
        } else {
            StatusCode::ACCEPTED
        },
        Json(view),
    )
        .into_response()
}
fn missing() -> AppError {
    AppError::NotFound("Local agent resource".into())
}
