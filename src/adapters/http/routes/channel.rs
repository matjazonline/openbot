use std::sync::Arc;

use axum::{
    Form, Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, put},
};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser, pages},
    app_error::AppResult,
    entities::channel::Channel,
    infra::config::AppConfig,
    services::email_parser::RawInboundPayload,
    use_cases::{
        agent::AgentUseCases,
        channel::ChannelUseCases,
        company::CompanyUseCases,
        thread::{SimulationMode, ThreadUseCases},
    },
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/companies/{company_id}/channels",
            get(list_channels_page).post(create_channel_handler),
        )
        .route(
            "/companies/{company_id}/channels/{id}",
            put(update_channel_handler).delete(delete_channel_handler),
        )
        .route(
            "/companies/{company_id}/channels/{id}/edit",
            get(edit_channel_form),
        )
        .route(
            "/companies/{company_id}/channels/{id}/cancel",
            get(cancel_channel_edit),
        )
        .route(
            "/companies/{company_id}/channels/{id}/simulate",
            get(simulate_channel_page).post(simulate_channel_handler),
        )
        .route(
            "/companies/{company_id}/channels/{id}/simulate/thread",
            get(open_simulated_thread_get).post(open_simulated_thread_post),
        )
        .route(
            "/api/companies/{company_id}/channels",
            get(list_channels_json).post(create_channel_json),
        )
        .route(
            "/api/companies/{company_id}/channels/{id}",
            get(get_channel_json)
                .put(update_channel_json)
                .delete(delete_channel_json),
        )
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChannelForm {
    pub name: String,
    pub slug: Option<String>,
    pub system_prompt: Option<String>,
    pub form_mode: Option<String>,
    pub api_key: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub participant_emails: Option<String>,
    pub agent_ids: Option<String>,
    pub channel_config: Option<String>,
    pub confirm_spam_disabled: Option<String>,
}

pub fn slugify(input: &str) -> String {
    let clean: String = input
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();

    let mut result = String::new();
    let mut last_was_hyphen = false;
    for c in clean.chars() {
        if c == '-' {
            if !last_was_hyphen {
                result.push('-');
                last_was_hyphen = true;
            }
        } else {
            result.push(c);
            last_was_hyphen = false;
        }
    }
    result.trim_matches('-').to_string()
}

fn parse_agent_ids_form(input: Option<String>) -> Option<Vec<Uuid>> {
    input.and_then(|s| {
        let list: Vec<Uuid> = s
            .split(&[',', ' ', ';', '\n'][..])
            .map(|e| e.trim())
            .filter(|e| !e.is_empty())
            .filter_map(|e| Uuid::parse_str(e).ok())
            .collect();
        if list.is_empty() { None } else { Some(list) }
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChannelJsonPayload {
    pub name: String,
    pub slug: Option<String>,
    pub system_prompt: Option<String>,
    pub api_key: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub participant_emails: Option<Vec<String>>,
    pub agent_ids: Option<Vec<Uuid>>,
    pub channel_config: Option<serde_json::Value>,
    pub confirm_spam_disabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelResponse {
    pub success: bool,
    pub channel: Channel,
}

fn parse_emails_form(input: Option<String>) -> Option<Vec<String>> {
    input.and_then(|s| {
        let list: Vec<String> = s
            .split(&[',', '\n', ';'][..])
            .map(|e| e.trim().to_string())
            .filter(|e| !e.is_empty())
            .collect();
        if list.is_empty() { None } else { Some(list) }
    })
}

fn parse_config_form(input: Option<String>) -> Result<Option<serde_json::Value>, String> {
    match input {
        Some(ref s) if !s.trim().is_empty() => serde_json::from_str(s.trim())
            .map(Some)
            .map_err(|e| format!("Invalid JSON config: {e}")),
        _ => Ok(None),
    }
}

/// GET /companies/{company_id}/channels - Full HTML page listing channels (Protected).
#[instrument(skip(company_use_cases, channel_use_cases, agent_use_cases, config, user))]
async fn list_channels_page(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")),
    };

    let channels = channel_use_cases
        .list_company_channels(user.id, company_id)
        .await
        .unwrap_or_default();

    let agents = agent_use_cases
        .list_company_agents(user.id, company_id)
        .await
        .unwrap_or_default();

    Html(pages::channels_page(
        &company,
        &config.app_domain_name,
        &channels,
        &agents,
        config.is_spam_scan_enabled(),
    ))
}

/// POST /companies/{company_id}/channels - HTMX create channel (Protected).
#[instrument(skip(
    company_use_cases,
    channel_use_cases,
    agent_use_cases,
    config,
    user,
    form
))]
async fn create_channel_handler(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Form(form): Form<ChannelForm>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")),
    };

    let slug = form
        .slug
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| slugify(&form.name));

    let emails = parse_emails_form(form.participant_emails);
    let mut agent_ids = parse_agent_ids_form(form.agent_ids);
    let confirm_spam_disabled = form.confirm_spam_disabled.as_deref() == Some("true")
        || form.confirm_spam_disabled.as_deref() == Some("on");

    let system_prompt_clean = form
        .system_prompt
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());

    if let Some(prompt) = system_prompt_clean {
        match agent_use_cases
            .create_agent(
                user.id,
                company_id,
                &form.name,
                &slug,
                form.provider.as_deref(),
                form.model.as_deref(),
                form.api_key.as_deref(),
                Some(prompt),
                None,
            )
            .await
        {
            Ok(agent) => {
                let mut ids = agent_ids.unwrap_or_default();
                ids.push(agent.id);
                agent_ids = Some(ids);
            }
            Err(err) => {
                let agents = agent_use_cases
                    .list_company_agents(user.id, company_id)
                    .await
                    .unwrap_or_default();
                let channels = channel_use_cases
                    .list_company_channels(user.id, company_id)
                    .await
                    .unwrap_or_default();
                let error_html =
                    pages::error_alert(&format!("Failed to create agent for channel: {err}"));
                return Html(format!(
                    "{}{}",
                    error_html,
                    pages::channel_list_fragment(
                        &company,
                        &config.app_domain_name,
                        &channels,
                        &agents
                    )
                ));
            }
        }
    }

    let channel_config = match parse_config_form(form.channel_config) {
        Ok(c) => c,
        Err(err) => {
            let agents = agent_use_cases
                .list_company_agents(user.id, company_id)
                .await
                .unwrap_or_default();
            let error_html = pages::error_alert(&err);
            let channels = channel_use_cases
                .list_company_channels(user.id, company_id)
                .await
                .unwrap_or_default();
            return Html(format!(
                "{}{}",
                error_html,
                pages::channel_list_fragment(&company, &config.app_domain_name, &channels, &agents)
            ));
        }
    };

    match channel_use_cases
        .create_channel(
            user.id,
            company_id,
            &form.name,
            &slug,
            form.api_key.as_deref(),
            form.provider.as_deref(),
            form.model.as_deref(),
            emails,
            agent_ids,
            channel_config,
            confirm_spam_disabled,
        )
        .await
    {
        Ok(_) => {
            let agents = agent_use_cases
                .list_company_agents(user.id, company_id)
                .await
                .unwrap_or_default();
            let channels = channel_use_cases
                .list_company_channels(user.id, company_id)
                .await
                .unwrap_or_default();
            Html(pages::channel_list_fragment(
                &company,
                &config.app_domain_name,
                &channels,
                &agents,
            ))
        }
        Err(err) => {
            let agents = agent_use_cases
                .list_company_agents(user.id, company_id)
                .await
                .unwrap_or_default();
            let error_html = pages::error_alert(&format!("Failed to create channel: {err}"));
            let channels = channel_use_cases
                .list_company_channels(user.id, company_id)
                .await
                .unwrap_or_default();
            Html(format!(
                "{}{}",
                error_html,
                pages::channel_list_fragment(&company, &config.app_domain_name, &channels, &agents)
            ))
        }
    }
}

/// GET /companies/{company_id}/channels/{id}/edit - HTMX edit channel form fragment (Protected).
#[instrument(skip(company_use_cases, channel_use_cases, agent_use_cases, config, user))]
async fn edit_channel_form(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")),
    };

    let agents = agent_use_cases
        .list_company_agents(user.id, company_id)
        .await
        .unwrap_or_default();

    if let Ok(Some(wf)) = channel_use_cases
        .get_company_channel(user.id, company_id, channel_id)
        .await
    {
        Html(pages::channel_edit_fragment(
            &company,
            &config.app_domain_name,
            &wf,
            &agents,
            config.is_spam_scan_enabled(),
        ))
    } else {
        Html(pages::error_alert("Channel not found."))
    }
}

/// GET /companies/{company_id}/channels/{id}/cancel - Cancel channel edit fragment (Protected).
#[instrument(skip(company_use_cases, channel_use_cases, agent_use_cases, config, user))]
async fn cancel_channel_edit(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")),
    };

    let agents = agent_use_cases
        .list_company_agents(user.id, company_id)
        .await
        .unwrap_or_default();

    if let Ok(Some(wf)) = channel_use_cases
        .get_company_channel(user.id, company_id, channel_id)
        .await
    {
        Html(pages::channel_row_fragment(
            &company,
            &config.app_domain_name,
            &wf,
            &agents,
        ))
    } else {
        Html(String::new())
    }
}

/// PUT /companies/{company_id}/channels/{id} - HTMX update channel (Protected).
#[instrument(skip(
    company_use_cases,
    channel_use_cases,
    agent_use_cases,
    config,
    user,
    form
))]
async fn update_channel_handler(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<ChannelForm>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")),
    };

    let agents = agent_use_cases
        .list_company_agents(user.id, company_id)
        .await
        .unwrap_or_default();

    let slug = form
        .slug
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| slugify(&form.name));

    let emails = parse_emails_form(form.participant_emails);
    let agent_ids = parse_agent_ids_form(form.agent_ids);
    let confirm_spam_disabled = form.confirm_spam_disabled.as_deref() == Some("true")
        || form.confirm_spam_disabled.as_deref() == Some("on");

    let channel_config = match parse_config_form(form.channel_config) {
        Ok(c) => c,
        Err(err) => return Html(pages::error_alert(&err)),
    };

    match channel_use_cases
        .update_channel(
            user.id,
            company_id,
            channel_id,
            &form.name,
            &slug,
            form.api_key.as_deref(),
            form.provider.as_deref(),
            form.model.as_deref(),
            emails,
            agent_ids,
            channel_config,
            confirm_spam_disabled,
        )
        .await
    {
        Ok(wf) => Html(pages::channel_row_fragment(
            &company,
            &config.app_domain_name,
            &wf,
            &agents,
        )),
        Err(err) => Html(pages::error_alert(&format!("Update failed: {err}"))),
    }
}

/// DELETE /companies/{company_id}/channels/{id} - HTMX delete channel (Protected).
#[instrument(skip(channel_use_cases, user))]
async fn delete_channel_handler(
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    let _ = channel_use_cases
        .delete_channel(user.id, company_id, channel_id)
        .await;
    Html(String::new())
}

#[derive(Debug, Clone, Deserialize)]
pub struct SimulationForm {
    pub to: String,
    pub from: String,
    pub subject: Option<String>,
    pub text_body: Option<String>,
    pub html_body: Option<String>,
    pub simulation_mode: Option<String>,
    pub in_reply_to: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SimulateQuery {
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenThreadParams {
    pub thread_id: Option<String>,
}

/// GET /companies/{company_id}/channels/{id}/simulate - Simulation page (Protected).
#[instrument(skip(company_use_cases, channel_use_cases, thread_use_cases, config, user))]
async fn simulate_channel_page(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<SimulateQuery>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")).into_response(),
    };

    let channel = match channel_use_cases
        .get_company_channel(user.id, company_id, channel_id)
        .await
    {
        Ok(Some(wf)) => wf,
        _ => return Html(pages::error_alert("Channel not found.")).into_response(),
    };

    let mut initial_thread_id: Option<String> = None;
    let mut initial_result_html: Option<String> = None;

    if let Some(ref tid_str) = query.thread_id {
        let trimmed = tid_str.trim();
        if !trimmed.is_empty() {
            initial_thread_id = Some(trimmed.to_string());
            match Uuid::parse_str(trimmed) {
                Ok(tid) => match thread_use_cases.get_thread(tid).await {
                    Ok(Some(thread)) if thread.channel_id == channel_id => {
                        let messages = thread_use_cases
                            .get_thread_history(thread.id)
                            .await
                            .unwrap_or_default();
                        let tasks = thread_use_cases
                            .list_company_tasks(company_id, Some(channel_id), None, true)
                            .await
                            .unwrap_or_default();
                        initial_result_html =
                            Some(pages::channel_simulation_loaded_thread_fragment(
                                &company,
                                &channel,
                                &config.app_domain_name,
                                &thread,
                                &messages,
                                &tasks,
                                false,
                            ));
                    }
                    Ok(Some(_)) => {
                        initial_result_html =
                            Some(pages::channel_simulation_thread_error_fragment(
                                company_id,
                                channel_id,
                                trimmed,
                                "Thread does not belong to this channel",
                                false,
                            ));
                    }
                    Ok(None) => {
                        initial_result_html =
                            Some(pages::channel_simulation_thread_error_fragment(
                                company_id,
                                channel_id,
                                trimmed,
                                "Thread not found",
                                false,
                            ));
                    }
                    Err(err) => {
                        initial_result_html =
                            Some(pages::channel_simulation_thread_error_fragment(
                                company_id,
                                channel_id,
                                trimmed,
                                &format!("Failed to retrieve thread: {err}"),
                                false,
                            ));
                    }
                },
                Err(_) => {
                    initial_result_html = Some(pages::channel_simulation_thread_error_fragment(
                        company_id,
                        channel_id,
                        trimmed,
                        "Invalid Thread ID format (must be a valid UUID)",
                        false,
                    ));
                }
            }
        }
    }

    Html(pages::channel_simulation_page(
        &company,
        &config.app_domain_name,
        &channel,
        initial_thread_id.as_deref(),
        initial_result_html.as_deref(),
    ))
    .into_response()
}

/// POST /companies/{company_id}/channels/{id}/simulate - Submit simulation form (Protected).
#[instrument(skip(
    company_use_cases,
    channel_use_cases,
    thread_use_cases,
    config,
    user,
    form
))]
async fn simulate_channel_handler(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<SimulationForm>,
) -> impl IntoResponse {
    let mode_str = form.simulation_mode.as_deref().unwrap_or("verify");
    let mode = match mode_str.to_lowercase().as_str() {
        "run_test" => SimulationMode::RunTest,
        "run" => SimulationMode::Run,
        _ => SimulationMode::Verify,
    };

    match mode {
        SimulationMode::Verify => {
            let inbound_email = crate::use_cases::channel::InboundEmail {
                to: form.to.clone(),
                from: form.from.clone(),
                subject: form.subject.clone(),
                text_body: form.text_body.clone(),
                html_body: form.html_body.clone(),
                raw_payload: None,
            };

            match channel_use_cases
                .process_inbound_email("simulation", inbound_email, &config.app_domain_name)
                .await
            {
                Ok(result) => Html(pages::channel_simulation_result_fragment(
                    company_id, channel_id, &result,
                ))
                .into_response(),
                Err(err) => {
                    let company = company_use_cases
                        .get_company(company_id)
                        .await
                        .ok()
                        .flatten();
                    let channel = channel_use_cases
                        .get_company_channel(user.id, company_id, channel_id)
                        .await
                        .ok()
                        .flatten();

                    Html(pages::channel_simulation_failure_fragment(
                        company_id,
                        channel_id,
                        company.as_ref(),
                        channel.as_ref(),
                        &form.to,
                        &form.from,
                        form.subject.as_deref().unwrap_or("(No subject)"),
                        &format!("Simulation failed: {err}"),
                    ))
                    .into_response()
                }
            }
        }
        SimulationMode::RunTest | SimulationMode::Run => {
            let mut headers = String::new();
            if let Some(ref reply_to) = form.in_reply_to {
                let trimmed = reply_to.trim();
                if !trimmed.is_empty() {
                    headers.push_str(&format!(
                        "In-Reply-To: {}\nReferences: {}\n",
                        trimmed, trimmed
                    ));
                }
            }

            let raw_payload = RawInboundPayload {
                to: form.to.clone(),
                from: form.from.clone(),
                subject: form.subject.clone(),
                text: form.text_body.clone(),
                html: form.html_body.clone(),
                headers: if headers.is_empty() {
                    None
                } else {
                    Some(headers)
                },
                ..Default::default()
            };

            match thread_use_cases.execute_simulation(raw_payload, mode).await {
                Ok(sim_res) => {
                    let messages = if let Some(ref thread) = sim_res.ingest_result.thread {
                        thread_use_cases
                            .get_thread_history(thread.id)
                            .await
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    };

                    let tasks = thread_use_cases
                        .list_company_tasks(company_id, Some(channel_id), None, true)
                        .await
                        .unwrap_or_default();

                    let html_res = pages::channel_simulation_execution_result_fragment(
                        company_id, channel_id, &sim_res, &messages, &tasks,
                    );

                    if let Some(ref thread) = sim_res.ingest_result.thread {
                        let push_url = format!(
                            "/companies/{company_id}/channels/{channel_id}/simulate?thread_id={}",
                            thread.id
                        );
                        ([("HX-Push-Url", push_url)], Html(html_res)).into_response()
                    } else {
                        Html(html_res).into_response()
                    }
                }
                Err(err) => {
                    let company = company_use_cases
                        .get_company(company_id)
                        .await
                        .ok()
                        .flatten();
                    let channel = channel_use_cases
                        .get_company_channel(user.id, company_id, channel_id)
                        .await
                        .ok()
                        .flatten();

                    Html(pages::channel_simulation_failure_fragment(
                        company_id,
                        channel_id,
                        company.as_ref(),
                        channel.as_ref(),
                        &form.to,
                        &form.from,
                        form.subject.as_deref().unwrap_or("(No subject)"),
                        &format!("Simulation execution failed: {err}"),
                    ))
                    .into_response()
                }
            }
        }
    }
}

async fn open_simulated_thread_logic(
    company_use_cases: Arc<CompanyUseCases>,
    channel_use_cases: Arc<ChannelUseCases>,
    thread_use_cases: Arc<ThreadUseCases>,
    config: Arc<AppConfig>,
    user: AuthenticatedUser,
    company_id: Uuid,
    channel_id: Uuid,
    thread_id_input: &str,
) -> axum::response::Response {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")).into_response(),
    };

    let channel = match channel_use_cases
        .get_company_channel(user.id, company_id, channel_id)
        .await
    {
        Ok(Some(wf)) => wf,
        _ => return Html(pages::error_alert("Channel not found.")).into_response(),
    };

    let trimmed = thread_id_input.trim();
    if trimmed.is_empty() {
        return Html(pages::channel_simulation_thread_error_fragment(
            company_id,
            channel_id,
            "",
            "Thread ID cannot be empty",
            true,
        ))
        .into_response();
    }

    let tid = match Uuid::parse_str(trimmed) {
        Ok(id) => id,
        Err(_) => {
            return Html(pages::channel_simulation_thread_error_fragment(
                company_id,
                channel_id,
                trimmed,
                "Invalid Thread ID format (must be a valid UUID)",
                true,
            ))
            .into_response();
        }
    };

    match thread_use_cases.get_thread(tid).await {
        Ok(Some(thread)) => {
            if thread.channel_id != channel_id {
                return Html(pages::channel_simulation_thread_error_fragment(
                    company_id,
                    channel_id,
                    trimmed,
                    "Thread does not belong to this channel",
                    true,
                ))
                .into_response();
            }

            let messages = thread_use_cases
                .get_thread_history(thread.id)
                .await
                .unwrap_or_default();

            let tasks = thread_use_cases
                .list_company_tasks(company_id, Some(channel_id), None, true)
                .await
                .unwrap_or_default();

            let fragment_html = pages::channel_simulation_loaded_thread_fragment(
                &company,
                &channel,
                &config.app_domain_name,
                &thread,
                &messages,
                &tasks,
                true,
            );

            let push_url = format!(
                "/companies/{company_id}/channels/{channel_id}/simulate?thread_id={}",
                thread.id
            );

            ([("HX-Push-Url", push_url)], Html(fragment_html)).into_response()
        }
        Ok(None) => Html(pages::channel_simulation_thread_error_fragment(
            company_id,
            channel_id,
            trimmed,
            "Thread not found",
            true,
        ))
        .into_response(),
        Err(err) => Html(pages::channel_simulation_thread_error_fragment(
            company_id,
            channel_id,
            trimmed,
            &format!("Failed to retrieve thread: {err}"),
            true,
        ))
        .into_response(),
    }
}

#[instrument(skip(company_use_cases, channel_use_cases, thread_use_cases, config, user))]
async fn open_simulated_thread_get(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
    Query(params): Query<OpenThreadParams>,
) -> impl IntoResponse {
    let tid_str = params.thread_id.as_deref().unwrap_or("");
    open_simulated_thread_logic(
        company_use_cases,
        channel_use_cases,
        thread_use_cases,
        config,
        user,
        company_id,
        channel_id,
        tid_str,
    )
    .await
}

#[instrument(skip(company_use_cases, channel_use_cases, thread_use_cases, config, user))]
async fn open_simulated_thread_post(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
    Form(params): Form<OpenThreadParams>,
) -> impl IntoResponse {
    let tid_str = params.thread_id.as_deref().unwrap_or("");
    open_simulated_thread_logic(
        company_use_cases,
        channel_use_cases,
        thread_use_cases,
        config,
        user,
        company_id,
        channel_id,
        tid_str,
    )
    .await
}

/// JSON API: List company channels (Protected).
async fn list_channels_json(
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let channels = channel_use_cases
        .list_company_channels(user.id, company_id)
        .await?;
    Ok((StatusCode::OK, Json(channels)))
}

/// JSON API: Create company channel (Protected).
async fn create_channel_json(
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Json(payload): Json<ChannelJsonPayload>,
) -> AppResult<impl IntoResponse> {
    let confirm_spam_disabled = payload.confirm_spam_disabled.unwrap_or(false);
    let slug = payload
        .slug
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| slugify(&payload.name));

    let mut agent_ids = payload.agent_ids;

    if let Some(ref prompt) = payload.system_prompt {
        let prompt_trimmed = prompt.trim();
        if !prompt_trimmed.is_empty() {
            let agent = agent_use_cases
                .create_agent(
                    user.id,
                    company_id,
                    &payload.name,
                    &slug,
                    payload.provider.as_deref(),
                    payload.model.as_deref(),
                    payload.api_key.as_deref(),
                    Some(prompt_trimmed),
                    None,
                )
                .await?;
            let mut ids = agent_ids.unwrap_or_default();
            ids.push(agent.id);
            agent_ids = Some(ids);
        }
    }

    let channel = channel_use_cases
        .create_channel(
            user.id,
            company_id,
            &payload.name,
            &slug,
            payload.api_key.as_deref(),
            payload.provider.as_deref(),
            payload.model.as_deref(),
            payload.participant_emails,
            agent_ids,
            payload.channel_config,
            confirm_spam_disabled,
        )
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(ChannelResponse {
            success: true,
            channel,
        }),
    ))
}

/// JSON API: Get company channel details (Protected).
async fn get_channel_json(
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
) -> AppResult<impl IntoResponse> {
    let channel = channel_use_cases
        .get_company_channel(user.id, company_id, channel_id)
        .await?
        .ok_or_else(|| crate::app_error::AppError::Internal("Channel not found.".into()))?;

    Ok((StatusCode::OK, Json(channel)))
}

/// JSON API: Update company channel (Protected).
async fn update_channel_json(
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
    Json(payload): Json<ChannelJsonPayload>,
) -> AppResult<impl IntoResponse> {
    let confirm_spam_disabled = payload.confirm_spam_disabled.unwrap_or(false);
    let slug = payload
        .slug
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| slugify(&payload.name));

    let channel = channel_use_cases
        .update_channel(
            user.id,
            company_id,
            channel_id,
            &payload.name,
            &slug,
            payload.api_key.as_deref(),
            payload.provider.as_deref(),
            payload.model.as_deref(),
            payload.participant_emails,
            payload.agent_ids,
            payload.channel_config,
            confirm_spam_disabled,
        )
        .await?;

    Ok((
        StatusCode::OK,
        Json(ChannelResponse {
            success: true,
            channel,
        }),
    ))
}

/// JSON API: Delete company channel (Protected).
async fn delete_channel_json(
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
) -> AppResult<impl IntoResponse> {
    channel_use_cases
        .delete_channel(user.id, company_id, channel_id)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use crate::entities::company::Company;
    use chrono::Utc;
    use serde_json::json;

    use super::*;

    #[test]
    fn channel_pages_and_fragments_render_correctly() {
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

        let channel = Channel {
            id: Uuid::new_v4(),
            company_id: company.id,
            name: "Auto Dispatcher".to_string(),
            slug: "auto-dispatcher".to_string(),
            api_key: None,
            provider: None,
            model: None,
            participant_emails: Some(vec!["agent@test.com".to_string()]),
            agent_ids: None,
            channel_config: Some(json!({ "mode": "async" })),
            created_at: Utc::now().naive_utc(),
        };

        let page_html =
            pages::channels_page(&company, "example.com", &[channel.clone()], &[], true);
        assert!(page_html.contains("Custom Channel Agent"));
        assert!(page_html.contains("LLM Provider (Optional Override)"));
        assert!(page_html.contains("LLM Model (Optional Override)"));
        assert!(page_html.contains("LLM API Key (Optional Override)"));
        assert!(page_html.contains("Channel Config (JSON, Optional)"));

        let row_html = pages::channel_row_fragment(&company, "example.com", &channel, &[]);
        assert!(row_html.contains("Auto Dispatcher"));
        assert!(row_html.contains("auto-dispatcher@acme.example.com"));
        assert!(row_html.contains("agent@test.com"));
        assert!(row_html.contains("async"));

        let edit_html = pages::channel_edit_fragment(&company, "example.com", &channel, &[], true);
        assert!(edit_html.contains("hx-put="));
        assert!(edit_html.contains("value=\"Auto Dispatcher\""));
        assert!(edit_html.contains("Custom Channel Agent"));

        let sim_html =
            pages::channel_simulation_page(&company, "example.com", &channel, None, None);
        assert!(sim_html.contains("Simulate Webhook: Auto Dispatcher"));
        assert!(sim_html.contains("auto-dispatcher@acme.example.com"));
        assert!(sim_html.contains("value=\"verify\""));
        assert!(sim_html.contains("value=\"run_test\""));
        assert!(sim_html.contains("value=\"run\""));
        assert!(sim_html.contains("Open Existing Thread by ID"));
        assert!(sim_html.contains("Simulated Webhook Payload"));

        let sim_html_with_thread = pages::channel_simulation_page(
            &company,
            "example.com",
            &channel,
            Some("0f5421b8-9e78-4f21-ac52-3af494c3f344"),
            None,
        );
        assert!(sim_html_with_thread.contains("Thread Loaded & Active"));
        assert!(sim_html_with_thread.contains("0f5421b8-9e78-4f21-ac52-3af494c3f344"));
        assert!(!sim_html_with_thread.contains("Simulated Webhook Payload"));

        let sim_result = crate::use_cases::channel::InboundEmailResult {
            resolved: true,
            sender_authorized: true,
            company_slug: Some("acme".to_string()),
            channel_slug: Some("auto-dispatcher".to_string()),
            company: Some(company.clone()),
            channel: Some(channel.clone()),
            email: crate::use_cases::channel::InboundEmail {
                to: "auto-dispatcher@acme.example.com".to_string(),
                from: "agent@test.com".to_string(),
                subject: Some("Test".to_string()),
                text_body: Some("Body text".to_string()),
                html_body: None,
                raw_payload: None,
            },
        };
        let sim_result_html =
            pages::channel_simulation_result_fragment(company.id, channel.id, &sim_result);
        assert!(sim_result_html.contains("Webhook Triggered & Channel Resolved Successfully!"));

        let full_sim_res = crate::use_cases::thread::SimulationExecutionResult {
            ingest_result: crate::use_cases::thread::InboundIngestResult {
                accepted: true,
                reason: None,
                thread: None,
                inbound_message: None,
                company: Some(company.clone()),
                channel: Some(channel.clone()),
                normalized_message: None,
                channel_matches: vec![],
                bounce_info: None,
                parsed_email: Some(crate::services::email_parser::ParsedEmail {
                    message_id: "<msg1@test>".to_string(),
                    in_reply_to: None,
                    references: vec![],
                    thread_index: None,
                    sender: "agent@test.com".to_string(),
                    recipients_to: vec!["auto-dispatcher@acme.example.com".to_string()],
                    recipients_cc: vec![],
                    subject: "Test".to_string(),
                    clean_text_body: "Body text".to_string(),
                    raw_text_body: None,
                    raw_html_body: None,
                    attachments: vec![],
                    prompt_text: "Body text".to_string(),
                    is_auto_reply: false,
                    is_forwarded: false,
                    channel_id_header: None,
                    hop_count: 0,
                    trace_channels: vec![],
                    spf_status: Some("pass".to_string()),
                    dkim_status: Some("pass".to_string()),
                    dmarc_status: Some("pass".to_string()),
                    spam_score: None,
                    is_context_only: false,
                }),
                task_id: None,
            },
            agent_execution: Some(crate::use_cases::thread::AgentExecutionResult {
                outbound_message_id: Some("<out1@test>".to_string()),
                agent_response: "Hello from Agent".to_string(),
                email_sent: false,
                token_usage: Some(crate::entities::task::TokenUsage::new(10, 5)),
            }),
            simulation_mode: crate::use_cases::thread::SimulationMode::RunTest,
        };
        let test_message = crate::entities::message::Message {
            id: Uuid::new_v4(),
            thread_id: Uuid::new_v4(),
            message_id: "<msg1@test>".to_string(),
            in_reply_to: None,
            references_list: vec![],
            sender: "agent@test.com".to_string(),
            recipients_to: vec!["auto-dispatcher@acme.example.com".to_string()],
            recipients_cc: vec![],
            subject: "Test".to_string(),
            clean_text_body: "Body text".to_string(),
            raw_text_body: None,
            raw_html_body: None,
            attachments: None,
            direction: crate::entities::message::MessageDirection::Inbound,
            role: crate::entities::message::MessageRole::Human,
            thread_index: None,
            created_at: Utc::now().naive_utc(),
        };

        let run_test_html = pages::channel_simulation_execution_result_fragment(
            company.id,
            channel.id,
            &full_sim_res,
            &[test_message.clone()],
            &[],
        );
        assert!(run_test_html.contains("Run_Test"));
        assert!(run_test_html.contains("Skipped (Run_Test Dry-Run)"));
        assert!(run_test_html.contains("Hello from Agent"));
        assert!(run_test_html.contains("Task Execution Parameters"));
        assert!(run_test_html.contains("LLM Provider:"));
        assert!(run_test_html.contains("LLM Model:"));
        assert!(run_test_html.contains("API Key Status:"));
        assert!(run_test_html.contains("hx-swap-oob=\"outerHTML\""));
        assert!(run_test_html.contains("Simulate New Thread"));
        assert!(run_test_html.contains("Simulate Reply Webhook Call"));
        assert!(run_test_html.contains("value=\"<msg1@test>\""));
        assert!(run_test_html.contains("Thread History"));

        let fail_html = pages::channel_simulation_failure_fragment(
            company.id,
            channel.id,
            Some(&company),
            Some(&channel),
            "test@recip.com",
            "sender@test.com",
            "Test Subject",
            "Anthropic API key is missing",
        );
        assert!(fail_html.contains("Simulation Execution Error"));
        assert!(fail_html.contains("Anthropic API key is missing"));
        assert!(fail_html.contains("LLM Provider:"));
        assert!(fail_html.contains("LLM Model:"));
        assert!(fail_html.contains("API Key Status:"));
        assert!(fail_html.contains("Simulate New Thread"));

        let sample_thread = crate::entities::thread::Thread {
            id: Uuid::new_v4(),
            channel_id: channel.id,
            subject: "Existing Thread Subject".to_string(),
            participant_emails: vec!["user@test.com".to_string()],
            created_at: Utc::now().naive_utc(),
            updated_at: Utc::now().naive_utc(),
        };

        let loaded_thread_html = pages::channel_simulation_loaded_thread_fragment(
            &company,
            &channel,
            "example.com",
            &sample_thread,
            &[test_message],
            &[],
            true,
        );
        assert!(loaded_thread_html.contains("Thread Loaded & Active"));
        assert!(loaded_thread_html.contains("Existing Thread Subject"));
        assert!(loaded_thread_html.contains("Task Execution Parameters"));
        assert!(loaded_thread_html.contains("Simulate Reply Webhook Call"));

        let error_thread_html = pages::channel_simulation_thread_error_fragment(
            company.id,
            channel.id,
            "invalid-uuid",
            "Thread not found",
            true,
        );
        assert!(error_thread_html.contains("Failed to Load Thread"));
        assert!(error_thread_html.contains("Thread not found"));
    }

    #[tokio::test]
    async fn test_slugify() {
        assert_eq!(slugify("Support Team"), "support-team");
        assert_eq!(
            slugify("  Inbound Email Handler!  "),
            "inbound-email-handler"
        );
        assert_eq!(slugify("Customer_Service 101"), "customer-service-101");
        assert_eq!(slugify("---"), "");
    }

    #[tokio::test]
    async fn test_channel_form_deserialization() {
        use axum::body::Body;
        use axum::extract::FromRequest;
        use axum::http::{Request, header};

        let req_simple = Request::builder()
            .method("POST")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(
                "name=Inbound+Support&system_prompt=You+are+a+support+agent.&form_mode=simple",
            ))
            .unwrap();
        let form_simple = Form::<ChannelForm>::from_request(req_simple, &())
            .await
            .unwrap()
            .0;
        assert_eq!(form_simple.name, "Inbound Support");
        assert_eq!(form_simple.slug, None);
        assert_eq!(
            form_simple.system_prompt,
            Some("You are a support agent.".to_string())
        );
        assert_eq!(slugify(&form_simple.name), "inbound-support");

        let req_omitted = Request::builder()
            .method("PUT")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from("name=My+Channel&slug=my-channel&agent_ids="))
            .unwrap();
        let form_omitted = Form::<ChannelForm>::from_request(req_omitted, &())
            .await
            .unwrap()
            .0;
        assert_eq!(parse_agent_ids_form(form_omitted.agent_ids), None);

        let req_single = Request::builder()
            .method("PUT")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(
                "name=My+Channel&slug=my-channel&agent_ids=00000000-0000-0000-0000-000000000001",
            ))
            .unwrap();
        let form_single = Form::<ChannelForm>::from_request(req_single, &())
            .await
            .unwrap()
            .0;
        assert_eq!(
            parse_agent_ids_form(form_single.agent_ids),
            Some(vec![
                Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap()
            ])
        );

        let req_multiple = Request::builder()
            .method("PUT")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from("name=My+Channel&slug=my-channel&agent_ids=00000000-0000-0000-0000-000000000001%2C00000000-0000-0000-0000-000000000002"))
            .unwrap();
        let form_multiple = Form::<ChannelForm>::from_request(req_multiple, &())
            .await
            .unwrap()
            .0;
        assert_eq!(
            parse_agent_ids_form(form_multiple.agent_ids),
            Some(vec![
                Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
                Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap(),
            ])
        );
    }
}
