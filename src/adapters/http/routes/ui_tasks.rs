//! `/ui/tasks` — the Tasks workspace: the mailbox shell with the company's background tasks
//! watched rather than its mail read.
//!
//! The shell and the company scoping are shared: chrome comes from
//! [`crate::adapters::http::pages::ui_shell`] and the company from
//! [`super::ui::load_managed_company`].
//! Which page of tasks a request means is one [`TaskFilter`], the same value the classic tasks page
//! pages by, so the two UIs cannot disagree about what `?page=2` contains.

use std::{convert::Infallible, sync::Arc, time::Duration};

use axum::{
    Router,
    extract::{FromRequestParts, Path, Query, State},
    http::request::Parts,
    response::{
        Html, IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
    routing::{get, post},
};
use serde::Deserialize;
use tokio_stream::{Stream, StreamExt};
use tracing::{instrument, warn};
use uuid::Uuid;

use crate::{
    adapters::http::{
        app_state::AppState,
        auth::{AuthError, AuthenticatedUser},
        pages,
    },
    app_error::{AppError, AppResult},
    domain::monitoring::{MonitoringService, record_pagination_observation},
    entities::{
        channel::Channel,
        company::Company,
        correlation::CorrelationId,
        task::{
            BackgroundTask, ResumeActor, StopActor, TaskBoardFilter, TaskChainBoard,
            TaskChainDetail, TaskFilter,
        },
        value_objects::EmailAddress,
    },
    infra::{config::AppConfig, events::MailboxEvents},
    services::task_worker::TaskWorker,
    use_cases::{
        channel::ChannelUseCases, company::CompanyUseCases, thread::ThreadUseCases,
        user::UserUseCases,
    },
};

use super::{
    live_updates::{Wake, task_chain_wake_ups},
    task::deserialize_empty_string_as_none,
    ui::{load_account, load_managed_company, managed_company_membership, workspace_user},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ui/tasks", get(tasks_page))
        .route("/ui/tasks/board", get(task_board_fragment))
        .route("/ui/tasks/list", get(task_list_fragment))
        .route("/ui/tasks/events", get(task_board_stream))
        .route("/ui/tasks/chains/{correlation_id}", get(task_chain_pane))
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
    pub view: Option<String>,
    #[serde(default, deserialize_with = "deserialize_empty_string_as_none")]
    pub correlation_id: Option<Uuid>,
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

    fn board_filter(&self) -> TaskBoardFilter {
        TaskBoardFilter::new(self.channel_id, chrono::Utc::now())
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
    monitoring: Arc<dyn MonitoringService>,
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
            monitoring: state.monitoring.clone(),
            user_id: user.id,
        })
    }
}

impl Workspace {
    /// The company a request is scoped to, always picked from the caller's own companies so a
    /// guessed `company_id` cannot reach another user's tasks.
    async fn scoped_company(&self, company_id: Option<Uuid>) -> AppResult<Company> {
        let (_, company) =
            load_managed_company(&self.company_use_cases, self.user_id, company_id).await?;
        company.ok_or_else(|| AppError::NotFound("Company not found".into()))
    }

    fn view<'a>(&'a self, company: &'a Company) -> TaskMonitorView<'a> {
        TaskMonitorView {
            channel_use_cases: &self.channel_use_cases,
            thread_use_cases: &self.thread_use_cases,
            config: &self.config,
            monitoring: self.monitoring.as_ref(),
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
    let workspace_user = workspace_user(&account, &account_email, &workspace.config);

    let (companies, company) = load_managed_company(
        &workspace.company_use_cases,
        workspace.user_id,
        query.company_id,
    )
    .await?;
    let Some(company) = company else {
        return Ok(Html(pages::mailbox_no_company_page(&workspace_user)));
    };
    let workspace_user = workspace_user
        .with_company_membership(managed_company_membership(&company, workspace.user_id));

    let view = workspace.view(&company);
    let channels = view.channels().await?;
    if query.view.as_deref() != Some("list") {
        let board_filter = query.board_filter();
        let board = view.board(board_filter).await?;
        let selected = query.correlation_id.map(CorrelationId::from);
        let pane_html = match selected {
            Some(correlation_id) => match view.chain(correlation_id).await? {
                Some(detail) => pages::task_chain_detail_pane(&detail, None),
                None => pages::task_chain_empty_pane("Task chain not found."),
            },
            None => pages::task_chain_empty_pane("Select a chain to inspect its full timeline."),
        };
        return Ok(Html(pages::task_board_page(&pages::TaskBoardPage {
            user: &workspace_user,
            companies: &companies,
            company: &company,
            channels: &channels,
            board: &board,
            filter: board_filter,
            selected_correlation_id: selected,
            pane_html: &pane_html,
        })));
    }

    let filter = query.filter();
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

/// GET /ui/tasks/board - Reconcile the bounded six-column board.
#[instrument(skip(workspace))]
async fn task_board_fragment(
    workspace: Workspace,
    Query(query): Query<TasksQuery>,
) -> AppResult<Response> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    let filter = query.board_filter();
    let board = view.board(filter).await?;
    let selected = query.correlation_id.map(CorrelationId::from);
    let push_url = format!(
        "/ui/tasks?{}",
        pages::task_board_query(company.id, filter.channel_id, selected)
    );
    Ok((
        [("HX-Push-Url", push_url)],
        Html(pages::task_board_fragment(
            company.id,
            &board,
            filter,
            selected,
            pages::FragmentSwap::Inline,
        )),
    )
        .into_response())
}

/// GET /ui/tasks/chains/{correlation_id} - Complete company-scoped chain detail.
#[instrument(skip(workspace))]
async fn task_chain_pane(
    workspace: Workspace,
    Path(correlation_id): Path<Uuid>,
    Query(query): Query<TasksQuery>,
) -> AppResult<Html<String>> {
    let company = workspace.scoped_company(query.company_id).await?;
    let view = workspace.view(&company);
    let detail = view
        .chain(correlation_id.into())
        .await?
        .ok_or_else(|| AppError::NotFound("Task chain not found".into()))?;
    Ok(Html(pages::task_chain_detail_pane(&detail, None)))
}

/// How long one logical transaction's burst of task/outbox/approval rows is absorbed before the
/// board is redrawn once for all of them.
const WAKE_COALESCE_WINDOW: Duration = Duration::from_millis(75);

/// How often the board is redrawn with no event to prompt it.
///
/// The board's cutoff is "now minus seven days", and a connection that only recomputed it on an
/// event would keep answering with the cutoff it opened with — a mailbox left open overnight would
/// still be showing chains that aged out hours ago.
const BOARD_WINDOW_REFRESH: Duration = Duration::from_secs(60);

/// Whether a wake-up means the *selected* chain pane has to be rebuilt.
///
/// The board itself is redrawn on every wake, because any chain in the company can move its column
/// counts. The pane is a much more expensive projection of one chain, so it is rebuilt only when
/// the event names that chain — or when lag means we cannot tell what we missed and have to
/// reconcile everything, which is the same contract the thread streams keep.
fn wake_touches(wake: &Wake, selected: Option<CorrelationId>) -> bool {
    match (wake, selected) {
        (Wake::Lagged, _) => true,
        (Wake::Event(event), Some(correlation_id)) => event.is_task_chain(correlation_id.as_uuid()),
        (Wake::Event(_), None) => false,
    }
}

/// Why the board stream is about to redraw, and how much of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoardRedraw {
    /// Something in the company changed. The selected pane comes with the board only when one of
    /// the coalesced wake-ups was about that chain.
    Changed { refresh_selected: bool },
    /// Nothing happened; the window simply slid, so the board's cutoff moves and the pane does not.
    WindowSlid,
    /// The event source is gone, and with it the connection.
    Closed,
}

/// Wait for the next reason to redraw, absorbing a burst of wake-ups into one answer.
///
/// A single logical transaction commits task, outbox and approval rows together, so the wake-ups
/// arrive in a clump; redrawing per row would send the client several fragments describing the
/// same instant. Returning the decision rather than taking it inline is also what lets a
/// paused-time test drive the timer and the coalescing window with no HTTP request or database
/// behind them.
async fn next_board_redraw<Changes>(
    changes: &mut Changes,
    selected: Option<CorrelationId>,
    window_tick: &mut tokio::time::Interval,
) -> BoardRedraw
where
    Changes: Stream<Item = Wake> + Unpin,
{
    tokio::select! {
        first = changes.next() => {
            let Some(first) = first else { return BoardRedraw::Closed };
            let mut refresh_selected = wake_touches(&first, selected);
            while let Ok(Some(wake)) =
                tokio::time::timeout(WAKE_COALESCE_WINDOW, changes.next()).await
            {
                refresh_selected |= wake_touches(&wake, selected);
            }
            BoardRedraw::Changed { refresh_selected }
        }
        _ = window_tick.tick() => BoardRedraw::WindowSlid,
    }
}

/// GET /ui/tasks/events - Company-filtered board wake-ups.
///
/// Notifications carry identifiers only. They are coalesced and every emitted fragment is a
/// fresh bounded database projection, including the selected pane when that chain changed.
#[instrument(skip(workspace, events))]
async fn task_board_stream(
    workspace: Workspace,
    State(events): State<MailboxEvents>,
    Query(query): Query<TasksQuery>,
) -> AppResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let company = workspace.scoped_company(query.company_id).await?;
    let company_id = company.id;
    let selected = query.correlation_id.map(CorrelationId::from);
    let mut changes = Box::pin(task_chain_wake_ups(&events, "task-board", company_id));

    let stream = async_stream::stream! {
        // The first pass always paints the pane: the client has nothing yet.
        let mut refresh_selected = true;
        let mut window_tick = tokio::time::interval_at(
            tokio::time::Instant::now() + BOARD_WINDOW_REFRESH,
            BOARD_WINDOW_REFRESH,
        );
        loop {
            // Recomputed every pass so the seven-day window slides with the connection rather than
            // freezing at whatever it was when the mailbox was opened.
            let filter = query.board_filter();
            match workspace.view(&company).board(filter).await {
                Ok(board) => yield Ok(Event::default().event("task-board").data(
                    pages::task_board_fragment(
                        company_id,
                        &board,
                        filter,
                        selected,
                        pages::FragmentSwap::Inline,
                    )
                )),
                Err(error) => {
                    warn!(%error, %company_id, "Task board stream query failed");
                    return;
                }
            }
            if refresh_selected && let Some(correlation_id) = selected {
                match workspace.view(&company).chain(correlation_id).await {
                    Ok(Some(detail)) => yield Ok(Event::default().event("task-chain").data(
                        pages::task_chain_detail_pane(&detail, None)
                    )),
                    Ok(None) => yield Ok(Event::default().event("task-chain").data(
                        pages::task_chain_empty_pane("Task chain not found.")
                    )),
                    Err(error) => {
                        warn!(%error, %correlation_id, "Task chain stream query failed");
                        return;
                    }
                }
            }

            refresh_selected = match
                next_board_redraw(&mut changes, selected, &mut window_tick).await
            {
                BoardRedraw::Changed { refresh_selected } => refresh_selected,
                // The window slid; nothing about the selected chain changed, so redrawing its
                // pane would only cost a query and blink the operator's scroll position.
                BoardRedraw::WindowSlid => false,
                BoardRedraw::Closed => return,
            };
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
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

    let stopped = view
        .worker()
        .await
        .stop_task_and_notify(task.id, StopActor::Operator(workspace.user_id))
        .await;
    view.after_write(
        &task,
        &query,
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

    let resumed = view
        .worker()
        .await
        .resume_task(task.id, ResumeActor::Operator(workspace.user_id))
        .await;
    view.after_write(
        &task,
        &query,
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
    monitoring: &'a dyn MonitoringService,
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
        record_pagination_observation(self.monitoring, "tasks", filter.offset());
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

    async fn board(&self, filter: TaskBoardFilter) -> AppResult<TaskChainBoard> {
        self.thread_use_cases
            .get_task_persistence()
            .await
            .list_task_chain_board(self.company.id, filter)
            .await
    }

    async fn chain(&self, correlation_id: CorrelationId) -> AppResult<Option<TaskChainDetail>> {
        self.thread_use_cases
            .get_task_persistence()
            .await
            .get_task_chain_detail(self.company.id, correlation_id)
            .await
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

        // The transport is a separate process and never writes back into the task, so its state is
        // joined in here, at render time.
        let persistence = self.thread_use_cases.get_task_persistence().await;
        let (deliveries, delivery_error) = match persistence.list_task_deliveries(task.id).await {
            Ok(deliveries) => (deliveries, None),
            Err(error) => {
                warn!(task_id = %task.id, %error, "Could not load task delivery details");
                (
                    Vec::new(),
                    Some("Delivery details could not be loaded. Reload the task to try again."),
                )
            }
        };
        let (attempts, attempts_error) = match persistence
            .list_task_attempts(self.company.id, task.id)
            .await
        {
            Ok(attempts) => (attempts, None),
            Err(error) => {
                warn!(task_id = %task.id, %error, "Could not load task execution attempts");
                (
                    Vec::new(),
                    Some("Execution attempts could not be loaded. Reload the task to try again."),
                )
            }
        };

        Ok(pages::task_detail_pane(&pages::TaskDetailPane {
            company_id: self.company.id,
            task,
            channel: channel.as_ref(),
            deliveries: &deliveries,
            delivery_error,
            attempts: &attempts,
            attempts_error,
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
        query: &TasksQuery,
        error: Option<String>,
    ) -> AppResult<Response> {
        if query.view.as_deref() != Some("list") {
            let correlation_id = query
                .correlation_id
                .map(CorrelationId::from)
                .unwrap_or(task.correlation_id);
            let detail = self
                .chain(correlation_id)
                .await?
                .ok_or_else(|| AppError::NotFound("Task chain not found".into()))?;
            let pane = pages::task_chain_detail_pane(&detail, error.as_deref());
            let board_filter = query.board_filter();
            let board = self.board(board_filter).await?;
            let board = pages::task_board_fragment(
                self.company.id,
                &board,
                board_filter,
                Some(correlation_id),
                pages::FragmentSwap::OutOfBand,
            );
            return Ok(Html(format!("{pane}{board}")).into_response());
        }

        let filter = query.filter();
        let refreshed = self.require_task(task.id).await?;
        let pane = self.pane(&refreshed, error.as_deref()).await?;

        let (tasks, has_next) = self.page(&filter).await?;
        let list = pages::task_monitor_list(
            &self.list(&tasks, has_next, &filter, Some(refreshed.id)),
            pages::FragmentSwap::OutOfBand,
        );

        Ok(Html(format!("{pane}{list}")).into_response())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::events::{MailboxEvent, TaskChainScope, ThreadScope};

    fn chain_event(company_id: Uuid, correlation_id: Uuid) -> Wake {
        Wake::Event(MailboxEvent::TaskChainChanged(TaskChainScope {
            company_id,
            correlation_id,
        }))
    }

    #[test]
    fn only_the_selected_chain_or_an_unknown_gap_rebuilds_the_pane() {
        let company_id = Uuid::new_v4();
        let selected = CorrelationId::from(Uuid::new_v4());
        let other = Uuid::new_v4();

        assert!(wake_touches(&Wake::Lagged, Some(selected)));
        assert!(
            wake_touches(&Wake::Lagged, None),
            "lag means we cannot tell what was missed, so everything is redrawn"
        );
        assert!(wake_touches(
            &chain_event(company_id, selected.as_uuid()),
            Some(selected)
        ));
        assert!(!wake_touches(
            &chain_event(company_id, other),
            Some(selected)
        ));
        assert!(!wake_touches(
            &chain_event(company_id, selected.as_uuid()),
            None
        ));

        // A thread event reaching this stream at all would be a filter bug, but it is not a reason
        // to re-project a chain.
        let thread = Wake::Event(MailboxEvent::ActivityChanged(ThreadScope {
            thread_id: Uuid::new_v4(),
            channel_id: Uuid::new_v4(),
            company_id,
        }));
        assert!(!wake_touches(&thread, Some(selected)));
    }

    /// The window has to keep sliding on a connection nobody is writing to: a board left open
    /// overnight would otherwise still be answering with the cutoff it was opened with.
    #[tokio::test(start_paused = true)]
    async fn a_quiet_connection_still_slides_its_window_without_touching_the_pane() {
        let events = MailboxEvents::new();
        let company_id = Uuid::new_v4();
        let selected = CorrelationId::from(Uuid::new_v4());
        let mut changes = Box::pin(task_chain_wake_ups(&events, "test", company_id));
        let mut window_tick = tokio::time::interval_at(
            tokio::time::Instant::now() + BOARD_WINDOW_REFRESH,
            BOARD_WINDOW_REFRESH,
        );

        assert_eq!(
            next_board_redraw(&mut changes, Some(selected), &mut window_tick).await,
            BoardRedraw::WindowSlid
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_burst_redraws_the_pane_only_when_one_of_its_wake_ups_names_that_chain() {
        let events = MailboxEvents::new();
        let company_id = Uuid::new_v4();
        let selected = CorrelationId::from(Uuid::new_v4());
        let mut changes = Box::pin(task_chain_wake_ups(&events, "test", company_id));
        let mut window_tick = tokio::time::interval_at(
            tokio::time::Instant::now() + BOARD_WINDOW_REFRESH,
            BOARD_WINDOW_REFRESH,
        );

        for _ in 0..3 {
            events.publish(MailboxEvent::TaskChainChanged(TaskChainScope {
                company_id,
                correlation_id: Uuid::new_v4(),
            }));
        }
        assert_eq!(
            next_board_redraw(&mut changes, Some(selected), &mut window_tick).await,
            BoardRedraw::Changed {
                refresh_selected: false
            },
            "other chains move the board's counts, not the open pane"
        );

        // The selected chain arrives last in the burst, so a decision taken on the first wake-up
        // alone would miss it.
        for correlation_id in [Uuid::new_v4(), Uuid::new_v4(), selected.as_uuid()] {
            events.publish(MailboxEvent::TaskChainChanged(TaskChainScope {
                company_id,
                correlation_id,
            }));
        }
        assert_eq!(
            next_board_redraw(&mut changes, Some(selected), &mut window_tick).await,
            BoardRedraw::Changed {
                refresh_selected: true
            }
        );
    }

    #[tokio::test]
    async fn a_closed_event_source_ends_the_connection() {
        let events = MailboxEvents::new();
        let mut changes = Box::pin(tokio_stream::iter(Vec::<Wake>::new()));
        let mut window_tick = tokio::time::interval_at(
            tokio::time::Instant::now() + BOARD_WINDOW_REFRESH,
            BOARD_WINDOW_REFRESH,
        );
        drop(events);

        assert_eq!(
            next_board_redraw(&mut changes, None, &mut window_tick).await,
            BoardRedraw::Closed
        );
    }

    /// The cutoff is computed per pass, not captured once — which is the whole reason the stream
    /// calls this inside its loop rather than before it.
    #[test]
    fn the_board_cutoff_moves_with_the_clock() {
        let query = TasksQuery {
            company_id: None,
            view: None,
            correlation_id: None,
            task_id: None,
            channel_id: None,
            status: None,
            sort: None,
            page: None,
            limit: None,
        };
        let first = query.board_filter();
        std::thread::sleep(Duration::from_millis(2));
        assert!(query.board_filter().terminal_since > first.terminal_since);
    }
}
