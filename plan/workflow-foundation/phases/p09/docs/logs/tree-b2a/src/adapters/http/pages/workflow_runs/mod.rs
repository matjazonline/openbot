//! The `/ui/workflow-runs` workspace (p09 B2a): the company's runs in the sidebar, filtered by
//! state and binding, and one run in `#run-pane`. Live updates follow the race-safe rules of
//! `src/adapters/http/AGENTS.md`: the `<main>` owns the run event stream and stays mounted; its
//! events are wake-ups only, and the list and the pane each read their own current state again,
//! with `hx-sync` replace so the last read wins.
use super::*;
use crate::application::workflow::reads::RunSummary;
use std::collections::HashMap;

mod detail;
mod step;

pub use detail::*;
pub use step::*;

/// The states the sidebar can filter by, as stored.
const STATES: [&str; 6] = [
    "queued",
    "running",
    "waiting",
    "succeeded",
    "failed",
    "cancelled",
];

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
                r##"<li><a href="/ui/workflow-runs?company_id={company_id}&amp;run_id={id}"
                    hx-get="/ui/workflow-runs/{id}?company_id={company_id}" hx-target="#run-pane" hx-swap="outerHTML"
                    hx-sync="#run-pane:replace" hx-push-url="/ui/workflow-runs?company_id={company_id}&amp;run_id={id}"
                    class="flex flex-col items-start gap-1{active}">
                    <span class="flex w-full items-center gap-2"><span class="min-w-0 truncate font-semibold">{title}</span>{state}</span>
                    <span class="font-mono text-[11px] opacity-70">run {short} · {created}</span>{lineage}</a></li>"##,
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
            r#"<li class="px-3 py-1 text-xs opacity-70">{} run{} of this page {} on channels you cannot see.</li>"#,
            view.hidden,
            if view.hidden == 1 { "" } else { "s" },
            if view.hidden == 1 { "is" } else { "are" },
        ));
    }
    if let Some(next) = view.next {
        html.push_str(&format!(
            r##"<li id="run-list-more"><button type="button" class="btn btn-ghost btn-sm"
                hx-get="/ui/workflow-runs/list?{query}&amp;after={next}&amp;more=1" hx-target="#run-list-more"
                hx-swap="outerHTML">Load more</button></li>"##,
            query = view.filter.query(company_id),
        ));
    }
    if view.items.is_empty() && view.hidden == 0 && view.next.is_none() {
        html.push_str(r#"<li class="px-3 py-2 text-sm opacity-70">No runs match.</li>"#);
    }
    html
}

/// The whole list region: the filter and the first page. It reloads on the stream's `runs` and
/// `resync` wake-ups (reading again, never taking state from the event), through an empty leaf
/// that syncs on `#run-list` with the filter, so the later of two reads wins; on the region itself
/// `hx-sync` would be inherited by "Load more" and its links.
pub fn run_list(view: &RunListView<'_>) -> String {
    let company_id = view.company_id;
    let states: String = std::iter::once(("", "Any state"))
        .chain(STATES.iter().map(|state| (*state, *state)))
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
            <span hidden hx-get="/ui/workflow-runs/list?{query}" hx-trigger="sse:runs, sse:resync"
                hx-target="#run-list" hx-swap="outerHTML" hx-sync="#run-list:replace"></span>
            <form class="space-y-1 px-3 pb-2" hx-get="/ui/workflow-runs/list" hx-target="#run-list" hx-swap="outerHTML"
                hx-trigger="change" hx-sync="#run-list:replace">
                <input type="hidden" name="company_id" value="{company_id}">{binding}
                <select class="select select-sm w-full" name="state" aria-label="Filter by state">{states}</select>
            </form>
            <ul class="menu w-full flex-1 flex-nowrap gap-1 overflow-y-auto px-2">{entries}</ul>
        </div>"##,
        query = view.filter.query(company_id),
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
