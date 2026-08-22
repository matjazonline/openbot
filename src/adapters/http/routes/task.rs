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
    entities::task::{TaskFilter, TaskStatus},
    services::task_worker::TaskWorker,
    use_cases::{channel::ChannelUseCases, company::CompanyUseCases, thread::ThreadUseCases},
};

use super::company_load_error;

const DEFAULT_TASK_PAGE_SIZE: usize = TaskFilter::DEFAULT_PAGE_SIZE;
const MAX_TASK_PAGE_SIZE: usize = TaskFilter::MAX_PAGE_SIZE;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/companies/{id}/tasks", get(list_company_tasks_page))
        .route("/companies/{id}/tasks/filter", get(filter_company_tasks))
        .route(
            "/companies/{id}/tasks/{task_id}/stop",
            post(stop_company_task),
        )
        .route(
            "/companies/{id}/tasks/{task_id}/resume",
            post(resume_company_task),
        )
}

pub(super) fn deserialize_empty_string_as_none<'de, D, T>(
    deserializer: D,
) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: FromStr,
    T::Err: std::fmt::Display,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    match opt {
        Some(s) if !s.trim().is_empty() => {
            s.parse::<T>().map(Some).map_err(serde::de::Error::custom)
        }
        _ => Ok(None),
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskFilterQuery {
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub channel_id: Option<Uuid>,
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub status: Option<String>,
    pub sort: Option<String>,
    pub page: Option<usize>,
    pub limit: Option<usize>,
}

impl TaskFilterQuery {
    fn page(&self) -> usize {
        self.page.unwrap_or(1).max(1)
    }

    fn limit(&self) -> usize {
        self.limit
            .unwrap_or(DEFAULT_TASK_PAGE_SIZE)
            .clamp(1, MAX_TASK_PAGE_SIZE)
    }
}

#[instrument(skip(company_use_cases, channel_use_cases, thread_use_cases, _user))]
async fn list_company_tasks_page(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    _user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Query(query): Query<TaskFilterQuery>,
) -> impl IntoResponse {
    let company = match company_use_cases.owned_company(_user.id, company_id).await {
        Ok(company) => company,
        Err(error) => return Html(pages::error_alert(&company_load_error(&error))),
    };

    let channels = channel_use_cases
        .list_company_channels(_user.id, company_id)
        .await
        .unwrap_or_default();

    let status_enum = query
        .status
        .as_deref()
        .and_then(|s| s.parse::<TaskStatus>().ok());
    let sort_asc = query.sort.as_deref() == Some("asc");

    let page = query.page();
    let limit = query.limit();
    let mut tasks = thread_use_cases
        .list_company_tasks_page(
            company_id,
            query.channel_id,
            status_enum,
            sort_asc,
            task_page_offset(page, limit),
            (limit + 1) as i64,
        )
        .await
        .unwrap_or_default();
    let has_next = tasks.len() > limit;
    tasks.truncate(limit);
    let pagination = build_task_pagination(
        company_id,
        query.channel_id,
        status_enum.as_ref(),
        sort_asc,
        page,
        limit,
        has_next,
    );

    Html(pages::company_tasks_page(
        &company,
        &channels,
        &tasks,
        query.channel_id,
        status_enum,
        sort_asc,
        &pagination,
    ))
}

fn task_page_offset(page: usize, limit: usize) -> i64 {
    page.saturating_sub(1)
        .saturating_mul(limit)
        .min(i64::MAX as usize) as i64
}

fn build_tasks_url(
    company_id: Uuid,
    channel_id: Option<Uuid>,
    status: Option<&TaskStatus>,
    sort_asc: bool,
    page: usize,
    limit: usize,
    filter_endpoint: bool,
) -> String {
    let mut params = Vec::new();
    if let Some(channel_id) = channel_id {
        params.push(format!("channel_id={channel_id}"));
    }
    if let Some(status) = status {
        params.push(format!("status={}", status.as_str()));
    }
    if sort_asc {
        params.push("sort=asc".to_string());
    }
    if limit != DEFAULT_TASK_PAGE_SIZE {
        params.push(format!("limit={limit}"));
    }
    if page > 1 {
        params.push(format!("page={page}"));
    }

    let suffix = if filter_endpoint { "/filter" } else { "" };
    let base = format!("/companies/{company_id}/tasks{suffix}");
    if params.is_empty() {
        base
    } else {
        format!("{base}?{}", params.join("&"))
    }
}

fn build_task_pagination(
    company_id: Uuid,
    channel_id: Option<Uuid>,
    status: Option<&TaskStatus>,
    sort_asc: bool,
    page: usize,
    limit: usize,
    has_next: bool,
) -> pages::TaskPagination {
    let build_link = |target_page| pages::TaskPageLink {
        href: build_tasks_url(
            company_id,
            channel_id,
            status,
            sort_asc,
            target_page,
            limit,
            false,
        ),
        hx_get: build_tasks_url(
            company_id,
            channel_id,
            status,
            sort_asc,
            target_page,
            limit,
            true,
        ),
    };

    pages::TaskPagination {
        current_page: page,
        limit,
        previous: (page > 1).then(|| build_link(page - 1)),
        next: has_next.then(|| build_link(page.saturating_add(1))),
    }
}

#[instrument(skip(thread_use_cases, company_use_cases, _user))]
async fn filter_company_tasks(
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    _user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Query(query): Query<TaskFilterQuery>,
) -> impl IntoResponse {
    if let Err(error) = company_use_cases.owned_company(_user.id, company_id).await {
        return (
            [("HX-Push-Url", format!("/companies/{company_id}/tasks"))],
            Html(pages::error_alert(&company_load_error(&error))),
        );
    }

    let status_enum = query
        .status
        .as_deref()
        .and_then(|s| s.parse::<TaskStatus>().ok());
    let sort_asc = query.sort.as_deref() == Some("asc");

    let page = query.page();
    let limit = query.limit();
    let mut tasks = thread_use_cases
        .list_company_tasks_page(
            company_id,
            query.channel_id,
            status_enum,
            sort_asc,
            task_page_offset(page, limit),
            (limit + 1) as i64,
        )
        .await
        .unwrap_or_default();
    let has_next = tasks.len() > limit;
    tasks.truncate(limit);
    let pagination = build_task_pagination(
        company_id,
        query.channel_id,
        status_enum.as_ref(),
        sort_asc,
        page,
        limit,
        has_next,
    );

    let push_url = build_tasks_url(
        company_id,
        query.channel_id,
        status_enum.as_ref(),
        sort_asc,
        page,
        limit,
        false,
    );

    (
        [("HX-Push-Url", push_url)],
        Html(pages::task_list_fragment(company_id, &tasks, &pagination)),
    )
}

#[instrument(skip(thread_use_cases, company_use_cases, config, _user))]
async fn stop_company_task(
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(config): State<Arc<crate::infra::config::AppConfig>>,
    _user: AuthenticatedUser,
    Path((company_id, task_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    if let Err(error) = company_use_cases.owned_company(_user.id, company_id).await {
        return Html(pages::error_alert(&company_load_error(&error)));
    }
    let task_persistence = thread_use_cases.get_task_persistence().await;
    if !matches!(task_persistence.get_task_by_id(task_id).await, Ok(Some(task)) if task.company_id == company_id)
    {
        return Html(pages::error_alert("Task not found."));
    }
    let worker = TaskWorker::new(task_persistence.clone(), thread_use_cases.clone(), config);

    if let Err(error) = worker.stop_task_and_notify(task_id).await {
        return Html(pages::error_alert(&format!("Failed to stop task: {error}")));
    }

    if let Ok(Some(updated_task)) = task_persistence.get_task_by_id(task_id).await {
        Html(pages::task_row_fragment(company_id, &updated_task))
    } else {
        Html(pages::error_alert("Failed to stop task."))
    }
}

#[instrument(skip(thread_use_cases, company_use_cases, config, _user))]
async fn resume_company_task(
    State(thread_use_cases): State<Arc<ThreadUseCases>>,
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(config): State<Arc<crate::infra::config::AppConfig>>,
    _user: AuthenticatedUser,
    Path((company_id, task_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    if let Err(error) = company_use_cases.owned_company(_user.id, company_id).await {
        return Html(pages::error_alert(&company_load_error(&error)));
    }
    let task_persistence = thread_use_cases.get_task_persistence().await;
    if !matches!(task_persistence.get_task_by_id(task_id).await, Ok(Some(task)) if task.company_id == company_id)
    {
        return Html(pages::error_alert("Task not found."));
    }
    let worker = TaskWorker::new(task_persistence.clone(), thread_use_cases.clone(), config);

    if let Err(error) = worker.resume_task(task_id).await {
        return Html(pages::error_alert(&format!(
            "Failed to resume task: {error}"
        )));
    }

    if let Ok(Some(updated_task)) = task_persistence.get_task_by_id(task_id).await {
        Html(pages::task_row_fragment(company_id, &updated_task))
    } else {
        Html(pages::error_alert("Failed to resume task."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::task::BackgroundTask;

    #[test]
    fn test_task_filter_query_deserialization_empty_channel() {
        let uri: axum::http::Uri =
            "/companies/123/tasks/filter?channel_id=&status=completed&sort=desc"
                .parse()
                .unwrap();
        let Query(query) =
            Query::<TaskFilterQuery>::try_from_uri(&uri).expect("Should deserialize");
        assert_eq!(query.channel_id, None);
        assert_eq!(query.status, Some("completed".to_string()));
        assert_eq!(query.sort, Some("desc".to_string()));
        assert_eq!(query.page(), 1);
        assert_eq!(query.limit(), DEFAULT_TASK_PAGE_SIZE);
    }

    #[test]
    fn test_task_filter_query_deserialization_with_channel() {
        let ch_id = Uuid::new_v4();
        let uri_str = format!(
            "/companies/123/tasks/filter?channel_id={}&status=pending&sort=asc&page=2&limit=25",
            ch_id
        );
        let uri: axum::http::Uri = uri_str.parse().unwrap();
        let Query(query) =
            Query::<TaskFilterQuery>::try_from_uri(&uri).expect("Should deserialize");
        assert_eq!(query.channel_id, Some(ch_id));
        assert_eq!(query.status, Some("pending".to_string()));
        assert_eq!(query.sort, Some("asc".to_string()));
        assert_eq!(query.page(), 2);
        assert_eq!(query.limit(), 25);
    }

    #[test]
    fn test_task_filter_query_deserialization_all_empty() {
        let uri: axum::http::Uri = "/companies/123/tasks/filter?channel_id=&status=&sort="
            .parse()
            .unwrap();
        let Query(query) =
            Query::<TaskFilterQuery>::try_from_uri(&uri).expect("Should deserialize");
        assert_eq!(query.channel_id, None);
        assert_eq!(query.status, None);
        assert_eq!(query.sort, Some("".to_string()));
        assert_eq!(query.page(), 1);
        assert_eq!(query.limit(), DEFAULT_TASK_PAGE_SIZE);
    }

    #[test]
    fn test_task_row_fragment_renders_thread_link() {
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let thread_id = Uuid::new_v4();

        let task = crate::entities::task::BackgroundTask {
            id: Uuid::new_v4(),
            company_id,
            channel_id: channel_id,
            thread_id: Some(thread_id),
            task_type: "agent_execution".to_string(),
            status: TaskStatus::Completed,
            payload: serde_json::json!({}),
            retry_count: 0,
            max_retries: 3,
            last_error: None,
            worker_id: None,
            locked_at: None,
            lock_expires_at: None,
            run_at: chrono::Utc::now(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let html = pages::task_row_fragment(company_id, &task);
        assert!(html.contains("Open Thread"));
        assert!(html.contains(&format!(
            "/companies/{company_id}/channels/{channel_id}/simulate?thread_id={thread_id}"
        )));
    }

    #[test]
    fn test_task_row_fragment_renders_expandable_parameters_and_masks_api_key() {
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let task = BackgroundTask {
            id: Uuid::new_v4(),
            company_id,
            channel_id: channel_id,
            thread_id: None,
            task_type: "email_agent_dispatch".to_string(),
            status: TaskStatus::Completed,
            payload: serde_json::json!({
                "execution_parameters": {
                    "provider": "google",
                    "model": "gemini-2.5-flash",
                    "config": {
                        "api_key": "secret-key-12345"
                    }
                },
                "parsed_email": {
                    "sender": "user@example.com",
                    "subject": "Need assistance"
                }
            }),
            retry_count: 0,
            max_retries: 3,
            last_error: None,
            worker_id: None,
            locked_at: None,
            lock_expires_at: None,
            run_at: chrono::Utc::now(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let html = pages::task_row_fragment(company_id, &task);
        assert!(html.contains("Task Execution Parameters"));
        assert!(html.contains("Provider: google"));
        assert!(html.contains("Model: gemini-2.5-flash"));
        assert!(html.contains("Sender: user@example.com"));
        assert!(html.contains("Subject: Need assistance"));
        assert!(html.contains("***masked***"));
        assert!(!html.contains("secret-key-12345"));
    }

    #[test]
    fn test_build_tasks_url() {
        let company_id = Uuid::new_v4();
        let ch_id = Uuid::new_v4();

        assert_eq!(
            build_tasks_url(company_id, None, None, false, 1, 50, false),
            format!("/companies/{company_id}/tasks")
        );

        let status = TaskStatus::Pending;
        assert_eq!(
            build_tasks_url(company_id, Some(ch_id), Some(&status), true, 2, 25, false),
            format!(
                "/companies/{company_id}/tasks?channel_id={ch_id}&status=pending&sort=asc&limit=25&page=2"
            )
        );
        assert_eq!(
            build_tasks_url(company_id, Some(ch_id), None, false, 1, 50, true),
            format!("/companies/{company_id}/tasks/filter?channel_id={ch_id}")
        );
        assert!(!build_tasks_url(company_id, None, None, false, 1, 50, false).ends_with('?'));
    }

    #[test]
    fn test_task_filter_query_bounds_pagination() {
        let uri: axum::http::Uri = "/companies/123/tasks?limit=1000&page=0".parse().unwrap();
        let Query(query) =
            Query::<TaskFilterQuery>::try_from_uri(&uri).expect("Should deserialize");
        assert_eq!(query.page(), 1);
        assert_eq!(query.limit(), MAX_TASK_PAGE_SIZE);
        assert_eq!(task_page_offset(3, 25), 50);
    }

    #[test]
    fn test_task_pagination_preserves_filters_in_both_links() {
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let status = TaskStatus::Completed;
        let pagination = build_task_pagination(
            company_id,
            Some(channel_id),
            Some(&status),
            true,
            2,
            25,
            true,
        );

        let html = pages::task_list_fragment(company_id, &[], &pagination);
        assert!(html.contains("Page 2"));
        assert!(html.contains("&larr; Previous"));
        assert!(html.contains("Next &rarr;"));
        assert!(html.contains(&format!("channel_id={channel_id}")));
        assert!(html.contains("status=completed&amp;") || html.contains("status=completed&"));
        assert!(html.contains("/tasks/filter?"));
        assert!(!html.contains(&format!("channel_id={channel_id}?")));
    }

    #[test]
    fn test_channel_row_fragment_renders_tasks_link() {
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();

        let company = crate::entities::company::Company {
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Test Co".to_string(),
            slug: "test-co".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            created_at: chrono::Utc::now(),
        };

        let channel = crate::entities::channel::Channel {
            enabled: true,
            add_3rd_party: true,
            id: channel_id,
            company_id,
            name: "Test WF".to_string(),
            description: None,
            slug: "test-wf".into(),
            alias_slugs: Vec::new(),
            provider: None,
            model: None,
            api_key: None,
            participant_emails: None,
            agent_ids: None,
            channel_config: None,
            retrieve_company_memory: false,
            retrieve_agent_memory: false,
            retrieve_user_memory: false,
            persist_company_memory: false,
            persist_agent_memory: false,
            persist_user_memory: false,
            memory_recall_mode: crate::entities::memory::MemoryRecallMode::Fast,
            memory_max_results: 5,
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: chrono::Utc::now(),
        };

        let html = pages::channel_row_fragment(&company, "example.com", &channel, &[]);
        assert!(html.contains("Task Executions"));
        assert!(html.contains(&format!(
            "/companies/{company_id}/tasks?channel_id={channel_id}"
        )));
    }

    #[test]
    fn test_task_row_and_company_page_renders_token_meter() {
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();

        let company = crate::entities::company::Company {
            id: company_id,
            user_id: Uuid::new_v4(),
            name: "Token Test Co".to_string(),
            slug: "token-co".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            created_at: chrono::Utc::now(),
        };

        let task = BackgroundTask {
            id: Uuid::new_v4(),
            company_id,
            channel_id: channel_id,
            thread_id: None,
            task_type: "email_agent_dispatch".to_string(),
            status: TaskStatus::Completed,
            payload: serde_json::json!({
                "execution_result": {
                    "response": "Hello world",
                    "email_sent": true,
                    "token_usage": {
                        "prompt_tokens": 120,
                        "completion_tokens": 45,
                        "total_tokens": 165
                    }
                }
            }),
            retry_count: 0,
            max_retries: 3,
            last_error: None,
            worker_id: None,
            locked_at: None,
            lock_expires_at: None,
            run_at: chrono::Utc::now(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        assert_eq!(
            task.token_usage(),
            Some(crate::entities::task::TokenUsage {
                prompt_tokens: 120,
                completion_tokens: 45,
                total_tokens: 165
            })
        );

        let row_html = pages::task_row_fragment(company_id, &task);
        assert!(row_html.contains("Token Meter:"));
        assert!(row_html.contains("165 total"));
        assert!(row_html.contains("Prompt: 120 • Completion: 45"));

        let pagination = pages::TaskPagination {
            current_page: 1,
            limit: DEFAULT_TASK_PAGE_SIZE,
            previous: None,
            next: None,
        };
        let page_html =
            pages::company_tasks_page(&company, &[], &[task], None, None, false, &pagination);
        assert!(page_html.contains("Token Meter Summary"));
        assert!(page_html.contains("120"));
        assert!(page_html.contains("45"));
        assert!(page_html.contains("165"));
    }
}
