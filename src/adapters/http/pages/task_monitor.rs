//! The `/ui` Tasks workspace: the same shell as the mailbox, with the company's background tasks
//! watched rather than its mail read.
//!
//! The sidebar filters and picks a task, and that task's detail is swapped into `#task-pane` over
//! htmx, the way picking an agent swaps `#agent-pane`. Filtering and paging swap only `#task-list`,
//! so the filter form keeps what the user chose; stopping or resuming a task re-renders the pane
//! and sends the list along out of band, so the new status shows up in both places at once.

use super::*;

/// The task list in the sidebar: what one filtered page of tasks looks like.
pub struct TaskMonitorList<'a> {
    pub company: &'a Company,
    pub tasks: &'a [BackgroundTask],
    pub filter: &'a TaskFilter,
    /// Whether a further page follows this one, so the pager knows to offer Next.
    pub has_next: bool,
    pub selected_task_id: Option<Uuid>,
}

/// The Tasks workspace for one request.
pub struct TaskMonitorPage<'a> {
    pub user: &'a MailboxUser<'a>,
    pub companies: &'a [Company],
    /// The company's channels, as the sidebar's channel filter offers them.
    pub channels: &'a [Channel],
    pub list: &'a TaskMonitorList<'a>,
    /// Pre-rendered right-hand pane: one task's detail, or a placeholder.
    pub pane_html: &'a str,
}

/// The pane for one task: everything the queue recorded about a single run.
pub struct TaskDetailPane<'a> {
    pub company_id: Uuid,
    pub task: &'a BackgroundTask,
    /// The channel this task ran for, when it is still one of the company's.
    pub channel: Option<&'a Channel>,
    /// What the transport did with the emails this task produced.
    ///
    /// The task never fails because a delivery failed — composition and transport are separate
    /// processes — so this is the only place an undelivered reply becomes visible.
    pub deliveries: &'a [OutboxEntry],
    /// A delivery read failure is different from a task that produced no mail.
    pub delivery_error: Option<&'a str>,
    /// Durable history for every run, including failures overwritten by a later task payload.
    pub attempts: &'a [TaskAttemptRecord],
    /// Why the attempt ledger could not be loaded, when the rest of the task still can render.
    pub attempts_error: Option<&'a str>,
    /// Why a stop or resume did not happen, when one was asked for and refused.
    pub error: Option<&'a str>,
}

/// The `/ui/tasks` URL for a given selection, i.e. what a click on it should leave in the address
/// bar. The fragment endpoints take exactly the same parameters, so they share this.
pub fn task_monitor_query(
    company_id: Uuid,
    filter: &TaskFilter,
    selected_task_id: Option<Uuid>,
) -> String {
    let mut params = vec![format!("company_id={company_id}"), "view=list".to_string()];
    if let Some(channel_id) = filter.channel_id {
        params.push(format!("channel_id={channel_id}"));
    }
    if let Some(status) = filter.status {
        params.push(format!("status={}", status.as_str()));
    }
    if filter.sort_asc {
        params.push("sort=asc".to_string());
    }
    if filter.limit() != TaskFilter::DEFAULT_PAGE_SIZE {
        params.push(format!("limit={}", filter.limit()));
    }
    if filter.page() > 1 {
        params.push(format!("page={}", filter.page()));
    }
    if let Some(task_id) = selected_task_id {
        params.push(format!("task_id={task_id}"));
    }
    params.join("&")
}

/// Where a selection points: the workspace itself for the address bar, `/ui/tasks/list` for the
/// htmx swap that actually fetches it.
fn task_monitor_url(
    company_id: Uuid,
    filter: &TaskFilter,
    selected_task_id: Option<Uuid>,
) -> String {
    format!(
        "/ui/tasks?{}",
        task_monitor_query(company_id, filter, selected_task_id)
    )
}

fn task_list_url(company_id: Uuid, filter: &TaskFilter, selected_task_id: Option<Uuid>) -> String {
    format!(
        "/ui/tasks/list?{}",
        task_monitor_query(company_id, filter, selected_task_id)
    )
}

pub fn task_monitor_page(page: &TaskMonitorPage<'_>) -> String {
    let company = page.list.company;
    let content = format!(
        r##"
        <aside class="ui-pane-list flex w-80 shrink-0 flex-col border-r border-base-300 bg-base-200">
            {header}
            <div class="join mx-3 mb-3" aria-label="Tasks view">
                <a class="btn btn-sm join-item flex-1" href="/ui/tasks?company_id={company_id}&amp;view=board">Board</a>
                <a class="btn btn-sm join-item btn-primary flex-1" aria-current="page">List</a>
            </div>
            {filters}
            {list_html}
        </aside>
        {pane_html}
        "##,
        header = sidebar_header("Tasks", "Background worker execution queue and states."),
        filters = task_filter_form(company.id, page.channels, page.list.filter),
        list_html = task_monitor_list(page.list, FragmentSwap::Inline),
        pane_html = page.pane_html,
        company_id = company.id,
    );

    ui_shell(&UiShell {
        title: &format!("{} Tasks", company.name),
        user: page.user,
        company: Some(company),
        section: UiSection::Tasks,
        content: &content,
    })
}

/// Channel, status and order, as three selects that re-fetch the list between them.
///
/// The form sits outside `#task-list`, so a swap never takes the controls out from under the
/// pointer; a change resets to the first page simply by not sending one.
fn task_filter_form(company_id: Uuid, channels: &[Channel], filter: &TaskFilter) -> String {
    let channel_options = channel_filter_options(channels, filter.channel_id);

    let status_options: String = TASK_STATUS_FILTERS
        .iter()
        .map(|status| {
            format!(
                r##"<option value="{value}"{selected}>{label}</option>"##,
                value = status.as_str(),
                selected = selected_when(filter.status == Some(*status)),
                label = task_status_label(*status),
            )
        })
        .collect();

    format!(
        r##"
            <form class="space-y-2 border-b border-base-300 px-3 pb-3"
                hx-get="/ui/tasks/list" hx-trigger="change"
                hx-target="#task-list" hx-swap="outerHTML" hx-sync="#task-list:replace">
                <input type="hidden" name="company_id" value="{company_id}">
                <input type="hidden" name="limit" value="{limit}">
                <select name="channel_id" class="select select-sm w-full" aria-label="Filter by channel">
                    <option value="">All channels</option>
                    {channel_options}
                </select>
                <select name="status" class="select select-sm w-full" aria-label="Filter by status">
                    <option value="">All statuses</option>
                    {status_options}
                </select>
                <select name="sort" class="select select-sm w-full" aria-label="Sort by time">
                    <option value="desc"{newest_selected}>Newest first</option>
                    <option value="asc"{oldest_selected}>Oldest first</option>
                </select>
            </form>
        "##,
        limit = filter.limit(),
        newest_selected = selected_when(!filter.sort_asc),
        oldest_selected = selected_when(filter.sort_asc),
    )
}

/// One filtered page of tasks, rendered out of band after a write so a stop or a resume shows its
/// new status in the list as well as in the pane.
pub fn task_monitor_list(list: &TaskMonitorList<'_>, swap: FragmentSwap) -> String {
    let company_id = list.company.id;
    let entries: String = list
        .tasks
        .iter()
        .map(|task| task_menu_entry(list, task))
        .collect();

    let menu_body = if list.tasks.is_empty() {
        r##"<li class="px-2 py-6 text-center text-xs opacity-60">No tasks match these filters.</li>"##
            .to_string()
    } else {
        entries
    };

    format!(
        r##"
            <div id="task-list"{LIST_SKELETON} class="flex min-h-0 flex-1 flex-col"{oob}>
                <div class="flex items-center justify-between gap-2 px-3 py-2 text-[11px] opacity-60">
                    <span class="truncate">{summary}</span>
                    <button type="button" class="btn btn-ghost btn-xs" title="Reload this page of tasks"
                        hx-get="{reload_url}" hx-target="#task-list" hx-swap="outerHTML" hx-sync="#task-list:replace">{reload_glyph}</button>
                </div>
                <ul id="task-menu" class="menu w-full flex-1 flex-nowrap gap-1 overflow-y-auto px-2">{menu_body}</ul>
                {pager}
            </div>
        "##,
        oob = swap.oob_attribute(),
        summary = escape_html_text(&task_page_summary(list)),
        reload_glyph = icon(Icon::Sync, BUTTON_ICON),
        reload_url = task_list_url(company_id, list.filter, list.selected_task_id),
        pager = task_pager(list),
    )
}

/// What this page of tasks adds up to: how many ran, and what they cost in tokens.
fn task_page_summary(list: &TaskMonitorList<'_>) -> String {
    let totals = total_token_usage(list.tasks);
    let count = list.tasks.len();
    let page = list.filter.page();

    let counted = if count == 1 {
        "1 task".to_string()
    } else {
        format!("{count} tasks")
    };

    if totals.total == 0 {
        format!("{counted} · page {page}")
    } else {
        format!("{counted} · {} tokens · page {page}", totals.total)
    }
}

fn task_menu_entry(list: &TaskMonitorList<'_>, task: &BackgroundTask) -> String {
    let company_id = list.company.id;
    let tokens = match task.token_usage() {
        Some(usage) => format!(" · {} tokens", usage.total_tokens),
        None => String::new(),
    };

    format!(
        r##"
                <li>
                    <a class="flex flex-col items-start gap-0.5 {active}"
                        hx-get="/ui/tasks/{task_id}?company_id={company_id}"
                        hx-target="#task-pane" hx-swap="outerHTML"
                        hx-sync="#task-pane:replace"
                        hx-push-url="{push_url}"
                        data-action="select-sidebar-item">
                        <span class="flex w-full items-center gap-2">
                            <span class="badge badge-sm shrink-0 {status_style}">{status_label}</span>
                            <span class="min-w-0 truncate text-xs">{task_type}</span>
                        </span>
                        <span class="w-full truncate font-mono text-[11px] opacity-60">{enqueued}{tokens}</span>
                    </a>
                </li>
        "##,
        active = if list.selected_task_id == Some(task.id) {
            "menu-active"
        } else {
            ""
        },
        task_id = task.id,
        push_url = task_monitor_url(company_id, list.filter, Some(task.id)),
        status_style = task_status_style(task.status),
        status_label = task_status_label(task.status),
        task_type = escape_html_text(&task.task_type),
        enqueued = enqueued_at(task),
    )
}

/// Previous / next for the filtered list; absent entirely on a single-page list.
fn task_pager(list: &TaskMonitorList<'_>) -> String {
    let company_id = list.company.id;
    let filter = list.filter;
    let button = |page: usize, label: &str| {
        format!(
            r##"<button type="button" class="btn btn-ghost btn-xs"
                        hx-get="{url}" hx-target="#task-list" hx-swap="outerHTML" hx-sync="#task-list:replace">{label}</button>"##,
            url = task_list_url(company_id, &filter.on_page(page), list.selected_task_id),
        )
    };

    // Which way "back" runs depends on the order the list is in, so the labels follow the sort
    // rather than claiming the newest tasks are always first.
    let (back, forward) = (
        icon(Icon::ArrowLeft, BUTTON_ICON),
        icon(Icon::ArrowRight, BUTTON_ICON),
    );
    let (previous_label, next_label) = if filter.sort_asc {
        (format!("{back} Older"), format!("Newer {forward}"))
    } else {
        (format!("{back} Newer"), format!("Older {forward}"))
    };
    let previous = if filter.page() > 1 {
        button(filter.page() - 1, &previous_label)
    } else {
        String::new()
    };
    let next = if list.has_next {
        button(filter.page().saturating_add(1), &next_label)
    } else {
        String::new()
    };

    if previous.is_empty() && next.is_empty() {
        return String::new();
    }

    format!(
        r##"<nav aria-label="Task pages" class="flex items-center justify-between gap-2 border-t border-base-300 p-2">
                    <div>{previous}</div>
                    <div>{next}</div>
                </nav>"##
    )
}

/// The pane before a task is picked.
pub fn task_monitor_empty_pane(message: &str, swap: FragmentSwap) -> String {
    format!(
        r##"
        <section id="task-pane"{PANE_SKELETON} data-pane-empty class="ui-pane-detail flex min-w-0 flex-1 items-center justify-center bg-base-100 p-8"{oob}>
            <p class="text-center text-sm opacity-60">{message}</p>
        </section>
        "##,
        oob = swap.oob_attribute(),
        message = escape_html_text(message),
    )
}

pub fn task_detail_pane(pane: &TaskDetailPane<'_>) -> String {
    let task = pane.task;
    let company_id = pane.company_id;

    format!(
        r##"
        <section id="task-pane"{PANE_SKELETON} class="ui-pane-detail flex min-w-0 flex-1 flex-col bg-base-100">
            <div class="flex flex-wrap items-start justify-between gap-3 border-b border-base-300 px-4 py-4 sm:px-6">
                <div class="min-w-0 grow basis-48">
                    <h2 class="flex items-center gap-2 truncate text-xl font-bold">
                        <span class="badge {status_style}">{status_label}</span>
                        <span class="truncate">{task_type}</span>
                    </h2>
                    <p class="truncate font-mono text-xs opacity-60">{task_id}</p>
                </div>
                <div class="flex shrink-0 flex-wrap items-center gap-2">
                    {thread_link}
                    {action_button}
                    <button type="button" class="btn btn-ghost btn-sm btn-square" title="Reload this task"
                        hx-get="/ui/tasks/{task_id}?company_id={company_id}"
                        hx-target="#task-pane" hx-swap="outerHTML" hx-sync="#task-pane:replace">{reload_glyph}</button>
                </div>
            </div>
            <div class="flex-1 space-y-4 overflow-y-auto px-4 py-4 sm:px-6">
                {error_html}
                {token_stats}
                {latest_execution}
                {facts}
                {queue_diagnostics}
                {attempts_error}
                {attempts}
                {delivery_error}
                {deliveries}
                {last_error}
                {payload}
            </div>
        </section>
        "##,
        status_style = task_status_style(task.status),
        status_label = task_status_label(task.status),
        task_type = escape_html_text(&task.task_type),
        task_id = task.id,
        reload_glyph = icon(Icon::Sync, BUTTON_ICON),
        thread_link = task_thread_link(pane),
        action_button = task_action_button(company_id, task),
        error_html = form_error_banner(pane.error),
        token_stats = task_token_stats(task, pane.attempts),
        latest_execution = task_latest_execution(task),
        facts = task_facts(pane),
        queue_diagnostics = task_queue_diagnostics(task),
        attempts_error = data_load_warning(pane.attempts_error),
        attempts = task_attempts(pane.attempts),
        delivery_error = data_load_warning(pane.delivery_error),
        deliveries = task_deliveries(pane),
        last_error = task_last_error(task),
        payload = render_message_task_parameters_html(&task.payload),
    )
}

/// A task that produced a thread is worth reading, so the pane opens it in the mailbox rather than
/// only naming its id.
fn task_thread_link(pane: &TaskDetailPane<'_>) -> String {
    let Some(thread_id) = pane.task.thread_id else {
        return String::new();
    };

    format!(
        r##"<a href="/ui?company_id={company_id}&channel_id={channel_id}&thread_id={thread_id}"
                        class="btn btn-outline btn-sm">Open Thread</a>"##,
        company_id = pane.company_id,
        channel_id = pane.task.channel_id,
    )
}

/// Stop what is still running, resume what has given up; a finished task offers neither.
fn task_action_button(company_id: Uuid, task: &BackgroundTask) -> String {
    let (path, label, style, confirm) = match task.status {
        TaskStatus::Pending | TaskStatus::Processing | TaskStatus::Failed => (
            "stop",
            "Stop Task",
            "btn-error btn-outline",
            "Stop this task? Anything it was about to send will not be sent.",
        ),
        TaskStatus::Stopped | TaskStatus::DeadLetter => (
            "resume",
            "Resume Task",
            "btn-success",
            "Resume this task? It will be picked up by a worker again.",
        ),
        _ => return String::new(),
    };

    format!(
        r##"<button type="button" class="btn btn-sm {style}"
                        hx-post="/ui/tasks/{task_id}/{path}?company_id={company_id}"
                        hx-target="#task-pane" hx-swap="outerHTML"
                        hx-confirm="{confirm}">{label}</button>"##,
        task_id = task.id,
    )
}

/// What the run cost, when the model reported it.
fn task_token_stats(task: &BackgroundTask, attempts: &[TaskAttemptRecord]) -> String {
    let ledger_usage = attempts
        .iter()
        .fold((0_i64, 0_i64, false), |totals, attempt| {
            (
                totals.0 + i64::from(attempt.prompt_tokens.unwrap_or(0)),
                totals.1 + i64::from(attempt.completion_tokens.unwrap_or(0)),
                totals.2 || attempt.prompt_tokens.is_some() || attempt.completion_tokens.is_some(),
            )
        });
    let (prompt, completion, scope) = if ledger_usage.2 {
        (ledger_usage.0, ledger_usage.1, "All attempts")
    } else {
        let Some(usage) = task.token_usage() else {
            return String::new();
        };
        (
            usage.prompt_tokens as i64,
            usage.completion_tokens as i64,
            "Latest execution tokens",
        )
    };

    format!(
        r##"
                <div class="space-y-1">
                <h3 class="text-xs font-semibold uppercase opacity-60">{scope}</h3>
                <div class="stats stats-horizontal w-full border border-base-300 bg-base-200">
                    <div class="stat py-3">
                        <div class="stat-title text-xs">Prompt</div>
                        <div class="stat-value text-lg">{prompt}</div>
                    </div>
                    <div class="stat py-3">
                        <div class="stat-title text-xs">Completion</div>
                        <div class="stat-value text-lg">{completion}</div>
                    </div>
                    <div class="stat py-3">
                        <div class="stat-title text-xs">Total tokens</div>
                        <div class="stat-value text-lg text-primary">{total}</div>
                    </div>
                </div>
                </div>
        "##,
        total = prompt + completion,
    )
}

/// Promote the most useful fields out of the raw payload so an operator does not need to inspect
/// JSON to learn which model ran, how it stopped, or whether it produced a message.
fn task_latest_execution(task: &BackgroundTask) -> String {
    let payload = &task.payload;
    let parameters = payload.get("execution_parameters");
    let result = payload.get("execution_result");
    let metadata = result
        .and_then(|value| value.get("metadata"))
        .or_else(|| payload.get("metadata"));
    let diagnostics = metadata.and_then(|value| value.get("execution_diagnostics"));
    let observation = metadata
        .and_then(|value| value.get("observability"))
        .and_then(|value| value.get("summary"));

    let mut rows = Vec::new();
    push_execution_text(
        &mut rows,
        "Provider",
        parameters
            .and_then(|value| value.get("provider"))
            .and_then(|value| value.as_str()),
    );
    push_execution_text(
        &mut rows,
        "Model",
        parameters
            .and_then(|value| value.get("model"))
            .and_then(|value| value.as_str()),
    );
    push_execution_text(
        &mut rows,
        "Agent",
        parameters
            .and_then(|value| value.get("agent_name"))
            .and_then(|value| value.as_str()),
    );
    if let Some(executed_at) = parameters
        .and_then(|value| value.get("executed_at"))
        .and_then(|value| value.as_str())
    {
        let rendered = chrono::DateTime::parse_from_rfc3339(executed_at)
            .map(|at| super::format_time(at.with_timezone(&chrono::Utc)))
            .unwrap_or_else(|_| escape_html_text(executed_at));
        rows.push(("Executed", rendered));
    }
    if let Some(duration_ms) = diagnostics
        .and_then(|value| value.get("duration_ms"))
        .and_then(|value| value.as_u64())
    {
        rows.push((
            "Duration",
            format_duration_ms(i64::try_from(duration_ms).unwrap_or(i64::MAX)),
        ));
    }
    push_execution_text(
        &mut rows,
        "Finish reason",
        metadata
            .and_then(|value| {
                value
                    .get("finish_reason")
                    .or_else(|| value.get("stop_reason"))
            })
            .and_then(|value| value.as_str()),
    );
    if let Some(count) = diagnostics
        .and_then(|value| value.get("tool_call_count"))
        .and_then(|value| value.as_u64())
    {
        rows.push(("Tool calls", count.to_string()));
    }
    if let Some(count) = observation
        .and_then(|value| value.get("total_llm_calls"))
        .and_then(|value| value.as_u64())
    {
        rows.push(("LLM calls", count.to_string()));
    }
    if let Some(count) = observation
        .and_then(|value| value.get("total_events"))
        .and_then(|value| value.as_u64())
    {
        rows.push(("Observed events", count.to_string()));
    }
    if let Some(count) = diagnostics
        .and_then(|value| value.get("response_characters"))
        .and_then(|value| value.as_u64())
    {
        rows.push(("Response size", format!("{count} characters")));
    }
    push_execution_text(
        &mut rows,
        "Token source",
        diagnostics
            .and_then(|value| value.get("token_usage_source"))
            .and_then(|value| value.as_str()),
    );
    if let Some(sent) = result
        .and_then(|value| value.get("email_sent"))
        .and_then(|value| value.as_bool())
    {
        rows.push((
            "Email produced",
            if sent { "Yes" } else { "No" }.to_string(),
        ));
    }
    push_execution_text(
        &mut rows,
        "Outbound message",
        result
            .and_then(|value| value.get("outbound_message_id"))
            .and_then(|value| value.as_str()),
    );
    push_execution_text(
        &mut rows,
        "Execution error",
        result
            .and_then(|value| value.get("error"))
            .and_then(|value| value.as_str()),
    );

    if rows.is_empty() {
        return String::new();
    }

    format!(
        r##"<div class="space-y-2">
                    <h3 class="text-xs font-semibold uppercase opacity-60">Latest execution</h3>
                    <dl class="grid gap-px overflow-hidden rounded-box border border-base-300 bg-base-300 sm:grid-cols-2">{}</dl>
                </div>"##,
        rows.into_iter()
            .map(|(label, value)| execution_fact(label, &value))
            .collect::<String>()
    )
}

fn push_execution_text(
    rows: &mut Vec<(&'static str, String)>,
    label: &'static str,
    value: Option<&str>,
) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        rows.push((label, escape_html_text(value)));
    }
}

fn execution_fact(label: &str, value_html: &str) -> String {
    format!(
        r##"<div class="min-w-0 bg-base-200 px-4 py-2">
                    <dt class="text-[11px] uppercase opacity-60">{label}</dt>
                    <dd class="truncate font-mono text-xs">{value_html}</dd>
                </div>"##
    )
}

fn format_duration_ms(duration_ms: i64) -> String {
    if duration_ms < 1_000 {
        format!("{duration_ms} ms")
    } else {
        format!("{:.2} s", duration_ms as f64 / 1_000.0)
    }
}

/// The queue's own record of the task: where it ran, when, and how often it has been tried.
fn task_facts(pane: &TaskDetailPane<'_>) -> String {
    let task = pane.task;
    let channel = match pane.channel {
        Some(channel) => escape_html_text(&channel.name),
        None => task.channel_id.to_string(),
    };
    let thread = match task.thread_id {
        Some(thread_id) => thread_id.to_string(),
        None => "—".to_string(),
    };
    let worker = match task.worker_id {
        Some(worker_id) => worker_id.to_string(),
        None => "—".to_string(),
    };

    let rows: String = [
        ("Channel", channel),
        ("Thread", thread),
        ("Enqueued", enqueued_at(task)),
        ("Runs at", super::format_time(task.run_at)),
        ("Updated", super::format_time(task.updated_at)),
        (
            "Retries",
            format!("{}/{}", task.retry_count, task.max_retries),
        ),
        ("Worker", worker),
    ]
    .into_iter()
    .map(|(label, value)| {
        format!(
            r##"<div class="flex items-baseline justify-between gap-4 px-4 py-2">
                        <dt class="text-xs opacity-60">{label}</dt>
                        <dd class="min-w-0 truncate font-mono text-xs">{value}</dd>
                    </div>"##
        )
    })
    .collect();

    format!(
        r##"<dl class="divide-y divide-base-300 rounded-box border border-base-300 bg-base-200">{rows}</dl>"##
    )
}

fn task_queue_diagnostics(task: &BackgroundTask) -> String {
    let mut rows = vec![("Correlation ID", task.correlation_id.to_string())];
    if let Some(locked_at) = task.locked_at {
        rows.push(("Locked at", super::format_time(locked_at)));
    }
    if let Some(lock_expires_at) = task.lock_expires_at {
        rows.push(("Lease expires", super::format_time(lock_expires_at)));
    }
    if let Some(generation) = task.execution_generation {
        rows.push(("Execution generation", generation.to_string()));
    }

    let rows = rows
        .into_iter()
        .map(|(label, value)| {
            format!(
                r##"<div class="flex items-baseline justify-between gap-4 px-4 py-2">
                        <dt class="text-xs opacity-60">{label}</dt>
                        <dd class="min-w-0 truncate font-mono text-xs">{value}</dd>
                    </div>"##
            )
        })
        .collect::<String>();

    format!(
        r##"<details class="rounded-box border border-base-300 bg-base-200">
                    <summary class="cursor-pointer px-4 py-2 text-xs font-semibold uppercase opacity-70">Queue diagnostics</summary>
                    <dl class="divide-y divide-base-300 border-t border-base-300">{rows}</dl>
                </details>"##
    )
}

fn task_attempts(attempts: &[TaskAttemptRecord]) -> String {
    if attempts.is_empty() {
        return String::new();
    }

    let total_tokens = attempts
        .iter()
        .filter_map(TaskAttemptRecord::total_tokens)
        .sum::<i64>();
    let summary = if total_tokens > 0 {
        format!("{} runs · {total_tokens} tokens", attempts.len())
    } else {
        format!("{} runs", attempts.len())
    };
    let rows = attempts
        .iter()
        .rev()
        .map(task_attempt_row)
        .collect::<String>();

    format!(
        r##"<div class="space-y-2">
                    <div class="flex items-baseline justify-between gap-3">
                        <h3 class="text-xs font-semibold uppercase opacity-60">Execution attempts</h3>
                        <span class="text-[11px] opacity-60">{summary}</span>
                    </div>
                    <div class="space-y-2">{rows}</div>
                </div>"##
    )
}

fn task_attempt_row(attempt: &TaskAttemptRecord) -> String {
    let (status_label, status_style) = match attempt.status {
        TaskAttemptRecordStatus::Processing => ("Processing", "badge-info animate-pulse"),
        TaskAttemptRecordStatus::Completed => ("Completed", "badge-success"),
        TaskAttemptRecordStatus::Failed => ("Failed", "badge-error"),
    };
    let finished = attempt
        .finished_at
        .map(super::format_time)
        .unwrap_or_else(|| "Still running".to_string());
    let duration = attempt
        .duration_ms()
        .map(format_duration_ms)
        .unwrap_or_else(|| "—".to_string());
    let stop_reason = attempt
        .stop_reason
        .map(|reason| reason.to_string())
        .unwrap_or_else(|| "—".to_string());
    let tokens = attempt
        .total_tokens()
        .map(|total| {
            format!(
                "{total} total ({} prompt, {} completion)",
                attempt.prompt_tokens.unwrap_or(0),
                attempt.completion_tokens.unwrap_or(0)
            )
        })
        .unwrap_or_else(|| "—".to_string());
    let error = attempt.error.as_deref().filter(|error| !error.is_empty()).map(|error| {
        format!(
            r##"<div class="alert alert-error mt-3 text-xs"><span class="font-mono">{}</span></div>"##,
            escape_html_text(error)
        )
    }).unwrap_or_default();
    let result = attempt
        .result
        .as_ref()
        .map(attempt_result)
        .unwrap_or_default();

    format!(
        r##"<details class="rounded-box border border-base-300 bg-base-200">
                    <summary class="flex cursor-pointer items-center gap-2 px-4 py-3">
                        <span class="badge badge-sm {status_style}">{status_label}</span>
                        <span class="font-semibold">Attempt {attempt_number}</span>
                        <span class="ml-auto text-[11px] opacity-60">{duration}</span>
                    </summary>
                    <div class="border-t border-base-300 px-4 py-3">
                        <dl class="grid gap-x-6 gap-y-2 sm:grid-cols-2">
                            <div><dt class="text-[11px] uppercase opacity-60">Started</dt><dd class="font-mono text-xs">{started}</dd></div>
                            <div><dt class="text-[11px] uppercase opacity-60">Finished</dt><dd class="font-mono text-xs">{finished}</dd></div>
                            <div><dt class="text-[11px] uppercase opacity-60">Stop reason</dt><dd class="font-mono text-xs">{stop_reason}</dd></div>
                            <div><dt class="text-[11px] uppercase opacity-60">Tokens</dt><dd class="font-mono text-xs">{tokens}</dd></div>
                            <div class="sm:col-span-2"><dt class="text-[11px] uppercase opacity-60">Execution generation</dt><dd class="truncate font-mono text-xs">{generation}</dd></div>
                        </dl>
                        {error}
                        {result}
                    </div>
                </details>"##,
        attempt_number = attempt.attempt_number,
        started = super::format_time(attempt.started_at),
        generation = attempt.execution_generation,
    )
}

fn attempt_result(result: &serde_json::Value) -> String {
    let sanitized = sanitize_json_payload(result);
    let json = serde_json::to_string_pretty(&sanitized).unwrap_or_else(|_| sanitized.to_string());
    format!(
        r##"<details class="mt-3 border-t border-base-300 pt-3">
                    <summary class="cursor-pointer text-xs font-semibold opacity-70">Attempt result</summary>
                    <pre class="mt-2 max-h-80 overflow-auto whitespace-pre-wrap rounded-box bg-base-300 p-3 font-mono text-[11px]">{}</pre>
                </details>"##,
        escape_html_text(&json)
    )
}

fn data_load_warning(error: Option<&str>) -> String {
    error
        .map(|error| {
            format!(
                r##"<div class="alert alert-warning text-xs"><span>{}</span></div>"##,
                escape_html_text(error)
            )
        })
        .unwrap_or_default()
}

/// The transport's side of the story: one row per email this task handed off.
///
/// Renders nothing when the task sent nothing, so tasks that never produce mail are unchanged.
fn task_deliveries(pane: &TaskDetailPane<'_>) -> String {
    if pane.deliveries.is_empty() {
        return String::new();
    }

    let rows: String = pane
        .deliveries
        .iter()
        .map(|delivery| delivery_row(pane.company_id, delivery))
        .collect();
    format!(
        r##"<div class="space-y-2">
                    <h3 class="text-xs font-semibold uppercase opacity-60">Delivery</h3>
                    <div class="space-y-2">{rows}</div>
                </div>"##
    )
}

/// One queued email, as a link into the Outbox workspace — this row says *that* something went
/// wrong, and that workspace says what.
fn delivery_row(company_id: Uuid, delivery: &OutboxEntry) -> String {
    // A pending row is waiting out its backoff; a failed one has exhausted every attempt and is
    // the case this whole section exists to surface.
    let detail = match delivery.status {
        OutboxStatus::Sent => match delivery.sent_at {
            Some(sent_at) => format!("sent {}", super::format_time(sent_at)),
            None => "sent".to_string(),
        },
        OutboxStatus::Sending => "delivering now".to_string(),
        OutboxStatus::Failed => "gave up after every attempt".to_string(),
        OutboxStatus::Pending => {
            format!("next attempt {}", super::format_time(delivery.available_at))
        }
    };

    let attempts = if delivery.retry_count > 0 {
        format!(
            r##"<span class="opacity-60">· {} failed attempt(s)</span>"##,
            delivery.retry_count
        )
    } else {
        String::new()
    };

    let error = match delivery.last_error.as_deref() {
        Some(error) if !error.is_empty() => format!(
            r##"<div class="mt-1 font-mono text-[11px] opacity-70">{}</div>"##,
            escape_html_text(error)
        ),
        _ => String::new(),
    };

    format!(
        r##"<a class="block rounded-box border border-base-300 bg-base-200 px-4 py-2 text-xs hover:border-primary"
                        href="/ui/outbox?company_id={company_id}&entry_id={entry_id}">
                        <div class="flex flex-wrap items-center gap-2">
                            <span class="badge badge-sm {style}">{label}</span>
                            <span class="min-w-0 truncate">{subject}</span>
                            <span class="opacity-60">{detail}</span>
                            {attempts}
                        </div>
                        {error}
                    </a>"##,
        entry_id = delivery.id,
        style = outbox_status_style(delivery.status),
        label = delivery.status.label(),
        subject = escape_html_text(delivery.subject().unwrap_or("(no subject)")),
    )
}

fn task_last_error(task: &BackgroundTask) -> String {
    match task.last_error.as_deref() {
        Some(error) if !error.is_empty() => format!(
            r##"<div class="alert alert-error text-xs"><span class="font-mono">{}</span></div>"##,
            escape_html_text(error)
        ),
        _ => String::new(),
    }
}

fn enqueued_at(task: &BackgroundTask) -> String {
    super::format_time(task.created_at)
}

/// The statuses the sidebar filter offers, in the order a task moves through them.
const TASK_STATUS_FILTERS: [TaskStatus; 8] = [
    TaskStatus::Pending,
    TaskStatus::Processing,
    TaskStatus::PendingApproval,
    TaskStatus::WaitingForThirdPartyReply,
    TaskStatus::Completed,
    TaskStatus::Failed,
    TaskStatus::DeadLetter,
    TaskStatus::Stopped,
];

/// One name per status, shared by the filter, the list and the pane so a task reads the same
/// wherever it appears.
pub(crate) fn task_status_label(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "Pending",
        TaskStatus::Processing => "Processing",
        TaskStatus::PendingApproval => "Awaiting Approval",
        TaskStatus::WaitingForThirdPartyReply => "Awaiting Reply",
        TaskStatus::Completed => "Completed",
        TaskStatus::Failed => "Failed",
        TaskStatus::DeadLetter => "Dead Letter",
        TaskStatus::Stopped => "Stopped",
    }
}

pub(crate) fn task_status_style(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "badge-warning",
        TaskStatus::Processing => "badge-info animate-pulse",
        TaskStatus::PendingApproval => "badge-accent",
        TaskStatus::WaitingForThirdPartyReply => "badge-info badge-outline",
        TaskStatus::Completed => "badge-success",
        TaskStatus::Failed => "badge-error",
        TaskStatus::DeadLetter => "badge-error badge-outline",
        TaskStatus::Stopped => "badge-ghost",
    }
}
