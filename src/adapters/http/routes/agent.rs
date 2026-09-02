use crate::use_cases::agent::AgentWrite;
use std::sync::Arc;

use axum::{
    Form, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser, pages},
    app_error::{AppError, AppResult},
    entities::{
        agent::Agent,
        channel::Channel,
        memory::{MemoryPersistenceMode, MemoryRecallMode, default_memory_max_results},
        value_objects::AvatarUrl,
    },
    use_cases::{
        agent::{AgentUseCases, ProvisioningWarning},
        company::CompanyUseCases,
    },
};

use super::channel::{parse_config_form, slugify};
use super::task::deserialize_empty_string_as_none;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/companies/{company_id}/agents",
            get(list_agents_page).post(create_agent_handler),
        )
        .route(
            "/companies/{company_id}/agents/{id}",
            put(update_agent_handler).delete(delete_agent_handler),
        )
        .route(
            "/companies/{company_id}/agents/{id}/edit",
            get(edit_agent_form),
        )
        .route(
            "/companies/{company_id}/agents/generate-prompt",
            post(generate_agent_prompt_handler),
        )
        .route(
            "/companies/{company_id}/agents/{id}/cancel",
            get(cancel_agent_edit),
        )
        .route(
            "/api/companies/{company_id}/agents",
            get(list_agents_json).post(create_agent_json),
        )
        .route(
            "/api/companies/{company_id}/agents/generate-prompt",
            post(generate_agent_prompt_json),
        )
        .route(
            "/api/companies/{company_id}/agents/{id}",
            get(get_agent_json)
                .put(update_agent_json)
                .delete(delete_agent_json),
        )
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentForm {
    pub name: String,
    /// Blank falls back to [`slugify`] of the name, so a form need not show the field at all.
    pub slug: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    /// Left blank to inherit the deployment default. A blank number input posts `""`, which
    /// plain `Option<u32>` rejects with a 422 before the handler ever runs, so it is parsed the
    /// same way every other optional form number is.
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub run_timeout_secs: Option<u32>,
    pub system_prompt: Option<String>,
    /// Short statement of what this agent is for, shown to sibling agents by the directory tool.
    pub description: Option<String>,
    pub config_json: Option<String>,
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub memory_enabled: bool,
    pub memory_persistence_mode: Option<MemoryPersistenceMode>,
    pub memory_recall_mode: Option<MemoryRecallMode>,
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub memory_max_results: Option<u8>,
    /// `"simple"` means `system_prompt` holds instructions to expand, not the prompt itself.
    pub form_mode: Option<String>,
}

impl AgentForm {
    /// The avatar this submission means, or why it cannot be stored.
    ///
    /// A form that has no avatar field at all clears nothing and stores nothing -- the same as one
    /// submitted blank, since a blank field is how an avatar is removed.
    pub(super) fn avatar_url(&self) -> Result<Option<AvatarUrl>, String> {
        AvatarUrl::parse(self.avatar_url.as_deref().unwrap_or_default())
    }

    /// The slug this submission means: what was typed, or the name slugified.
    pub(super) fn slug(&self) -> String {
        self.slug
            .as_deref()
            .map(str::trim)
            .filter(|slug| !slug.is_empty())
            .map(String::from)
            .unwrap_or_else(|| slugify(&self.name))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentJsonPayload {
    pub name: String,
    pub slug: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub run_timeout_secs: Option<u32>,
    pub system_prompt: Option<String>,
    /// Short statement of what this agent is for, shown to sibling agents by the directory tool.
    pub description: Option<String>,
    pub config_json: Option<serde_json::Value>,
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub memory_enabled: bool,
    #[serde(default)]
    pub memory_persistence_mode: MemoryPersistenceMode,
    #[serde(default)]
    pub memory_recall_mode: MemoryRecallMode,
    #[serde(default = "default_memory_max_results")]
    pub memory_max_results: u8,
}

impl AgentJsonPayload {
    /// The avatar this payload means, or why it cannot be stored.
    pub(super) fn avatar_url(&self) -> Result<Option<AvatarUrl>, String> {
        AvatarUrl::parse(self.avatar_url.as_deref().unwrap_or_default())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentResponse {
    pub success: bool,
    pub agent: Agent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<Channel>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ProvisioningWarning>,
}

/// What an agent answers with when it should not take the company's LLM settings.
///
/// The model selection an agent may override. Credentials remain company-owned.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct ModelOverrides<'a> {
    pub provider: Option<&'a str>,
    pub model: Option<&'a str>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct AgentInstructionRequest<'a> {
    pub user_id: Uuid,
    pub company_id: Uuid,
    pub name: &'a str,
    pub slug: &'a str,
    pub instructions: &'a str,
    pub overrides: ModelOverrides<'a>,
    pub run_timeout_secs: Option<u32>,
    pub avatar_url: Option<&'a AvatarUrl>,
}

/// An agent built from plain-language instructions rather than a written system prompt.
///
/// Used by the `/ui` Agents workspace's Simple tab so prompt expansion and atomic address
/// provisioning share the same model-validation path as Advanced creation.
pub(super) async fn create_agent_from_instructions(
    agent_use_cases: &AgentUseCases,
    request: AgentInstructionRequest<'_>,
) -> Result<crate::use_cases::agent::ProvisionedAgent, String> {
    let write = agent_write_from_instructions(agent_use_cases, request).await?;
    agent_use_cases
        .create_addressable_agent(request.user_id, request.company_id, write)
        .await
        .map_err(|err| format!("Failed to create agent: {err}"))
}

pub(super) async fn agent_write_from_instructions(
    agent_use_cases: &AgentUseCases,
    request: AgentInstructionRequest<'_>,
) -> Result<AgentWrite, String> {
    // Expansion runs on the company's own credentials: the overrides are what the *agent* will
    // answer with, not necessarily a model that can write its prompt.
    let system_prompt = agent_use_cases
        .generate_system_prompt(
            request.user_id,
            request.company_id,
            request.instructions,
            None,
            None,
        )
        .await
        .map_err(|err| format!("Failed to generate agent prompt: {err}"))?;

    Ok(AgentWrite {
        name: request.name.to_string(),
        slug: request.slug.to_string(),
        provider: request.overrides.provider.map(str::to_string),
        model: request.overrides.model.map(str::to_string),
        run_timeout_secs: request.run_timeout_secs,
        system_prompt: Some(system_prompt),
        avatar_url: request.avatar_url.cloned(),
        ..AgentWrite::default()
    })
}

/// GET /companies/{company_id}/agents - Full HTML page listing agents (Protected).
#[instrument(skip(company_use_cases, agent_use_cases, user))]
async fn list_agents_page(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")),
    };

    let agents = agent_use_cases
        .list_company_agents(user.id, company_id)
        .await
        .unwrap_or_default();

    Html(pages::agents_page(&company, &agents))
}

/// POST /companies/{company_id}/agents - HTMX create agent (Protected).
#[instrument(skip(company_use_cases, agent_use_cases, user, form))]
async fn create_agent_handler(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Form(form): Form<AgentForm>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")),
    };

    let submitted = parse_config_form(form.config_json.clone())
        .and_then(|config_json| Ok((config_json, form.avatar_url()?)));

    let (config_json, avatar_url) = match submitted {
        Ok(fields) => fields,
        Err(err) => {
            let error_html = pages::error_alert(&err);
            let agents = agent_use_cases
                .list_company_agents(user.id, company_id)
                .await
                .unwrap_or_default();
            return Html(format!(
                "{}{}",
                error_html,
                pages::agent_list_fragment(&company, &agents)
            ));
        }
    };

    match agent_use_cases
        .create_addressable_agent(
            user.id,
            company_id,
            AgentWrite {
                name: form.name.clone(),
                slug: form.slug(),
                provider: form.provider.clone(),
                model: form.model.clone(),
                run_timeout_secs: form.run_timeout_secs,
                system_prompt: form.system_prompt.clone(),
                description: form.description.clone(),
                config_json,
                memory_enabled: form.memory_enabled,
                memory_persistence_mode: form.memory_persistence_mode.unwrap_or_default(),
                memory_recall_mode: form.memory_recall_mode.unwrap_or_default(),
                memory_max_results: form
                    .memory_max_results
                    .unwrap_or_else(default_memory_max_results),
                avatar_url,
                created_by: None,
            },
        )
        .await
    {
        Ok(provisioned) => {
            let agents = agent_use_cases
                .list_company_agents(user.id, company_id)
                .await
                .unwrap_or_default();
            let warnings = provisioned
                .warnings
                .iter()
                .map(|warning| pages::error_alert(&warning.message))
                .collect::<String>();
            Html(format!(
                "{warnings}{}",
                pages::agent_list_fragment(&company, &agents)
            ))
        }
        Err(err) => {
            let error_html = pages::error_alert(&format!("Failed to create agent: {err}"));
            let agents = agent_use_cases
                .list_company_agents(user.id, company_id)
                .await
                .unwrap_or_default();
            Html(format!(
                "{}{}",
                error_html,
                pages::agent_list_fragment(&company, &agents)
            ))
        }
    }
}

/// GET /companies/{company_id}/agents/{id}/edit - HTMX edit agent form fragment (Protected).
#[instrument(skip(company_use_cases, agent_use_cases, user))]
async fn edit_agent_form(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, agent_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")),
    };

    if let Ok(Some(ag)) = agent_use_cases
        .get_company_agent(user.id, company_id, agent_id)
        .await
    {
        Html(pages::agent_edit_fragment(&company, &ag))
    } else {
        Html(pages::error_alert("Agent not found."))
    }
}

/// GET /companies/{company_id}/agents/{id}/cancel - Cancel agent edit fragment (Protected).
#[instrument(skip(company_use_cases, agent_use_cases, user))]
async fn cancel_agent_edit(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, agent_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")),
    };

    if let Ok(Some(ag)) = agent_use_cases
        .get_company_agent(user.id, company_id, agent_id)
        .await
    {
        Html(pages::agent_row_fragment(&company, &ag))
    } else {
        Html(String::new())
    }
}

/// PUT /companies/{company_id}/agents/{id} - HTMX update agent (Protected).
#[instrument(skip(company_use_cases, agent_use_cases, user, form))]
async fn update_agent_handler(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, agent_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<AgentForm>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")),
    };

    let submitted = parse_config_form(form.config_json.clone())
        .and_then(|config_json| Ok((config_json, form.avatar_url()?)));

    let (config_json, avatar_url) = match submitted {
        Ok(fields) => fields,
        Err(err) => return Html(pages::error_alert(&err)),
    };

    match agent_use_cases
        .update_agent(
            user.id,
            company_id,
            agent_id,
            AgentWrite {
                name: form.name.clone(),
                slug: form.slug(),
                provider: form.provider.clone(),
                model: form.model.clone(),
                run_timeout_secs: form.run_timeout_secs,
                system_prompt: form.system_prompt.clone(),
                description: form.description.clone(),
                config_json,
                memory_enabled: form.memory_enabled,
                memory_persistence_mode: form.memory_persistence_mode.unwrap_or_default(),
                memory_recall_mode: form.memory_recall_mode.unwrap_or_default(),
                memory_max_results: form
                    .memory_max_results
                    .unwrap_or_else(default_memory_max_results),
                avatar_url,
                created_by: None,
            },
        )
        .await
    {
        Ok(ag) => Html(pages::agent_row_fragment(&company, &ag)),
        Err(err) => Html(pages::error_alert(&format!("Update failed: {err}"))),
    }
}

/// DELETE /companies/{company_id}/agents/{id} - HTMX delete agent (Protected).
#[instrument(skip(agent_use_cases, user))]
async fn delete_agent_handler(
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, agent_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    let _ = agent_use_cases
        .delete_agent(user.id, company_id, agent_id)
        .await;
    Html(String::new())
}

/// JSON API: List company agents (Protected).
async fn list_agents_json(
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let agents = agent_use_cases
        .list_company_agents(user.id, company_id)
        .await?;
    Ok((StatusCode::OK, Json(agents)))
}

/// JSON API: Create company agent (Protected).
async fn create_agent_json(
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Json(payload): Json<AgentJsonPayload>,
) -> AppResult<impl IntoResponse> {
    let avatar_url = payload.avatar_url().map_err(AppError::BadRequest)?;

    let provisioned = agent_use_cases
        .create_addressable_agent(
            user.id,
            company_id,
            AgentWrite {
                name: payload.name.clone(),
                slug: payload.slug.clone(),
                provider: payload.provider.clone(),
                model: payload.model.clone(),
                run_timeout_secs: payload.run_timeout_secs,
                system_prompt: payload.system_prompt.clone(),
                description: payload.description.clone(),
                config_json: payload.config_json.clone(),
                memory_enabled: payload.memory_enabled,
                memory_persistence_mode: payload.memory_persistence_mode,
                memory_recall_mode: payload.memory_recall_mode,
                memory_max_results: payload.memory_max_results,
                avatar_url,
                created_by: None,
            },
        )
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(AgentResponse {
            success: true,
            agent: provisioned.agent,
            channel: Some(provisioned.channel),
            warnings: provisioned.warnings,
        }),
    ))
}

/// JSON API: Get company agent details (Protected).
async fn get_agent_json(
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, agent_id)): Path<(Uuid, Uuid)>,
) -> AppResult<impl IntoResponse> {
    let agent = agent_use_cases
        .get_company_agent(user.id, company_id, agent_id)
        .await?
        .ok_or_else(crate::use_cases::agent::agent_not_found)?;

    Ok((StatusCode::OK, Json(agent)))
}

/// JSON API: Update company agent (Protected).
async fn update_agent_json(
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, agent_id)): Path<(Uuid, Uuid)>,
    Json(payload): Json<AgentJsonPayload>,
) -> AppResult<impl IntoResponse> {
    let avatar_url = payload.avatar_url().map_err(AppError::BadRequest)?;

    let agent = agent_use_cases
        .update_agent(
            user.id,
            company_id,
            agent_id,
            AgentWrite {
                name: payload.name.clone(),
                slug: payload.slug.clone(),
                provider: payload.provider.clone(),
                model: payload.model.clone(),
                run_timeout_secs: payload.run_timeout_secs,
                system_prompt: payload.system_prompt.clone(),
                description: payload.description.clone(),
                config_json: payload.config_json.clone(),
                memory_enabled: payload.memory_enabled,
                memory_persistence_mode: payload.memory_persistence_mode,
                memory_recall_mode: payload.memory_recall_mode,
                memory_max_results: payload.memory_max_results,
                avatar_url,
                created_by: None,
            },
        )
        .await?;

    Ok((
        StatusCode::OK,
        Json(AgentResponse {
            success: true,
            agent,
            channel: None,
            warnings: Vec::new(),
        }),
    ))
}

/// JSON API: Delete company agent (Protected).
async fn delete_agent_json(
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, agent_id)): Path<(Uuid, Uuid)>,
) -> AppResult<impl IntoResponse> {
    agent_use_cases
        .delete_agent(user.id, company_id, agent_id)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Clone, Deserialize)]
pub struct GeneratePromptForm {
    pub user_instructions: Option<String>,
    pub instructions: Option<String>,
    pub prompt: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub target_id: Option<String>,
    pub gen_box_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GeneratePromptJsonPayload {
    pub instructions: String,
    pub provider: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GeneratePromptJsonResponse {
    pub success: bool,
    pub system_prompt: String,
}

#[instrument(skip(agent_use_cases, user, form))]
pub async fn generate_agent_prompt_handler(
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Form(form): Form<GeneratePromptForm>,
) -> AppResult<impl IntoResponse> {
    let instructions = form
        .user_instructions
        .or(form.instructions)
        .or(form.prompt)
        .unwrap_or_default();

    if instructions.trim().is_empty() {
        return Ok((
            StatusCode::BAD_REQUEST,
            Html(pages::error_alert(
                "Please enter instructions for prompt generation.",
            )),
        )
            .into_response());
    }

    let provider_override = form.provider.filter(|s| !s.trim().is_empty());
    let model_override = form.model.filter(|s| !s.trim().is_empty());
    let target_id = form
        .target_id
        .unwrap_or_else(|| "agent_system_prompt".to_string());
    let gen_box_id = form.gen_box_id.unwrap_or_default();

    match agent_use_cases
        .generate_system_prompt(
            user.id,
            company_id,
            &instructions,
            provider_override.as_deref(),
            model_override.as_deref(),
        )
        .await
    {
        Ok(generated_prompt) => {
            // The prompt and both element ids travel as escaped attribute data, not as an inline
            // `<script>`: `script-src 'self'` blocks inline scripts, and `target_id`/`gen_box_id`
            // arrive from the submitted form. `applyGeneratedPrompt` picks this up on htmx swap.
            let html = format!(
                r#"
                <div class="p-2 my-1 bg-emerald-500/10 border border-emerald-500/30 rounded text-emerald-300 text-xs flex items-center justify-between font-medium">
                    <span class="inline-flex items-center gap-1.5">{check} System prompt generated successfully!</span>
                </div>
                <div data-after-swap="apply-generated-prompt" data-target-id="{target_id}" data-gen-box="{gen_box_id}" data-generated-prompt="{prompt}"></div>
                "#,
                check = pages::icon(pages::Icon::Check, pages::BUTTON_ICON),
                target_id = pages::escape_html_text(&target_id),
                gen_box_id = pages::escape_html_text(&gen_box_id),
                prompt = pages::escape_html_text(&generated_prompt),
            );
            Ok(Html(html).into_response())
        }
        Err(err) => Ok((
            StatusCode::OK,
            Html(pages::error_alert(&format!(
                "Prompt generation failed: {err}"
            ))),
        )
            .into_response()),
    }
}

#[instrument(skip(agent_use_cases, user, payload))]
pub async fn generate_agent_prompt_json(
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Json(payload): Json<GeneratePromptJsonPayload>,
) -> AppResult<impl IntoResponse> {
    let generated = agent_use_cases
        .generate_system_prompt(
            user.id,
            company_id,
            &payload.instructions,
            payload.provider.as_deref(),
            payload.model.as_deref(),
        )
        .await?;

    Ok(Json(GeneratePromptJsonResponse {
        success: true,
        system_prompt: generated,
    }))
}

#[cfg(test)]
mod tests {
    use crate::entities::company::Company;
    use chrono::Utc;
    use serde_json::json;

    use super::*;

    /// A blank number input posts `name=`, and plain `Option<u32>`/`Option<u8>` answer that with
    /// a 422 before the handler runs -- which is what "leave blank to inherit" used to do.
    #[tokio::test]
    async fn blank_optional_numbers_mean_absent_rather_than_a_rejected_form() {
        async fn parse(body: &'static str) -> Result<AgentForm, StatusCode> {
            use axum::extract::{FromRequest, Request};

            let request = Request::post("/ui/agents")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(axum::body::Body::from(body))
                .unwrap();

            Form::<AgentForm>::from_request(request, &())
                .await
                .map(|Form(form)| form)
                .map_err(|rejection| rejection.status())
        }

        let form = parse(
            "name=Support&system_prompt=Help&run_timeout_secs=&memory_max_results=&form_mode=simple",
        )
        .await
        .expect("a blank number field is an omitted one");
        assert_eq!(form.run_timeout_secs, None);
        assert_eq!(form.memory_max_results, None);

        let filled = parse("name=Support&run_timeout_secs=90&memory_max_results=7")
            .await
            .expect("a filled number field still parses");
        assert_eq!(filled.run_timeout_secs, Some(90));
        assert_eq!(filled.memory_max_results, Some(7));

        assert_eq!(
            parse("name=Support&run_timeout_secs=soon").await.err(),
            Some(StatusCode::UNPROCESSABLE_ENTITY),
            "a non-numeric timeout is still a bad form, not a silent None"
        );
    }

    #[test]
    fn agent_pages_and_fragments_render_correctly() {
        let company = Company {
            channel_defaults: Default::default(),
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            created_at: Utc::now(),
        };

        let agent = Agent {
            memory_enabled: false,
            memory_persistence_mode: crate::entities::memory::MemoryPersistenceMode::AudienceOnly,
            memory_recall_mode: crate::entities::memory::MemoryRecallMode::Fast,
            memory_max_results: 5,
            id: Uuid::new_v4(),
            company_id: Some(company.id),
            name: "Support Agent".to_string(),
            slug: "support-agent".to_string(),
            provider: Some("openai".to_string()),
            model: Some("gpt-4o".to_string()),
            run_timeout_secs: None,
            system_prompt: Some("You are a helpful agent.".to_string()),
            description: None,
            config_json: Some(json!({ "temperature": 0.5 })),
            avatar_url: None,
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
        };

        let row_html = pages::agent_row_fragment(&company, &agent);
        assert!(row_html.contains("Support Agent"));
        assert!(row_html.contains("@support-agent"));
        assert!(row_html.contains("openai"));
        assert!(row_html.contains("gpt-4o"));
        assert!(!row_html.contains("Key Configured"));

        let edit_html = pages::agent_edit_fragment(&company, &agent);
        assert!(edit_html.contains("hx-put="));
        assert!(edit_html.contains("value=\"Support Agent\""));

        let page_html = pages::agents_page(&company, std::slice::from_ref(&agent));
        assert!(page_html.contains("Acme Corp Agents"));
        assert!(page_html.contains("Add New Agent"));
        assert!(page_html.contains("id=\"agent-form-toggle\""));
        assert!(page_html.contains("id=\"agent-form-card\" class=\"hidden"));
        assert!(page_html.contains("aria-controls=\"agent-form-card\""));

        let selection_html = pages::render_agents_selection(
            company.id,
            std::slice::from_ref(&agent),
            Some(&[agent.id]),
            "new",
        );
        assert!(selection_html.contains("agents-selection-new"));
        assert!(!selection_html.contains("inline-agent-form-new"));
        assert!(!selection_html.contains("/agents/inline"));
        assert!(selection_html.contains("/ui/agents?company_id="));
        assert!(selection_html.contains("Manage agents in the Agents workspace"));
        assert!(selection_html.contains("checked"));

        let selection_error_html = pages::render_agents_selection_full(
            company.id,
            &[agent],
            None,
            "wf-1",
            Some("Slug is already taken"),
        );
        assert!(selection_error_html.contains("Slug is already taken"));
        assert!(selection_error_html.contains("agents-selection-wf-1"));
    }

    #[test]
    fn test_ai_prompt_generator_renders_link_button_and_textarea() {
        let company_id = Uuid::new_v4();
        let html = pages::render_ai_prompt_generator(
            company_id,
            "test_sys_prompt",
            "test_gen_box",
            "test_gen_input",
            "test_gen_status",
            ", #test_provider",
        );

        assert!(html.contains("Generate with AI"));
        assert!(html.contains("Generate System Prompt with AI"));
        assert!(html.contains("hx-post="));
        assert!(html.contains(&format!("/companies/{company_id}/agents/generate-prompt")));
        assert!(html.contains("target_id\": \"test_sys_prompt\""));
        assert!(html.contains("gen_box_id\": \"test_gen_box\""));
        assert!(html.contains("animate-spin"));
        assert!(html.contains("Generating..."));
        assert!(html.contains("hx-disabled-elt=\"this\""));
    }
}
