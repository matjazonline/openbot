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
    entities::workflow::Workflow,
    infra::config::AppConfig,
    services::email_parser::RawInboundPayload,
    use_cases::{
        agent::AgentUseCases,
        company::CompanyUseCases,
        thread::{SimulationMode, ThreadUseCases},
        workflow::WorkflowUseCases,
    },
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/companies/{company_id}/workflows",
            get(list_workflows_page).post(create_workflow_handler),
        )
        .route(
            "/companies/{company_id}/workflows/{id}",
            put(update_workflow_handler).delete(delete_workflow_handler),
        )
        .route(
            "/companies/{company_id}/workflows/{id}/edit",
            get(edit_workflow_form),
        )
        .route(
            "/companies/{company_id}/workflows/{id}/cancel",
            get(cancel_workflow_edit),
        )
        .route(
            "/companies/{company_id}/workflows/{id}/simulate",
            get(simulate_workflow_page).post(simulate_workflow_handler),
        )
        .route(
            "/companies/{company_id}/workflows/{id}/simulate/thread",
            get(open_simulated_thread_get).post(open_simulated_thread_post),
        )
        .route(
            "/api/companies/{company_id}/workflows",
            get(list_workflows_json).post(create_workflow_json),
        )
        .route(
            "/api/companies/{company_id}/workflows/{id}",
            get(get_workflow_json)
                .put(update_workflow_json)
                .delete(delete_workflow_json),
        )
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowForm {
    pub name: String,
    pub slug: String,
    pub api_key: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub participant_emails: Option<String>,
    pub agent_ids: Option<String>,
    pub workflow_config: Option<String>,
}

fn parse_agent_ids_form(input: Option<String>) -> Option<Vec<Uuid>> {
    input.and_then(|s| {
        let list: Vec<Uuid> = s
            .split(&[',', ' ', ';', '\n'][..])
            .map(|e| e.trim())
            .filter(|e| !e.is_empty())
            .filter_map(|e| Uuid::parse_str(e).ok())
            .collect();
        if list.is_empty() {
            None
        } else {
            Some(list)
        }
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowJsonPayload {
    pub name: String,
    pub slug: String,
    pub api_key: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub participant_emails: Option<Vec<String>>,
    pub agent_ids: Option<Vec<Uuid>>,
    pub workflow_config: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowResponse {
    pub success: bool,
    pub workflow: Workflow,
}

fn parse_emails_form(input: Option<String>) -> Option<Vec<String>> {
    input.and_then(|s| {
        let list: Vec<String> = s
            .split(&[',', '\n', ';'][..])
            .map(|e| e.trim().to_string())
            .filter(|e| !e.is_empty())
            .collect();
        if list.is_empty() {
            None
        } else {
            Some(list)
        }
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

/// GET /companies/{company_id}/workflows - Full HTML page listing workflows (Protected).
#[instrument(skip(company_use_cases, workflow_use_cases, agent_use_cases, config, user))]
async fn list_workflows_page(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(workflow_use_cases): State<Arc<WorkflowUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")),
    };

    let workflows = workflow_use_cases
        .list_company_workflows(user.id, company_id)
        .await
        .unwrap_or_default();

    let agents = agent_use_cases
        .list_company_agents(user.id, company_id)
        .await
        .unwrap_or_default();

    Html(pages::workflows_page(&company, &config.app_domain_name, &workflows, &agents))
}

/// POST /companies/{company_id}/workflows - HTMX create workflow (Protected).
#[instrument(skip(company_use_cases, workflow_use_cases, agent_use_cases, config, user, form))]
async fn create_workflow_handler(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(workflow_use_cases): State<Arc<WorkflowUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Form(form): Form<WorkflowForm>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")),
    };

    let agents = agent_use_cases
        .list_company_agents(user.id, company_id)
        .await
        .unwrap_or_default();

    let emails = parse_emails_form(form.participant_emails);
    let agent_ids = parse_agent_ids_form(form.agent_ids);
    let workflow_config = match parse_config_form(form.workflow_config) {
        Ok(c) => c,
        Err(err) => {
            let error_html = pages::error_alert(&err);
            let workflows = workflow_use_cases
                .list_company_workflows(user.id, company_id)
                .await
                .unwrap_or_default();
            return Html(format!(
                "{}{}",
                error_html,
                pages::workflow_list_fragment(&company, &config.app_domain_name, &workflows, &agents)
            ));
        }
    };

    match workflow_use_cases
        .create_workflow(
            user.id,
            company_id,
            &form.name,
            &form.slug,
            form.api_key.as_deref(),
            form.provider.as_deref(),
            form.model.as_deref(),
            emails,
            agent_ids,
            workflow_config,
        )
        .await
    {
        Ok(_) => {
            let workflows = workflow_use_cases
                .list_company_workflows(user.id, company_id)
                .await
                .unwrap_or_default();
            Html(pages::workflow_list_fragment(
                &company,
                &config.app_domain_name,
                &workflows,
                &agents,
            ))
        }
        Err(err) => {
            let error_html = pages::error_alert(&format!("Failed to create workflow: {err}"));
            let workflows = workflow_use_cases
                .list_company_workflows(user.id, company_id)
                .await
                .unwrap_or_default();
            Html(format!(
                "{}{}",
                error_html,
                pages::workflow_list_fragment(&company, &config.app_domain_name, &workflows, &agents)
            ))
        }
    }
}

/// GET /companies/{company_id}/workflows/{id}/edit - HTMX edit workflow form fragment (Protected).
#[instrument(skip(company_use_cases, workflow_use_cases, agent_use_cases, config, user))]
async fn edit_workflow_form(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(workflow_use_cases): State<Arc<WorkflowUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, workflow_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")),
    };

    let agents = agent_use_cases
        .list_company_agents(user.id, company_id)
        .await
        .unwrap_or_default();

    if let Ok(Some(wf)) = workflow_use_cases
        .get_company_workflow(user.id, company_id, workflow_id)
        .await
    {
        Html(pages::workflow_edit_fragment(
            &company,
            &config.app_domain_name,
            &wf,
            &agents,
        ))
    } else {
        Html(pages::error_alert("Workflow not found."))
    }
}

/// GET /companies/{company_id}/workflows/{id}/cancel - Cancel workflow edit fragment (Protected).
#[instrument(skip(company_use_cases, workflow_use_cases, agent_use_cases, config, user))]
async fn cancel_workflow_edit(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(workflow_use_cases): State<Arc<WorkflowUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, workflow_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")),
    };

    let agents = agent_use_cases
        .list_company_agents(user.id, company_id)
        .await
        .unwrap_or_default();

    if let Ok(Some(wf)) = workflow_use_cases
        .get_company_workflow(user.id, company_id, workflow_id)
        .await
    {
        Html(pages::workflow_row_fragment(
            &company,
            &config.app_domain_name,
            &wf,
            &agents,
        ))
    } else {
        Html(String::new())
    }
}

/// PUT /companies/{company_id}/workflows/{id} - HTMX update workflow (Protected).
#[instrument(skip(company_use_cases, workflow_use_cases, agent_use_cases, config, user, form))]
async fn update_workflow_handler(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(workflow_use_cases): State<Arc<WorkflowUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, workflow_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<WorkflowForm>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")),
    };

    let agents = agent_use_cases
        .list_company_agents(user.id, company_id)
        .await
        .unwrap_or_default();

    let emails = parse_emails_form(form.participant_emails);
    let agent_ids = parse_agent_ids_form(form.agent_ids);
    let workflow_config = match parse_config_form(form.workflow_config) {
        Ok(c) => c,
        Err(err) => return Html(pages::error_alert(&err)),
    };

    match workflow_use_cases
        .update_workflow(
            user.id,
            company_id,
            workflow_id,
            &form.name,
            &form.slug,
            form.api_key.as_deref(),
            form.provider.as_deref(),
            form.model.as_deref(),
            emails,
            agent_ids,
            workflow_config,
        )
        .await
    {
        Ok(wf) => Html(pages::workflow_row_fragment(
            &company,
            &config.app_domain_name,
            &wf,
            &agents,
        )),
        Err(err) => Html(pages::error_alert(&format!("Update failed: {err}"))),
    }
}

/// DELETE /companies/{company_id}/workflows/{id} - HTMX delete workflow (Protected).
#[instrument(skip(workflow_use_cases, user))]
async fn delete_workflow_handler(
    State(workflow_use_cases): State<Arc<WorkflowUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, workflow_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    let _ = workflow_use_cases
        .delete_workflow(user.id, company_id, workflow_id)
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

/// GET /companies/{company_id}/workflows/{id}/simulate - Simulation page (Protected).
#[instrument(skip(company_use_cases, workflow_use_cases, thread_use_cases, config, user))]
async fn simulate_workflow_page(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(workflow_use_cases): State<Arc<WorkflowUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, workflow_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<SimulateQuery>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")).into_response(),
    };

    let workflow = match workflow_use_cases
        .get_company_workflow(user.id, company_id, workflow_id)
        .await
    {
        Ok(Some(wf)) => wf,
        _ => return Html(pages::error_alert("Workflow not found.")).into_response(),
    };

    let mut initial_thread_id: Option<String> = None;
    let mut initial_result_html: Option<String> = None;

    if let Some(ref tid_str) = query.thread_id {
        let trimmed = tid_str.trim();
        if !trimmed.is_empty() {
            initial_thread_id = Some(trimmed.to_string());
            match Uuid::parse_str(trimmed) {
                Ok(tid) => match thread_use_cases.get_thread(tid).await {
                    Ok(Some(thread)) if thread.workflow_id == workflow_id => {
                        let messages = thread_use_cases
                            .get_thread_history(thread.id)
                            .await
                            .unwrap_or_default();
                        let tasks = thread_use_cases
                            .list_company_tasks(company_id, Some(workflow_id), None, true)
                            .await
                            .unwrap_or_default();
                        initial_result_html = Some(pages::workflow_simulation_loaded_thread_fragment(
                            &company,
                            &workflow,
                            &config.app_domain_name,
                            &thread,
                            &messages,
                            &tasks,
                            false,
                        ));
                    }
                    Ok(Some(_)) => {
                        initial_result_html = Some(pages::workflow_simulation_thread_error_fragment(
                            company_id,
                            workflow_id,
                            trimmed,
                            "Thread does not belong to this workflow",
                            false,
                        ));
                    }
                    Ok(None) => {
                        initial_result_html = Some(pages::workflow_simulation_thread_error_fragment(
                            company_id,
                            workflow_id,
                            trimmed,
                            "Thread not found",
                            false,
                        ));
                    }
                    Err(err) => {
                        initial_result_html = Some(pages::workflow_simulation_thread_error_fragment(
                            company_id,
                            workflow_id,
                            trimmed,
                            &format!("Failed to retrieve thread: {err}"),
                            false,
                        ));
                    }
                },
                Err(_) => {
                    initial_result_html = Some(pages::workflow_simulation_thread_error_fragment(
                        company_id,
                        workflow_id,
                        trimmed,
                        "Invalid Thread ID format (must be a valid UUID)",
                        false,
                    ));
                }
            }
        }
    }

    Html(pages::workflow_simulation_page(
        &company,
        &config.app_domain_name,
        &workflow,
        initial_thread_id.as_deref(),
        initial_result_html.as_deref(),
    )).into_response()
}

/// POST /companies/{company_id}/workflows/{id}/simulate - Submit simulation form (Protected).
#[instrument(skip(company_use_cases, workflow_use_cases, thread_use_cases, config, user, form))]
async fn simulate_workflow_handler(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(workflow_use_cases): State<Arc<WorkflowUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, workflow_id)): Path<(Uuid, Uuid)>,
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
            let inbound_email = crate::use_cases::workflow::InboundEmail {
                to: form.to.clone(),
                from: form.from.clone(),
                subject: form.subject.clone(),
                text_body: form.text_body.clone(),
                html_body: form.html_body.clone(),
                raw_payload: None,
            };

            match workflow_use_cases
                .process_inbound_email("simulation", inbound_email, &config.app_domain_name)
                .await
            {
                Ok(result) => Html(pages::workflow_simulation_result_fragment(
                    company_id,
                    workflow_id,
                    &result,
                )).into_response(),
                Err(err) => {
                    let company = company_use_cases.get_company(company_id).await.ok().flatten();
                    let workflow = workflow_use_cases
                        .get_company_workflow(user.id, company_id, workflow_id)
                        .await
                        .ok()
                        .flatten();

                    Html(pages::workflow_simulation_failure_fragment(
                        company_id,
                        workflow_id,
                        company.as_ref(),
                        workflow.as_ref(),
                        &form.to,
                        &form.from,
                        form.subject.as_deref().unwrap_or("(No subject)"),
                        &format!("Simulation failed: {err}"),
                    )).into_response()
                }
            }
        }
        SimulationMode::RunTest | SimulationMode::Run => {
            let mut headers = String::new();
            if let Some(ref reply_to) = form.in_reply_to {
                let trimmed = reply_to.trim();
                if !trimmed.is_empty() {
                    headers.push_str(&format!("In-Reply-To: {}\nReferences: {}\n", trimmed, trimmed));
                }
            }

            let raw_payload = RawInboundPayload {
                to: form.to.clone(),
                from: form.from.clone(),
                subject: form.subject.clone(),
                text: form.text_body.clone(),
                html: form.html_body.clone(),
                headers: if headers.is_empty() { None } else { Some(headers) },
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
                        .list_company_tasks(company_id, Some(workflow_id), None, true)
                        .await
                        .unwrap_or_default();

                    let html_res = pages::workflow_simulation_execution_result_fragment(
                        company_id,
                        workflow_id,
                        &sim_res,
                        &messages,
                        &tasks,
                    );

                    if let Some(ref thread) = sim_res.ingest_result.thread {
                        let push_url = format!(
                            "/companies/{company_id}/workflows/{workflow_id}/simulate?thread_id={}",
                            thread.id
                        );
                        ([("HX-Push-Url", push_url)], Html(html_res)).into_response()
                    } else {
                        Html(html_res).into_response()
                    }
                }
                Err(err) => {
                    let company = company_use_cases.get_company(company_id).await.ok().flatten();
                    let workflow = workflow_use_cases
                        .get_company_workflow(user.id, company_id, workflow_id)
                        .await
                        .ok()
                        .flatten();

                    Html(pages::workflow_simulation_failure_fragment(
                        company_id,
                        workflow_id,
                        company.as_ref(),
                        workflow.as_ref(),
                        &form.to,
                        &form.from,
                        form.subject.as_deref().unwrap_or("(No subject)"),
                        &format!("Simulation execution failed: {err}"),
                    )).into_response()
                }
            }
        }
    }
}

async fn open_simulated_thread_logic(
    company_use_cases: Arc<CompanyUseCases>,
    workflow_use_cases: Arc<WorkflowUseCases>,
    thread_use_cases: Arc<ThreadUseCases>,
    config: Arc<AppConfig>,
    user: AuthenticatedUser,
    company_id: Uuid,
    workflow_id: Uuid,
    thread_id_input: &str,
) -> axum::response::Response {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")).into_response(),
    };

    let workflow = match workflow_use_cases
        .get_company_workflow(user.id, company_id, workflow_id)
        .await
    {
        Ok(Some(wf)) => wf,
        _ => return Html(pages::error_alert("Workflow not found.")).into_response(),
    };

    let trimmed = thread_id_input.trim();
    if trimmed.is_empty() {
        return Html(pages::workflow_simulation_thread_error_fragment(
            company_id,
            workflow_id,
            "",
            "Thread ID cannot be empty",
            true,
        )).into_response();
    }

    let tid = match Uuid::parse_str(trimmed) {
        Ok(id) => id,
        Err(_) => {
            return Html(pages::workflow_simulation_thread_error_fragment(
                company_id,
                workflow_id,
                trimmed,
                "Invalid Thread ID format (must be a valid UUID)",
                true,
            )).into_response();
        }
    };

    match thread_use_cases.get_thread(tid).await {
        Ok(Some(thread)) => {
            if thread.workflow_id != workflow_id {
                return Html(pages::workflow_simulation_thread_error_fragment(
                    company_id,
                    workflow_id,
                    trimmed,
                    "Thread does not belong to this workflow",
                    true,
                )).into_response();
            }

            let messages = thread_use_cases
                .get_thread_history(thread.id)
                .await
                .unwrap_or_default();

            let tasks = thread_use_cases
                .list_company_tasks(company_id, Some(workflow_id), None, true)
                .await
                .unwrap_or_default();

            let fragment_html = pages::workflow_simulation_loaded_thread_fragment(
                &company,
                &workflow,
                &config.app_domain_name,
                &thread,
                &messages,
                &tasks,
                true,
            );

            let push_url = format!(
                "/companies/{company_id}/workflows/{workflow_id}/simulate?thread_id={}",
                thread.id
            );

            ([("HX-Push-Url", push_url)], Html(fragment_html)).into_response()
        }
        Ok(None) => Html(pages::workflow_simulation_thread_error_fragment(
            company_id,
            workflow_id,
            trimmed,
            "Thread not found",
            true,
        )).into_response(),
        Err(err) => Html(pages::workflow_simulation_thread_error_fragment(
            company_id,
            workflow_id,
            trimmed,
            &format!("Failed to retrieve thread: {err}"),
            true,
        )).into_response(),
    }
}

#[instrument(skip(company_use_cases, workflow_use_cases, thread_use_cases, config, user))]
async fn open_simulated_thread_get(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(workflow_use_cases): State<Arc<WorkflowUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, workflow_id)): Path<(Uuid, Uuid)>,
    Query(params): Query<OpenThreadParams>,
) -> impl IntoResponse {
    let tid_str = params.thread_id.as_deref().unwrap_or("");
    open_simulated_thread_logic(
        company_use_cases,
        workflow_use_cases,
        thread_use_cases,
        config,
        user,
        company_id,
        workflow_id,
        tid_str,
    ).await
}

#[instrument(skip(company_use_cases, workflow_use_cases, thread_use_cases, config, user))]
async fn open_simulated_thread_post(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(workflow_use_cases): State<Arc<WorkflowUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, workflow_id)): Path<(Uuid, Uuid)>,
    Form(params): Form<OpenThreadParams>,
) -> impl IntoResponse {
    let tid_str = params.thread_id.as_deref().unwrap_or("");
    open_simulated_thread_logic(
        company_use_cases,
        workflow_use_cases,
        thread_use_cases,
        config,
        user,
        company_id,
        workflow_id,
        tid_str,
    ).await
}

/// JSON API: List company workflows (Protected).
async fn list_workflows_json(
    State(workflow_use_cases): State<Arc<WorkflowUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let workflows = workflow_use_cases
        .list_company_workflows(user.id, company_id)
        .await?;
    Ok((StatusCode::OK, Json(workflows)))
}

/// JSON API: Create company workflow (Protected).
async fn create_workflow_json(
    State(workflow_use_cases): State<Arc<WorkflowUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Json(payload): Json<WorkflowJsonPayload>,
) -> AppResult<impl IntoResponse> {
    let workflow = workflow_use_cases
        .create_workflow(
            user.id,
            company_id,
            &payload.name,
            &payload.slug,
            payload.api_key.as_deref(),
            payload.provider.as_deref(),
            payload.model.as_deref(),
            payload.participant_emails,
            payload.agent_ids,
            payload.workflow_config,
        )
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(WorkflowResponse {
            success: true,
            workflow,
        }),
    ))
}

/// JSON API: Get company workflow details (Protected).
async fn get_workflow_json(
    State(workflow_use_cases): State<Arc<WorkflowUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, workflow_id)): Path<(Uuid, Uuid)>,
) -> AppResult<impl IntoResponse> {
    let workflow = workflow_use_cases
        .get_company_workflow(user.id, company_id, workflow_id)
        .await?
        .ok_or_else(|| crate::app_error::AppError::Internal("Workflow not found.".into()))?;

    Ok((StatusCode::OK, Json(workflow)))
}

/// JSON API: Update company workflow (Protected).
async fn update_workflow_json(
    State(workflow_use_cases): State<Arc<WorkflowUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, workflow_id)): Path<(Uuid, Uuid)>,
    Json(payload): Json<WorkflowJsonPayload>,
) -> AppResult<impl IntoResponse> {
    let workflow = workflow_use_cases
        .update_workflow(
            user.id,
            company_id,
            workflow_id,
            &payload.name,
            &payload.slug,
            payload.api_key.as_deref(),
            payload.provider.as_deref(),
            payload.model.as_deref(),
            payload.participant_emails,
            payload.agent_ids,
            payload.workflow_config,
        )
        .await?;

    Ok((
        StatusCode::OK,
        Json(WorkflowResponse {
            success: true,
            workflow,
        }),
    ))
}

/// JSON API: Delete company workflow (Protected).
async fn delete_workflow_json(
    State(workflow_use_cases): State<Arc<WorkflowUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, workflow_id)): Path<(Uuid, Uuid)>,
) -> AppResult<impl IntoResponse> {
    workflow_use_cases
        .delete_workflow(user.id, company_id, workflow_id)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;
    use crate::entities::company::Company;

    use super::*;

    #[test]
    fn workflow_pages_and_fragments_render_correctly() {
        let company = Company {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            slug: "acme".to_string(),
            api_key: None,
            provider: None,
            model: None,
            created_at: Utc::now().naive_utc(),
        };

        let workflow = Workflow {
            id: Uuid::new_v4(),
            company_id: company.id,
            name: "Auto Dispatcher".to_string(),
            slug: "auto-dispatcher".to_string(),
            api_key: None,
            provider: None,
            model: None,
            participant_emails: Some(vec!["agent@test.com".to_string()]),
            agent_ids: None,
            workflow_config: Some(json!({ "mode": "async" })),
            created_at: Utc::now().naive_utc(),
        };

        let row_html = pages::workflow_row_fragment(&company, "example.com", &workflow, &[]);
        assert!(row_html.contains("Auto Dispatcher"));
        assert!(row_html.contains("auto-dispatcher@acme.example.com"));
        assert!(row_html.contains("agent@test.com"));
        assert!(row_html.contains("async"));

        let edit_html = pages::workflow_edit_fragment(&company, "example.com", &workflow, &[]);
        assert!(edit_html.contains("hx-put="));
        assert!(edit_html.contains("value=\"Auto Dispatcher\""));

        let sim_html = pages::workflow_simulation_page(&company, "example.com", &workflow, None, None);
        assert!(sim_html.contains("Simulate Webhook: Auto Dispatcher"));
        assert!(sim_html.contains("auto-dispatcher@acme.example.com"));
        assert!(sim_html.contains("value=\"verify\""));
        assert!(sim_html.contains("value=\"run_test\""));
        assert!(sim_html.contains("value=\"run\""));
        assert!(sim_html.contains("Open Existing Thread by ID"));
        assert!(sim_html.contains("Simulated Webhook Payload"));

        let sim_html_with_thread = pages::workflow_simulation_page(&company, "example.com", &workflow, Some("0f5421b8-9e78-4f21-ac52-3af494c3f344"), None);
        assert!(sim_html_with_thread.contains("Thread Loaded & Active"));
        assert!(sim_html_with_thread.contains("0f5421b8-9e78-4f21-ac52-3af494c3f344"));
        assert!(!sim_html_with_thread.contains("Simulated Webhook Payload"));

        let sim_result = crate::use_cases::workflow::InboundEmailResult {
            resolved: true,
            sender_authorized: true,
            company_slug: Some("acme".to_string()),
            workflow_slug: Some("auto-dispatcher".to_string()),
            company: Some(company.clone()),
            workflow: Some(workflow.clone()),
            email: crate::use_cases::workflow::InboundEmail {
                to: "auto-dispatcher@acme.example.com".to_string(),
                from: "agent@test.com".to_string(),
                subject: Some("Test".to_string()),
                text_body: Some("Body text".to_string()),
                html_body: None,
                raw_payload: None,
            },
        };
        let sim_result_html = pages::workflow_simulation_result_fragment(company.id, workflow.id, &sim_result);
        assert!(sim_result_html.contains("Webhook Triggered & Workflow Resolved Successfully!"));

        let full_sim_res = crate::use_cases::thread::SimulationExecutionResult {
            ingest_result: crate::use_cases::thread::InboundIngestResult {
                accepted: true,
                reason: None,
                thread: None,
                inbound_message: None,
                company: Some(company.clone()),
                workflow: Some(workflow.clone()),
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
                    workflow_id_header: None,
                    hop_count: 0,
                    trace_workflows: vec![],
                    spf_status: Some("pass".to_string()),
                    dkim_status: Some("pass".to_string()),
                    spam_score: None,
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

        let run_test_html = pages::workflow_simulation_execution_result_fragment(company.id, workflow.id, &full_sim_res, &[test_message.clone()], &[]);
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

        let fail_html = pages::workflow_simulation_failure_fragment(
            company.id,
            workflow.id,
            Some(&company),
            Some(&workflow),
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
            workflow_id: workflow.id,
            subject: "Existing Thread Subject".to_string(),
            participant_emails: vec!["user@test.com".to_string()],
            created_at: Utc::now().naive_utc(),
            updated_at: Utc::now().naive_utc(),
        };

        let loaded_thread_html = pages::workflow_simulation_loaded_thread_fragment(
            &company,
            &workflow,
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

        let error_thread_html = pages::workflow_simulation_thread_error_fragment(
            company.id,
            workflow.id,
            "invalid-uuid",
            "Thread not found",
            true,
        );
        assert!(error_thread_html.contains("Failed to Load Thread"));
        assert!(error_thread_html.contains("Thread not found"));
    }

    #[tokio::test]
    async fn test_workflow_form_deserialization() {
        use axum::extract::FromRequest;
        use axum::http::{header, Request};
        use axum::body::Body;

        let req_omitted = Request::builder()
            .method("PUT")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from("name=My+Workflow&slug=my-workflow&agent_ids="))
            .unwrap();
        let form_omitted = Form::<WorkflowForm>::from_request(req_omitted, &()).await.unwrap().0;
        assert_eq!(parse_agent_ids_form(form_omitted.agent_ids), None);

        let req_single = Request::builder()
            .method("PUT")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from("name=My+Workflow&slug=my-workflow&agent_ids=00000000-0000-0000-0000-000000000001"))
            .unwrap();
        let form_single = Form::<WorkflowForm>::from_request(req_single, &()).await.unwrap().0;
        assert_eq!(parse_agent_ids_form(form_single.agent_ids), Some(vec![Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap()]));

        let req_multiple = Request::builder()
            .method("PUT")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from("name=My+Workflow&slug=my-workflow&agent_ids=00000000-0000-0000-0000-000000000001%2C00000000-0000-0000-0000-000000000002"))
            .unwrap();
        let form_multiple = Form::<WorkflowForm>::from_request(req_multiple, &()).await.unwrap().0;
        assert_eq!(parse_agent_ids_form(form_multiple.agent_ids), Some(vec![
            Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
            Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap(),
        ]));
    }
}
