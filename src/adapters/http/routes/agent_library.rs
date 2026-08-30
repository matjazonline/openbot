//! Authenticated reads and operator-only management of global agent definitions.

use std::sync::Arc;

use axum::{
    Form, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::Html,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser, pages},
    app_error::{AppError, AppResult},
    entities::{agent::Agent, value_objects::AvatarUrl},
    infra::config::AppConfig,
    use_cases::{
        agent::{AgentUseCases, AgentWrite},
        user::UserUseCases,
    },
};

use super::{agent::AgentJsonPayload, ui::workspace_user};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/agent-library", get(list_json).post(create_json))
        .route(
            "/api/agent-library/{id}",
            get(get_json).put(update_json).delete(delete_json),
        )
        .route("/api/agent-library/generate-prompt", post(generate_prompt))
        .route("/ui/agent-library", get(workspace))
        .route(
            "/ui/agent-library/generate-prompt",
            post(generate_prompt_fragment),
        )
}

#[derive(Serialize)]
struct LibraryAgentResponse {
    id: Uuid,
    scope: &'static str,
    name: String,
    slug: String,
    provider: Option<String>,
    model: Option<String>,
    system_prompt: Option<String>,
    description: Option<String>,
    config_json: Option<serde_json::Value>,
    memory_persistence_mode: crate::entities::memory::MemoryPersistenceMode,
    memory_recall_mode: crate::entities::memory::MemoryRecallMode,
    memory_max_results: u8,
    avatar_url: Option<AvatarUrl>,
    created_by: crate::entities::creation::CreationProvenance,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl From<Agent> for LibraryAgentResponse {
    fn from(agent: Agent) -> Self {
        Self {
            id: agent.id,
            scope: "library",
            name: agent.name,
            slug: agent.slug,
            provider: agent.provider,
            model: agent.model,
            system_prompt: agent.system_prompt,
            description: agent.description,
            config_json: agent.config_json,
            memory_persistence_mode: agent.memory_persistence_mode,
            memory_recall_mode: agent.memory_recall_mode,
            memory_max_results: agent.memory_max_results,
            avatar_url: agent.avatar_url,
            created_by: agent.created_by,
            created_at: agent.created_at,
        }
    }
}

async fn require_operator(
    user: &AuthenticatedUser,
    users: &UserUseCases,
    config: &AppConfig,
) -> AppResult<()> {
    let account = users
        .get_user_by_id(user.id)
        .await?
        .ok_or(AppError::InvalidCredentials)?;
    if config.is_operator(&account.email.as_str().into()) {
        Ok(())
    } else {
        Err(AppError::NotFound(
            "Agent library workspace not found.".into(),
        ))
    }
}

fn write(payload: AgentJsonPayload) -> Result<AgentWrite, AppError> {
    let avatar_url = payload.avatar_url().map_err(AppError::BadRequest)?;
    Ok(AgentWrite {
        name: payload.name,
        slug: payload.slug,
        provider: payload.provider,
        model: payload.model,
        run_timeout_secs: payload.run_timeout_secs,
        system_prompt: payload.system_prompt,
        description: payload.description,
        config_json: payload.config_json,
        memory_persistence_mode: payload.memory_persistence_mode,
        memory_recall_mode: payload.memory_recall_mode,
        memory_max_results: payload.memory_max_results,
        avatar_url,
        created_by: None,
    })
}

async fn list_json(
    State(agents): State<Arc<AgentUseCases>>,
    _user: AuthenticatedUser,
) -> AppResult<Json<Vec<LibraryAgentResponse>>> {
    Ok(Json(
        agents
            .list_library_agents()
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

async fn get_json(
    State(agents): State<Arc<AgentUseCases>>,
    _user: AuthenticatedUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<LibraryAgentResponse>> {
    agents
        .get_library_agent(id)
        .await?
        .map(|agent| Json(agent.into()))
        .ok_or_else(|| AppError::NotFound("Library agent not found.".into()))
}

async fn create_json(
    State(agents): State<Arc<AgentUseCases>>,
    State(users): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Json(payload): Json<AgentJsonPayload>,
) -> AppResult<(StatusCode, Json<LibraryAgentResponse>)> {
    require_operator(&user, &users, &config).await?;
    let agent = agents.create_library_agent(write(payload)?).await?;
    Ok((StatusCode::CREATED, Json(agent.into())))
}

async fn update_json(
    State(agents): State<Arc<AgentUseCases>>,
    State(users): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path(id): Path<Uuid>,
    Json(payload): Json<AgentJsonPayload>,
) -> AppResult<Json<LibraryAgentResponse>> {
    require_operator(&user, &users, &config).await?;
    Ok(Json(
        agents
            .update_library_agent(id, write(payload)?)
            .await?
            .into(),
    ))
}

async fn delete_json(
    State(agents): State<Arc<AgentUseCases>>,
    State(users): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    require_operator(&user, &users, &config).await?;
    agents.delete_library_agent(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct PromptPayload {
    instructions: String,
    provider: Option<String>,
    model: Option<String>,
}

#[derive(Serialize)]
struct PromptResponse {
    system_prompt: String,
}

#[derive(Deserialize)]
struct PromptForm {
    instructions: String,
    provider: Option<String>,
    model: Option<String>,
    id_prefix: Option<String>,
}

async fn generate_prompt_fragment(
    State(agents): State<Arc<AgentUseCases>>,
    State(users): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Form(form): Form<PromptForm>,
) -> AppResult<Html<String>> {
    require_operator(&user, &users, &config).await?;
    let prompt = agents
        .generate_library_system_prompt(
            &form.instructions,
            form.provider.as_deref(),
            form.model.as_deref(),
            None,
        )
        .await?;
    Ok(Html(pages::agent_prompt_generated(
        form.id_prefix.as_deref().unwrap_or("new"),
        &prompt,
    )))
}

async fn generate_prompt(
    State(agents): State<Arc<AgentUseCases>>,
    State(users): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Json(payload): Json<PromptPayload>,
) -> AppResult<Json<PromptResponse>> {
    require_operator(&user, &users, &config).await?;
    let system_prompt = agents
        .generate_library_system_prompt(
            &payload.instructions,
            payload.provider.as_deref(),
            payload.model.as_deref(),
            None,
        )
        .await?;
    Ok(Json(PromptResponse { system_prompt }))
}

async fn workspace(
    State(agents): State<Arc<AgentUseCases>>,
    State(users): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
) -> AppResult<Html<String>> {
    require_operator(&user, &users, &config).await?;
    let account = users
        .get_user_by_id(user.id)
        .await?
        .ok_or(AppError::InvalidCredentials)?;
    let account_email = account.email.as_str().into();
    let workspace_user = workspace_user(&account, &account_email, &config);
    let rows = agents
        .list_library_agents()
        .await?
        .into_iter()
        .map(|agent| {
            let config_json = pages::stored_agent_config(&agent);
            let draft = pages::AgentDraft {
                name: &agent.name,
                slug: &agent.slug,
                system_prompt: agent.system_prompt.as_deref().unwrap_or(""),
                description: agent.description.as_deref().unwrap_or(""),
                provider: agent.provider.as_deref().unwrap_or(""),
                model: agent.model.as_deref().unwrap_or(""),
                run_timeout_secs: agent.run_timeout_secs,
                memory_persistence_mode: agent.memory_persistence_mode.as_str(),
                memory_recall_mode: agent.memory_recall_mode.as_str(),
                memory_max_results: agent.memory_max_results,
                config_json: &config_json,
                avatar_url: agent
                    .avatar_url
                    .as_ref()
                    .map(AvatarUrl::as_str)
                    .unwrap_or(""),
                advanced: true,
            };
            format!(
                r#"<form class="card bg-base-200 p-4 space-y-4" data-submit="save-library-agent" data-agent-id="{id}">{fields}<div class="flex gap-2"><button class="btn btn-primary btn-sm">Save</button><button type="button" class="btn btn-error btn-outline btn-sm" data-action="delete-library-agent" data-agent-id="{id}">Delete</button></div></form>"#,
                id = agent.id,
                fields = pages::library_agent_fields(&draft, Some(agent.id)),
            )
        })
        .collect::<String>();
    let create_fields = pages::library_agent_fields(
        &pages::AgentDraft {
            advanced: true,
            ..pages::AgentDraft::default()
        },
        None,
    );
    let content = format!(
        r#"<main class="flex-1 overflow-auto p-8"><div class="mx-auto max-w-4xl"><h1 class="text-2xl font-bold">Agent library</h1><p class="mb-6 opacity-70">Live global definitions available to every company.</p>
    <form class="card mb-6 bg-base-200 p-4 space-y-2" data-submit="create-library-agent">
      <h2 class="font-bold">New library agent</h2>
      {create_fields}
      <div><button class="btn btn-primary btn-sm">Create</button></div>
    </form>
    <div class="space-y-3">{}</div></div></main>"#,
        if rows.is_empty() {
            "<p class=\"opacity-60\">No library agents yet.</p>".into()
        } else {
            rows
        },
        create_fields = create_fields,
    );
    Ok(Html(pages::ui_shell(&pages::UiShell {
        title: "Agent library",
        user: &workspace_user,
        company: None,
        section: pages::UiSection::Dashboard,
        content: &content,
    })))
}
