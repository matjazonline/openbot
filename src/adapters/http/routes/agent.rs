use std::sync::Arc;

use axum::{
    Form, Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser, pages},
    app_error::AppResult,
    entities::agent::Agent,
    use_cases::{
        agent::AgentUseCases,
        company::CompanyUseCases,
    },
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/companies/{company_id}/agents",
            get(list_agents_page).post(create_agent_handler),
        )
        .route(
            "/companies/{company_id}/agents/inline",
            post(create_agent_inline_handler),
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
    pub slug: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub system_prompt: Option<String>,
    pub config_json: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InlineAgentQuery {
    #[serde(default = "default_container_id")]
    pub container_id: String,
}

fn default_container_id() -> String {
    "new".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct InlineAgentForm {
    pub inline_agent_name: Option<String>,
    pub inline_agent_slug: Option<String>,
    pub inline_agent_provider: Option<String>,
    pub inline_agent_model: Option<String>,
    pub inline_agent_api_key: Option<String>,
    pub inline_agent_system_prompt: Option<String>,
    pub inline_agent_config_json: Option<String>,

    pub name: Option<String>,
    pub slug: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub system_prompt: Option<String>,
    pub config_json: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentJsonPayload {
    pub name: String,
    pub slug: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub system_prompt: Option<String>,
    pub config_json: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentResponse {
    pub success: bool,
    pub agent: Agent,
}

fn parse_config_form(input: Option<String>) -> Result<Option<serde_json::Value>, String> {
    match input {
        Some(ref s) if !s.trim().is_empty() => serde_json::from_str(s.trim())
            .map(Some)
            .map_err(|e| format!("Invalid JSON config: {e}")),
        _ => Ok(None),
    }
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

    let config_json = match parse_config_form(form.config_json) {
        Ok(c) => c,
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
            &form.name,
            &form.slug,
            form.provider.as_deref(),
            form.model.as_deref(),
            form.api_key.as_deref(),
            form.system_prompt.as_deref(),
            config_json,
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

/// POST /companies/{company_id}/agents/inline - HTMX inline agent creation for channel page (Protected).
#[instrument(skip(company_use_cases, agent_use_cases, user, form))]
async fn create_agent_inline_handler(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Query(query): Query<InlineAgentQuery>,
    Form(form): Form<InlineAgentForm>,
) -> impl IntoResponse {
    let _company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => {
            let agents = agent_use_cases
                .list_company_agents(user.id, company_id)
                .await
                .unwrap_or_default();
            return Html(pages::render_agents_selection_full(
                company_id,
                &agents,
                None,
                &query.container_id,
                Some("Company not found."),
            ));
        }
    };

    let name = form
        .inline_agent_name
        .filter(|s| !s.trim().is_empty())
        .or_else(|| form.name.filter(|s| !s.trim().is_empty()))
        .unwrap_or_default();

    let slug = form
        .inline_agent_slug
        .filter(|s| !s.trim().is_empty())
        .or_else(|| form.slug.filter(|s| !s.trim().is_empty()))
        .unwrap_or_default();

    let provider = form
        .inline_agent_provider
        .filter(|s| !s.trim().is_empty())
        .or_else(|| form.provider.filter(|s| !s.trim().is_empty()));

    let model = form
        .inline_agent_model
        .filter(|s| !s.trim().is_empty())
        .or_else(|| form.model.filter(|s| !s.trim().is_empty()));

    let api_key = form
        .inline_agent_api_key
        .filter(|s| !s.trim().is_empty())
        .or_else(|| form.api_key.filter(|s| !s.trim().is_empty()));

    let system_prompt = form
        .inline_agent_system_prompt
        .filter(|s| !s.trim().is_empty())
        .or_else(|| form.system_prompt.filter(|s| !s.trim().is_empty()));

    let config_json_raw = form
        .inline_agent_config_json
        .filter(|s| !s.trim().is_empty())
        .or_else(|| form.config_json.filter(|s| !s.trim().is_empty()));

    let config_json = match parse_config_form(config_json_raw) {
        Ok(c) => c,
        Err(err) => {
            let agents = agent_use_cases
                .list_company_agents(user.id, company_id)
                .await
                .unwrap_or_default();
            return Html(pages::render_agents_selection_full(
                company_id,
                &agents,
                None,
                &query.container_id,
                Some(&err),
            ));
        }
    };

    match agent_use_cases
        .create_agent(
            user.id,
            company_id,
            &name,
            &slug,
            provider.as_deref(),
            model.as_deref(),
            api_key.as_deref(),
            system_prompt.as_deref(),
            config_json,
        )
        .await
    {
        Ok(agent) => {
            let agents = agent_use_cases
                .list_company_agents(user.id, company_id)
                .await
                .unwrap_or_default();
            Html(pages::render_agents_selection(
                company_id,
                &agents,
                Some(&[agent.id]),
                &query.container_id,
            ))
        }
        Err(err) => {
            let agents = agent_use_cases
                .list_company_agents(user.id, company_id)
                .await
                .unwrap_or_default();
            Html(pages::render_agents_selection_full(
                company_id,
                &agents,
                None,
                &query.container_id,
                Some(&format!("Failed to create agent: {err}")),
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

    let config_json = match parse_config_form(form.config_json) {
        Ok(c) => c,
        Err(err) => return Html(pages::error_alert(&err)),
    };

    match agent_use_cases
        .update_agent(
            user.id,
            company_id,
            agent_id,
            &form.name,
            &form.slug,
            form.provider.as_deref(),
            form.model.as_deref(),
            form.api_key.as_deref(),
            form.system_prompt.as_deref(),
            config_json,
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
    let agent = agent_use_cases
        .create_agent(
            user.id,
            company_id,
            &payload.name,
            &payload.slug,
            payload.provider.as_deref(),
            payload.model.as_deref(),
            payload.api_key.as_deref(),
            payload.system_prompt.as_deref(),
            payload.config_json,
        )
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(AgentResponse {
            success: true,
            agent,
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
        .ok_or_else(|| crate::app_error::AppError::Internal("Agent not found.".into()))?;

    Ok((StatusCode::OK, Json(agent)))
}

/// JSON API: Update company agent (Protected).
async fn update_agent_json(
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, agent_id)): Path<(Uuid, Uuid)>,
    Json(payload): Json<AgentJsonPayload>,
) -> AppResult<impl IntoResponse> {
    let agent = agent_use_cases
        .update_agent(
            user.id,
            company_id,
            agent_id,
            &payload.name,
            &payload.slug,
            payload.provider.as_deref(),
            payload.model.as_deref(),
            payload.api_key.as_deref(),
            payload.system_prompt.as_deref(),
            payload.config_json,
        )
        .await?;

    Ok((
        StatusCode::OK,
        Json(AgentResponse {
            success: true,
            agent,
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
    pub api_key: Option<String>,
    pub inline_agent_provider: Option<String>,
    pub inline_agent_model: Option<String>,
    pub inline_agent_api_key: Option<String>,
    pub target_id: Option<String>,
    pub gen_box_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GeneratePromptJsonPayload {
    pub instructions: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
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
            Html(pages::error_alert("Please enter instructions for prompt generation.")),
        )
            .into_response());
    }

    let provider_override = form
        .provider
        .filter(|s| !s.trim().is_empty())
        .or_else(|| form.inline_agent_provider.filter(|s| !s.trim().is_empty()));
    let model_override = form
        .model
        .filter(|s| !s.trim().is_empty())
        .or_else(|| form.inline_agent_model.filter(|s| !s.trim().is_empty()));
    let api_key_override = form
        .api_key
        .filter(|s| !s.trim().is_empty())
        .or_else(|| form.inline_agent_api_key.filter(|s| !s.trim().is_empty()));

    let target_id = form.target_id.unwrap_or_else(|| "agent_system_prompt".to_string());
    let gen_box_id = form.gen_box_id.unwrap_or_default();

    match agent_use_cases
        .generate_system_prompt(
            user.id,
            company_id,
            &instructions,
            provider_override.as_deref(),
            model_override.as_deref(),
            api_key_override.as_deref(),
        )
        .await
    {
        Ok(generated_prompt) => {
            let escaped_prompt = serde_json::to_string(&generated_prompt).unwrap_or_default();
            let html = format!(
                r#"
                <div class="p-2 my-1 bg-emerald-500/10 border border-emerald-500/30 rounded text-emerald-300 text-xs flex items-center justify-between font-medium">
                    <span>✓ System prompt generated successfully!</span>
                </div>
                <script>
                    (function() {{
                        const target = document.getElementById("{target_id}");
                        if (target) {{
                            target.value = {escaped_prompt};
                            target.dispatchEvent(new Event('input', {{ bubbles: true }}));
                            target.dispatchEvent(new Event('change', {{ bubbles: true }}));
                        }}
                        const genBox = document.getElementById("{gen_box_id}");
                        if (genBox) {{
                            setTimeout(function() {{ genBox.classList.add('hidden'); }}, 1000);
                        }}
                    }})();
                </script>
                "#,
                target_id = target_id,
                escaped_prompt = escaped_prompt,
                gen_box_id = gen_box_id
            );
            Ok(Html(html).into_response())
        }
        Err(err) => Ok((
            StatusCode::OK,
            Html(pages::error_alert(&format!("Prompt generation failed: {err}"))),
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
            payload.api_key.as_deref(),
        )
        .await?;

    Ok(Json(GeneratePromptJsonResponse {
        success: true,
        system_prompt: generated,
    }))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;
    use crate::entities::company::Company;

    use super::*;

    #[test]
    fn agent_pages_and_fragments_render_correctly() {
        let company = Company {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".to_string(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            created_at: Utc::now().naive_utc(),
        };

        let agent = Agent {
            id: Uuid::new_v4(),
            company_id: company.id,
            name: "Support Agent".to_string(),
            slug: "support-agent".to_string(),
            provider: Some("openai".to_string()),
            model: Some("gpt-4o".to_string()),
            api_key: Some("sk-test123".to_string()),
            system_prompt: Some("You are a helpful agent.".to_string()),
            config_json: Some(json!({ "temperature": 0.5 })),
            created_at: Utc::now().naive_utc(),
        };

        let row_html = pages::agent_row_fragment(&company, &agent);
        assert!(row_html.contains("Support Agent"));
        assert!(row_html.contains("@support-agent"));
        assert!(row_html.contains("openai"));
        assert!(row_html.contains("gpt-4o"));
        assert!(row_html.contains("Key Configured"));

        let edit_html = pages::agent_edit_fragment(&company, &agent);
        assert!(edit_html.contains("hx-put="));
        assert!(edit_html.contains("value=\"Support Agent\""));

        let page_html = pages::agents_page(&company, &[agent.clone()]);
        assert!(page_html.contains("Acme Corp Agents"));
        assert!(page_html.contains("Add New Agent"));

        let selection_html = pages::render_agents_selection(company.id, &[agent.clone()], Some(&[agent.id]), "new");
        assert!(selection_html.contains("agents-selection-new"));
        assert!(selection_html.contains("inline-agent-form-new"));
        assert!(selection_html.contains("/companies/"));
        assert!(selection_html.contains("/agents/inline?container_id=new"));
        assert!(selection_html.contains("Create & Select Agent"));
        assert!(selection_html.contains("checked"));

        let selection_error_html = pages::render_agents_selection_full(company.id, &[agent], None, "wf-1", Some("Slug is already taken"));
        assert!(selection_error_html.contains("Slug is already taken"));
        assert!(selection_error_html.contains("agents-selection-wf-1"));
    }

    #[tokio::test]
    async fn test_inline_agent_form_deserialization_with_parent_channel_fields() {
        use axum::extract::FromRequest;
        use axum::http::{header, Request};
        use axum::body::Body;

        let req = Request::builder()
            .method("POST")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from("name=&slug=&inline_agent_name=Billing+Bot&inline_agent_slug=billing-bot&inline_agent_provider=openai"))
            .unwrap();
        let form = Form::<InlineAgentForm>::from_request(req, &()).await.unwrap().0;

        let name = form
            .inline_agent_name
            .filter(|s| !s.trim().is_empty())
            .or_else(|| form.name.filter(|s| !s.trim().is_empty()))
            .unwrap_or_default();
        let slug = form
            .inline_agent_slug
            .filter(|s| !s.trim().is_empty())
            .or_else(|| form.slug.filter(|s| !s.trim().is_empty()))
            .unwrap_or_default();

        assert_eq!(name, "Billing Bot");
        assert_eq!(slug, "billing-bot");
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

        assert!(html.contains("✨ Generate with AI"));
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
