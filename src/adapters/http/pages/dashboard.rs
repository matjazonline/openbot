//! `/ui/dashboard` — how the system is performing, in one screen.
//!
//! The page is two kinds of number and says which is which. Everything under "Queues" and "Last
//! hour" is derived from Postgres: it survives a restart and means the same thing whichever machine
//! serves it. The strip at the bottom is this process's own since-boot counters, which reset on
//! deploy — kept because SMTP intake is recorded nowhere else.
//!
//! [`dashboard_panels`] is the whole body, and is what both the first render and every SSE tick
//! emit. One renderer, so the live view cannot drift from the page you loaded.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::{
    PANELS_SKELETON,
    chart::{self, ChartKind, Series, TimeChart, YUnit, thousands},
    escape_html_text,
    icon::{BUTTON_ICON, Icon, icon},
    mailbox::{MailboxUser, UiSection, UiShell, sidebar_header, ui_shell},
    panels_placeholder,
};
use crate::entities::{
    company::Company,
    dashboard::{
        AttemptStats, DashboardSnapshot, DashboardWindow, LatencyBucket, OUTSTANDING_LIMIT,
        OutboxHealth, OutstandingTask, ProcessGauges, TaskQueueHealth,
    },
    outbox::OutboxStatus,
    task::TaskStatus,
};

/// The id the SSE tick swaps, and the id the page renders it into.
pub const DASHBOARD_PANELS_ID: &str = "dashboard-panels";

/// The SSE event name the panels arrive under.
pub const DASHBOARD_EVENT: &str = "dashboard";

/// Which rollup is on screen.
///
/// An operator sees every company at once, and there is no company to switch between in that case —
/// which is why this is an enum rather than an `Option<&Company>` that each panel would have to
/// re-interpret.
#[derive(Debug, Clone, Copy)]
pub enum DashboardScopeView<'a> {
    Company(&'a Company),
    Global,
}

impl<'a> DashboardScopeView<'a> {
    /// The company on screen, for the chrome that draws it rather than just links to it.
    fn company(&self) -> Option<&'a Company> {
        match self {
            DashboardScopeView::Company(company) => Some(company),
            DashboardScopeView::Global => None,
        }
    }

    fn company_id(&self) -> Option<Uuid> {
        self.company().map(|company| company.id)
    }

    fn title(&self) -> String {
        match self {
            DashboardScopeView::Company(company) => format!("{} Dashboard", company.name),
            DashboardScopeView::Global => "System Dashboard".to_string(),
        }
    }
}

/// The dashboard's chrome for one request: what the shell needs, and nothing that costs a query.
///
/// The panels are fetched on their own once the shell is on screen, so this page renders while the
/// rollup's aggregates are still running rather than after them — which is the whole point of the
/// placeholder [`dashboard_page`] leaves in their place.
pub struct DashboardShell<'a> {
    pub user: &'a MailboxUser<'a>,
    pub scope: DashboardScopeView<'a>,
    /// The caller's own companies, used to resolve company-scoped navigation.
    pub companies: &'a [Company],
    /// The range the panels will be read over. The shell needs it before any query has run, because
    /// it is what the panel fetch and the stream subscription are both parameterised by.
    pub window: DashboardWindow,
}

pub struct DashboardPage<'a> {
    pub user: &'a MailboxUser<'a>,
    pub scope: DashboardScopeView<'a>,
    /// The caller's own companies — the set whose `/ui` pages will actually resolve for them.
    pub companies: &'a [Company],
    pub snapshot: &'a DashboardSnapshot,
    pub window: DashboardWindow,
    pub process: &'a ProcessGauges,
}

impl DashboardPage<'_> {
    /// Would a link into this company's `/ui` pages actually show the caller anything?
    ///
    /// `/ui` and `/ui/tasks` resolve a company out of the caller's *own* list, so a link into a
    /// company they do not belong to lands on "Create a company". An operator sees every company's
    /// rows in the rollup but is rarely a member of any of them, so this is the common case for
    /// them — and a link that goes nowhere is worse than plain text, which at least does not
    /// promise anything.
    fn can_open(&self, company_id: Uuid) -> bool {
        self.companies
            .iter()
            .any(|company| company.id == company_id)
    }
}

pub fn dashboard_page(shell: &DashboardShell<'_>) -> String {
    let header = sidebar_header("Dashboard", "System metrics, queue health and activity.");
    let range = range_picker(shell.scope.company_id(), shell.window);
    let sidebar = match shell.scope {
        DashboardScopeView::Company(_) => format!(
            r##"<aside class="flex w-72 shrink-0 flex-col border-r border-base-300 bg-base-200">
                {header}
                {range}
                {legend}
            </aside>"##,
            header = header,
            legend = scope_legend("This company"),
        ),
        DashboardScopeView::Global => format!(
            r##"<aside class="flex w-72 shrink-0 flex-col border-r border-base-300 bg-base-200">
                {header}
                <div class="p-3">
                    <div class="alert alert-info py-2">
                        <span class="text-sm font-semibold">Operator view</span>
                    </div>
                    <a href="/ui/agent-library" class="btn btn-primary btn-sm mt-3 w-full">Manage agent library</a>
                </div>
                {range}
                {legend}
            </aside>"##,
            header = header,
            legend = scope_legend("All companies"),
        ),
    };

    // `hx-ext="sse"` sits on the wrapper, not on the swapped element: a swap that replaced the
    // element carrying the connection would tear the stream down on its first tick.
    //
    // The panels themselves are fetched once the shell is on screen rather than rendered into it,
    // so the workspace paints immediately and the rollup's aggregates run against a placeholder.
    // The fetch is what fills it, not the stream: a reader whose SSE connection never establishes
    // still gets their numbers, and the stream then keeps them current.
    let content = format!(
        r##"
        {sidebar}
        <section class="flex flex-1 flex-col overflow-y-auto bg-base-100" hx-ext="sse"
            sse-connect="/ui/dashboard/events{query}">
            <div id="{panels_id}"{PANELS_SKELETON} sse-swap="{event}" hx-swap="innerHTML"
                hx-get="/ui/dashboard/panels{query}" hx-trigger="load">{placeholder}</div>
        </section>
        "##,
        query = dashboard_query(shell.scope.company_id(), shell.window),
        panels_id = DASHBOARD_PANELS_ID,
        event = DASHBOARD_EVENT,
        placeholder = panels_placeholder(),
    );

    ui_shell(&UiShell {
        title: &shell.scope.title(),
        user: shell.user,
        company: shell.scope.company(),
        section: UiSection::Dashboard,
        content: &content,
        script: "",
    })
}

/// The body of the page: every panel, and nothing that owns the SSE connection.
pub fn dashboard_panels(page: &DashboardPage<'_>) -> String {
    // Only a company-scoped page can link its figures into `/ui/tasks`, which resolves exactly one
    // company — and only when that company is one the caller can open. Individual task rows below
    // are judged the same way, each against its own company.
    let linked_company = page.scope.company_id().filter(|id| page.can_open(*id));

    format!(
        r##"
        <div class="flex flex-col gap-6 p-6">
            {chart_style}
            {alerts}
            <div>
                <h2 class="mb-3 text-lg font-semibold">Queues right now</h2>
                <div class="flex flex-col gap-3">
                    {tasks}
                    {outbox}
                    {outstanding}
                </div>
            </div>
            <div>
                <h2 class="mb-3 text-lg font-semibold">{window_label}</h2>
                <div class="flex flex-col gap-3">
                    {attempts}
                    {throughput}
                    {latency}
                    {queue_depth}
                </div>
            </div>
            {process}
        </div>
        "##,
        chart_style = chart::STYLE,
        alerts = health_alerts(&page.snapshot.tasks, &page.snapshot.outbox),
        tasks = task_queue_panel(&page.snapshot.tasks, linked_company),
        outbox = outbox_panel(&page.snapshot.outbox, linked_company),
        outstanding = outstanding_panel(page),
        window_label = page.window.label(),
        attempts = attempt_panel(&page.snapshot.attempts),
        throughput = throughput_panel(page.snapshot, page.window),
        latency = latency_panel(page.snapshot, page.window),
        queue_depth = queue_depth_panel(page.snapshot, page.window),
        process = process_panel(page.process),
    )
}

/// The two conditions worth interrupting someone over, shown only when they are non-zero.
///
/// A stalled task is a lease nobody is renewing: the row will be re-claimed and its agent will run
/// a second time, having already had its effects. An expired delivery lease is a send that will be
/// failed by the reaper without anyone knowing whether it went out.
fn health_alerts(tasks: &TaskQueueHealth, outbox: &OutboxHealth) -> String {
    let mut alerts = String::new();

    if tasks.stalled > 0 {
        alerts.push_str(&alert_row(
            &format!(
                "{} task{} claimed under a lapsed lease",
                tasks.stalled,
                plural(tasks.stalled)
            ),
            "Nothing is renewing the lease. Each will be re-claimed and its agent run again.",
        ));
    }

    if outbox.expired_leases > 0 {
        alerts.push_str(&alert_row(
            &format!(
                "{} deliver{} past its lease",
                outbox.expired_leases,
                if outbox.expired_leases == 1 {
                    "y"
                } else {
                    "ies"
                }
            ),
            "The next maintenance pass will fail these without a delivery result.",
        ));
    }

    alerts
}

/// Every alert here is the same kind of thing -- a lease nobody is renewing -- so they share one
/// glyph rather than taking it as a parameter.
fn alert_row(headline: &str, detail: &str) -> String {
    format!(
        r##"<div class="alert alert-warning">
            {glyph}
            <div>
                <div class="font-semibold">{headline}</div>
                <div class="text-sm opacity-80">{detail}</div>
            </div>
        </div>"##,
        glyph = icon(Icon::Alert, "h-5 w-5"),
        headline = escape_html_text(headline),
        detail = escape_html_text(detail),
    )
}

/// The tasks behind the numbers, each one a link to the conversation it belongs to.
///
/// This is what makes the counts actionable: "three stalled" is a fact, and this is the way to the
/// three. Trouble sorts to the top, so the row worth opening is the first one.
fn outstanding_panel(page: &DashboardPage<'_>) -> String {
    if page.snapshot.outstanding.is_empty() {
        return panel(
            "Outstanding tasks",
            r##"<div class="px-4 py-6 text-sm opacity-60">
                Nothing outstanding — every task has finished.
            </div>"##,
        );
    }

    // The company column is noise when every row shares it, which is the whole company-scoped view.
    let show_company = matches!(page.scope, DashboardScopeView::Global);
    let rows: String = page
        .snapshot
        .outstanding
        .iter()
        .map(|task| outstanding_row(task, show_company, page.can_open(task.company_id)))
        .collect();

    let footer = match page.scope.company_id().filter(|id| page.can_open(*id)) {
        Some(company_id) => format!(
            r##"<div class="border-t border-base-300 px-4 py-2 text-right">
                <a class="link text-xs opacity-70" href="/ui/tasks?company_id={company_id}">
                    Open the task monitor
                </a>
            </div>"##
        ),
        None => String::new(),
    };

    // The cap is a cap, not a total: say so when it bites, or the panel quietly under-reports.
    let capped = if page.snapshot.outstanding.len() as i64 >= OUTSTANDING_LIMIT {
        format!(
            r##"<div class="px-4 pb-2 text-xs opacity-60">Showing the first {OUTSTANDING_LIMIT}.</div>"##
        )
    } else {
        String::new()
    };

    panel(
        "Outstanding tasks",
        &format!(r##"<div class="divide-y divide-base-300">{rows}</div>{capped}{footer}"##),
    )
}

/// One outstanding task: what state it is in, whose it is, and — when there is one — the way to it.
///
/// `openable` is false for a company the caller does not belong to, which is the normal case in the
/// operator rollup. The row still shows everything it knows; it just does not offer a link that
/// would land on "Create a company".
fn outstanding_row(task: &OutstandingTask, show_company: bool, openable: bool) -> String {
    // Every reachable row goes somewhere. A task without a thread was queued outside a
    // conversation, so there is no thread to open — it points at the task monitor entry instead of
    // being a dead end.
    let destination = openable.then(|| match task.thread_id {
        Some(thread_id) => (
            format!(
                "/ui?company_id={company_id}&channel_id={channel_id}&thread_id={thread_id}",
                company_id = task.company_id,
                channel_id = task.channel_id,
            ),
            "Open thread",
        ),
        None => (
            format!(
                "/ui/tasks?company_id={company_id}&task_id={task_id}",
                company_id = task.company_id,
                task_id = task.id,
            ),
            "Open task",
        ),
    });

    let where_ = if show_company {
        format!("{} / {}", task.company_name, task.channel_name)
    } else {
        task.channel_name.clone()
    };

    // The error is why the row is here at all, so it earns a line — truncated, because a stack of
    // provider errors would push every other row off the panel.
    let detail = match task
        .last_error
        .as_deref()
        .filter(|_| task.needs_attention())
    {
        Some(error) => format!(
            r##"<div class="truncate text-xs opacity-60">{error}</div>"##,
            error = escape_html_text(&truncate(error, 120)),
        ),
        None => String::new(),
    };

    let retries = if task.retry_count > 0 {
        format!(
            r##"<span class="opacity-60"> · {} {}</span>"##,
            task.retry_count,
            if task.retry_count == 1 {
                "retry"
            } else {
                "retries"
            }
        )
    } else {
        String::new()
    };

    let body = format!(
        r##"{glyph}
            <span class="min-w-0 flex-1">
                <span class="flex items-baseline gap-2">
                    <span class="text-sm font-semibold {tone}">{state}</span>
                    <span class="truncate text-xs opacity-70">{where_}</span>
                </span>
                {detail}
            </span>
            <span class="shrink-0 text-xs opacity-60">{since}{retries}</span>
            <span class="shrink-0 opacity-40">{chevron}</span>"##,
        glyph = icon(outstanding_icon(task), BUTTON_ICON),
        tone = if task.needs_attention() {
            "text-error"
        } else {
            ""
        },
        state = escape_html_text(outstanding_state(task)),
        where_ = escape_html_text(&where_),
        since = escape_html_text(&relative_time(task.since)),
        // The chevron is the affordance, so an unlinked row must not show one.
        chevron = if destination.is_some() {
            icon(Icon::ChevronRight, "h-3 w-3")
        } else {
            String::new()
        },
    );

    match destination {
        Some((href, target)) => format!(
            r##"<a class="flex items-center gap-3 px-4 py-2 transition hover:bg-base-300" href="{href}" title="{target}">{body}</a>"##,
            href = escape_html_text(&href),
            target = escape_html_text(target),
        ),
        None => format!(
            r##"<div class="flex items-center gap-3 px-4 py-2" title="You are not a member of this company">{body}</div>"##
        ),
    }
}

/// What to call this task's state in one or two words.
///
/// "Stalled" is not a status — it is `processing` with a lapsed lease — and it is the one worth
/// naming, so it wins over the stored value.
fn outstanding_state(task: &OutstandingTask) -> &'static str {
    if task.stalled {
        return "Stalled";
    }
    match task.status {
        TaskStatus::Processing => "Running",
        TaskStatus::Pending => "Queued",
        TaskStatus::PendingApproval => "Awaiting approval",
        TaskStatus::WaitingForThirdPartyReply => "Awaiting reply",
        TaskStatus::DeadLetter => "Dead-lettered",
        TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Stopped => "Finished",
    }
}

/// The glyphs the thread column already uses for these states, so the two views agree.
fn outstanding_icon(task: &OutstandingTask) -> Icon {
    if task.stalled {
        return Icon::Alert;
    }
    match task.status {
        TaskStatus::DeadLetter => Icon::Alert,
        TaskStatus::PendingApproval => Icon::Hourglass,
        TaskStatus::WaitingForThirdPartyReply => Icon::Mail,
        _ => Icon::DotFill,
    }
}

/// Where a task-queue figure should link to, given the scope on screen.
///
/// `None` in the operator view: `/ui/tasks` resolves a single company, so a link from a
/// cross-company rollup would silently land on whichever one it picked first. A figure that cannot
/// go anywhere honest stays plain text.
fn task_filter_link(company_id: Option<Uuid>, status: Option<TaskStatus>) -> Option<String> {
    let company_id = company_id?;
    Some(match status {
        Some(status) => format!(
            "/ui/tasks?company_id={company_id}&status={status}",
            status = status.as_str()
        ),
        None => format!("/ui/tasks?company_id={company_id}"),
    })
}

fn task_queue_panel(tasks: &TaskQueueHealth, company_id: Option<Uuid>) -> String {
    let link = |status: Option<TaskStatus>| task_filter_link(company_id, status);
    let parked = tasks.count_of(TaskStatus::PendingApproval)
        + tasks.count_of(TaskStatus::WaitingForThirdPartyReply);
    let dead = tasks.count_of(TaskStatus::DeadLetter);

    stat_row(
        "Background tasks",
        &format!(
            "{due}{processing}{stalled}{parked}{dead}{total}",
            due = stat(
                Stat::new("Due now", &tasks.due_now.to_string(), "pending & ready")
                    .linked(link(Some(TaskStatus::Pending)))
            ),
            processing = stat(
                Stat::new(
                    "Processing",
                    &tasks.count_of(TaskStatus::Processing).to_string(),
                    "claimed",
                )
                .linked(link(Some(TaskStatus::Processing)))
            ),
            // Stalled has no status of its own, so it links to the `processing` list it is a
            // subset of rather than to a filter that would come back empty.
            stalled = stat(
                Stat::new("Stalled", &tasks.stalled.to_string(), "lapsed lease")
                    .alarming(tasks.stalled > 0)
                    .linked(link(Some(TaskStatus::Processing)))
            ),
            // Two statuses in one figure, so it links to the unfiltered list rather than picking
            // one of them to be the truth.
            parked = stat(
                Stat::new("Parked", &parked.to_string(), "awaiting a reply").linked(link(None))
            ),
            dead = stat(
                Stat::new("Dead-lettered", &dead.to_string(), "out of retries")
                    .alarming(dead > 0)
                    .linked(link(Some(TaskStatus::DeadLetter)))
            ),
            total =
                stat(Stat::new("Total", &tasks.total().to_string(), "all time").linked(link(None))),
        ),
    )
}

fn outbox_panel(outbox: &OutboxHealth, company_id: Option<Uuid>) -> String {
    let link = company_id.map(|company_id| format!("/ui/outbox?company_id={company_id}"));
    let failed = outbox.count_of(OutboxStatus::Failed);

    stat_row(
        "Email outbox",
        &format!(
            "{due}{sending}{expired}{sent}{failed}",
            due = stat(
                Stat::new("Due now", &outbox.due_now.to_string(), "queued & ready")
                    .linked(link.clone())
            ),
            sending = stat(
                Stat::new(
                    "Sending",
                    &outbox.count_of(OutboxStatus::Sending).to_string(),
                    "claimed",
                )
                .linked(link.clone())
            ),
            expired = stat(
                Stat::new(
                    "Lease expired",
                    &outbox.expired_leases.to_string(),
                    "will be reaped",
                )
                .alarming(outbox.expired_leases > 0)
                .linked(link.clone())
            ),
            sent = stat(
                Stat::new(
                    "Sent",
                    &outbox.count_of(OutboxStatus::Sent).to_string(),
                    "all time",
                )
                .linked(link.clone())
            ),
            failed = stat(
                Stat::new("Failed", &failed.to_string(), "all time")
                    .alarming(failed > 0)
                    .linked(link)
            ),
        ),
    )
}

fn attempt_panel(attempts: &AttemptStats) -> String {
    // `None` and `0` are different answers: nothing has finished yet versus something finished
    // instantly. An em dash says the first without pretending to be the second.
    let latency = |value: Option<i64>| match value {
        Some(ms) => format!("{ms} ms"),
        None => "—".to_string(),
    };
    let failure_rate = match attempts.failure_rate_percent() {
        Some(rate) => format!("{rate:.0}%"),
        None => "—".to_string(),
    };

    stat_row(
        "Agent attempts",
        &format!(
            "{ran}{failed}{p50}{p95}{tokens}",
            ran = stat(Stat::new(
                "Attempts",
                &attempts.attempts.to_string(),
                "started"
            )),
            failed = stat(
                Stat::new(
                    "Failure rate",
                    &failure_rate,
                    &format!("{} failed", attempts.failed),
                )
                .alarming(attempts.failed > 0)
            ),
            p50 = stat(Stat::new("p50", &latency(attempts.p50_ms), "median run")),
            p95 = stat(Stat::new("p95", &latency(attempts.p95_ms), "slow tail")),
            tokens = stat(Stat::new(
                "Tokens",
                &thousands(attempts.total_tokens()),
                &format!(
                    "{} in / {} out",
                    thousands(attempts.prompt_tokens),
                    thousands(attempts.completion_tokens)
                ),
            )),
        ),
    )
}

/// Terminal task outcomes per bucket, stacked so a column's height is everything that finished.
///
/// Completed and failed are the status colours doing status work, which is what they are reserved
/// for — they are not two categories that happen to need telling apart.
fn throughput_panel(snapshot: &DashboardSnapshot, window: DashboardWindow) -> String {
    if snapshot.throughput_total() == 0 {
        return panel(
            "Throughput",
            r##"<div class="px-4 py-6 text-sm opacity-60">Nothing has finished in this window yet.</div>"##,
        );
    }

    let series = [
        Series {
            label: "completed",
            color: "var(--color-success)",
            values: snapshot
                .throughput
                .iter()
                .map(|bucket| Some(bucket.completed as f64))
                .collect(),
        },
        Series {
            label: "failed",
            color: "var(--color-error)",
            values: snapshot
                .throughput
                .iter()
                .map(|bucket| Some(bucket.failed as f64))
                .collect(),
        },
    ];

    let buckets: Vec<_> = snapshot.throughput.iter().map(|slot| slot.bucket).collect();

    panel(
        "Throughput",
        &format!(
            "{chart}{footer}",
            chart = chart::time_chart(&TimeChart {
                buckets: &buckets,
                series: &series,
                kind: ChartKind::StackedColumn,
                unit: YUnit::Count,
                tick_format: window.tick_format(),
            }),
            footer = chart_footer(&format!(
                "{total} finished, peak {peak} per bucket",
                total = thousands(snapshot.throughput_total()),
                peak = thousands(snapshot.throughput_peak()),
            )),
        ),
    )
}

/// Attempt duration per bucket, at the median and at the tail.
///
/// Two quantiles of one measure, so they are two shades of one hue rather than two colours: a
/// categorical pair would say these are different things being compared, when p95 is the same
/// measurement asked a harder question.
fn latency_panel(snapshot: &DashboardSnapshot, window: DashboardWindow) -> String {
    let measured = snapshot
        .latency
        .iter()
        .any(|bucket| bucket.p50_ms.is_some());

    if !measured {
        return panel(
            "Attempt latency",
            r##"<div class="px-4 py-6 text-sm opacity-60">No attempt has finished in this window yet.</div>"##,
        );
    }

    let millis = |pick: fn(&LatencyBucket) -> Option<i64>| {
        snapshot
            .latency
            .iter()
            .map(|bucket| pick(bucket).map(|value| value as f64))
            .collect::<Vec<_>>()
    };

    let series = [
        Series {
            label: "p95",
            color: "var(--color-primary)",
            values: millis(|bucket| bucket.p95_ms),
        },
        Series {
            label: "p50",
            // The same hue mixed toward the surface: still obviously the same measure, and clearly
            // the lesser of the two without introducing a colour that means something else. The
            // share is set against how dark `primary` itself now is -- mixed any further down it
            // stops being a line and becomes a smudge on the panel.
            color: "color-mix(in oklab, var(--color-primary) 60%, var(--color-base-100))",
            values: millis(|bucket| bucket.p50_ms),
        },
    ];

    let buckets: Vec<_> = snapshot.latency.iter().map(|slot| slot.bucket).collect();

    panel(
        "Attempt latency",
        &format!(
            "{chart}{footer}",
            chart = chart::time_chart(&TimeChart {
                buckets: &buckets,
                series: &series,
                kind: ChartKind::Line,
                unit: YUnit::Millis,
                tick_format: window.tick_format(),
            }),
            footer = chart_footer(
                "Bucketed on when each attempt started; a gap is a bucket in which none finished",
            ),
        ),
    )
}

/// How much work was outstanding at each point in the window.
///
/// One series, so no legend — the heading already names it. Read the query's own comment before
/// trusting the shape too far: this is reconstructed from each task's last write rather than
/// sampled, so it gets the open-to-closed boundary right and smooths the path in between.
fn queue_depth_panel(snapshot: &DashboardSnapshot, window: DashboardWindow) -> String {
    if snapshot.queue_depth.is_empty() {
        return panel(
            "Queue depth",
            r##"<div class="px-4 py-6 text-sm opacity-60">No tasks have been queued yet.</div>"##,
        );
    }

    let series = [Series {
        label: "open tasks",
        color: "var(--color-info)",
        values: snapshot
            .queue_depth
            .iter()
            .map(|bucket| Some(bucket.open as f64))
            .collect(),
    }];

    let buckets: Vec<_> = snapshot
        .queue_depth
        .iter()
        .map(|slot| slot.bucket)
        .collect();

    panel(
        "Queue depth",
        &format!(
            "{chart}{footer}",
            chart = chart::time_chart(&TimeChart {
                buckets: &buckets,
                series: &series,
                kind: ChartKind::Area,
                unit: YUnit::Count,
                tick_format: window.tick_format(),
            }),
            footer = chart_footer(
                "Tasks created but not yet finished, reconstructed from each task's last update",
            ),
        ),
    )
}

/// The line under a chart: what it is measuring, or what it cannot quite claim to measure.
fn chart_footer(note: &str) -> String {
    format!(
        r##"<div class="px-4 pb-3 text-xs opacity-60">{note}</div>"##,
        note = escape_html_text(note),
    )
}

/// This process's own counters, fenced off and labelled so they are not read as system-wide truth.
fn process_panel(process: &ProcessGauges) -> String {
    format!(
        r##"
        <div>
            <h2 class="mb-1 text-lg font-semibold">This process</h2>
            <p class="mb-3 text-sm opacity-60">
                Counted in memory since this instance started, and reset by every deploy.
                Not company-scoped.
            </p>
            {row}
        </div>
        "##,
        row = stat_row(
            "SMTP and model calls",
            &format!(
                "{conns}{rejected}{ai}{ai_failed}{tokens}{latency}",
                conns = stat(Stat::new(
                    "SMTP in",
                    &thousands(process.smtp_total as i64),
                    "connections",
                )),
                rejected = stat(Stat::new(
                    "Turned away",
                    &thousands(process.smtp_rejected() as i64),
                    "blocked or rejected",
                )),
                ai = stat(Stat::new(
                    "Model calls",
                    &thousands(process.ai_total as i64),
                    "executions",
                )),
                ai_failed = stat(
                    Stat::new(
                        "Model failures",
                        &thousands(process.ai_failed as i64),
                        "errored",
                    )
                    .alarming(process.ai_failed > 0)
                ),
                tokens = stat(Stat::new(
                    "Tokens",
                    &thousands(process.ai_total_tokens as i64),
                    "since boot",
                )),
                latency = stat(Stat::new(
                    "Avg call",
                    &format!("{} ms", process.ai_avg_latency_ms),
                    "mean latency",
                )),
            ),
        ),
    )
}

/// The query string that carries a dashboard's identity: which companies, and over what range.
///
/// One function because it is appended to three URLs that must agree — the page's own links, the
/// panel fetch, and the SSE subscription. A range that reached the fetch but not the stream would
/// show the right chart for five seconds and then the wrong one for as long as the tab stayed open.
fn dashboard_query(company_id: Option<Uuid>, window: DashboardWindow) -> String {
    match company_id {
        Some(company_id) => format!("?company_id={company_id}&window={}", window.slug()),
        None => format!("?window={}", window.slug()),
    }
}

/// The trailing range every panel below is read over.
///
/// Plain links, and deliberately in the sidebar rather than above the panels: everything inside
/// `#dashboard-panels` is thrown away and rebuilt on every tick, so a control living there would be
/// re-created under the reader's pointer several times a minute. Navigating is also what
/// re-establishes the SSE subscription against the new range — a control that only re-fetched the
/// panels would leave the stream ticking on the old one and overwrite itself.
fn range_picker(company_id: Option<Uuid>, window: DashboardWindow) -> String {
    let options: String = DashboardWindow::PRESETS
        .into_iter()
        .map(|preset| {
            let selected = preset == window;

            format!(
                r##"<a class="btn btn-xs join-item{active}" href="/ui/dashboard{query}"{current}>{label}</a>"##,
                active = if selected { " btn-active" } else { "" },
                query = dashboard_query(company_id, preset),
                current = if selected { r#" aria-current="page""# } else { "" },
                label = preset.slug(),
            )
        })
        .collect();

    format!(
        r##"<div class="px-4 pb-4">
            <div class="mb-1 text-xs font-semibold uppercase tracking-wide opacity-60">Range</div>
            <div class="join">{options}</div>
        </div>"##
    )
}

fn scope_legend(scope: &str) -> String {
    format!(
        r##"<div class="px-4 pb-4 text-xs opacity-60">
            <div class="font-semibold uppercase tracking-wide">Scope</div>
            <div class="mt-1">{scope}</div>
        </div>"##,
        scope = escape_html_text(scope),
    )
}

fn panel(heading: &str, body: &str) -> String {
    format!(
        r##"<div class="rounded-box border border-base-300 bg-base-200">
            <div class="border-b border-base-300 px-4 py-2 text-sm font-semibold opacity-80">{heading}</div>
            {body}
        </div>"##,
        heading = escape_html_text(heading),
    )
}

fn stat_row(heading: &str, stats: &str) -> String {
    panel(
        heading,
        &format!(r##"<div class="stats stats-horizontal w-full overflow-x-auto">{stats}</div>"##),
    )
}

/// One figure in a stat row.
///
/// A struct rather than a fifth positional argument: `alarming` is already a bool, and a bare
/// `stat(.., false, None)` at seventeen call sites says nothing about which flag is which.
struct Stat<'a> {
    title: &'a str,
    value: &'a str,
    caption: &'a str,
    /// Tints the figure. Reserved for counts that mean someone should look.
    alarming: bool,
    /// Where clicking through goes, when there is anywhere for it to go.
    href: Option<String>,
}

impl<'a> Stat<'a> {
    fn new(title: &'a str, value: &'a str, caption: &'a str) -> Self {
        Self {
            title,
            value,
            caption,
            alarming: false,
            href: None,
        }
    }

    fn alarming(mut self, alarming: bool) -> Self {
        self.alarming = alarming;
        self
    }

    fn linked(mut self, href: Option<String>) -> Self {
        self.href = href;
        self
    }
}

/// Render one figure, as a link when it has somewhere to go and as plain text otherwise.
///
/// The two share every class, so a linked figure is not visually a different kind of thing — only
/// the cursor and the hover tint say it can be clicked.
fn stat(stat: Stat<'_>) -> String {
    let body = format!(
        r##"<div class="stat-title text-xs">{title}</div>
            <div class="stat-value text-2xl {tone}">{value}</div>
            <div class="stat-desc text-xs">{caption}</div>"##,
        title = escape_html_text(stat.title),
        tone = if stat.alarming { "text-error" } else { "" },
        value = escape_html_text(stat.value),
        caption = escape_html_text(stat.caption),
    );

    match stat.href {
        Some(href) => format!(
            r##"<a class="stat py-3 transition hover:bg-base-300" href="{href}">{body}</a>"##,
            href = escape_html_text(&href),
        ),
        None => format!(r##"<div class="stat py-3">{body}</div>"##),
    }
}

/// How long ago, in the coarsest unit that still says something useful.
///
/// Split from the clock so it can be tested: "how long between these two instants" is a decision,
/// and reading `Utc::now()` inside it would make every assertion about it a race.
fn elapsed_label(since: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = (now - since).num_seconds();
    // A clock that disagrees with the database's is not worth rendering as a negative age.
    if seconds < 60 {
        return "just now".to_string();
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m ago");
    }
    let hours = minutes / 60;
    if hours < 24 {
        return format!("{hours}h ago");
    }
    format!("{}d ago", hours / 24)
}

fn relative_time(since: DateTime<Utc>) -> String {
    elapsed_label(since, Utc::now())
}

/// Cut a message to `limit` characters, on a character boundary, marking that it was cut.
fn truncate(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }
    let kept: String = value.chars().take(limit).collect();
    format!("{}…", kept.trim_end())
}

fn plural(count: i64) -> &'static str {
    if count == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::dashboard::TaskStatusCount;

    fn snapshot() -> DashboardSnapshot {
        DashboardSnapshot::default()
    }

    #[test]
    fn thousands_groups_digits() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(thousands(-4_321), "-4,321");
    }

    #[test]
    fn a_healthy_system_raises_no_alerts() {
        let snapshot = snapshot();
        assert_eq!(health_alerts(&snapshot.tasks, &snapshot.outbox), "");
    }

    #[test]
    fn a_lapsed_lease_is_called_out() {
        let mut snapshot = snapshot();
        snapshot.tasks.stalled = 2;

        let alerts = health_alerts(&snapshot.tasks, &snapshot.outbox);
        assert!(
            alerts.contains("2 tasks claimed under a lapsed lease"),
            "{alerts}"
        );
        assert!(alerts.contains("run again"), "{alerts}");
    }

    #[test]
    fn one_stalled_task_reads_singular() {
        let mut snapshot = snapshot();
        snapshot.tasks.stalled = 1;

        assert!(
            health_alerts(&snapshot.tasks, &snapshot.outbox)
                .contains("1 task claimed under a lapsed lease")
        );
    }

    #[test]
    fn an_expired_delivery_lease_is_called_out() {
        let mut snapshot = snapshot();
        snapshot.outbox.expired_leases = 1;

        let alerts = health_alerts(&snapshot.tasks, &snapshot.outbox);
        assert!(alerts.contains("1 delivery past its lease"), "{alerts}");
    }

    #[test]
    fn latencies_render_as_a_dash_before_anything_finishes() {
        let panel = attempt_panel(&AttemptStats {
            attempts: 3,
            ..AttemptStats::default()
        });

        // Three attempts started, none finished: a "0 ms" p95 here would be a lie.
        assert!(panel.contains("—"), "{panel}");
        assert!(!panel.contains("0 ms"), "{panel}");
    }

    #[test]
    fn counts_come_from_the_matching_status() {
        let tasks = TaskQueueHealth {
            by_status: vec![
                TaskStatusCount {
                    status: TaskStatus::Processing,
                    count: 4,
                },
                TaskStatusCount {
                    status: TaskStatus::DeadLetter,
                    count: 2,
                },
            ],
            stalled: 0,
            due_now: 0,
        };

        assert_eq!(tasks.count_of(TaskStatus::Processing), 4);
        assert_eq!(tasks.count_of(TaskStatus::DeadLetter), 2);
        assert_eq!(tasks.count_of(TaskStatus::Completed), 0);
        assert_eq!(tasks.total(), 6);
    }

    fn company() -> Company {
        Company {
            id: Uuid::nil(),
            user_id: Uuid::nil(),
            name: "Acme".into(),
            slug: "acme".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            created_at: chrono::Utc::now(),
        }
    }

    /// The live view is three attributes agreeing with each other. If the event name the page
    /// listens for ever drifts from the one the stream sends, the page still renders and simply
    /// never updates — a silent failure no other test would notice.
    #[test]
    fn the_page_subscribes_to_the_event_the_stream_sends() {
        let companies = [company()];
        let email = crate::entities::value_objects::EmailAddress::from("ops@example.test");
        let user = MailboxUser {
            id: Uuid::new_v4(),
            username: "ops",
            email: &email,
            avatar_url: None,
            is_operator: false,
        };
        let html = dashboard_page(&DashboardShell {
            user: &user,
            scope: DashboardScopeView::Company(&companies[0]),
            companies: &companies,
            window: DashboardWindow::last_hour(),
        });

        assert!(
            html.contains(r#"hx-ext="sse""#),
            "the extension must be enabled"
        );
        assert!(
            html.contains(r#"sse-connect="/ui/dashboard/events?company_id="#),
            "the stream must be scoped to the company on screen"
        );
        assert!(
            html.contains(&format!(r#"sse-swap="{DASHBOARD_EVENT}""#)),
            "the page must listen for the name the stream actually sends"
        );
        assert!(html.contains(&format!(r#"id="{DASHBOARD_PANELS_ID}""#)));
    }

    /// The operator view has no company to scope to, and must not invent one.
    #[test]
    fn the_global_view_streams_unscoped_and_offers_no_switcher() {
        let email = crate::entities::value_objects::EmailAddress::from("ops@example.test");
        let user = MailboxUser {
            id: Uuid::new_v4(),
            username: "ops",
            email: &email,
            avatar_url: None,
            is_operator: false,
        };
        let html = dashboard_page(&DashboardShell {
            user: &user,
            scope: DashboardScopeView::Global,
            companies: &[],
            window: DashboardWindow::last_hour(),
        });

        assert!(
            html.contains(r#"sse-connect="/ui/dashboard/events?window=1h""#),
            "a global stream carries the range but no company_id"
        );
        assert!(
            !html.contains("company_id="),
            "no company scoping anywhere on the page"
        );
        assert!(html.contains("Operator view"));
    }

    fn outstanding(status: TaskStatus, stalled: bool) -> OutstandingTask {
        OutstandingTask {
            id: Uuid::nil(),
            company_id: Uuid::nil(),
            company_name: "Acme".into(),
            channel_name: "support".into(),
            channel_id: Uuid::nil(),
            thread_id: Some(Uuid::nil()),
            task_type: "email_agent_dispatch".into(),
            status,
            stalled,
            retry_count: 0,
            last_error: None,
            since: Utc::now(),
        }
    }

    /// `companies` is the caller's own list, which is what decides whether a row can be linked.
    fn rendered_for(
        scope: DashboardScopeView<'_>,
        companies: &[Company],
        snapshot: &DashboardSnapshot,
    ) -> String {
        let email = crate::entities::value_objects::EmailAddress::from("ops@example.test");
        let user = MailboxUser {
            id: Uuid::new_v4(),
            username: "ops",
            email: &email,
            avatar_url: None,
            is_operator: false,
        };
        let process = ProcessGauges::default();
        dashboard_panels(&DashboardPage {
            user: &user,
            scope,
            companies,
            snapshot,
            window: DashboardWindow::last_hour(),
            process: &process,
        })
    }

    /// The caller belongs to the company every fixture row names, so links are offered.
    fn rendered(scope: DashboardScopeView<'_>, snapshot: &DashboardSnapshot) -> String {
        rendered_for(scope, &[company()], snapshot)
    }

    #[test]
    fn an_outstanding_task_links_to_its_thread() {
        let mut snapshot = snapshot();
        snapshot.outstanding = vec![outstanding(TaskStatus::PendingApproval, false)];

        let html = rendered(DashboardScopeView::Global, &snapshot);
        // `&` is escaped as `&amp;` because it is inside an attribute value; browsers read it back
        // as `&`. Asserting on the escaped form is asserting the attribute is well-formed.
        assert!(
            html.contains(
                "/ui?company_id=00000000-0000-0000-0000-000000000000\
                 &amp;channel_id=00000000-0000-0000-0000-000000000000\
                 &amp;thread_id=00000000-0000-0000-0000-000000000000"
            ),
            "{html}"
        );
        assert!(html.contains("Awaiting approval"), "{html}");
    }

    /// A task queued outside a conversation has no thread to open, and must not become a dead end.
    #[test]
    fn a_task_with_no_thread_links_to_the_task_instead() {
        let mut task = outstanding(TaskStatus::DeadLetter, false);
        task.thread_id = None;
        let mut snapshot = snapshot();
        snapshot.outstanding = vec![task];

        let html = rendered(DashboardScopeView::Global, &snapshot);
        assert!(html.contains("/ui/tasks?company_id="), "{html}");
        assert!(!html.contains("/ui?company_id="), "no thread link: {html}");
    }

    /// "Stalled" is `processing` with a lapsed lease, and that is the fact worth showing.
    #[test]
    fn a_lapsed_lease_outranks_the_stored_status() {
        let task = outstanding(TaskStatus::Processing, true);
        assert_eq!(outstanding_state(&task), "Stalled");
        assert!(task.needs_attention());

        let running = outstanding(TaskStatus::Processing, false);
        assert_eq!(outstanding_state(&running), "Running");
        assert!(
            !running.needs_attention(),
            "a running task is not a problem"
        );
    }

    /// Parked tasks are waiting on someone by design; only real trouble is tinted.
    #[test]
    fn parked_tasks_are_not_treated_as_trouble() {
        assert!(!outstanding(TaskStatus::PendingApproval, false).needs_attention());
        assert!(!outstanding(TaskStatus::WaitingForThirdPartyReply, false).needs_attention());
        assert!(outstanding(TaskStatus::DeadLetter, false).needs_attention());
    }

    /// The operator rollup shows companies the caller does not belong to. `/ui` resolves a company
    /// out of the caller's own list, so linking there would land them on "Create a company".
    #[test]
    fn a_row_in_a_company_the_caller_cannot_open_is_not_a_link() {
        let mut snapshot = snapshot();
        snapshot.outstanding = vec![outstanding(TaskStatus::DeadLetter, false)];

        // No companies: the caller is an operator watching someone else's traffic.
        let html = rendered_for(DashboardScopeView::Global, &[], &snapshot);

        assert!(
            html.contains("Dead-lettered"),
            "the row still shows: {html}"
        );
        assert!(
            !html.contains("/ui?company_id="),
            "an unreachable row must not offer a thread link: {html}"
        );
        assert!(
            html.contains("You are not a member of this company"),
            "and it should say why: {html}"
        );
        assert!(
            !html.contains(&icon(Icon::ChevronRight, "h-3 w-3")),
            "no chevron without somewhere to go: {html}"
        );
    }

    #[test]
    fn an_empty_outstanding_list_says_so() {
        let html = rendered(DashboardScopeView::Global, &snapshot());
        assert!(html.contains("Nothing outstanding"), "{html}");
    }

    /// `/ui/tasks` resolves one company, so a cross-company rollup must not pretend to link there.
    #[test]
    fn stat_figures_link_only_when_a_company_is_in_scope() {
        let companies = [company()];
        let snapshot = snapshot();

        let scoped = rendered_for(
            DashboardScopeView::Company(&companies[0]),
            &companies,
            &snapshot,
        );
        assert!(scoped.contains("/ui/tasks?company_id="), "{scoped}");

        let global = rendered(DashboardScopeView::Global, &snapshot);
        assert!(
            !global.contains("/ui/tasks?company_id="),
            "the operator view must not link a figure into an arbitrary company: {global}"
        );
    }

    #[test]
    fn elapsed_reads_in_the_coarsest_useful_unit() {
        let now = Utc::now();
        let ago = |seconds: i64| elapsed_label(now - chrono::Duration::seconds(seconds), now);

        assert_eq!(ago(5), "just now");
        assert_eq!(ago(90), "1m ago");
        assert_eq!(ago(60 * 75), "1h ago");
        assert_eq!(ago(60 * 60 * 50), "2d ago");
        // A row whose timestamp is ahead of this process's clock is not rendered as negative.
        assert_eq!(
            elapsed_label(now + chrono::Duration::hours(1), now),
            "just now"
        );
    }

    #[test]
    fn truncation_marks_what_it_cut_and_leaves_short_text_alone() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("abcdefghij", 10), "abcdefghij");
        assert_eq!(truncate("abcdefghijk", 10), "abcdefghij…");
        // Multi-byte input must not be sliced mid-character.
        assert_eq!(truncate("ααααα", 3), "ααα…");
    }

    #[test]
    fn an_empty_window_says_so_instead_of_drawing_an_empty_chart() {
        let panel = throughput_panel(&snapshot(), DashboardWindow::last_hour());
        assert!(panel.contains("Nothing has finished"), "{panel}");
    }
}
