//! The `/ui/workflow-runs` workspace (p09 B2a): the company's runs in the sidebar, filtered by
//! state and binding, and one run in `#run-pane`. Live updates follow the race-safe rules of
//! `src/adapters/http/AGENTS.md`: the `<main>` owns the run event stream and stays mounted; its
//! events are wake-ups only, and the list and the pane each read their own current state again,
//! with `hx-sync` replace so the last read wins.
use super::*;
use crate::application::workflow::reads::{MAX_LISTED, RunStateName, RunSummary};
use std::collections::HashMap;

mod board;
mod detail;
mod step;

pub use board::*;
pub use detail::*;
pub use step::*;

/// A write may replace the pane while a wake-up GET is still in flight. Its old target is
/// detached, but htmx's preserve pass searches the live document before swapping: without this
/// guard the late response can steal disclosures from the new pane. Listen on the requesting
/// element itself because a detached element's events no longer bubble to document.
pub const WORKFLOW_RUNS_SCRIPT: &str = r#"
// A comment may return after another reviewer closed the decision. Keep the latest local
// draft in a read-only recovery panel; a comment never submitted these values.
document.addEventListener('htmx:beforeSwap', function (event) {
    var detail = event.detail;
    if (detail.target.id !== 'workflow-decision' || !detail.requestConfig.path.split('?')[0].endsWith('/comments')) return;
    var draft = detail.target.querySelector('form[id^="decision-inputs-"]');
    if (!draft) return;
    var incoming = document.createElement('template');
    incoming.innerHTML = detail.serverResponse;
    if (incoming.content.querySelector('form[id^="decision-inputs-"]')) return;
    var section = incoming.content.querySelector('#workflow-decision');
    if (!section) return;
    var values = {};
    new FormData(draft).forEach(function (value, key) {
        if (key === 'feedback' || key === 'data' || key.startsWith('data:')) values[key] = value;
    });
    var recovery = document.createElement('section');
    recovery.id = 'decision-draft-recovery';
    recovery.className = 'space-y-2';
    var title = document.createElement('h2');
    title.className = 'font-semibold';
    title.textContent = 'Your unfinished decision input';
    var explanation = document.createElement('p');
    explanation.textContent = 'Posting a comment did not submit this input.';
    var content = document.createElement('pre');
    content.className = 'whitespace-pre-wrap text-sm';
    content.textContent = JSON.stringify(values, null, 2);
    recovery.append(title, explanation, content);
    section.append(recovery);
    detail.serverResponse = incoming.innerHTML;
});
// A board filter or a card click marks itself and the region it replaces busy until its answer
// settles (pages/AGENTS.md), an aborted or failed one included. A mark never reaches a history
// snapshot: Back would restore a region that looks busy for good.
var busyMarks = new Set();
function clearBusy(node) {
    node.removeAttribute('aria-busy');
    busyMarks.delete(node);
}
document.addEventListener('htmx:beforeRequest', function (event) {
    var element = event.detail.elt;
    var selector = element.dataset && element.dataset.busy;
    if (!selector) return;
    var marked = [element, document.querySelector(selector)].filter(Boolean);
    marked.forEach(function (node) {
        node.setAttribute('aria-busy', 'true');
        busyMarks.add(node);
    });
    element.addEventListener('htmx:afterRequest', function () {
        marked.forEach(clearBusy);
    }, { once: true });
});
document.addEventListener('htmx:beforeHistorySave', function () {
    busyMarks.forEach(clearBusy);
});
// The board's reload leaf (`data-board-wake`) reads at most once per BOARD_WAKE_MS per tab, and
// always once more after the last wake-up of a burst (plan new-board-view 03). Spacing is counted
// from completion, as `ui_run_counts.rs`'s scheduler does: while any read of the board is in flight
// -- a wake-up's or a filter change's -- a wake-up is held, and the interval starts when that read
// settles (answered, failed or aborted). So reads never overlap however slow they are (a read that
// replaced the one before would starve the board), there is at most one per interval after the
// previous one ends, and a wake-up that arrived after a read began is always followed by another
// read. A held wake-up is replayed on whichever leaf is current then (`board-wake`), so a filter
// change that replaced the leaf meanwhile cannot drop it. Opening a run is the reader's own
// action: it reads at once unless a board read is in flight, and then right after it settles.
// Plain `throttle` drops the trailing event and `delay` starves under constant change.
var BOARD_WAKE_MS = 2000;
// A read that never settles must not hold the board forever; past this it no longer counts.
var BOARD_READ_STALE_MS = 30000;
var boardWake = { inFlight: new Map(), settled: 0, held: false, urgent: false, timer: null };
function boardReading() {
    var now = Date.now();
    boardWake.inFlight.forEach(function (started, xhr) {
        if (now - started > BOARD_READ_STALE_MS) boardWake.inFlight.delete(xhr);
    });
    return boardWake.inFlight.size > 0;
}
// A read of the board itself (its region or its columns), not a card opening the pane.
function isBoardRead(detail) {
    var target = detail.target;
    return !!(target && target.closest && target.closest('#run-board'));
}
function scheduleBoardWake() {
    if (!boardWake.held || boardReading() || boardWake.timer) return;
    var wait = boardWake.urgent ? 0 : Math.max(0, boardWake.settled + BOARD_WAKE_MS - Date.now());
    boardWake.timer = setTimeout(function () {
        boardWake.timer = null;
        if (boardReading()) return;
        var leaf = document.querySelector('[data-board-wake]');
        boardWake.held = false;
        boardWake.urgent = false;
        if (!leaf) return;
        // A leaf a filter change just swapped in may not be processed yet; processing is
        // idempotent, and without it the replayed wake-up would find no listener.
        htmx.process(leaf);
        htmx.trigger(leaf, 'board-wake');
    }, wait);
}
document.addEventListener('htmx:confirm', function (event) {
    var leaf = event.detail.elt;
    if (!leaf.hasAttribute || !leaf.hasAttribute('data-board-wake')) return;
    var trigger = event.detail.triggeringEvent;
    var type = trigger && trigger.type;
    if (type === 'board-wake') return;
    var reading = boardReading();
    var idle = !reading && !boardWake.timer && !boardWake.held;
    if (type === 'run-selected' ? !reading : idle && Date.now() - boardWake.settled >= BOARD_WAKE_MS) {
        return;
    }
    event.preventDefault();
    boardWake.held = true;
    if (type === 'run-selected') {
        boardWake.urgent = true;
        clearTimeout(boardWake.timer);
        boardWake.timer = null;
    }
    scheduleBoardWake();
});
document.addEventListener('htmx:beforeRequest', function (event) {
    if (!isBoardRead(event.detail)) return;
    var xhr = event.detail.xhr;
    var element = event.detail.elt;
    boardWake.inFlight.set(xhr, Date.now());
    // A read is starting: what it reads covers every wake-up so far (an event is sent after its
    // change commits), so none stays held; later ones are held until it settles.
    boardWake.held = false;
    boardWake.urgent = false;
    clearTimeout(boardWake.timer);
    boardWake.timer = null;
    // On the element itself (a replaced form's events no longer bubble), and for this request
    // only: an element with two requests in flight sees the first one's settle event twice.
    element.addEventListener('htmx:afterRequest', function settled(done) {
        if (done.detail.xhr !== xhr) return;
        element.removeEventListener('htmx:afterRequest', settled);
        boardWake.inFlight.delete(xhr);
        boardWake.settled = Date.now();
        scheduleBoardWake();
    });
});
function discardDetachedRunResponse(answer) {
    if (!answer.detail.target.isConnected) answer.preventDefault();
}
document.addEventListener('htmx:beforeRequest', function (event) {
    var element = event.detail.elt;
    if (!element.closest('#run-pane, #run-list, #run-board, #workflow-decision')) return;
    element.addEventListener('htmx:beforeOnLoad', discardDetachedRunResponse, { once: true });
});
"#;

/// A run state as a badge: what it is, and for a waiting run what it waits on.
pub fn run_state_badge(state: &str, waiting_reason: Option<&str>) -> String {
    let (class, label) = match (state, waiting_reason) {
        ("queued", _) => ("badge-ghost", "queued".to_owned()),
        ("running", _) => ("badge-info", "running".to_owned()),
        ("waiting", Some("decision")) => ("badge-warning", "waiting for a decision".to_owned()),
        ("waiting", Some("reconciliation")) => {
            ("badge-error", "waiting on reconciliation".to_owned())
        }
        ("waiting", Some("child_run")) => ("badge-warning", "waiting for a child run".to_owned()),
        ("waiting", Some("effect")) => ("badge-warning", "waiting for a delivery".to_owned()),
        ("waiting", Some(reason)) => ("badge-warning", format!("waiting ({reason})")),
        ("waiting", None) => ("badge-warning", "waiting".to_owned()),
        ("succeeded", _) => ("badge-success", "succeeded".to_owned()),
        ("failed", _) => ("badge-error", "failed".to_owned()),
        ("cancelled", _) => ("badge-ghost", "cancelled".to_owned()),
        (other, _) => ("badge-ghost", other.to_owned()),
    };
    format!(
        r#"<span class="badge badge-sm {class}">{}</span>"#,
        escape_html_text(&label)
    )
}

/// The runs workspace's filter: the scope both views share (binding, workflow, agent) and the
/// list's state. One query builder for every link and request that must keep it — the List/Board
/// switch, "open in list", "Load more" and the wake-up reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct RunFilterView<'a> {
    pub binding: Option<Uuid>,
    pub workflow: Option<Uuid>,
    pub agent: Option<Uuid>,
    pub state: Option<&'a str>,
}

impl RunFilterView<'_> {
    /// The scope alone, the state dropped: what the board and the view switch keep.
    pub fn scope(&self) -> Self {
        Self {
            state: None,
            ..*self
        }
    }

    /// The query string, already escaped for an attribute.
    fn query(&self, company_id: Uuid) -> String {
        let mut query = format!("company_id={company_id}");
        for (name, id) in [
            ("binding_id", self.binding),
            ("workflow_id", self.workflow),
            ("agent_id", self.agent),
        ] {
            if let Some(id) = id {
                query.push_str(&format!("&amp;{name}={id}"));
            }
        }
        if let Some(state) = self.state {
            query.push_str(&format!("&amp;state={}", escape_html_attr(state)));
        }
        query
    }
}

/// A workflow or an agent the scope controls offer.
#[derive(Debug, Clone, PartialEq)]
pub struct ScopeOption {
    pub id: Uuid,
    pub label: String,
}

/// What the scope controls choose from, each list at most `MAX_BINDING_OPTIONS` by label, and
/// whether it was cut there.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScopeOptions {
    pub workflows: Vec<ScopeOption>,
    pub workflows_cut: bool,
    pub agents: Vec<ScopeOption>,
    pub agents_cut: bool,
}

/// The workflow and agent selects of either view, each with its "any" choice, the chosen one
/// selected (and kept when it is past the bound), and a note when the list was cut. A binding scope
/// is a hidden field with a link that drops it.
pub fn scope_controls(
    company_id: Uuid,
    filter: RunFilterView<'_>,
    options: &ScopeOptions,
    view: RunsView,
) -> String {
    let select = |name: &str,
                  label: &str,
                  any: &str,
                  chosen: Option<Uuid>,
                  items: &[ScopeOption],
                  cut: bool| {
        let mut found = false;
        let mut html: String = items
            .iter()
            .map(|item| {
                let selected = chosen == Some(item.id);
                found |= selected;
                format!(
                    r#"<option value="{}"{}>{}</option>"#,
                    item.id,
                    if selected { " selected" } else { "" },
                    escape_html_text(&item.label),
                )
            })
            .collect();
        if let Some(chosen) = chosen.filter(|_| !found) {
            html.push_str(&format!(
                r#"<option value="{chosen}" selected>{}</option>"#,
                short_id(chosen)
            ));
        }
        let note = if cut {
            format!(
                r#"<span class="text-xs opacity-70">Only the first {} {label}s are offered.</span>"#,
                items.len()
            )
        } else {
            String::new()
        };
        format!(
            r#"<select class="select select-sm w-auto max-w-full" name="{name}" aria-label="Filter by {label}"><option value="">{any}</option>{html}</select>{note}"#
        )
    };
    let binding = filter
        .binding
        .map(|binding| {
            let without = RunFilterView {
                binding: None,
                ..filter
            };
            let view_query = match view {
                RunsView::List => "",
                RunsView::Board => "&amp;view=board",
            };
            format!(
                r#"<input type="hidden" name="binding_id" value="{binding}"><span class="text-xs opacity-70">Binding {short} only · <a class="link" href="/ui/workflow-runs?{query}{view_query}">any binding</a></span>"#,
                short = short_id(binding),
                query = without.query(company_id),
            )
        })
        .unwrap_or_default();
    format!(
        "{binding}{}{}",
        select(
            "workflow_id",
            "workflow",
            "Any workflow",
            filter.workflow,
            &options.workflows,
            options.workflows_cut,
        ),
        select(
            "agent_id",
            "agent",
            "Any agent",
            filter.agent,
            &options.agents,
            options.agents_cut,
        ),
    )
}

pub struct RunListView<'a> {
    pub company_id: Uuid,
    pub items: &'a [RunSummary],
    /// Rows of this page on channels the viewer cannot see.
    pub hidden: usize,
    pub next: Option<Uuid>,
    pub selected: Option<Uuid>,
    /// Rows the list holds after this answer, visible or not: what a wake-up reads again.
    pub read: usize,
    pub filter: RunFilterView<'a>,
    /// Workflow titles by workflow id.
    pub titles: &'a HashMap<Uuid, String>,
    /// What the scope controls offer; read only for the whole region.
    pub options: &'a ScopeOptions,
}

/// The list's entries and its "Load more" row, which appends the next page in its place.
pub fn run_list_entries(view: &RunListView<'_>) -> String {
    let company_id = view.company_id;
    let mut html: String = view
        .items
        .iter()
        .map(|run| {
            let id = run.run.as_uuid();
            let title = view
                .titles
                .get(&run.workflow.as_uuid())
                .map(String::as_str)
                .unwrap_or("(workflow)");
            let lineage = match (run.parent, run.rerun_of) {
                (Some(parent), _) => format!(
                    r#"<span class="badge badge-xs badge-outline">child of {}</span>"#,
                    short_id(parent.as_uuid())
                ),
                (None, Some(original)) => format!(
                    r#"<span class="badge badge-xs badge-outline">rerun of {}</span>"#,
                    short_id(original.as_uuid())
                ),
                (None, None) => String::new(),
            };
            format!(
                r##"<li><a {attrs}
                    class="flex flex-col items-start gap-1{active}">
                    <span class="flex w-full items-center gap-2"><span class="min-w-0 truncate font-semibold">{title}</span>{state}</span>
                    <span class="font-mono text-[11px] opacity-70">run {short} · {created}</span>{lineage}</a></li>"##,
                attrs = run_pane_attrs(company_id, id),
                active = if view.selected == Some(id) {
                    " menu-active"
                } else {
                    ""
                },
                title = escape_html_text(title),
                state = run_state_badge(&run.state, run.waiting_reason.as_deref()),
                short = short_id(id),
                created = format_date_time(run.created_at),
            )
        })
        .collect();
    if view.hidden > 0 {
        html.push_str(&format!(
            r#"<li class="px-3 py-1 text-xs opacity-70">{} run{} of these {} on channels you cannot see.</li>"#,
            view.hidden,
            if view.hidden == 1 { "" } else { "s" },
            if view.hidden == 1 { "is" } else { "are" },
        ));
    }
    if let Some(next) = view.next.filter(|_| view.read < MAX_LISTED as usize) {
        html.push_str(&format!(
            r##"<li id="run-list-more"><button type="button" class="btn btn-ghost btn-sm"
                hx-get="/ui/workflow-runs/list?after={next}&amp;part=more" hx-target="#run-list-more"
                hx-swap="outerHTML" hx-sync="#run-list:replace" hx-include="#run-list-shown, #run-selected, #run-list-filters">Load more</button></li>"##,
        ));
    } else if view.next.is_some() {
        html.push_str(&format!(r#"<li class="px-3 py-1 text-xs opacity-70">Showing the newest {MAX_LISTED} matching runs. Older matching runs are not shown.</li>"#));
    }
    if view.items.is_empty() && view.hidden == 0 && view.next.is_none() {
        html.push_str(r#"<li class="px-3 py-2 text-sm opacity-70">No runs match.</li>"#);
    }
    html
}

/// How many rows the list holds, for a wake-up and "Load more" to send back.
fn shown_input(view: &RunListView<'_>, out_of_band: bool) -> String {
    format!(
        r#"<input type="hidden" id="run-list-shown" name="shown" value="{}"{}>"#,
        view.read,
        if out_of_band {
            r#" hx-swap-oob="true""#
        } else {
            ""
        },
    )
}

/// Updated entries (a wake-up or "Load more") and the number of rows the list now holds.
/// Wake-ups must update this too: the first page can grow while the workspace is open.
pub fn run_list_update(view: &RunListView<'_>) -> String {
    format!("{}{}", run_list_entries(view), shown_input(view, true))
}

/// The whole list region: the filter and the first page. Its entries read again on the stream's
/// `runs` and `resync` wake-ups and when another run is opened (never taking state from the
/// event), as many rows as the list holds and with the open run highlighted, in place, so what
/// "Load more" added and the scroll position stay. The reload is issued by an empty leaf that
/// syncs on `#run-list` with the filter, so the later of two reads wins; on the region itself
/// `hx-sync` would be inherited by "Load more" and its links. The leaf and "Load more" read the
/// filter form as it is now (`#run-list-filters`), never the scope the region was rendered with,
/// so a wake-up that cancels a filter change in flight reads the new choice instead of undoing it.
pub fn run_list(view: &RunListView<'_>) -> String {
    let company_id = view.company_id;
    let states: String = std::iter::once(("", "Any state"))
        .chain(
            RunStateName::ALL
                .iter()
                .map(|state| (state.as_str(), state.as_str())),
        )
        .map(|(value, label)| {
            format!(
                r#"<option value="{value}"{selected}>{label}</option>"#,
                selected = if view.filter.state.unwrap_or("") == value {
                    " selected"
                } else {
                    ""
                },
            )
        })
        .collect();
    format!(
        r##"<div id="run-list" class="flex min-h-0 flex-1 flex-col">
            <span hidden hx-get="/ui/workflow-runs/list?part=entries" hx-trigger="sse:runs, sse:resync, run-selected from:body"
                hx-target="#run-list-entries" hx-swap="innerHTML" hx-sync="#run-list:replace"
                hx-include="#run-list-shown, #run-selected, #run-list-filters"></span>{shown}
            <div class="px-3 pb-2">{toggle}</div>
            <form id="run-list-filters" class="flex flex-col gap-1 px-3 pb-2" hx-get="/ui/workflow-runs/list" hx-target="#run-list" hx-swap="outerHTML"
                hx-trigger="change" hx-sync="#run-list:replace" hx-include="#run-selected">
                <input type="hidden" name="company_id" value="{company_id}">
                <select class="select select-sm w-full" name="state" aria-label="Filter by state">{states}</select>{scope}
            </form>
            <ul id="run-list-entries" class="menu w-full flex-1 flex-nowrap gap-1 overflow-y-auto px-2">{entries}</ul>
        </div>"##,
        shown = shown_input(view, false),
        toggle = runs_view_toggle(company_id, view.filter.scope(), RunsView::List),
        scope = scope_controls(company_id, view.filter, view.options, RunsView::List),
        entries = run_list_entries(view),
    )
}

pub struct RunsWorkspace<'a> {
    pub user: &'a MailboxUser<'a>,
    pub company: &'a Company,
    /// Pre-rendered `#run-list`.
    pub list_html: &'a str,
    /// Pre-rendered `#run-pane`.
    pub pane_html: &'a str,
}

/// `<main>` takes `min-h-0`: under the shell's phone layout (a column) it would otherwise grow to
/// its content, and the page does not scroll, so a long list or board column could not either.
pub fn runs_workspace_page(page: &RunsWorkspace<'_>) -> String {
    let company_id = page.company.id;
    let content = format!(
        r##"<main class="flex min-h-0 min-w-0 flex-1" hx-ext="sse" sse-connect="/ui/workflow-runs/events?company_id={company_id}">
            <aside class="ui-pane-list flex w-72 shrink-0 flex-col border-r border-base-300 bg-base-200">
                {header}
                {list}
            </aside>
            {pane}
        </main>"##,
        header = sidebar_header_with_action(
            "Runs",
            "Every run of this company's workflows, newest first.",
            &format!(
                r#"<a class="btn btn-ghost btn-xs shrink-0" href="/ui/workflows?company_id={company_id}">Workflows</a>"#
            ),
        ),
        list = page.list_html,
        pane = page.pane_html,
    );
    ui_shell(&UiShell {
        title: &format!("{} Runs", page.company.name),
        user: page.user,
        company: Some(page.company),
        section: UiSection::Runs,
        content: &content,
    })
}

/// `#run-pane` with nothing selected (or nothing to show), marked empty so a phone opens on the
/// list (the shell's `syncMobilePane`).
pub fn run_empty_pane(message: &str) -> String {
    format!(
        r#"<section id="run-pane" data-pane-empty class="ui-pane-detail flex min-w-0 flex-1 items-center justify-center bg-base-100 p-8"><p class="text-sm opacity-70">{}</p></section>"#,
        escape_html_text(message)
    )
}

/// Full navigation from a conversation, schedule or queue into the run workspace.
pub fn run_href(company_id: Uuid, run_id: Uuid) -> String {
    format!("/ui/workflow-runs?company_id={company_id}&run_id={run_id}")
}

/// The destination reauthorizes company and channel access. Non-operators get attribution only.
#[derive(Clone, Copy)]
pub enum RunLinkAccess<'a> {
    Authorized(&'a std::collections::HashSet<Uuid>),
    Inspect,
    Label,
}

pub fn run_chip(company_id: Uuid, run_id: Uuid, access: RunLinkAccess<'_>) -> String {
    let short = short_id(run_id);
    let access = match access {
        RunLinkAccess::Authorized(ids) if ids.contains(&run_id) => RunLinkAccess::Inspect,
        RunLinkAccess::Authorized(_) => RunLinkAccess::Label,
        other => other,
    };
    match access {
        RunLinkAccess::Inspect => format!(
            r#"<a class="link font-mono" href="{}" title="Run {run_id}">run {short}</a>"#,
            escape_html_attr(&run_href(company_id, run_id))
        ),
        RunLinkAccess::Label | RunLinkAccess::Authorized(_) => {
            format!(r#"<span class="font-mono" title="Run {run_id}">run {short}</span>"#)
        }
    }
}

pub fn message_run_attribution(
    message: &ThreadMessageView,
    company_id: Uuid,
    access: RunLinkAccess<'_>,
) -> String {
    let chips = message_run_chips(&message.run_ids, message.direction, company_id, access);
    format!(
        r#"<span id="message-runs-{}">{chips}</span>"#,
        message.canonical_id.as_uuid()
    )
}

fn message_run_chips(
    run_ids: &[Uuid],
    direction: MessageDirection,
    company_id: Uuid,
    access: RunLinkAccess<'_>,
) -> String {
    let chips = run_ids
        .iter()
        .map(|id| run_chip(company_id, *id, access))
        .collect::<Vec<_>>()
        .join(", ");
    if chips.is_empty() {
        String::new()
    } else {
        let label = if direction == MessageDirection::Outbound {
            "Written by"
        } else {
            "Started"
        };
        format!("{label} {chips}")
    }
}

/// Admission can follow the message's initial render. Reconcile attribution on the same wakeup
/// as activity without replacing bubbles, scrolling the thread or disturbing the composer.
pub fn thread_run_attribution_updates(
    runs: &crate::application::thread_activity::ThreadRuns,
    company_id: Uuid,
    access: RunLinkAccess<'_>,
) -> String {
    runs.message_runs
        .iter()
        .map(|message| {
            format!(
                r#"<span id="message-runs-{}" hx-swap-oob="innerHTML">{}</span>"#,
                message.canonical_id,
                message_run_chips(&message.run_ids, message.direction, company_id, access)
            )
        })
        .collect()
}
