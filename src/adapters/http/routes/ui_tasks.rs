//! `/ui/tasks` — the Tasks workspace: the mailbox shell with the company's background tasks
//! watched rather than its mail read.
//!
//! The shell and the company scoping are shared: chrome comes from
//! [`crate::adapters::http::pages::ui_shell`] and the company from [`super::ui::load_scoped_company`].
//! Which page of tasks a request means is one [`TaskFilter`], the same value the classic tasks page
//! pages by, so the two UIs cannot disagree about what `?page=2` contains.

use std::sync::Arc;

use axum::{
    Router,
    extract::{FromRequestParts, Path, Query},
    http::request::Parts,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use tracing::instrument;
use uuid::Uuid;

use crate::{
    adapters::http::{
        app_state::AppState,
        auth::{AuthError, AuthenticatedUser},
        pages,
    },
    app_error::{AppError, AppResult},
    entities::{
        channel::Channel,
        company::Company,
        task::{BackgroundTask, TaskFilter},
        value_objects::EmailAddress,
    },
    infra::config::AppConfig,
    services::task_worker::TaskWorker,
    use_cases::{
        channel::ChannelUseCases, company::CompanyUseCases, thread::ThreadUseCases,
        user::UserUseCases,
    },
};

use super::{
    task::deserialize_empty_string_as_none,
    ui::{load_account, load_scoped_company},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui/tasks", get(tasks_page))
        .route("/ui/tasks/list", get(task_list_fragment))
        .route("/ui/tasks/{task_id}", get(task_pane))
        .route("/ui/tasks/{task_id}/stop", post(stop_task))
        .route("/ui/tasks/{task_id}/resume", post(resume_task))
}

/// What the workspace has selected and filtered by, all optional so `/ui/tasks` alone is a valid
/// entry point. Every fragment and every write carries the same set, so a stop or a page change
/// comes back to the list the user was actually looking at.
///
/// The selects submit an empty option for "no filter", which is why the ids are read through
/// [`deserialize_empty_string_as_none`] rather than as plain `Option`s.
#[derive(Debug, Clone, Deserialize)]
pub struct TasksQuery {
    pub company_id: Option<Uuid>,
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub task_id: Option<Uuid>,
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub channel_id: Option<Uuid>,
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub status: Option<String>,
    pub sort: Option<String>,
    pub page: Option<usize>,
    pub limit: Option<usize>,
}

impl TasksQuery {
    /// The page of tasks this request is asking for, with the paging clamped to what the list
    /// will serve.
    fn filter(&self) -> TaskFilter {
        TaskFilter::new(
            self.channel_id,
            self.status
                .as_deref()
                .and_then(|status| status.parse().ok()),
            self.sort.as_deref() == Some("asc"),
            self.page,
            self.limit,
        )
    }
}

const NO_SELECTION: &str = "Select a task to see what it ran, and what it cost.";

/// The use cases and the caller every Tasks handler starts from.
///
/// Extracted as one value rather than five `State`s per handler: each of these routes needs the
/// same set, and a handler's own parameters should be what makes it different from its siblings.
struct Workspace {
    company_use_cases: Arc<CompanyUseCases>,
    channel_use_cases: Arc<ChannelUseCases>,
    thread_use_cases: Arc<ThreadUseCases>,
    user_use_cases: Arc<UserUseCases>,
    config: Arc<AppConfig>,
    user_id: Uuid,
}

impl FromRequestParts<AppState> for Workspace {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = AuthenticatedUser::from_request_parts(parts, state).await?;

        Ok(Self {
            company_use_cases: state.company_use_cases.clone(),
            channel_use_cases: state.channel_use_cases.clone(),
            thread_use_cases: state.thread_use_cases.clone(),
            user_use_cases: state.user_use_cases.clone(),
            config: state.config.clone(),
            user_id: user.id,
        })
    }
}

impl Workspace {
    /// The company a request is scoped to, always picked from the caller's own companies so a
    /// guessed `company_id` cannot reach another user's tasks.
    async fn scoped_company(&self, company_id: Option<Uuid>) -> AppResult<Company> {
        let (_, company) =
            load_scoped_company(&self.company_use_cases, self.user_id, company_id).await?;
        company.ok_or_else(|| AppError::NotFound("Company not found".into()))
    }

    fn view<'a>(&'a self, company: &'a Company) -> TaskMonitorView<'a> {
        TaskMonitorView {
            channel_use_cases: &self.channel_use_cases,
            thread_use_cases: &self.thread_use_cases,
            config: &self.config,
            user_id: self.user_id,
            company,
        }
    }
}

/// GET /ui/tasks - The Tasks workspace for the selected company / filters / task (Protected).
#[instrument(skip(workspace))]
async fn tasks_page(
    workspace: Workspace,
    Query(query): Query<TasksQuery>,
) -> AppResult<Html<String>> {
    let account = load_account(&workspace.user_use_cases, workspace.user_id).await?;
    let account_email = EmailAddress::from(account.email.as_str());
    let workspace_user = pages::MailboxUser {
        username: &account.username,
        email: &account_email,
    };

    let (companies, company) = load_scoped_company(
        &workspace.company_use_cases,
        workspace.user_id,
        query.company_id,
    )
    .await?;
    let Some(company) = company else {
        return Ok(Html(pages::mailbox_no_company_page(&workspace_user)));
    };

    let view = workspace.view(&company);
    let filter = query.filter();
    let channels = view.channels().await?;
    let (tasks, has_next) = view.page(&filter).await?;

    // A task the filters exclude is still worth showing: the URL named it, and it may well be on
    // another page of this same list.
    let selected = match query.task_id {
        Some(task_id) => view.task(task_id).await?,
        None => None,
    };
    let pane_html = match &selected {
        Some(task) => view.pane(task, None).await?,
        None => pages::task_monitor_empty_pane(NO_SELECTION, pages::FragmentSwap::Inline),
    };

    let list = view.list(
        &tasks,
        has_next,
        &filter,
        selected.as_ref().map(|task| task.id),
    );
    Ok(Html(pages::task_monitor_page(&pages::TaskMonitorPage {
        user: &workspace_user,
        companies: &companies,
        channels: &channels,
        list: &list,
        pane_html: &pane_html,
    })))
}

/// GET /ui/tasks/list - One filtered page of tasks for the sidebar (Protected).
///
/// Answers with the address-bar URL as well as the list, so filtering and paging stay linkable
/// even though only the sidebar is swapped.
#[instrument(skip(workspace))]
async fn task_list_fragment(
    workspace: Workspace,
    Query(query): Query<TasksQuery>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    let filter = query.filter();
    let (tasks, has_next) = view.page(&filter).await?;

    let list = view.list(&tasks, has_next, &filter, query.task_id);
    Ok((
        [("HX-Push-Url", view.workspace_url(&filter, query.task_id))],
        Html(pages::task_monitor_list(&list, pages::FragmentSwap::Inline)),
    )
        .into_response())
}

/// GET /ui/tasks/{task_id} - One task's detail for the pane (Protected).
#[instrument(skip(workspace))]
async fn task_pane(
    workspace: Workspace,
    Path(task_id): Path<Uuid>,
    Query(query): Query<TasksQuery>,
) -> AppResult<Html<String>> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    let task = view.require_task(task_id).await?;

    Ok(Html(view.pane(&task, None).await?))
}

/// POST /ui/tasks/{task_id}/stop - Stop a task that is still queued or running (Protected).
#[instrument(skip(workspace))]
async fn stop_task(
    workspace: Workspace,
    Path(task_id): Path<Uuid>,
    Query(query): Query<TasksQuery>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    let task = view.require_task(task_id).await?;

    let stopped = view.worker().await.stop_task_and_notify(task.id).await;
    view.after_write(
        &task,
        &query.filter(),
        stopped
            .err()
            .map(|err| format!("Failed to stop task: {err}")),
    )
    .await
}

/// POST /ui/tasks/{task_id}/resume - Put a stopped or dead-lettered task back on the queue
/// (Protected).
#[instrument(skip(workspace))]
async fn resume_task(
    workspace: Workspace,
    Path(task_id): Path<Uuid>,
    Query(query): Query<TasksQuery>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    let task = view.require_task(task_id).await?;

    let resumed = view.worker().await.resume_task(task.id).await;
    view.after_write(
        &task,
        &query.filter(),
        resumed
            .err()
            .map(|err| format!("Failed to resume task: {err}")),
    )
    .await
}

/// Everything the workspace renders from, so each handler names its data once.
struct TaskMonitorView<'a> {
    channel_use_cases: &'a ChannelUseCases,
    thread_use_cases: &'a Arc<ThreadUseCases>,
    config: &'a Arc<AppConfig>,
    user_id: Uuid,
    company: &'a Company,
}

impl TaskMonitorView<'_> {
    async fn channels(&self) -> AppResult<Vec<Channel>> {
        self.channel_use_cases
            .list_company_channels(self.user_id, self.company.id)
            .await
    }

    /// One filtered page of tasks, plus whether another follows it.
    async fn page(&self, filter: &TaskFilter) -> AppResult<(Vec<BackgroundTask>, bool)> {
        let probed = self
            .thread_use_cases
            .list_company_tasks_page(
                self.company.id,
                filter.channel_id,
                filter.status,
                filter.sort_asc,
                filter.offset(),
                filter.probe_limit(),
            )
            .await?;

        Ok(filter.split_probe(probed))
    }

    /// One task, but only when it really belongs to the company the request is scoped to — the id
    /// comes from the URL, so a guessed one must not reach another company's queue.
    async fn task(&self, task_id: Uuid) -> AppResult<Option<BackgroundTask>> {
        let task = self
            .thread_use_cases
            .get_task_persistence()
            .await
            .get_task_by_id(task_id)
            .await?;

        Ok(task.filter(|task| task.company_id == self.company.id))
    }

    async fn require_task(&self, task_id: Uuid) -> AppResult<BackgroundTask> {
        self.task(task_id)
            .await?
            .ok_or_else(|| AppError::NotFound("Task not found".into()))
    }

    async fn worker(&self) -> TaskWorker {
        TaskWorker::new(
            self.thread_use_cases.get_task_persistence().await,
            self.thread_use_cases.clone(),
            self.config.clone(),
        )
    }

    fn list<'a>(
        &'a self,
        tasks: &'a [BackgroundTask],
        has_next: bool,
        filter: &'a TaskFilter,
        selected_task_id: Option<Uuid>,
    ) -> pages::TaskMonitorList<'a> {
        pages::TaskMonitorList {
            company: self.company,
            tasks,
            filter,
            has_next,
            selected_task_id,
        }
    }

    fn workspace_url(&self, filter: &TaskFilter, selected_task_id: Option<Uuid>) -> String {
        format!(
            "/ui/tasks?{}",
            pages::task_monitor_query(self.company.id, filter, selected_task_id)
        )
    }

    async fn pane(&self, task: &BackgroundTask, error: Option<&str>) -> AppResult<String> {
        // A task outlives the channel it ran for, so the pane falls back to the raw id rather
        // than refusing to render.
        let channel = self
            .channel_use_cases
            .get_company_channel(self.user_id, self.company.id, task.channel_id)
            .await?;

        Ok(pages::task_detail_pane(&pages::TaskDetailPane {
            company_id: self.company.id,
            task,
            channel: channel.as_ref(),
            error,
        }))
    }

    /// What a stop or a resume returns: the task as it now stands, with the sidebar list refreshed
    /// beside it so its new status shows in both places at once.
    ///
    /// A refused write is reported in the pane rather than as a page error — the task is still
    /// there, and its detail is what says why nothing happened.
    async fn after_write(
        &self,
        task: &BackgroundTask,
        filter: &TaskFilter,
        error: Option<String>,
    ) -> AppResult<Response> {
        let refreshed = self.require_task(task.id).await?;
        let pane = self.pane(&refreshed, error.as_deref()).await?;

        let (tasks, has_next) = self.page(filter).await?;
        let list = pages::task_monitor_list(
            &self.list(&tasks, has_next, filter, Some(refreshed.id)),
            pages::FragmentSwap::OutOfBand,
        );

        Ok(Html(format!("{pane}{list}")).into_response())
    }
}
