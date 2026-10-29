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
        agent::{Agent, MAX_AGENT_SKILLS},
        value_objects::AvatarUrl,
    },
    use_cases::{agent::AgentUseCases, company::CompanyUseCases, skill::SkillUseCases},
};

use super::channel::slugify;

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
    pub system_prompt: Option<String>,
    /// Short statement of what this agent is for, shown to sibling agents by the directory tool.
    pub description: Option<String>,
    #[serde(default)]
    pub skill_ids: Option<String>,
    pub avatar_url: Option<String>,
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

    pub(super) fn capabilities(&self) -> Result<SubmittedCapabilities, String> {
        let skill_ids = parse_uuid_csv(self.skill_ids.as_deref(), MAX_AGENT_SKILLS, "skills")?;
        Ok(SubmittedCapabilities { skill_ids })
    }
}

#[derive(Debug, Clone)]
pub(super) struct SubmittedCapabilities {
    pub skill_ids: Vec<Uuid>,
}

const MAX_CAPABILITY_CSV_BYTES: usize = 8 * 1024;

fn parse_csv(value: Option<&str>, max: usize, label: &str) -> Result<Vec<String>, String> {
    let value = value.unwrap_or_default();
    if value.len() > MAX_CAPABILITY_CSV_BYTES {
        return Err(format!("The {label} selection is too large."));
    }
    let mut parsed = Vec::new();
    for item in value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        if !parsed.iter().any(|stored| stored == item) {
            parsed.push(item.to_string());
        }
        if parsed.len() > max {
            return Err(format!(
                "Too many {label} were selected; the limit is {max}."
            ));
        }
    }
    Ok(parsed)
}

pub(super) fn parse_uuid_csv(
    value: Option<&str>,
    max: usize,
    label: &str,
) -> Result<Vec<Uuid>, String> {
    parse_csv(value, max, label)?
        .into_iter()
        .map(|value| {
            Uuid::parse_str(&value).map_err(|_| format!("A selected {label} id is invalid."))
        })
        .collect()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentJsonPayload {
    pub name: String,
    pub slug: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    /// Short statement of what this agent is for, shown to sibling agents by the directory tool.
    pub description: Option<String>,
    #[serde(default)]
    pub skill_ids: Option<Vec<Uuid>>,
    pub avatar_url: Option<String>,
}

impl AgentJsonPayload {
    pub(super) fn preserve_capabilities(
        &mut self,
        stored: &crate::use_cases::skill::StoredAgentCapabilities,
    ) {
        self.skill_ids
            .get_or_insert_with(|| stored.skills.iter().map(|skill| skill.id).collect());
    }

    /// The avatar this payload means, or why it cannot be stored.
    pub(super) fn avatar_url(&self) -> Result<Option<AvatarUrl>, String> {
        AvatarUrl::parse(self.avatar_url.as_deref().unwrap_or_default())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentResponse {
    pub success: bool,
    pub agent: Agent,
    pub skill_ids: Vec<Uuid>,
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
    pub avatar_url: Option<&'a AvatarUrl>,
}

/// An agent built from plain-language instructions rather than a written system prompt.
///
/// Used by the `/ui` Agents workspace's Simple tab so prompt expansion shares the same
/// model-validation path as Advanced creation.
pub(super) async fn create_agent_from_instructions(
    agent_use_cases: &AgentUseCases,
    request: AgentInstructionRequest<'_>,
) -> Result<Agent, String> {
    let write = agent_write_from_instructions(agent_use_cases, request).await?;
    agent_use_cases
        .create_agent(request.user_id, request.company_id, write)
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
        // Workflow tool grants are edited in phase 09; keep the stored list.
        workflow_tools: None,
        name: request.name.to_string(),
        slug: request.slug.to_string(),
        provider: request.overrides.provider.map(str::to_string),
        model: request.overrides.model.map(str::to_string),
        system_prompt: Some(system_prompt),
        avatar_url: request.avatar_url.cloned(),
        ..AgentWrite::default()
    })
}

/// GET /companies/{company_id}/agents - Full HTML page listing agents (Protected).
#[instrument(skip_all)]
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
#[instrument(skip_all)]
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

    let submitted = form
        .capabilities()
        .and_then(|capabilities| Ok((capabilities, form.avatar_url()?)));

    let (capabilities, avatar_url) = match submitted {
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
        .create_agent(
            user.id,
            company_id,
            AgentWrite {
                // Workflow tool grants are edited in phase 09; keep the stored list.
                workflow_tools: None,
                name: form.name.clone(),
                slug: form.slug(),
                provider: form.provider.clone(),
                model: form.model.clone(),
                system_prompt: form.system_prompt.clone(),
                description: form.description.clone(),
                skill_ids: capabilities.skill_ids,
                avatar_url,
                created_by: None,
            },
        )
        .await
    {
        Ok(_) => {
            let agents = agent_use_cases
                .list_company_agents(user.id, company_id)
                .await
                .unwrap_or_default();
            Html(pages::agent_list_fragment(&company, &agents))
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
#[instrument(skip_all)]
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
#[instrument(skip_all)]
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
#[instrument(skip_all)]
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

    let submitted = form
        .capabilities()
        .and_then(|capabilities| Ok((capabilities, form.avatar_url()?)));

    let (capabilities, avatar_url) = match submitted {
        Ok(fields) => fields,
        Err(err) => return Html(pages::error_alert(&err)),
    };

    match agent_use_cases
        .update_agent(
            user.id,
            company_id,
            agent_id,
            AgentWrite {
                // Workflow tool grants are edited in phase 09; keep the stored list.
                workflow_tools: None,
                name: form.name.clone(),
                slug: form.slug(),
                provider: form.provider.clone(),
                model: form.model.clone(),
                system_prompt: form.system_prompt.clone(),
                description: form.description.clone(),
                skill_ids: capabilities.skill_ids,
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
#[instrument(skip_all)]
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
    State(skill_use_cases): State<Arc<SkillUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let agents = agent_use_cases
        .list_company_agents(user.id, company_id)
        .await?;
    let mut response = Vec::with_capacity(agents.len());
    for agent in agents {
        let capabilities = skill_use_cases
            .company_agent_capabilities(user.id, company_id, agent.id)
            .await?
            .ok_or_else(crate::use_cases::agent::agent_not_found)?;
        response.push(AgentResponse {
            success: true,
            agent,
            skill_ids: capabilities.skills.iter().map(|skill| skill.id).collect(),
        });
    }
    Ok((StatusCode::OK, Json(response)))
}

/// JSON API: Create company agent (Protected).
async fn create_agent_json(
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Json(payload): Json<AgentJsonPayload>,
) -> AppResult<impl IntoResponse> {
    let avatar_url = payload.avatar_url().map_err(AppError::BadRequest)?;

    let agent = agent_use_cases
        .create_agent(
            user.id,
            company_id,
            AgentWrite {
                // Workflow tool grants are edited in phase 09; keep the stored list.
                workflow_tools: None,
                name: payload.name.clone(),
                slug: payload.slug.clone(),
                provider: payload.provider.clone(),
                model: payload.model.clone(),
                system_prompt: payload.system_prompt.clone(),
                description: payload.description.clone(),
                skill_ids: payload.skill_ids.clone().unwrap_or_default(),
                avatar_url,
                created_by: None,
            },
        )
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(AgentResponse {
            success: true,
            agent,
            skill_ids: payload.skill_ids.unwrap_or_default(),
        }),
    ))
}

/// JSON API: Get company agent details (Protected).
async fn get_agent_json(
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    State(skill_use_cases): State<Arc<SkillUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, agent_id)): Path<(Uuid, Uuid)>,
) -> AppResult<impl IntoResponse> {
    let agent = agent_use_cases
        .get_company_agent(user.id, company_id, agent_id)
        .await?
        .ok_or_else(crate::use_cases::agent::agent_not_found)?;

    let capabilities = skill_use_cases
        .company_agent_capabilities(user.id, company_id, agent_id)
        .await?
        .ok_or_else(crate::use_cases::agent::agent_not_found)?;
    Ok((
        StatusCode::OK,
        Json(AgentResponse {
            success: true,
            agent,
            skill_ids: capabilities.skills.iter().map(|skill| skill.id).collect(),
        }),
    ))
}

/// JSON API: Update company agent (Protected).
async fn update_agent_json(
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    State(skills): State<Arc<SkillUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, agent_id)): Path<(Uuid, Uuid)>,
    Json(mut payload): Json<AgentJsonPayload>,
) -> AppResult<impl IntoResponse> {
    let stored = skills
        .company_agent_capabilities(user.id, company_id, agent_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Agent not found".into()))?;
    payload.preserve_capabilities(&stored);
    let avatar_url = payload.avatar_url().map_err(AppError::BadRequest)?;

    let agent = agent_use_cases
        .update_agent(
            user.id,
            company_id,
            agent_id,
            AgentWrite {
                // Workflow tool grants are edited in phase 09; keep the stored list.
                workflow_tools: None,
                name: payload.name.clone(),
                slug: payload.slug.clone(),
                provider: payload.provider.clone(),
                model: payload.model.clone(),
                system_prompt: payload.system_prompt.clone(),
                description: payload.description.clone(),
                skill_ids: payload.skill_ids.clone().unwrap_or_default(),
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
            skill_ids: payload.skill_ids.unwrap_or_default(),
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

#[instrument(skip_all)]
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

#[instrument(skip_all)]
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

    use super::*;

    #[tokio::test]
    async fn clearing_every_capability_checkbox_submits_empty_lists() {
        use axum::extract::{FromRequest, Request};

        let request = Request::post("/ui/agents")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(axum::body::Body::from("name=Support&skill_ids="))
            .unwrap();
        let Form(form) = Form::<AgentForm>::from_request(request, &())
            .await
            .expect("an empty selection parses");
        let parsed = form.capabilities().expect("empty selections are valid");
        assert!(parsed.skill_ids.is_empty());
    }

    #[test]
    fn json_payload_round_trips_every_capability_field() {
        let skill_id = Uuid::new_v4();
        let payload: AgentJsonPayload = serde_json::from_value(serde_json::json!({
            "name": "Support",
            "slug": "support",
            "provider": null,
            "model": null,
            "system_prompt": "Help",
            "description": "Answers support mail",
            "avatar_url": null,
            "skill_ids": [skill_id]
        }))
        .unwrap();
        let encoded = serde_json::to_value(&payload).unwrap();
        assert_eq!(encoded["skill_ids"][0], skill_id.to_string());
    }

    #[test]
    fn agent_pages_and_fragments_render_correctly() {
        let company = Company {
            channel_defaults: Default::default(),
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".into(),
            avatar_url: None,
            created_at: Utc::now(),
        };

        let agent = Agent {
            id: Uuid::new_v4(),
            company_id: Some(company.id),
            name: "Support Agent".to_string(),
            slug: "support-agent".to_string(),
            provider: Some("openai".to_string()),
            model: Some("gpt-4o".to_string()),
            system_prompt: Some("You are a helpful agent.".to_string()),
            description: None,
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

#[cfg(test)]
mod harness_payload_tests {
    use super::*;
    #[test]
    fn retired_agent_fields_are_unknown() {
        for retired in [
            "harness_kind",
            "granted_tool_ids",
            "native_tool_policy",
            "config_json",
            "run_timeout_secs",
            "response_contract",
            "sub_agent_ids",
        ] {
            let mut value = serde_json::json!({"name":"test","slug":"test"});
            value[retired] = serde_json::json!(null);
            assert!(
                serde_json::from_value::<AgentJsonPayload>(value).is_err(),
                "{retired}"
            );
        }
        assert!(serde_json::from_value::<AgentJsonPayload>(serde_json::json!({"name":"test","slug":"test","mcp_connections":[{"endpoint_url":"https://example.com","token":"secret"}]})).is_err());
    }
}

#[cfg(test)]
#[path = "agent_api_tests.rs"]
mod api_tests;
