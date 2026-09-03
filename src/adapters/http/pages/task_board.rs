//! Correlation-chain Kanban for `/ui/tasks?view=board`.

use super::*;

pub struct TaskBoardPage<'a> {
    pub user: &'a MailboxUser<'a>,
    pub companies: &'a [Company],
    pub company: &'a Company,
    pub channels: &'a [Channel],
    pub board: &'a TaskChainBoard,
    pub filter: TaskBoardFilter,
    pub selected_correlation_id: Option<CorrelationId>,
    pub pane_html: &'a str,
}

pub fn task_board_page(page: &TaskBoardPage<'_>) -> String {
    let query = task_board_query(
        page.company.id,
        page.filter.channel_id,
        page.selected_correlation_id,
    );
    let content = format!(
        r##"
        <section class="flex min-w-0 flex-1 overflow-hidden bg-base-100"
            hx-ext="sse" sse-connect="/ui/tasks/events?{query}">
            <div class="flex min-w-0 flex-1 flex-col overflow-hidden">
                {toolbar}
                {board}
            </div>
            {pane}
        </section>
        "##,
        toolbar = task_board_toolbar(page.company, page.channels, page.filter),
        board = task_board_fragment(
            page.company.id,
            page.board,
            page.filter,
            page.selected_correlation_id,
            FragmentSwap::Inline,
        ),
        pane = page.pane_html,
    );

    ui_shell(&UiShell {
        title: &format!("{} Tasks", page.company.name),
        user: page.user,
        company: Some(page.company),
        section: UiSection::Tasks,
        content: &content,
    })
}

pub fn task_board_query(
    company_id: Uuid,
    channel_id: Option<Uuid>,
    correlation_id: Option<CorrelationId>,
) -> String {
    let mut params = vec![format!("company_id={company_id}"), "view=board".to_string()];
    if let Some(channel_id) = channel_id {
        params.push(format!("channel_id={channel_id}"));
    }
    if let Some(correlation_id) = correlation_id {
        params.push(format!("correlation_id={correlation_id}"));
    }
    params.join("&")
}

fn task_board_toolbar(company: &Company, channels: &[Channel], filter: TaskBoardFilter) -> String {
    let options = channel_filter_options(channels, filter.channel_id);
    let list_url = format!(
        "/ui/tasks?company_id={}&view=list{}",
        company.id,
        filter
            .channel_id
            .map(|id| format!("&channel_id={id}"))
            .unwrap_or_default()
    );
    format!(
        r##"<header class="flex flex-wrap items-center justify-between gap-3 border-b border-base-300 px-4 py-3">
            <div>
                <h1 class="text-lg font-bold">Task chains</h1>
                <p class="text-xs opacity-60">One card per correlation ID and multi-agent run.</p>
            </div>
            <div class="flex flex-wrap items-center gap-2">
                <div class="join" aria-label="Tasks view">
                    <a class="btn btn-sm join-item btn-primary" aria-current="page">Board</a>
                    <a class="btn btn-sm join-item" href="{list_url}">List</a>
                </div>
                <form hx-get="/ui/tasks/board" hx-target="#task-board" hx-swap="outerHTML"
                    hx-push-url="true" hx-sync="#task-board:replace">
                    <input type="hidden" name="company_id" value="{company_id}">
                    <input type="hidden" name="view" value="board">
                    <select name="channel_id" class="select select-sm" aria-label="Filter by channel"
                        data-action="submit-form">
                        <option value="">All channels</option>{options}
                    </select>
                </form>
            </div>
        </header>"##,
        company_id = company.id,
        list_url = escape_html_attr(&list_url),
    )
}

pub fn task_board_fragment(
    company_id: Uuid,
    board: &TaskChainBoard,
    filter: TaskBoardFilter,
    selected_correlation_id: Option<CorrelationId>,
    swap: FragmentSwap,
) -> String {
    let columns = ChainStage::ALL
        .into_iter()
        .map(|stage| task_board_column(company_id, board, filter, selected_correlation_id, stage))
        .collect::<String>();
    format!(
        r##"<div id="task-board" class="flex min-h-0 flex-1 gap-3 overflow-x-auto p-3"
            sse-swap="task-board" hx-target="this" hx-swap="outerHTML"{oob}>{columns}</div>"##,
        oob = swap.oob_attribute(),
    )
}

fn task_board_column(
    company_id: Uuid,
    board: &TaskChainBoard,
    filter: TaskBoardFilter,
    selected_correlation_id: Option<CorrelationId>,
    stage: ChainStage,
) -> String {
    let total = board.total(stage);
    let cards = board
        .cards(stage)
        .iter()
        .map(|card| task_chain_card(company_id, filter, selected_correlation_id, card))
        .collect::<String>();
    let empty = if cards.is_empty() {
        r##"<p class="rounded-box border border-dashed border-base-300 p-4 text-center text-xs opacity-50">No chains</p>"##
    } else {
        ""
    };
    let overflow = if total > board.per_column_limit as i64 {
        format!(
            r##"<a class="btn btn-ghost btn-xs w-full" href="/ui/tasks?company_id={company_id}&amp;view=list">View all {total} in List</a>"##
        )
    } else {
        String::new()
    };
    format!(
        r##"<section class="flex w-72 shrink-0 flex-col rounded-box border border-base-300 bg-base-200">
            <header class="flex items-center justify-between border-b border-base-300 px-3 py-2">
                <h2 class="text-xs font-bold uppercase tracking-wide">{label}</h2>
                <span class="badge badge-sm {style}">{total}</span>
            </header>
            <div class="min-h-0 flex-1 space-y-2 overflow-y-auto p-2">{cards}{empty}</div>
            {overflow}
        </section>"##,
        label = chain_stage_label(stage),
        style = chain_stage_style(stage),
    )
}

fn task_chain_card(
    company_id: Uuid,
    filter: TaskBoardFilter,
    selected_correlation_id: Option<CorrelationId>,
    card: &TaskChainCard,
) -> String {
    let correlation_id = card.correlation_id;
    let selected = if selected_correlation_id == Some(correlation_id) {
        "border-primary ring-1 ring-primary"
    } else {
        "border-base-300"
    };
    let channels = card.channel_names.join(", ");
    let agents = card.agent_names.join(", ");
    let state_counts = chain_state_counts(&card.counts);
    let retry = if card.retry_count > 0 {
        format!(
            r##"<span class="badge badge-xs badge-warning">{} retries</span>"##,
            card.retry_count
        )
    } else {
        String::new()
    };
    let delivery = if card.counts.total_deliveries > 0 {
        format!(
            r##"<span class="badge badge-xs">Delivery {}/{}</span>"##,
            card.counts.delivery_delivered, card.counts.total_deliveries
        )
    } else {
        String::new()
    };
    let failure = card
        .failure_summary
        .as_deref()
        .map(|error| {
            format!(
                r##"<p class="line-clamp-2 text-[11px] text-error">{}</p>"##,
                escape_html_text(error)
            )
        })
        .unwrap_or_default();
    let next = card
        .next_action_at
        .map(|at| format!(r##"<span>Next {}</span>"##, format_time(at)))
        .unwrap_or_default();
    let query = task_board_query(company_id, filter.channel_id, Some(correlation_id));
    format!(
        r##"<article class="rounded-box border bg-base-100 shadow-sm {selected}">
            <button class="block w-full space-y-2 p-3 text-left"
                hx-get="/ui/tasks/chains/{correlation_id}?company_id={company_id}"
                hx-target="#task-chain-pane" hx-swap="outerHTML" hx-sync="#task-chain-pane:replace"
                hx-push-url="/ui/tasks?{query}">
                <div class="flex items-start justify-between gap-2">
                    <h3 class="line-clamp-2 text-sm font-semibold">{title}</h3>
                    <span class="font-mono text-[10px] opacity-50">{short_id}</span>
                </div>
                <p class="truncate text-[11px] opacity-65">{channels}</p>
                <p class="truncate text-[11px] opacity-65">{agents}</p>
                <div class="flex flex-wrap gap-1">{state_counts}{retry}{delivery}</div>
                {failure}
                <div class="flex flex-wrap justify-between gap-2 text-[10px] opacity-55">
                    <span>Started {created}</span><span>Active {updated}</span>{next}
                </div>
            </button>
        </article>"##,
        title = escape_html_text(&card.title),
        short_id = &correlation_id.to_string()[..8],
        channels = escape_html_text(&channels),
        agents = escape_html_text(&agents),
        created = format_time(card.created_at),
        updated = format_time(card.last_activity_at),
    )
}

fn chain_state_counts(counts: &TaskChainCounts) -> String {
    [
        (counts.pending, "Queued", "badge-warning"),
        (counts.processing, "Running", "badge-info"),
        (counts.pending_approval, "Approval", "badge-accent"),
        (counts.waiting_reply, "Reply", "badge-info badge-outline"),
        (counts.completed, "Done", "badge-success"),
        (counts.dead_letter + counts.failed, "Failed", "badge-error"),
        (counts.stopped, "Stopped", "badge-ghost"),
    ]
    .into_iter()
    .filter(|(count, _, _)| *count > 0)
    .map(|(count, label, style)| {
        format!(r##"<span class="badge badge-xs {style}">{count} {label}</span>"##)
    })
    .collect()
}

pub fn task_chain_empty_pane(message: &str) -> String {
    format!(
        r##"<aside id="task-chain-pane" class="hidden w-[30rem] shrink-0 border-l border-base-300 bg-base-100 p-6 xl:flex xl:items-center xl:justify-center"
            sse-swap="task-chain" hx-target="this" hx-swap="outerHTML">
            <p class="text-center text-sm opacity-60">{}</p>
        </aside>"##,
        escape_html_text(message)
    )
}

pub fn task_chain_detail_pane(detail: &TaskChainDetail, error: Option<&str>) -> String {
    let timeline = chain_timeline(detail);
    let tasks = detail
        .tasks
        .iter()
        .map(|item| chain_task(item, detail.company_id))
        .collect::<String>();
    let participants = format!(
        "{} · {}",
        detail.channel_names.join(", "),
        detail.agent_names.join(", ")
    );
    format!(
        r##"<aside id="task-chain-pane" class="fixed inset-y-0 right-0 z-30 flex w-full flex-col border-l border-base-300 bg-base-100 shadow-2xl sm:w-[34rem] xl:static xl:z-auto xl:shadow-none"
            sse-swap="task-chain" hx-target="this" hx-swap="outerHTML" data-correlation-id="{correlation_id}">
            <header class="border-b border-base-300 px-4 py-3">
                <div class="flex items-start justify-between gap-3">
                    <div class="min-w-0"><h2 class="truncate text-lg font-bold">{title}</h2>
                    <p class="truncate text-xs opacity-60">{participants}</p>
                    <p class="font-mono text-[11px] opacity-50">{correlation_id}</p></div>
                    <a class="btn btn-ghost btn-sm btn-square" href="/ui/tasks?company_id={company_id}&amp;view=board" aria-label="Close chain detail">×</a>
                </div>
            </header>
            <div class="flex-1 space-y-5 overflow-y-auto p-4">
                {error}
                {truncation}
                <section><h3 class="mb-2 text-xs font-bold uppercase opacity-60">Chronological timeline</h3>{timeline}</section>
                <section><h3 class="mb-2 text-xs font-bold uppercase opacity-60">Tasks in chain</h3><div class="space-y-2">{tasks}</div></section>
            </div>
        </aside>"##,
        correlation_id = detail.correlation_id,
        title = escape_html_text(&detail.title),
        participants = escape_html_text(&participants),
        company_id = detail.company_id,
        error = form_error_banner(error),
        truncation = chain_truncation_notice(detail.truncated),
    )
}

/// Say plainly that the pane is showing part of a chain.
///
/// Without this a truncated timeline is indistinguishable from a complete one, and an operator
/// reading it would conclude that nothing happened after the last row drawn.
fn chain_truncation_notice(truncated: bool) -> &'static str {
    if truncated {
        r##"<div class="alert alert-warning text-xs"><span>This chain is larger than the pane shows. Some tasks, attempts, deliveries or events are omitted.</span></div>"##
    } else {
        ""
    }
}

/// One row of the merged chain timeline.
///
/// `kind` orders entries that share a task and timestamp, so the ordering comes from the enum's
/// declaration order rather than from synthetic sort offsets stacked above the real
/// `task_status_events.sequence` space.
struct TimelineEntry {
    at: DateTime<Utc>,
    task_id: Uuid,
    kind: TimelineKind,
    /// The row's own position within its kind: a real event sequence or attempt number where one
    /// exists, otherwise the index the batched read already returned the row at. Every source is
    /// ordered deterministically by the query, so this is stable across renders.
    sequence: i32,
    html: String,
}

/// What a timeline row is, in the order rows of the same task and instant should read.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TimelineKind {
    StatusEvent,
    Attempt,
    Delivery,
    Approval,
    Outreach,
}

fn chain_timeline(detail: &TaskChainDetail) -> String {
    let mut entries: Vec<TimelineEntry> = Vec::new();
    for event in &detail.events {
        let from = event
            .from_status
            .map(task_status_label)
            .unwrap_or("Created");
        entries.push(TimelineEntry {
            at: event.transitioned_at,
            task_id: event.task_id,
            kind: TimelineKind::StatusEvent,
            sequence: event.sequence,
            html: format!(
                r##"<li class="border-l-2 border-primary pl-3"><div class="text-[11px] opacity-55">{} · task {}</div><div class="text-xs"><strong>{}</strong> → <strong>{}</strong></div><div class="font-mono text-[11px] opacity-65">{}</div></li>"##,
                format_time(event.transitioned_at), &event.task_id.to_string()[..8],
                from, task_status_label(event.to_status), escape_html_text(&event.reason.to_string())
            ),
        });
    }
    for item in &detail.tasks {
        for attempt in &item.attempts {
            let label = match attempt.status {
                TaskAttemptRecordStatus::Processing => "Attempt started",
                TaskAttemptRecordStatus::Completed => "Attempt completed",
                TaskAttemptRecordStatus::Failed => "Attempt failed",
            };
            entries.push(TimelineEntry {
                at: attempt.finished_at.unwrap_or(attempt.started_at),
                task_id: item.task.id,
                kind: TimelineKind::Attempt,
                sequence: attempt.attempt_number,
                html: format!(
                    r##"<li class="border-l-2 border-base-300 pl-3"><div class="text-[11px] opacity-55">{} · task {}</div><div class="text-xs"><strong>{label}</strong> #{}</div><div class="text-[11px] opacity-65">{} · {} tokens</div></li>"##,
                    format_time(attempt.finished_at.unwrap_or(attempt.started_at)), &item.task.id.to_string()[..8], attempt.attempt_number,
                    attempt.duration_ms().map(|ms| format!("{ms} ms")).unwrap_or_else(|| "in progress".into()),
                    attempt.total_tokens().unwrap_or(0)
                ),
            });
        }
        for (index, delivery) in item.deliveries.iter().enumerate() {
            entries.push(TimelineEntry {
                at: delivery.updated_at,
                task_id: item.task.id,
                kind: TimelineKind::Delivery,
                sequence: index as i32,
                html: format!(
                    r##"<li class="border-l-2 border-base-300 pl-3"><div class="text-[11px] opacity-55">{} · task {}</div><div class="text-xs"><strong>{} delivery {}</strong></div><div class="text-[11px] opacity-65">{} of {} attempts spent</div></li>"##,
                    format_time(delivery.updated_at),
                    &item.task.id.to_string()[..8],
                    delivery.transport.label(),
                    delivery.status.label(),
                    delivery.attempt_count,
                    delivery.max_attempts
                ),
            });
        }
    }
    for (index, approval) in detail.approvals.iter().enumerate() {
        entries.push(TimelineEntry {
            at: approval.updated_at,
            task_id: approval.task_id,
            kind: TimelineKind::Approval,
            sequence: index as i32,
            html: format!(
                r##"<li class="border-l-2 border-accent pl-3"><div class="text-[11px] opacity-55">{} · task {}</div><div class="text-xs"><strong>Approval {}</strong></div><div class="text-[11px] opacity-65">{} · approval {}</div></li>"##,
                format_time(approval.updated_at),
                &approval.task_id.to_string()[..8],
                escape_html_text(&approval.status),
                escape_html_text(&approval.action_title),
                &approval.id.to_string()[..8]
            ),
        });
    }
    for (index, outreach) in detail.outreaches.iter().enumerate() {
        entries.push(TimelineEntry {
            at: outreach.created_at,
            task_id: outreach.task_id,
            kind: TimelineKind::Outreach,
            sequence: index as i32,
            html: format!(
                r##"<li class="border-l-2 border-info pl-3"><div class="text-[11px] opacity-55">{} · task {}</div><div class="text-xs"><strong>Outreach {}</strong></div><div class="text-[11px] opacity-65">{} of {} replies · {:.0}% required · deadline {} · outreach {}</div></li>"##,
                format_time(outreach.created_at),
                &outreach.task_id.to_string()[..8],
                escape_html_text(&outreach.status),
                outreach.response_count,
                outreach.target_count,
                outreach.required_threshold_percent,
                format_time(outreach.expires_at),
                &outreach.id.to_string()[..8]
            ),
        });
    }
    entries.sort_by_key(|entry| (entry.at, entry.task_id, entry.kind, entry.sequence));
    let body = entries
        .into_iter()
        .map(|entry| entry.html)
        .collect::<String>();
    format!(r##"<ol class="space-y-3">{body}</ol>"##)
}

fn chain_task(item: &TaskChainTaskDetail, company_id: Uuid) -> String {
    let task = &item.task;
    let action = match task.status {
        TaskStatus::Pending
        | TaskStatus::Processing
        | TaskStatus::PendingApproval
        | TaskStatus::WaitingForThirdPartyReply
        | TaskStatus::Failed
        | TaskStatus::DeadLetter => ("stop", "Stop", "Stopping…"),
        TaskStatus::Stopped => ("resume", "Resume", "Resuming…"),
        TaskStatus::Completed => ("", "", ""),
    };
    let button = if action.0.is_empty() {
        String::new()
    } else {
        format!(
            r##"<button class="btn btn-xs" hx-post="/ui/tasks/{}/{action}?company_id={company_id}&amp;view=board&amp;correlation_id={}"
            hx-target="#task-chain-pane" hx-swap="outerHTML" hx-disabled-elt="this" data-pending-label="{}">{}</button>"##,
            task.id,
            task.correlation_id,
            action.2,
            action.1,
            action = action.0
        )
    };
    let thread = task.thread_id.map(|id| format!(
        r##"<a class="link text-[11px]" href="/ui?company_id={company_id}&amp;channel_id={}&amp;thread_id={id}">Open thread</a>"##,
        task.channel_id
    )).unwrap_or_default();
    format!(
        r##"<details class="rounded-box border border-base-300 bg-base-200">
            <summary class="flex cursor-pointer items-center gap-2 px-3 py-2 text-xs"><span class="badge badge-xs {}">{}</span><span class="truncate">{}</span><span class="ml-auto font-mono opacity-50">{}</span></summary>
            <div class="flex flex-wrap items-center gap-2 border-t border-base-300 px-3 py-2">{thread}{button}<span class="text-[11px] opacity-55">{} attempt(s), {} delivery(s)</span></div>
        </details>"##,
        task_status_style(task.status),
        task_status_label(task.status),
        escape_html_text(&task.task_type),
        &task.id.to_string()[..8],
        item.attempts.len(),
        item.deliveries.len()
    )
}

fn chain_stage_label(stage: ChainStage) -> &'static str {
    match stage {
        ChainStage::Queued => "Queued",
        ChainStage::Running => "Running",
        ChainStage::WaitingApproval => "Waiting Approval",
        ChainStage::WaitingReply => "Waiting Reply",
        ChainStage::Completed => "Completed",
        ChainStage::NeedsAttention => "Needs Attention",
    }
}

fn chain_stage_style(stage: ChainStage) -> &'static str {
    match stage {
        ChainStage::Queued => "badge-warning",
        ChainStage::Running => "badge-info",
        ChainStage::WaitingApproval => "badge-accent",
        ChainStage::WaitingReply => "badge-info badge-outline",
        ChainStage::Completed => "badge-success",
        ChainStage::NeedsAttention => "badge-error",
    }
}
