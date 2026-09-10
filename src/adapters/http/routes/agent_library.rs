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
        skill::{MAX_SKILL_PAGE_SIZE, SkillPageRequest, SkillUseCases},
        user::UserUseCases,
    },
};

use super::{agent::AgentJsonPayload, ui::workspace_user};

#[path = "agent_library_forms.rs"]
mod forms;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/ui/agent-library/create",
            axum::routing::post(forms::create),
        )
        .route(
            "/ui/agent-library/{agent_id}/save",
            axum::routing::post(forms::update),
        )
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
    run_timeout_secs: Option<u32>,
    system_prompt: Option<String>,
    description: Option<String>,
    config_json: Option<serde_json::Value>,
    response_contract: Option<crate::entities::response_contract::ResponseContract>,
    memory_enabled: bool,
    memory_persistence_mode: crate::entities::memory::MemoryPersistenceMode,
    memory_recall_mode: crate::entities::memory::MemoryRecallMode,
    memory_max_results: u8,
    avatar_url: Option<AvatarUrl>,
    created_by: crate::entities::creation::CreationProvenance,
    created_at: chrono::DateTime<chrono::Utc>,
    harness_kind: crate::entities::harness::HarnessKind,
    granted_tool_ids: Vec<crate::entities::value_objects::ToolId>,
    native_tool_policy: crate::entities::harness::NativeToolPolicy,
    skill_ids: Vec<Uuid>,
    sub_agent_ids: Vec<Uuid>,
}

impl LibraryAgentResponse {
    fn new(agent: Agent, skill_ids: Vec<Uuid>, sub_agent_ids: Vec<Uuid>) -> Self {
        Self {
            id: agent.id,
            scope: "library",
            name: agent.name,
            slug: agent.slug,
            provider: agent.provider,
            model: agent.model,
            run_timeout_secs: agent.run_timeout_secs,
            system_prompt: agent.system_prompt,
            description: agent.description,
            config_json: agent.config_json,
            response_contract: agent.response_contract,
            memory_enabled: agent.memory_enabled,
            memory_persistence_mode: agent.memory_persistence_mode,
            memory_recall_mode: agent.memory_recall_mode,
            memory_max_results: agent.memory_max_results,
            avatar_url: agent.avatar_url,
            created_by: agent.created_by,
            created_at: agent.created_at,
            harness_kind: agent.harness_kind,
            granted_tool_ids: agent.granted_tool_ids,
            native_tool_policy: agent.native_tool_policy,
            skill_ids,
            sub_agent_ids,
        }
    }
}

pub(super) async fn require_operator(
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
        Err(AppError::NotFound("Library workspace not found.".into()))
    }
}

fn write(payload: AgentJsonPayload) -> Result<AgentWrite, AppError> {
    let avatar_url = payload.avatar_url().map_err(AppError::BadRequest)?;
    Ok(AgentWrite {
        response_contract: payload.response_contract,
        name: payload.name,
        slug: payload.slug,
        provider: payload.provider,
        model: payload.model,
        run_timeout_secs: payload.run_timeout_secs,
        system_prompt: payload.system_prompt,
        description: payload.description,
        harness_kind: payload.harness_kind,
        granted_tool_ids: payload.granted_tool_ids.unwrap_or_default(),
        native_tool_policy: payload.native_tool_policy.unwrap_or_default(),
        skill_ids: payload.skill_ids.unwrap_or_default(),
        sub_agent_ids: payload.sub_agent_ids.unwrap_or_default(),
        config_json: payload.config_json,
        memory_enabled: payload.memory_enabled,
        memory_persistence_mode: payload.memory_persistence_mode,
        memory_recall_mode: payload.memory_recall_mode,
        memory_max_results: payload.memory_max_results,
        avatar_url,
        created_by: None,
    })
}

async fn list_json(
    State(agents): State<Arc<AgentUseCases>>,
    State(skills): State<Arc<SkillUseCases>>,
    State(users): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
) -> AppResult<Json<Vec<LibraryAgentResponse>>> {
    require_operator(&user, &users, &config).await?;
    let mut response = Vec::new();
    for agent in agents.list_library_agents().await? {
        let capabilities = skills
            .library_agent_capabilities(agent.id)
            .await?
            .ok_or_else(|| AppError::NotFound("Library agent not found.".into()))?;
        response.push(LibraryAgentResponse::new(
            agent,
            capabilities.skills.iter().map(|skill| skill.id).collect(),
            capabilities.sub_agent_scope.allowed_ids().to_vec(),
        ));
    }
    Ok(Json(response))
}

async fn get_json(
    State(agents): State<Arc<AgentUseCases>>,
    State(skills): State<Arc<SkillUseCases>>,
    State(users): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path(id): Path<Uuid>,
) -> AppResult<Json<LibraryAgentResponse>> {
    require_operator(&user, &users, &config).await?;
    let agent = agents
        .get_library_agent(id)
        .await?
        .ok_or_else(|| AppError::NotFound("Library agent not found.".into()))?;
    let capabilities = skills
        .library_agent_capabilities(id)
        .await?
        .ok_or_else(|| AppError::NotFound("Library agent not found.".into()))?;
    Ok(Json(LibraryAgentResponse::new(
        agent,
        capabilities.skills.iter().map(|skill| skill.id).collect(),
        capabilities.sub_agent_scope.allowed_ids().to_vec(),
    )))
}

async fn create_json(
    State(agents): State<Arc<AgentUseCases>>,
    State(users): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    State(skills): State<Arc<SkillUseCases>>,
    user: AuthenticatedUser,
    Json(payload): Json<AgentJsonPayload>,
) -> AppResult<(StatusCode, Json<LibraryAgentResponse>)> {
    require_operator(&user, &users, &config).await?;
    let agent = agents.create_library_agent(write(payload)?).await?;
    let capabilities = skills
        .library_agent_capabilities(agent.id)
        .await?
        .ok_or_else(|| AppError::NotFound("Library agent not found.".into()))?;
    Ok((
        StatusCode::CREATED,
        Json(LibraryAgentResponse::new(
            agent,
            capabilities.skills.iter().map(|skill| skill.id).collect(),
            capabilities.sub_agent_scope.allowed_ids().to_vec(),
        )),
    ))
}

async fn update_json(
    State(agents): State<Arc<AgentUseCases>>,
    State(users): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    State(skills): State<Arc<SkillUseCases>>,
    user: AuthenticatedUser,
    Path(id): Path<Uuid>,
    Json(mut payload): Json<AgentJsonPayload>,
) -> AppResult<Json<LibraryAgentResponse>> {
    require_operator(&user, &users, &config).await?;
    let stored = skills
        .library_agent_capabilities(id)
        .await?
        .ok_or_else(|| AppError::NotFound("Library agent not found".into()))?;
    payload.preserve_capabilities(&stored);
    let agent = agents.update_library_agent(id, write(payload)?).await?;
    let capabilities = skills
        .library_agent_capabilities(id)
        .await?
        .ok_or_else(|| AppError::NotFound("Library agent not found.".into()))?;
    Ok(Json(LibraryAgentResponse::new(
        agent,
        capabilities.skills.iter().map(|skill| skill.id).collect(),
        capabilities.sub_agent_scope.allowed_ids().to_vec(),
    )))
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
    State(skills): State<Arc<SkillUseCases>>,
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
    let skill_page = skills
        .list_library_page(SkillPageRequest {
            before: None,
            limit: MAX_SKILL_PAGE_SIZE,
        })
        .await?;
    let mut rows = String::new();
    for agent in agents.list_library_agents().await? {
        let capabilities = skills
            .library_agent_capabilities(agent.id)
            .await?
            .ok_or_else(|| AppError::NotFound("Library agent not found.".into()))?;
        let config_json = pages::stored_agent_config(&agent);
        let draft = pages::AgentDraft {
            name: &agent.name,
            slug: &agent.slug,
            system_prompt: agent.system_prompt.as_deref().unwrap_or(""),
            description: agent.description.as_deref().unwrap_or(""),
            provider: agent.provider.as_deref().unwrap_or(""),
            model: agent.model.as_deref().unwrap_or(""),
            run_timeout_secs: agent.run_timeout_secs,
            memory_enabled: agent.memory_enabled,
            memory_persistence_mode: agent.memory_persistence_mode.as_str(),
            memory_recall_mode: agent.memory_recall_mode.as_str(),
            memory_max_results: agent.memory_max_results,
            config_json: &config_json,
            response_format: if agent.response_contract.is_some() {
                "json_schema"
            } else {
                "text"
            },
            response_schema: pages::agent_response_schema(&agent),
            harness_kind_raw: None,
            harness_kind: Some(agent.harness_kind),
            granted_tool_ids: agent.granted_tool_ids.clone(),
            skill_ids: capabilities.skills.iter().map(|skill| skill.id).collect(),
            sub_agent_ids: Vec::new(),
            native_tool_policy: agent.native_tool_policy.clone(),
            avatar_url: agent
                .avatar_url
                .as_ref()
                .map(AvatarUrl::as_str)
                .unwrap_or(""),
            advanced: true,
        };
        rows.push_str(&format!(
                r#"<form method="post" action="/ui/agent-library/{id}/save" class="card bg-base-200 p-4 space-y-4" data-submit="save-library-agent" data-agent-id="{id}">{fields}<div class="flex gap-2"><button class="btn btn-primary btn-sm">Save</button><button type="button" class="btn btn-error btn-outline btn-sm" data-action="delete-library-agent" data-agent-id="{id}">Delete</button></div></form>"#,
                id = agent.id,
                fields = pages::library_agent_fields_with_capabilities(
                    &draft,
                    Some(agent.id),
                    pages::AgentCapabilityOptions { skills: &skill_page.items, sub_agents: &[] },
                ),
            ));
    }
    let create_fields = pages::library_agent_fields_with_capabilities(
        &pages::AgentDraft {
            advanced: true,
            harness_kind: Some(config.default_agent_harness),
            ..pages::AgentDraft::default()
        },
        None,
        pages::AgentCapabilityOptions {
            skills: &skill_page.items,
            sub_agents: &[],
        },
    );
    let content = format!(
        r#"<main class="flex-1 overflow-auto p-8"><div class="mx-auto max-w-4xl"><h1 class="text-2xl font-bold">Agent library</h1><p class="mb-6 opacity-70">Live global definitions available to every company.</p>
    <div class="alert alert-warning mb-6 text-sm">A library agent assigned directly to a company channel can reach every sibling in that company. Prefer copy-on-pick when a restricted scope is needed.</div>
    <form method="post" action="/ui/agent-library/create" class="card mb-6 bg-base-200 p-4 space-y-2" data-submit="create-library-agent">
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

#[cfg(test)]
mod structured_contract_tests {
    use super::*;
    #[test]
    fn library_api_preserves_the_response_contract_patch() {
        let payload: AgentJsonPayload = serde_json::from_value(serde_json::json!({
            "name":"Structured", "slug":"structured", "harness_kind":"rig",
            "response_contract":{"version":1,"format":"json_schema","schema":{"type":"object"}}
        }))
        .unwrap();
        let contract = payload.response_contract.0.clone();
        assert_eq!(write(payload).unwrap().response_contract.0, contract);
    }
}
