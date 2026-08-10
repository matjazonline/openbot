use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, Query, State},
    response::{Html, IntoResponse},
    routing::{get, post},
};
use serde::Deserialize;
use tracing::instrument;
use uuid::Uuid;

use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser, pages},
    entities::task::TaskStatus,
    services::task_worker::TaskWorker,
    use_cases::{company::CompanyUseCases, thread::ThreadUseCases, workflow::WorkflowUseCases},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/companies/{id}/tasks", get(list_company_tasks_page))
        .route("/companies/{id}/tasks/filter", get(filter_company_tasks))
        .route("/companies/{id}/tasks/{task_id}/stop", post(stop_company_task))
        .route("/companies/{id}/tasks/{task_id}/resume", post(resume_company_task))
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskFilterQuery {
    pub workflow_id: Option<Uuid>,
    pub status: Option<String>,
    pub sort: Option<String>,
}

#[instrument(skip(company_use_cases, workflow_use_cases, thread_use_cases, _user))]
async fn list_company_tasks_page(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(workflow_use_cases): State<Arc<WorkflowUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    _user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Query(query): Query<TaskFilterQuery>,
) -> impl IntoResponse {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(c)) => c,
        _ => return Html(pages::error_alert("Company not found.")),
    };

    let workflows = workflow_use_cases
        .list_company_workflows(_user.id, company_id)
        .await
        .unwrap_or_default();

    let status_enum = query.status.as_deref().and_then(|s| s.parse::<TaskStatus>().ok());
    let sort_asc = query.sort.as_deref() == Some("asc");

    let tasks = thread_use_cases
        .list_company_tasks(company_id, query.workflow_id, status_enum.clone(), sort_asc)
        .await
        .unwrap_or_default();

    Html(pages::company_tasks_page(
        &company,
        &workflows,
        &tasks,
        query.workflow_id,
        status_enum,
        sort_asc,
    ))
}

#[instrument(skip(thread_use_cases, _user))]
async fn filter_company_tasks(
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    _user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Query(query): Query<TaskFilterQuery>,
) -> impl IntoResponse {
    let status_enum = query.status.as_deref().and_then(|s| s.parse::<TaskStatus>().ok());
    let sort_asc = query.sort.as_deref() == Some("asc");

    let tasks = thread_use_cases
        .list_company_tasks(company_id, query.workflow_id, status_enum, sort_asc)
        .await
        .unwrap_or_default();

    Html(pages::task_list_fragment(company_id, &tasks))
}

#[instrument(skip(thread_use_cases, config, _user))]
async fn stop_company_task(
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(config): State<Arc<crate::infra::config::AppConfig>>,
    _user: AuthenticatedUser,
    Path((company_id, task_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    let task_persistence = thread_use_cases.get_task_persistence().await;
    let worker = TaskWorker::new(task_persistence.clone(), thread_use_cases.clone(), config);

    let _ = worker.stop_task_and_notify(task_id).await;

    if let Ok(Some(updated_task)) = task_persistence.get_task_by_id(task_id).await {
        Html(pages::task_row_fragment(company_id, &updated_task))
    } else {
        Html(pages::error_alert("Failed to stop task."))
    }
}

#[instrument(skip(thread_use_cases, config, _user))]
async fn resume_company_task(
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(config): State<Arc<crate::infra::config::AppConfig>>,
    _user: AuthenticatedUser,
    Path((company_id, task_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    let task_persistence = thread_use_cases.get_task_persistence().await;
    let worker = TaskWorker::new(task_persistence.clone(), thread_use_cases.clone(), config);

    let _ = worker.resume_task(task_id).await;

    if let Ok(Some(updated_task)) = task_persistence.get_task_by_id(task_id).await {
        Html(pages::task_row_fragment(company_id, &updated_task))
    } else {
        Html(pages::error_alert("Failed to resume task."))
    }
}
