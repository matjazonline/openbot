use std::{str::FromStr, sync::Arc};

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

fn deserialize_empty_string_as_none<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: FromStr,
    T::Err: std::fmt::Display,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    match opt {
        Some(s) if !s.trim().is_empty() => s.parse::<T>().map(Some).map_err(serde::de::Error::custom),
        _ => Ok(None),
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskFilterQuery {
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub workflow_id: Option<Uuid>,
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_filter_query_deserialization_empty_workflow() {
        let uri: axum::http::Uri = "/companies/123/tasks/filter?workflow_id=&status=completed&sort=desc".parse().unwrap();
        let Query(query) = Query::<TaskFilterQuery>::try_from_uri(&uri).expect("Should deserialize");
        assert_eq!(query.workflow_id, None);
        assert_eq!(query.status, Some("completed".to_string()));
        assert_eq!(query.sort, Some("desc".to_string()));
    }

    #[test]
    fn test_task_filter_query_deserialization_with_workflow() {
        let wf_id = Uuid::new_v4();
        let uri_str = format!("/companies/123/tasks/filter?workflow_id={}&status=pending&sort=asc", wf_id);
        let uri: axum::http::Uri = uri_str.parse().unwrap();
        let Query(query) = Query::<TaskFilterQuery>::try_from_uri(&uri).expect("Should deserialize");
        assert_eq!(query.workflow_id, Some(wf_id));
        assert_eq!(query.status, Some("pending".to_string()));
        assert_eq!(query.sort, Some("asc".to_string()));
    }

    #[test]
    fn test_task_filter_query_deserialization_all_empty() {
        let uri: axum::http::Uri = "/companies/123/tasks/filter?workflow_id=&status=&sort=".parse().unwrap();
        let Query(query) = Query::<TaskFilterQuery>::try_from_uri(&uri).expect("Should deserialize");
        assert_eq!(query.workflow_id, None);
        assert_eq!(query.status, None);
        assert_eq!(query.sort, Some("".to_string()));
    }

    #[test]
    fn test_task_row_fragment_renders_simulation_link() {
        let company_id = Uuid::new_v4();
        let workflow_id = Uuid::new_v4();
        let thread_id = Uuid::new_v4();

        let task = crate::entities::task::BackgroundTask {
            id: Uuid::new_v4(),
            company_id,
            workflow_id,
            thread_id: Some(thread_id),
            task_type: "agent_execution".to_string(),
            status: TaskStatus::Completed,
            payload: serde_json::json!({}),
            retry_count: 0,
            max_retries: 3,
            last_error: None,
            run_at: chrono::Utc::now().naive_utc(),
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
        };

        let html = pages::task_row_fragment(company_id, &task);
        assert!(html.contains("Open Simulation"));
        assert!(html.contains(&format!("/companies/{company_id}/workflows/{workflow_id}/simulate?thread_id={thread_id}")));
    }
}
