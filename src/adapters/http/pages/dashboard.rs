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

use super::dashboard_query_health::database_query_health_section;
use super::dashboard_runtime::machine_section;
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
        AttemptStats, DashboardSnapshot, DashboardWindow, DeliveryHealth, LatencyBucket,
        OUTSTANDING_LIMIT, OutstandingTask, ProcessGauges, RetryRateBucket, TaskQueueHealth,
    },
    runtime_metrics::{MachineIdentity, RuntimeMetricSnapshot},
    task::TaskStatus,
    transport::DeliveryStatus,
};
use crate::services::database_query_health::DatabaseQueryHealth;

/// The id the SSE tick swaps, and the id the page renders it into.
pub const DASHBOARD_PANELS_ID: &str = "dashboard-panels";

/// The SSE event name the panels arrive under.
pub const DASHBOARD_EVENT: &str = "dashboard";

/// Which rollup is on screen.
///
/// An operator can switch between one company and every company. The active rollup remains an enum
/// rather than an `Option<&Company>` so each panel does not have to re-interpret `None`.
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
    /// The company workspace around the dashboard. In the system scope it is retained solely for
    /// the switch-back link and shared navigation; it never becomes a persistence filter.
    pub selected_company: Option<&'a Company>,
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
    pub process: Option<&'a ProcessGauges>,
    /// `None` hides the operator-only section; `Some(Unavailable)` shows a collection warning.
    pub query_health: Option<&'a DatabaseQueryHealth>,
    /// Present only after an operator-authorized runtime query.
    pub runtime: Option<&'a RuntimeMetricSnapshot>,
    pub machine: &'a MachineIdentity,
}

impl DashboardPage<'_> {
    /// Would a link into this company's `/ui` pages actually show the caller anything?
    ///
    /// `/ui` resolves a company out of the caller's *own* list, so a link into a
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
    let target = dashboard_target(shell.scope, shell.selected_company);
    let range = range_picker(target, shell.window);
    let operator_actions = matches!(shell.scope, DashboardScopeView::Global).then(|| {
        r##"<div class="p-3">
                <div class="alert alert-info py-2">
                    <span class="text-sm font-semibold">Operator view</span>
                </div>
                <a href="/ui/agent-library" class="btn btn-primary btn-sm mt-3 w-full">Manage agent library</a>
            </div>"##
    });
    let sidebar = format!(
        r##"<aside class="ui-pane-stacked flex w-72 shrink-0 flex-col border-r border-base-300 bg-base-200">
            {header}
            {operator_actions}
            {range}
            {scope}
        </aside>"##,
        operator_actions = operator_actions.unwrap_or_default(),
        scope = scope_picker(shell),
    );

    // `hx-ext="sse"` sits on the wrapper, not on the swapped element: a swap that replaced the
    // element carrying the connection would tear the stream down on its first tick.
    //
    // The immediate SSE snapshot is the only writer that fills the placeholder. Starting a
    // separate load request here lets its older response overwrite that snapshot; the live-status
    // banner already makes a connection failure visible and the stream reconnects itself.
    let content = format!(
        r##"
        {sidebar}
        <section class="flex min-w-0 flex-1 flex-col overflow-y-auto bg-base-100" hx-ext="sse"
            sse-connect="/ui/dashboard/events{query}">
            <div id="{panels_id}"{PANELS_SKELETON} sse-swap="{event}" hx-swap="innerHTML">{placeholder}</div>
        </section>
        "##,
        query = dashboard_query(target, shell.window),
        panels_id = DASHBOARD_PANELS_ID,
        event = DASHBOARD_EVENT,
        placeholder = panels_placeholder(),
    );

    ui_shell(&UiShell {
        title: &shell.scope.title(),
        user: shell.user,
        company: shell.selected_company,
        section: UiSection::Dashboard,
        content: &content,
    })
}

/// The body of the page: every panel, and nothing that owns the SSE connection.
pub fn dashboard_panels(page: &DashboardPage<'_>) -> String {
    // Only a company-scoped page can link its figures into `/ui`, which resolves exactly one
    // company — and only when that company is one the caller can open. Individual job rows below
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
                    {deliveries}
                    {outstanding}
                </div>
            </div>
            <div>
                <h2 class="mb-3 text-lg font-semibold">{window_label}</h2>
                <div class="flex flex-col gap-3">
                    {attempts}
                    {retry_rate}
                    {throughput}
                    {latency}
                    {queue_depth}
                </div>
            </div>
            {machine}
            {query_health}
        </div>
        "##,
        chart_style = chart::STYLE,
        alerts = health_alerts(&page.snapshot.tasks, &page.snapshot.deliveries),
        tasks = task_queue_panel(&page.snapshot.tasks, linked_company),
        deliveries = delivery_panel(&page.snapshot.deliveries, linked_company),
        outstanding = outstanding_panel(page),
        window_label = page.window.label(),
        attempts = attempt_panel(&page.snapshot.attempts),
        retry_rate = retry_rate_panel(page.snapshot, page.window),
        throughput = throughput_panel(page.snapshot, page.window),
        latency = latency_panel(page.snapshot, page.window),
        queue_depth = queue_depth_panel(page.snapshot, page.window),
        machine = machine_section(page),
        query_health = database_query_health_section(page.query_health),
    )
}

/// The two conditions worth interrupting someone over, shown only when they are non-zero.
///
/// A stalled task is a lease nobody is renewing: the row will be re-claimed and its agent will run
/// a second time, having already had its effects. An expired delivery lease is a send the sweep
/// will charge an attempt for without anyone knowing whether it went out, and an unconfirmed
/// outcome is one nothing will ever retry.
fn health_alerts(tasks: &TaskQueueHealth, deliveries: &DeliveryHealth) -> String {
    let mut alerts = String::new();

    if tasks.stalled > 0 {
        alerts.push_str(&alert_row(
            &format!(
                "{} workflow job{} claimed under a lapsed lease",
                tasks.stalled,
                plural(tasks.stalled)
            ),
            "Nothing is renewing the lease. Each will be re-claimed under a new fence.",
        ));
    }

    if deliveries.expired_leases > 0 {
        alerts.push_str(&alert_row(
            &format!(
                "{} deliver{} past its lease",
                deliveries.expired_leases,
                if deliveries.expired_leases == 1 {
                    "y"
                } else {
                    "ies"
                }
            ),
            "The next sweep will charge each an attempt without a provider result.",
        ));
    }

    // Nothing retries these, by design: an ambiguous provider outcome is exactly what must not be
    // re-sent. They are therefore stuck until a human or a reconciler clears them, which is why
    // they are worth interrupting someone over.
    let unconfirmed = deliveries.count_of(DeliveryStatus::OutcomeUnknown);
    if unconfirmed > 0 {
        alerts.push_str(&alert_row(
            &format!(
                "{} deliver{} with an unconfirmed provider outcome",
                unconfirmed,
                if unconfirmed == 1 { "y" } else { "ies" }
            ),
            "Nothing will retry these -- re-sending is how a duplicate is created. They need              reconciling by hand.",
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

/// The jobs behind the numbers, each one with a conversation a link to it.
///
/// This is what makes the counts actionable: "three stalled" is a fact, and this is the way to the
/// three. Trouble sorts to the top, so the row worth opening is the first one.
fn outstanding_panel(page: &DashboardPage<'_>) -> String {
    if page.snapshot.outstanding.is_empty() {
        return panel(
            "Outstanding workflow step jobs",
            r##"<div class="px-4 py-6 text-sm opacity-60">
                Nothing outstanding — every job has finished.
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

    // The cap is a cap, not a total: say so when it bites, or the panel quietly under-reports.
    let capped = if page.snapshot.outstanding.len() as i64 >= OUTSTANDING_LIMIT {
        format!(
            r##"<div class="px-4 pb-2 text-xs opacity-60">Showing the first {OUTSTANDING_LIMIT}.</div>"##
        )
    } else {
        String::new()
    };

    panel(
        "Outstanding workflow step jobs",
        &format!(r##"<div class="divide-y divide-base-300">{rows}</div>{capped}"##),
    )
}

/// One outstanding job: what state it is in, where it is, and — when there is one — the way to it.
///
/// `openable` is false for a company the caller does not belong to, which is the normal case in the
/// operator rollup. The row still shows everything it knows; it just does not offer a link that
/// would land on "Create a company".
fn outstanding_row(task: &OutstandingTask, show_company: bool, openable: bool) -> String {
    // A job in a conversation links to its thread; any other to the run it is a step of. The run
    // page re-checks company and channel access itself (`RunLinkAccess`), so a run on a channel
    // the caller cannot see answers not found rather than showing it.
    let destination = match (openable, task.channel_id, task.thread_id, task.run_id) {
        (true, Some(channel_id), Some(thread_id), _) => Some((
            format!(
                "/ui?company_id={company_id}&channel_id={channel_id}&thread_id={thread_id}",
                company_id = task.company_id,
            ),
            "Open thread",
        )),
        (true, _, None, Some(run_id)) => {
            Some((super::run_href(task.company_id, run_id), "Open run"))
        }
        _ => None,
    };

    let channel = task.channel_name.as_deref().unwrap_or("Company-wide");
    let where_ = if show_company {
        format!("{} / {channel}", task.company_name)
    } else {
        channel.to_owned()
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
        None if !openable => format!(
            r##"<div class="flex items-center gap-3 px-4 py-2" title="You are not a member of this company">{body}</div>"##
        ),
        None => format!(r##"<div class="flex items-center gap-3 px-4 py-2">{body}</div>"##),
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
        TaskStatus::Completed | TaskStatus::Failed => "Finished",
    }
}

/// The glyphs the thread column already uses for these states, so the two views agree.
fn outstanding_icon(task: &OutstandingTask) -> Icon {
    if task.stalled {
        Icon::Alert
    } else {
        Icon::DotFill
    }
}

/// The workflow queue's jobs (p08 D4.5): `background_tasks`, one per step a run takes, not runs.
/// The figures are plain: no page lists jobs by status. Run-level state is the runs board, linked
/// when the page is scoped to a company the caller can open.
fn task_queue_panel(tasks: &TaskQueueHealth, company_id: Option<Uuid>) -> String {
    let board = company_id
        .map(|company_id| {
            format!(
                r##"<div class="px-4 pb-2 text-xs opacity-70">Each job is one step of a run. <a class="link" href="/ui/workflow-runs?company_id={company_id}&amp;view=board">Runs by state</a></div>"##
            )
        })
        .unwrap_or_else(|| {
            r##"<div class="px-4 pb-2 text-xs opacity-70">Each job is one step of a run.</div>"##
                .to_owned()
        });
    panel(
        "Workflow step jobs",
        &format!(
            r##"<div class="stats stats-horizontal w-full overflow-x-auto">{due}{processing}{stalled}{total}</div>{board}"##,
            due = stat(Stat::new(
                "Due now",
                &tasks.due_now.to_string(),
                "pending & ready"
            )),
            processing = stat(Stat::new(
                "Processing",
                &tasks.count_of(TaskStatus::Processing).to_string(),
                "claimed",
            )),
            stalled = stat(
                Stat::new("Stalled", &tasks.stalled.to_string(), "lapsed lease")
                    .alarming(tasks.stalled > 0)
            ),
            total = stat(Stat::new("Total", &tasks.total().to_string(), "all time")),
        ),
    )
}

/// The delivery queue, transport-neutral.
///
/// "Unconfirmed" gets a column of its own rather than being folded into the failures: nothing
/// retries an ambiguous provider outcome -- doing so is how one message becomes two -- so it is a
/// distinct thing for a human to resolve, and a panel that could not tell it from a dead letter
/// would ask for the wrong action.
fn delivery_panel(deliveries: &DeliveryHealth, company_id: Option<Uuid>) -> String {
    let link = company_id.map(|company_id| format!("/ui/deliveries?company_id={company_id}"));
    let dead = deliveries.count_of(DeliveryStatus::DeadLetter);
    let unconfirmed = deliveries.count_of(DeliveryStatus::OutcomeUnknown);

    stat_row(
        "Deliveries",
        &format!(
            "{due}{sending}{expired}{delivered}{unconfirmed}{dead}",
            due = stat(
                Stat::new("Due now", &deliveries.due_now.to_string(), "queued & ready")
                    .linked(link.clone())
            ),
            sending = stat(
                Stat::new(
                    "Sending",
                    &deliveries.count_of(DeliveryStatus::Sending).to_string(),
                    "claimed",
                )
                .linked(link.clone())
            ),
            expired = stat(
                Stat::new(
                    "Lease expired",
                    &deliveries.expired_leases.to_string(),
                    "will be reaped",
                )
                .alarming(deliveries.expired_leases > 0)
                .linked(link.clone())
            ),
            delivered = stat(
                Stat::new(
                    "Delivered",
                    &deliveries.count_of(DeliveryStatus::Delivered).to_string(),
                    "all time",
                )
                .linked(link.clone())
            ),
            unconfirmed = stat(
                Stat::new("Unconfirmed", &unconfirmed.to_string(), "needs reconciling")
                    .alarming(unconfirmed > 0)
                    .linked(link.clone())
            ),
            dead = stat(
                Stat::new("Dead letter", &dead.to_string(), "all time")
                    .alarming(dead > 0)
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
    let retry_rate = match attempts.retry_rate_percent() {
        Some(rate) => format!("{rate:.0}%"),
        None => "—".to_string(),
    };

    stat_row(
        "Agent attempts",
        &format!(
            "{ran}{retries}{failed}{p50}{p95}{tokens}",
            ran = stat(Stat::new(
                "Attempts",
                &attempts.attempts.to_string(),
                "started"
            )),
            retries = stat(Stat::new(
                "Retry rate",
                &retry_rate,
                &format!("{} retry attempts", attempts.retries),
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

fn retry_rate_panel(snapshot: &DashboardSnapshot, window: DashboardWindow) -> String {
    if !snapshot.retry_rate.iter().any(|bucket| bucket.attempts > 0) {
        return panel(
            "Retry rate",
            r##"<div class="px-4 py-6 text-sm opacity-60">No attempt has started in this window yet.</div>"##,
        );
    }

    let series = [Series {
        label: "retry attempts",
        color: "var(--color-warning)",
        values: snapshot
            .retry_rate
            .iter()
            .map(RetryRateBucket::rate_percent)
            .collect(),
    }];
    let buckets: Vec<_> = snapshot
        .retry_rate
        .iter()
        .map(|bucket| bucket.bucket)
        .collect();

    panel(
        "Retry rate",
        &format!(
            "{chart}{footer}",
            chart = chart::time_chart(&TimeChart {
                buckets: &buckets,
                series: &series,
                kind: ChartKind::Line,
                unit: YUnit::Percent,
                tick_format: window.tick_format(),
            }),
            footer = chart_footer(
                "Attempts numbered two or higher divided by all attempts started in each bucket",
            ),
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
pub(super) fn chart_footer(note: &str) -> String {
    format!(
        r##"<div class="px-4 pb-3 text-xs opacity-60">{note}</div>"##,
        note = escape_html_text(note),
    )
}

/// This process's own counters, fenced off and labelled so they are not read as system-wide truth.
pub(super) fn process_panel(process: &ProcessGauges) -> String {
    let activity = stat_row(
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
    );
    let evidence = stat_row(
        "Operator evidence signals",
        &format!(
            "{tasks}{deliveries}{connections}",
            tasks = stat(Stat::new(
                "Deep task pages",
                &thousands(process.deep_task_pagination as i64),
                "1000+ offset observations since boot",
            )),
            deliveries = stat(Stat::new(
                "Deep delivery pages",
                &thousands(process.deep_delivery_pagination as i64),
                "1000+ offset observations since boot",
            )),
            connections = stat(Stat::new(
                "Dashboard streams",
                &thousands(process.active_dashboard_sse_connections as i64),
                "active now on this process",
            )),
        ),
    );
    let rows = format!("{activity}{evidence}");
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
        row = rows,
    )
}

/// The rollup a dashboard URL requests. A system target may retain a company id as navigation
/// context, but the route authorizes `scope=system` before treating that id as anything but a way
/// back to the company view.
#[derive(Debug, Clone, Copy)]
enum DashboardTarget {
    Company(Uuid),
    System(Option<Uuid>),
}

fn dashboard_target(
    scope: DashboardScopeView<'_>,
    selected_company: Option<&Company>,
) -> DashboardTarget {
    match scope {
        DashboardScopeView::Company(company) => DashboardTarget::Company(company.id),
        DashboardScopeView::Global => {
            DashboardTarget::System(selected_company.map(|company| company.id))
        }
    }
}

/// The query string that carries a dashboard's identity: active scope, company context, and range.
///
/// One function because it is appended to three URLs that must agree — the page's own links, the
/// panel fetch, and the SSE subscription. A range that reached the fetch but not the stream would
/// show the right chart for five seconds and then the wrong one for as long as the tab stayed open.
fn dashboard_query(target: DashboardTarget, window: DashboardWindow) -> String {
    match target {
        DashboardTarget::Company(company_id) => {
            format!("?company_id={company_id}&amp;window={}", window.slug())
        }
        DashboardTarget::System(Some(company_id)) => format!(
            "?scope=system&amp;company_id={company_id}&amp;window={}",
            window.slug()
        ),
        DashboardTarget::System(None) => format!("?scope=system&amp;window={}", window.slug()),
    }
}

/// The trailing range every panel below is read over.
///
/// Plain links, and deliberately in the sidebar rather than above the panels: everything inside
/// `#dashboard-panels` is thrown away and rebuilt on every tick, so a control living there would be
/// re-created under the reader's pointer several times a minute. Navigating is also what
/// re-establishes the SSE subscription against the new range — a control that only re-fetched the
/// panels would leave the stream ticking on the old one and overwrite itself.
fn range_picker(target: DashboardTarget, window: DashboardWindow) -> String {
    let options: String = DashboardWindow::PRESETS
        .into_iter()
        .map(|preset| {
            let selected = preset == window;

            format!(
                r##"<a class="btn btn-xs join-item{active}" href="/ui/dashboard{query}"{current}>{label}</a>"##,
                active = if selected { " btn-active" } else { "" },
                query = dashboard_query(target, preset),
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

fn scope_picker(shell: &DashboardShell<'_>) -> String {
    if !shell.user.is_operator {
        return scope_legend("This company");
    }

    let company_link = shell.selected_company.map(|company| {
        let active = matches!(shell.scope, DashboardScopeView::Company(_));
        format!(
            r##"<a class="btn btn-sm join-item justify-start{active}" href="/ui/dashboard{query}"{current}>{name}</a>"##,
            active = if active { " btn-active" } else { "" },
            query = dashboard_query(DashboardTarget::Company(company.id), shell.window),
            current = if active { r##" aria-current="page""## } else { "" },
            name = escape_html_text(&company.name),
        )
    });
    let system_active = matches!(shell.scope, DashboardScopeView::Global);
    let system_link = format!(
        r##"<a class="btn btn-sm join-item justify-start{active}" href="/ui/dashboard{query}"{current}>Whole system</a>"##,
        active = if system_active { " btn-active" } else { "" },
        query = dashboard_query(
            DashboardTarget::System(shell.selected_company.map(|company| company.id)),
            shell.window,
        ),
        current = if system_active {
            r##" aria-current="page""##
        } else {
            ""
        },
    );

    format!(
        r##"<div class="px-4 pb-4">
            <div class="mb-1 text-xs font-semibold uppercase tracking-wide opacity-60">Scope</div>
            <div class="join join-vertical w-full">{company_link}{system_link}</div>
        </div>"##,
        company_link = company_link.unwrap_or_default(),
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

pub(super) fn panel(heading: &str, body: &str) -> String {
    format!(
        r##"<div class="rounded-box border border-base-300 bg-base-200">
            <div class="border-b border-base-300 px-4 py-2 text-sm font-semibold opacity-80">{heading}</div>
            {body}
        </div>"##,
        heading = escape_html_text(heading),
    )
}

pub(super) fn stat_row(heading: &str, stats: &str) -> String {
    panel(
        heading,
        &format!(r##"<div class="stats stats-horizontal w-full overflow-x-auto">{stats}</div>"##),
    )
}

/// One figure in a stat row.
///
/// A struct rather than a fifth positional argument: `alarming` is already a bool, and a bare
/// `stat(.., false, None)` at seventeen call sites says nothing about which flag is which.
pub(super) struct Stat<'a> {
    title: &'a str,
    value: &'a str,
    caption: &'a str,
    /// Tints the figure. Reserved for counts that mean someone should look.
    alarming: bool,
    /// Where clicking through goes, when there is anywhere for it to go.
    href: Option<String>,
}

impl<'a> Stat<'a> {
    pub(super) fn new(title: &'a str, value: &'a str, caption: &'a str) -> Self {
        Self {
            title,
            value,
            caption,
            alarming: false,
            href: None,
        }
    }

    pub(super) fn alarming(mut self, alarming: bool) -> Self {
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
pub(super) fn stat(stat: Stat<'_>) -> String {
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
pub(super) fn elapsed_label(since: DateTime<Utc>, now: DateTime<Utc>) -> String {
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

pub(super) fn relative_time(since: DateTime<Utc>) -> String {
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
    use crate::entities::{company_member::CompanyMembership, dashboard::TaskStatusCount};

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
        assert_eq!(health_alerts(&snapshot.tasks, &snapshot.deliveries), "");
    }

    #[test]
    fn a_lapsed_lease_is_called_out() {
        let mut snapshot = snapshot();
        snapshot.tasks.stalled = 2;

        let alerts = health_alerts(&snapshot.tasks, &snapshot.deliveries);
        assert!(
            alerts.contains("2 workflow jobs claimed under a lapsed lease"),
            "{alerts}"
        );
        assert!(alerts.contains("re-claimed under a new fence"), "{alerts}");
    }

    #[test]
    fn one_stalled_task_reads_singular() {
        let mut snapshot = snapshot();
        snapshot.tasks.stalled = 1;

        assert!(
            health_alerts(&snapshot.tasks, &snapshot.deliveries)
                .contains("1 workflow job claimed under a lapsed lease")
        );
    }

    #[test]
    fn an_expired_delivery_lease_is_called_out() {
        let mut snapshot = snapshot();
        snapshot.deliveries.expired_leases = 1;

        let alerts = health_alerts(&snapshot.tasks, &snapshot.deliveries);
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
                    status: TaskStatus::Failed,
                    count: 2,
                },
            ],
            stalled: 0,
            due_now: 0,
        };

        assert_eq!(tasks.count_of(TaskStatus::Processing), 4);
        assert_eq!(tasks.count_of(TaskStatus::Failed), 2);
        assert_eq!(tasks.count_of(TaskStatus::Completed), 0);
        assert_eq!(tasks.total(), 6);
    }

    fn company() -> Company {
        Company {
            channel_defaults: Default::default(),
            id: Uuid::nil(),
            user_id: Uuid::nil(),
            name: "Acme".into(),
            slug: "acme".into(),
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
            company_membership: CompanyMembership::Admin,
        };
        let html = dashboard_page(&DashboardShell {
            user: &user,
            scope: DashboardScopeView::Company(&companies[0]),
            selected_company: Some(&companies[0]),
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
        assert!(
            !html.contains("Whole system"),
            "a non-operator must not be offered the global rollup"
        );
    }

    /// A system reading stays unscoped while retaining the selected company only as navigation
    /// context, so either side of the scope control can reconstruct the other.
    #[test]
    fn the_global_view_streams_unscoped_and_can_switch_back_to_the_company() {
        let companies = [company()];
        let email = crate::entities::value_objects::EmailAddress::from("ops@example.test");
        let user = MailboxUser {
            id: Uuid::new_v4(),
            username: "ops",
            email: &email,
            avatar_url: None,
            is_operator: true,
            company_membership: CompanyMembership::Admin,
        };
        let company_html = dashboard_page(&DashboardShell {
            user: &user,
            scope: DashboardScopeView::Company(&companies[0]),
            selected_company: Some(&companies[0]),
            companies: &companies,
            window: DashboardWindow::last_hour(),
        });
        assert!(
            company_html.contains(
                r#"href="/ui/dashboard?scope=system&amp;company_id=00000000-0000-0000-0000-000000000000&amp;window=1h""#
            ),
            "the company dashboard offers the whole-system scope: {company_html}"
        );

        let html = dashboard_page(&DashboardShell {
            user: &user,
            scope: DashboardScopeView::Global,
            selected_company: Some(&companies[0]),
            companies: &companies,
            window: DashboardWindow::last_hour(),
        });

        assert!(
            html.contains(
                r#"sse-connect="/ui/dashboard/events?scope=system&amp;company_id=00000000-0000-0000-0000-000000000000&amp;window=1h""#
            ),
            "the system scope is explicit while the company id remains navigation context"
        );
        assert!(
            html.contains(
                r#"href="/ui/dashboard?company_id=00000000-0000-0000-0000-000000000000&amp;window=1h""#
            ),
            "the selected company is one click away"
        );
        assert!(html.contains("Whole system"));
        assert!(html.contains("Acme"));
        assert!(html.contains("Operator view"));
    }

    fn outstanding(status: TaskStatus, stalled: bool) -> OutstandingTask {
        OutstandingTask {
            id: Uuid::nil(),
            company_id: Uuid::nil(),
            company_name: "Acme".into(),
            channel_name: Some("support".into()),
            channel_id: Some(Uuid::nil()),
            thread_id: Some(Uuid::nil()),
            run_id: Some(Uuid::nil()),
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
            company_membership: CompanyMembership::None,
        };
        let process = ProcessGauges::default();
        let machine = MachineIdentity {
            id: crate::entities::runtime_metrics::MachineId::new("machine-test"),
            region: None,
        };
        dashboard_panels(&DashboardPage {
            user: &user,
            scope,
            companies,
            snapshot,
            window: DashboardWindow::last_hour(),
            process: Some(&process),
            query_health: None,
            runtime: None,
            machine: &machine,
        })
    }

    /// The caller belongs to the company every fixture row names, so links are offered.
    fn rendered(scope: DashboardScopeView<'_>, snapshot: &DashboardSnapshot) -> String {
        rendered_for(scope, &[company()], snapshot)
    }

    #[test]
    fn infrastructure_panels_are_operator_only_and_missing_samples_render_safely() {
        let company = company();
        let companies = [company.clone()];
        let email = crate::entities::value_objects::EmailAddress::from("ops@example.test");
        let user = MailboxUser {
            id: Uuid::new_v4(),
            username: "ops",
            email: &email,
            avatar_url: None,
            is_operator: true,
            company_membership: CompanyMembership::None,
        };
        let machine = MachineIdentity {
            id: crate::entities::runtime_metrics::MachineId::new("serving-machine"),
            region: None,
        };
        let snapshot = DashboardSnapshot::default();
        let process = ProcessGauges::default();
        let runtime = RuntimeMetricSnapshot::default();

        let hidden = dashboard_panels(&DashboardPage {
            user: &user,
            scope: DashboardScopeView::Company(&company),
            companies: &companies,
            snapshot: &snapshot,
            window: DashboardWindow::last_hour(),
            process: None,
            query_health: None,
            runtime: None,
            machine: &machine,
        });
        assert!(!hidden.contains("This machine"), "{hidden}");
        assert!(!hidden.contains("serving-machine"), "{hidden}");

        let visible = dashboard_panels(&DashboardPage {
            user: &user,
            scope: DashboardScopeView::Company(&company),
            companies: &companies,
            snapshot: &snapshot,
            window: DashboardWindow::last_hour(),
            process: Some(&process),
            query_health: None,
            runtime: Some(&runtime),
            machine: &machine,
        });
        assert!(visible.contains("This machine"), "{visible}");
        assert!(visible.contains("serving-machine"), "{visible}");
        assert!(
            visible.contains('—'),
            "missing values use em dashes: {visible}"
        );
        assert!(visible.contains("Suggested maximum"), "{visible}");
        assert!(visible.contains("needs 24h and 100 samples"), "{visible}");
    }

    #[test]
    fn attempt_summary_and_chart_report_actual_retries() {
        let attempts = attempt_panel(&AttemptStats {
            attempts: 4,
            retries: 1,
            ..AttemptStats::default()
        });
        assert!(attempts.contains("Retry rate"), "{attempts}");
        assert!(attempts.contains("25%"), "{attempts}");
        assert!(attempts.contains("1 retry attempts"), "{attempts}");

        let mut snapshot = DashboardSnapshot::default();
        snapshot.retry_rate.push(RetryRateBucket {
            bucket: Utc::now(),
            attempts: 4,
            retries: 1,
        });
        let chart = retry_rate_panel(&snapshot, DashboardWindow::last_hour());
        assert!(chart.contains("Retry rate"), "{chart}");
        assert!(chart.contains("25%"), "{chart}");
    }

    #[test]
    fn an_outstanding_task_links_to_its_thread() {
        let mut snapshot = snapshot();
        snapshot.outstanding = vec![outstanding(TaskStatus::Pending, false)];

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
        assert!(html.contains("Queued"), "{html}");
    }

    /// A job outside a conversation, or of a run without a channel, has no thread to open: it links
    /// to the run it is a step of (plan new-board-view 04). A job whose execution is gone has no
    /// run either, and stays plain, without a chevron promising somewhere to go.
    #[test]
    fn a_job_with_no_thread_links_to_its_run() {
        let run = Uuid::from_u128(7);
        let mut threadless = outstanding(TaskStatus::Processing, true);
        threadless.thread_id = None;
        threadless.run_id = Some(run);
        let mut company_wide = outstanding(TaskStatus::Processing, true);
        company_wide.channel_id = None;
        company_wide.channel_name = None;
        company_wide.thread_id = None;
        company_wide.run_id = Some(run);
        let companies = [company()];
        let mut snapshot = snapshot();
        snapshot.outstanding = vec![threadless, company_wide];

        let html = rendered_for(
            DashboardScopeView::Company(&companies[0]),
            &companies,
            &snapshot,
        );
        let href = format!(
            r#"href="/ui/workflow-runs?company_id={}&amp;run_id={run}" title="Open run""#,
            Uuid::nil()
        );
        assert_eq!(html.matches(&href).count(), 2, "{html}");
        assert!(!html.contains("/ui?company_id="), "no thread link: {html}");
        assert!(html.contains("Company-wide"), "{html}");

        let mut orphan = outstanding(TaskStatus::Processing, true);
        orphan.thread_id = None;
        orphan.run_id = None;
        snapshot.outstanding = vec![orphan];
        let html = rendered_for(
            DashboardScopeView::Company(&companies[0]),
            &companies,
            &snapshot,
        );
        assert!(!html.contains("Open run"), "{html}");
        assert!(
            !html.contains(&icon(Icon::ChevronRight, "h-3 w-3")),
            "no chevron without somewhere to go: {html}"
        );
    }

    /// The job figures count `background_tasks` step jobs, which read easily as runs: the panel
    /// says so and, for a company the caller can open, points at the runs board.
    #[test]
    fn the_job_panel_counts_step_jobs_and_links_the_board() {
        let companies = [company()];
        let scoped = rendered_for(
            DashboardScopeView::Company(&companies[0]),
            &companies,
            &snapshot(),
        );
        assert!(scoped.contains("Workflow step jobs"), "{scoped}");
        assert!(scoped.contains("Each job is one step of a run."));
        assert!(scoped.contains(&format!(
            r#"href="/ui/workflow-runs?company_id={}&amp;view=board">Runs by state</a>"#,
            companies[0].id
        )));
        // The rollup and a company the caller cannot open name no board.
        for html in [
            rendered_for(DashboardScopeView::Global, &companies, &snapshot()),
            rendered_for(DashboardScopeView::Company(&companies[0]), &[], &snapshot()),
        ] {
            assert!(html.contains("Workflow step jobs"), "{html}");
            assert!(!html.contains("Runs by state"), "{html}");
        }
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

    /// A queued job is on its way; only a lapsed lease is trouble.
    #[test]
    fn a_queued_job_is_not_treated_as_trouble() {
        assert!(!outstanding(TaskStatus::Pending, false).needs_attention());
        assert!(outstanding(TaskStatus::Processing, true).needs_attention());
    }

    /// The operator rollup shows companies the caller does not belong to. `/ui` resolves a company
    /// out of the caller's own list, so linking there would land them on "Create a company".
    #[test]
    fn a_row_in_a_company_the_caller_cannot_open_is_not_a_link() {
        let mut snapshot = snapshot();
        snapshot.outstanding = vec![outstanding(TaskStatus::Processing, true)];

        // No companies: the caller is an operator watching someone else's traffic.
        let html = rendered_for(DashboardScopeView::Global, &[], &snapshot);

        assert!(html.contains("Stalled"), "the row still shows: {html}");
        assert!(
            !html.contains("/ui?company_id="),
            "an unreachable row must not offer a thread link: {html}"
        );
        assert!(
            !html.contains("/ui/workflow-runs"),
            "nor a run link: {html}"
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

    /// No page lists workflow jobs by status, so the job figures themselves are plain text (the
    /// panel's note links the runs board); the delivery figures still link to the company's
    /// deliveries.
    #[test]
    fn job_figures_are_plain_and_delivery_figures_link_for_a_company() {
        let companies = [company()];
        let snapshot = snapshot();

        let scoped = rendered_for(
            DashboardScopeView::Company(&companies[0]),
            &companies,
            &snapshot,
        );
        assert!(scoped.contains("Workflow step jobs"), "{scoped}");
        assert!(!scoped.contains("/ui/tasks"), "{scoped}");
        assert!(scoped.contains("/ui/deliveries?company_id="), "{scoped}");

        let global = rendered(DashboardScopeView::Global, &snapshot);
        assert!(!global.contains("/ui/deliveries?company_id="), "{global}");
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
