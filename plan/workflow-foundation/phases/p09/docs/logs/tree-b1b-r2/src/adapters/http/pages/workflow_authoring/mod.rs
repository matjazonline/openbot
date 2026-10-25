//! The `/ui/workflows` workspace (p09 B1a, B1b): the company's workflow library in the sidebar,
//! and one workflow's editor, validation preview, versions and usage, and bindings in
//! `#workflow-pane`. Every command a page sends is one of the lifecycle commands the JSON
//! routes call; the pages only render what those commands and the read models answer.
use super::*;
use crate::application::workflow::reads::DefinitionSummary;
use crate::application::workflow::templates::WorkflowTemplate;

mod bindings;
mod catalogue;
mod editor;
mod graph;
mod preview;
mod tools;
mod versions;

pub use bindings::*;
pub use catalogue::*;
pub use editor::*;
pub use graph::*;
pub use preview::*;
pub use tools::*;
pub use versions::*;

/// The sidebar list: one page of definitions, and the cursor for the next.
pub struct WorkflowList<'a> {
    pub company_id: Uuid,
    pub items: &'a [DefinitionSummary],
    pub selected: Option<Uuid>,
    pub next: Option<Uuid>,
}

pub struct WorkflowWorkspace<'a> {
    pub user: &'a MailboxUser<'a>,
    pub company: &'a Company,
    pub list: &'a WorkflowList<'a>,
    /// Pre-rendered `#workflow-pane`.
    pub pane_html: &'a str,
}

pub fn workflow_workspace_page(page: &WorkflowWorkspace<'_>) -> String {
    let company_id = page.company.id;
    let content = format!(
        r##"
        <aside class="ui-pane-list flex w-64 shrink-0 flex-col border-r border-base-300 bg-base-200">
            {header}
            <ul id="workflow-menu" class="menu w-full flex-1 flex-nowrap gap-1 overflow-y-auto px-2">{entries}</ul>
        </aside>
        {pane}
        "##,
        header = sidebar_header_with_action(
            "Workflows",
            "Drafts, published versions and where they run.",
            &format!(
                r##"<div class="flex shrink-0 gap-1"><button type="button" class="btn btn-ghost btn-xs"
                    hx-get="/ui/workflows/tools?company_id={company_id}"
                    hx-target="#workflow-pane" hx-swap="outerHTML" hx-sync="#workflow-pane:replace">Tools</button>
                <button type="button" class="btn btn-primary btn-xs"
                    hx-get="/ui/workflows/new?company_id={company_id}"
                    hx-target="#workflow-pane" hx-swap="outerHTML" hx-sync="#workflow-pane:replace"
                    hx-push-url="/ui/workflows?company_id={company_id}&new=1">{plus} New</button></div>"##,
                plus = icon(Icon::Plus, BUTTON_ICON),
            ),
        ),
        entries = workflow_list_entries(page.list),
        pane = page.pane_html,
    );
    ui_shell(&UiShell {
        title: &format!("{} Workflows", page.company.name),
        user: page.user,
        company: Some(page.company),
        section: UiSection::Workflows,
        content: &content,
    })
}

/// The list entries, and a "Load more" row that appends the next page in its place.
pub fn workflow_list_entries(list: &WorkflowList<'_>) -> String {
    let company_id = list.company_id;
    if list.items.is_empty() && list.next.is_none() {
        return r##"<li class="px-2 py-6 text-center text-xs opacity-60">No workflows yet. Start one from a template or a blank draft.</li>"##.to_owned();
    }
    let entries: String = list
        .items
        .iter()
        .map(|item| {
            let workflow_id = item.workflow.as_uuid();
            let badge = if item.archived {
                r#"<span class="badge badge-ghost badge-xs">archived</span>"#
            } else {
                ""
            };
            format!(
                r##"<li><a class="flex flex-col items-start gap-0.5 {active}"
                    href="/ui/workflows?company_id={company_id}&amp;workflow_id={workflow_id}"
                    hx-get="/ui/workflows/{workflow_id}?company_id={company_id}"
                    hx-target="#workflow-pane" hx-swap="outerHTML" hx-sync="#workflow-pane:replace"
                    hx-push-url="/ui/workflows?company_id={company_id}&amp;workflow_id={workflow_id}"
                    data-action="select-sidebar-item">
                    <span class="flex w-full min-w-0 items-center gap-2"><span class="min-w-0 truncate">{title}</span>{badge}</span>
                    <span class="w-full truncate font-mono text-[11px] opacity-60">draft revision {revision}</span>
                </a></li>"##,
                active = if list.selected == Some(workflow_id) {
                    "menu-active"
                } else {
                    ""
                },
                title = escape_html_text(&item.title),
                revision = item.revision,
            )
        })
        .collect();
    let more = match list.next {
        Some(after) => format!(
            r##"<li id="workflow-menu-more"><button type="button" class="btn btn-ghost btn-xs w-full"
                hx-get="/ui/workflows/list?company_id={company_id}&amp;after={after}"
                hx-target="#workflow-menu-more" hx-swap="outerHTML"
                hx-disabled-elt="this">Load more</button></li>"##
        ),
        None => String::new(),
    };
    format!("{entries}{more}")
}

/// The pane before a workflow is picked.
pub fn workflow_empty_pane(message: &str) -> String {
    format!(
        r##"<section id="workflow-pane" class="ui-pane-detail flex min-w-0 flex-1 items-center justify-center bg-base-100 p-8">
            <p class="text-center text-sm opacity-60">{}</p>
        </section>"##,
        escape_html_text(message)
    )
}

/// The shipped templates and the blank-draft form, for the "New" pane.
pub struct WorkflowNewPane<'a> {
    pub company_id: Uuid,
    pub templates: &'a [WorkflowTemplate],
    pub error: Option<&'a str>,
}

pub fn workflow_new_pane(pane: &WorkflowNewPane<'_>) -> String {
    let company_id = pane.company_id;
    let templates: String = pane
        .templates
        .iter()
        .map(|template| {
            let id = template.id().as_str();
            format!(
                r##"<li class="card bg-base-200">
                    <div class="card-body gap-2 p-4">
                        <h3 class="font-semibold">{title}</h3>
                        <p class="text-sm opacity-70">{description}</p>
                        <form hx-post="/ui/workflows/templates/{id_attr}/copy?company_id={company_id}"
                            hx-target="#workflow-pane" hx-swap="outerHTML"
                            hx-disabled-elt="find button[type='submit']">
                            <input type="hidden" name="revision" value="{revision}">
                            <input type="hidden" name="command_key" value="{key}">
                            <button type="submit" class="btn btn-sm btn-primary">
                                <span class="loading loading-spinner loading-xs hidden [.htmx-request_&]:inline-block"></span>
                                <span class="[.htmx-request_&]:hidden">Copy into this company</span>
                                <span class="hidden [.htmx-request_&]:inline">Copying…</span>
                            </button>
                        </form>
                        <p class="font-mono text-[11px] opacity-60">template {id_text} · revision {revision}</p>
                    </div>
                </li>"##,
                title = escape_html_text(template.title()),
                description = escape_html_text(template.description()),
                id_attr = escape_html_attr(id),
                id_text = escape_html_text(id),
                revision = template.revision().get(),
                key = Uuid::new_v4(),
            )
        })
        .collect();
    format!(
        r##"<section id="workflow-pane" class="ui-pane-detail flex min-w-0 flex-1 flex-col overflow-y-auto bg-base-100 px-4 py-4 sm:px-6">
            <h2 class="text-xl font-bold">New workflow</h2>
            {error}
            <div class="mt-4 space-y-3">
                <h3 class="font-semibold">Start from a shipped template</h3>
                <p class="text-sm opacity-70">A copy is a draft of this company's; the template itself never changes.</p>
                <ul class="grid gap-3">{templates}</ul>
            </div>
            <form class="mt-8 space-y-3" hx-post="/ui/workflows/new?company_id={company_id}"
                hx-target="#workflow-pane" hx-swap="outerHTML"
                hx-disabled-elt="find button[type='submit']">
                <h3 class="font-semibold">Or start a blank draft</h3>
                <input type="hidden" name="command_key" value="{key}">
                <label class="fieldset">
                    <span class="fieldset-legend">Title</span>
                    <input class="input w-full" name="title" required maxlength="256" autocomplete="off">
                </label>
                <button type="submit" class="btn btn-primary">
                    <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                    <span class="[.htmx-request_&]:hidden">Create draft</span>
                    <span class="hidden [.htmx-request_&]:inline">Creating…</span>
                </button>
            </form>
        </section>"##,
        error = form_error_banner(pane.error),
        key = Uuid::new_v4(),
    )
}

/// Which of a workflow's tabs the pane shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowTab {
    Editor,
    Versions,
    Bindings,
}
impl WorkflowTab {
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("versions") => Self::Versions,
            Some("bindings") => Self::Bindings,
            _ => Self::Editor,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Editor => "editor",
            Self::Versions => "versions",
            Self::Bindings => "bindings",
        }
    }
}

/// What the pane's header says about the draft.
pub struct WorkflowHeader<'a> {
    pub company_id: Uuid,
    pub workflow: Uuid,
    pub title: &'a str,
    pub revision: u64,
    pub archived: bool,
    pub versions: usize,
    pub template: Option<&'a str>,
}

/// One workflow's pane: its header, the tab strip and the tab's body. Also swapped in whole by
/// every write, so the header and the tabs always agree with the body.
pub fn workflow_pane(header: &WorkflowHeader<'_>, tab: WorkflowTab, body: &str) -> String {
    let (company_id, workflow) = (header.company_id, header.workflow);
    let tab_link = |target: WorkflowTab, label: &str| {
        format!(
            r##"<a role="tab" class="tab{active}" aria-selected="{selected}"
                href="/ui/workflows?company_id={company_id}&amp;workflow_id={workflow}&amp;tab={name}"
                hx-get="/ui/workflows/{workflow}?company_id={company_id}&amp;tab={name}"
                hx-target="#workflow-pane" hx-swap="outerHTML" hx-sync="#workflow-pane:replace"
                hx-push-url="/ui/workflows?company_id={company_id}&amp;workflow_id={workflow}&amp;tab={name}">{label}</a>"##,
            active = if tab == target { " tab-active" } else { "" },
            selected = tab == target,
            name = target.name(),
        )
    };
    let archived = if header.archived {
        r#"<span class="badge badge-ghost">archived</span>"#
    } else {
        ""
    };
    let origin = header
        .template
        .map(|template| {
            format!(
                r#" · copied from template <span class="font-mono">{}</span>"#,
                escape_html_text(template)
            )
        })
        .unwrap_or_default();
    format!(
        r##"<section id="workflow-pane" class="ui-pane-detail flex min-w-0 flex-1 flex-col bg-base-100">
            <div class="border-b border-base-300 px-4 pt-4 sm:px-6">
                <div class="flex flex-wrap items-center gap-2">
                    <h2 class="min-w-0 truncate text-xl font-bold">{title}</h2>{archived}
                </div>
                <p class="mt-1 text-xs opacity-60">Draft revision {revision} · {versions} published version{plural}{origin}</p>
                <nav role="tablist" class="tabs tabs-border mt-3" aria-label="Workflow views">{editor}{versions_tab}{bindings_tab}</nav>
            </div>
            <div class="flex-1 overflow-y-auto px-4 py-4 sm:px-6">{body}</div>
        </section>"##,
        title = escape_html_text(header.title),
        revision = header.revision,
        versions = header.versions,
        plural = if header.versions == 1 { "" } else { "s" },
        editor = tab_link(WorkflowTab::Editor, "Editor"),
        versions_tab = tab_link(WorkflowTab::Versions, "Versions and usage"),
        bindings_tab = tab_link(WorkflowTab::Bindings, "Bindings"),
    )
}

/// A JSON value as a readable, escaped block.
pub(crate) fn json_block(value: &serde_json::Value) -> String {
    format!(
        r#"<pre class="overflow-x-auto rounded-box bg-base-200 p-3 font-mono text-xs">{}</pre>"#,
        escape_html_text(&serde_json::to_string_pretty(value).unwrap_or_default())
    )
}

/// What a write did, above the pane it re-rendered.
pub(crate) fn status_banner(message: &str) -> String {
    format!(
        r#"<div role="status" class="alert alert-success mb-4 text-sm">{}</div>"#,
        escape_html_text(message)
    )
}
