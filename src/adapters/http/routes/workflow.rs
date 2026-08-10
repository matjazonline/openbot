use std::sync::Arc;

use axum::{
    Form, Json, Router,
    extract::{Path, State},
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
    pub workflow_config: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowJsonPayload {
    pub name: String,
    pub slug: String,
    pub api_key: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub participant_emails: Option<Vec<String>>,
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
#[instrument(skip(company_use_cases, workflow_use_cases, config, user))]
async fn list_workflows_page(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(workflow_use_cases): State<Arc<WorkflowUseCases>>,
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

    Html(pages::workflows_page(&company, &config.app_domain_name, &workflows))
}

/// POST /companies/{company_id}/workflows - HTMX create workflow (Protected).
#[instrument(skip(company_use_cases, workflow_use_cases, config, user, form))]
async fn create_workflow_handler(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(workflow_use_cases): State<Arc<WorkflowUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Form(form): Form<WorkflowForm>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")),
    };

    let emails = parse_emails_form(form.participant_emails);
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
                pages::workflow_list_fragment(&company, &config.app_domain_name, &workflows)
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
                pages::workflow_list_fragment(&company, &config.app_domain_name, &workflows)
            ))
        }
    }
}

/// GET /companies/{company_id}/workflows/{id}/edit - HTMX edit workflow form fragment (Protected).
#[instrument(skip(company_use_cases, workflow_use_cases, config, user))]
async fn edit_workflow_form(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(workflow_use_cases): State<Arc<WorkflowUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, workflow_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")),
    };

    if let Ok(Some(wf)) = workflow_use_cases
        .get_company_workflow(user.id, company_id, workflow_id)
        .await
    {
        Html(pages::workflow_edit_fragment(
            &company,
            &config.app_domain_name,
            &wf,
        ))
    } else {
        Html(pages::error_alert("Workflow not found."))
    }
}

/// GET /companies/{company_id}/workflows/{id}/cancel - Cancel workflow edit fragment (Protected).
#[instrument(skip(company_use_cases, workflow_use_cases, config, user))]
async fn cancel_workflow_edit(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(workflow_use_cases): State<Arc<WorkflowUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, workflow_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")),
    };

    if let Ok(Some(wf)) = workflow_use_cases
        .get_company_workflow(user.id, company_id, workflow_id)
        .await
    {
        Html(pages::workflow_row_fragment(
            &company,
            &config.app_domain_name,
            &wf,
        ))
    } else {
        Html(String::new())
    }
}

/// PUT /companies/{company_id}/workflows/{id} - HTMX update workflow (Protected).
#[instrument(skip(company_use_cases, workflow_use_cases, config, user, form))]
async fn update_workflow_handler(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(workflow_use_cases): State<Arc<WorkflowUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, workflow_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<WorkflowForm>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")),
    };

    let emails = parse_emails_form(form.participant_emails);
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
            workflow_config,
        )
        .await
    {
        Ok(wf) => Html(pages::workflow_row_fragment(
            &company,
            &config.app_domain_name,
            &wf,
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
}

/// GET /companies/{company_id}/workflows/{id}/simulate - Simulation page (Protected).
#[instrument(skip(company_use_cases, workflow_use_cases, config, user))]
async fn simulate_workflow_page(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(workflow_use_cases): State<Arc<WorkflowUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, workflow_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")),
    };

    let workflow = match workflow_use_cases
        .get_company_workflow(user.id, company_id, workflow_id)
        .await
    {
        Ok(Some(wf)) => wf,
        _ => return Html(pages::error_alert("Workflow not found.")),
    };

    Html(pages::workflow_simulation_page(
        &company,
        &config.app_domain_name,
        &workflow,
    ))
}

/// POST /companies/{company_id}/workflows/{id}/simulate - Submit simulation form (Protected).
#[instrument(skip(workflow_use_cases, thread_use_cases, config, _user, form))]
async fn simulate_workflow_handler(
    State(workflow_use_cases): State<Arc<WorkflowUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(config): State<Arc<AppConfig>>,
    _user: AuthenticatedUser,
    Path((_company_id, _workflow_id)): Path<(Uuid, Uuid)>,
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
                to: form.to,
                from: form.from,
                subject: form.subject,
                text_body: form.text_body,
                html_body: form.html_body,
                raw_payload: None,
            };

            match workflow_use_cases
                .process_inbound_email("simulation", inbound_email, &config.app_domain_name)
                .await
            {
                Ok(result) => Html(pages::workflow_simulation_result_fragment(&result)),
                Err(err) => Html(pages::error_alert(&format!("Simulation failed: {err}"))),
            }
        }
        SimulationMode::RunTest | SimulationMode::Run => {
            let raw_payload = RawInboundPayload {
                to: form.to,
                from: form.from,
                subject: form.subject,
                text: form.text_body,
                html: form.html_body,
                ..Default::default()
            };

            match thread_use_cases.execute_simulation(raw_payload, mode).await {
                Ok(sim_res) => Html(pages::workflow_simulation_execution_result_fragment(&sim_res)),
                Err(err) => Html(pages::error_alert(&format!("Simulation execution failed: {err}"))),
            }
        }
    }
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
            workflow_config: Some(json!({ "mode": "async" })),
            created_at: Utc::now().naive_utc(),
        };

        let row_html = pages::workflow_row_fragment(&company, "example.com", &workflow);
        assert!(row_html.contains("Auto Dispatcher"));
        assert!(row_html.contains("auto-dispatcher@acme.example.com"));
        assert!(row_html.contains("agent@test.com"));
        assert!(row_html.contains("async"));

        let edit_html = pages::workflow_edit_fragment(&company, "example.com", &workflow);
        assert!(edit_html.contains("hx-put="));
        assert!(edit_html.contains("value=\"Auto Dispatcher\""));

        let sim_html = pages::workflow_simulation_page(&company, "example.com", &workflow);
        assert!(sim_html.contains("Simulate Webhook: Auto Dispatcher"));
        assert!(sim_html.contains("auto-dispatcher@acme.example.com"));
        assert!(sim_html.contains("value=\"verify\""));
        assert!(sim_html.contains("value=\"run_test\""));
        assert!(sim_html.contains("value=\"run\""));

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
        let sim_result_html = pages::workflow_simulation_result_fragment(&sim_result);
        assert!(sim_result_html.contains("Webhook Triggered & Workflow Resolved Successfully!"));

        let full_sim_res = crate::use_cases::thread::SimulationExecutionResult {
            ingest_result: crate::use_cases::thread::InboundIngestResult {
                accepted: true,
                reason: None,
                thread: None,
                inbound_message: None,
                company: Some(company),
                workflow: Some(workflow),
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
            },
            agent_execution: Some(crate::use_cases::thread::AgentExecutionResult {
                outbound_message_id: Some("<out1@test>".to_string()),
                agent_response: "Hello from Agent".to_string(),
                email_sent: false,
            }),
            simulation_mode: crate::use_cases::thread::SimulationMode::RunTest,
        };
        let run_test_html = pages::workflow_simulation_execution_result_fragment(&full_sim_res);
        assert!(run_test_html.contains("Run_Test Mode"));
        assert!(run_test_html.contains("Skipped (Run_Test Dry-Run)"));
        assert!(run_test_html.contains("Hello from Agent"));
    }
}
