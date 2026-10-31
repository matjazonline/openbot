//! The `/ui/workflow-runs` workspace (p09 B2a): the company's runs in the sidebar, filtered by
//! state and binding, and one run in `#run-pane`. Live updates follow the race-safe rules of
//! `src/adapters/http/AGENTS.md`: the `<main>` owns the run event stream and stays mounted; its
//! events are wake-ups only, and the list and the pane each read their own current state again,
//! with `hx-sync` replace so the last read wins.
use super::*;
use crate::application::workflow::reads::{MAX_LISTED, RunStateName, RunSummary};
use std::collections::HashMap;

mod detail;
mod step;

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
function discardDetachedRunResponse(answer) {
    if (!answer.detail.target.isConnected) answer.preventDefault();
}
document.addEventListener('htmx:beforeRequest', function (event) {
    var element = event.detail.elt;
    if (!element.closest('#run-pane, #run-list, #workflow-decision')) return;
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

/// The sidebar's filter, carried by every list request and "Load more".
#[derive(Debug, Clone, Copy, Default)]
pub struct RunFilterView<'a> {
    pub binding: Option<Uuid>,
    pub state: Option<&'a str>,
}

impl RunFilterView<'_> {
    fn query(&self, company_id: Uuid) -> String {
        let mut query = format!("company_id={company_id}");
        if let Some(binding) = self.binding {
            query.push_str(&format!("&amp;binding_id={binding}"));
        }
        if let Some(state) = self.state {
            query.push_str(&format!("&amp;state={}", escape_html_attr(state)));
        }
        query
    }
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
                hx-get="/ui/workflow-runs/list?{query}&amp;after={next}&amp;part=more" hx-target="#run-list-more"
                hx-swap="outerHTML" hx-sync="#run-list:replace" hx-include="#run-list-shown, #run-selected">Load more</button></li>"##,
            query = view.filter.query(company_id),
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
/// `hx-sync` would be inherited by "Load more" and its links.
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
    let binding = view
        .filter
        .binding
        .map(|binding| {
            format!(
                r#"<input type="hidden" name="binding_id" value="{binding}"><p class="text-xs opacity-70">Binding {short} only · <a class="link" href="/ui/workflow-runs?company_id={company_id}">all runs</a></p>"#,
                short = short_id(binding),
            )
        })
        .unwrap_or_default();
    format!(
        r##"<div id="run-list" class="flex min-h-0 flex-1 flex-col">
            <span hidden hx-get="/ui/workflow-runs/list?{query}&amp;part=entries" hx-trigger="sse:runs, sse:resync, run-selected from:body"
                hx-target="#run-list-entries" hx-swap="innerHTML" hx-sync="#run-list:replace"
                hx-include="#run-list-shown, #run-selected"></span>{shown}
            <form class="space-y-1 px-3 pb-2" hx-get="/ui/workflow-runs/list" hx-target="#run-list" hx-swap="outerHTML"
                hx-trigger="change" hx-sync="#run-list:replace" hx-include="#run-selected">
                <input type="hidden" name="company_id" value="{company_id}">{binding}
                <select class="select select-sm w-full" name="state" aria-label="Filter by state">{states}</select>
            </form>
            <ul id="run-list-entries" class="menu w-full flex-1 flex-nowrap gap-1 overflow-y-auto px-2">{entries}</ul>
        </div>"##,
        query = view.filter.query(company_id),
        shown = shown_input(view, false),
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

pub fn runs_workspace_page(page: &RunsWorkspace<'_>) -> String {
    let company_id = page.company.id;
    let content = format!(
        r##"<main class="flex min-w-0 flex-1" hx-ext="sse" sse-connect="/ui/workflow-runs/events?company_id={company_id}">
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
        section: UiSection::Workflows,
        content: &content,
    })
}

/// `#run-pane` with nothing selected (or nothing to show).
pub fn run_empty_pane(message: &str) -> String {
    format!(
        r#"<section id="run-pane" class="ui-pane-detail flex min-w-0 flex-1 items-center justify-center bg-base-100 p-8"><p class="text-sm opacity-70">{}</p></section>"#,
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
