//! Authenticated reads and operator-only management of global agent definitions.

use std::sync::Arc;

use axum::{
    Json, Router,
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
    avatar_url: Option<AvatarUrl>,
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
            avatar_url: agent.avatar_url,
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
        api_key: payload.api_key,
        system_prompt: payload.system_prompt,
        description: payload.description,
        config_json: payload.config_json,
        avatar_url,
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
    api_key: Option<String>,
}

#[derive(Serialize)]
struct PromptResponse {
    system_prompt: String,
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
            payload.api_key.as_deref(),
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
    let workspace_user = workspace_user(&account, &account_email);
    let rows = agents.list_library_agents().await?.into_iter().map(|agent| format!(
        r#"<form class="card bg-base-200 p-4 space-y-2" onsubmit="saveLibraryAgent(event,'{id}')">
        <div class="grid gap-2 sm:grid-cols-2"><input class="input" name="name" value="{name}" required><input class="input" name="slug" value="{slug}" required></div>
        <div class="grid gap-2 sm:grid-cols-2"><input class="input" name="provider" value="{provider}" placeholder="Provider"><input class="input" name="model" value="{model}" placeholder="Model"></div>
        <input class="input w-full" type="password" name="api_key" placeholder="Replace API key (blank clears it)">
        <textarea class="textarea w-full" name="description" placeholder="Description">{description}</textarea>
        <textarea class="textarea h-40 w-full" name="system_prompt" placeholder="System prompt">{prompt}</textarea>
        <textarea class="textarea w-full font-mono text-xs" name="config_json" placeholder="JSON config">{config}</textarea>
        <div class="flex gap-2"><button class="btn btn-primary btn-sm">Save</button><button type="button" class="btn btn-error btn-outline btn-sm" onclick="deleteLibraryAgent('{id}')">Delete</button></div></form>"#,
        id=agent.id, name=pages::escape_html_text(&agent.name), slug=pages::escape_html_text(&agent.slug),
        provider=pages::escape_html_text(agent.provider.as_deref().unwrap_or("")), model=pages::escape_html_text(agent.model.as_deref().unwrap_or("")),
        description=pages::escape_html_text(agent.description.as_deref().unwrap_or("")), prompt=pages::escape_html_text(agent.system_prompt.as_deref().unwrap_or("")),
        config=pages::escape_html_text(&agent.config_json.map(|v| v.to_string()).unwrap_or_default()),
    )).collect::<String>();
    let content = format!(
        r#"<main class="flex-1 overflow-auto p-8"><div class="mx-auto max-w-4xl"><h1 class="text-2xl font-bold">Agent library</h1><p class="mb-6 opacity-70">Live global definitions available to every company.</p>
    <form class="card mb-6 bg-base-200 p-4 space-y-2" onsubmit="createLibraryAgent(event)"><h2 class="font-bold">New library agent</h2><div class="grid gap-2 sm:grid-cols-2"><input class="input" name="name" placeholder="Name" required><input class="input" name="slug" placeholder="Slug" required></div><textarea class="textarea w-full" name="instructions" placeholder="Describe the agent, then generate a prompt"></textarea><textarea class="textarea h-40 w-full" name="system_prompt" placeholder="System prompt"></textarea><div class="flex gap-2"><button class="btn btn-primary btn-sm">Create</button><button type="button" class="btn btn-outline btn-sm" onclick="generateLibraryPrompt(this.form)">Generate prompt</button></div></form>
    <div class="space-y-3">{}</div></div></main>"#,
        if rows.is_empty() {
            "<p class=\"opacity-60\">No library agents yet.</p>".into()
        } else {
            rows
        }
    );
    let script = r#"
function libraryPayload(form){const data=new FormData(form);let config=null;try{config=data.get('config_json')?JSON.parse(data.get('config_json')):null}catch(e){alert('Config must be valid JSON');throw e}return {name:data.get('name'),slug:data.get('slug'),provider:data.get('provider')||null,model:data.get('model')||null,api_key:data.get('api_key')||null,system_prompt:data.get('system_prompt')||null,description:data.get('description')||null,config_json:config,avatar_url:null}}
async function libraryRequest(url,method,body){const response=await fetch(url,{method,headers:{'content-type':'application/json'},body:body?JSON.stringify(body):undefined});if(!response.ok)throw new Error(await response.text());return response.status===204?null:response.json()}
async function createLibraryAgent(event){event.preventDefault();try{await libraryRequest('/api/agent-library','POST',libraryPayload(event.target));location.reload()}catch(e){alert(e.message)}}
async function saveLibraryAgent(event,id){event.preventDefault();try{await libraryRequest('/api/agent-library/'+id,'PUT',libraryPayload(event.target));location.reload()}catch(e){alert(e.message)}}
async function deleteLibraryAgent(id){if(!confirm('Delete this library agent?'))return;try{await libraryRequest('/api/agent-library/'+id,'DELETE');location.reload()}catch(e){alert(e.message)}}
async function generateLibraryPrompt(form){try{const data=new FormData(form);const result=await libraryRequest('/api/agent-library/generate-prompt','POST',{instructions:data.get('instructions'),provider:null,model:null,api_key:null});form.elements.system_prompt.value=result.system_prompt}catch(e){alert(e.message)}}
"#;
    Ok(Html(pages::ui_shell(&pages::UiShell {
        title: "Agent library",
        user: &workspace_user,
        company_id: None,
        section: pages::UiSection::Dashboard,
        content: &content,
        script,
    })))
}
