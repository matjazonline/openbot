//! Workflow tools (p09 B1b, notice WF-C003): the company's registrations with revoke, the
//! versions tab's "Use as a workflow tool" form and agent usage. Agent authoring owns typed
//! assignments and independent publication. Retained pins require the registration to keep
//! naming the same workflow, even when its current version changes.
use super::*;
use crate::application::workflow::RelatedAssociation;
use crate::application::workflow::reads::{BindingUse, ToolTarget, VersionSummary};
use crate::application::workflow::tools::WorkflowToolRegistration;

/// Registration authority is checked at agent publication, run admission and each new call.
pub const REVOKE_WARNING: &str = "Revoking refuses new runs whose selected agents need this workflow tool, and running agents' next calls to it. A call already in progress still returns its result. Workflow versions and binding configuration stay saved. Registering the name again does not restore revoked agent binding access. Select the tool again to create a fresh binding and publish the agent; the workflow that selects the agent keeps its version.";

/// A retained agent binding never follows a registration to another workflow.
pub const REPOINT_WARNING: &str = "It is registered for another workflow. Pointing it here refuses new runs and new calls through agent bindings to the old workflow. A call already in progress still returns its result. Select this target in the agent draft and publish a new agent revision to use it; the workflow that selects the agent keeps its version.";

pub const REPIN_NOTE: &str = "It is registered for this workflow already. Re-pinning changes the choices offered for new agent tool selections. Published agents and running work keep their pinned workflow version.";

fn target_label(targets: &[ToolTarget], registration: &WorkflowToolRegistration) -> String {
    match targets
        .iter()
        .find(|target| target.version == registration.version)
    {
        Some(target) => format!(
            "{} · version {} (draft revision {}){}",
            target.workflow_title,
            short_id(target.version.as_uuid()),
            target.draft_revision,
            if target.archived { " · archived" } else { "" },
        ),
        None => format!(
            "workflow {} · version {}",
            short_id(registration.workflow.as_uuid()),
            short_id(registration.version.as_uuid())
        ),
    }
}

pub struct ToolsView<'a> {
    pub company_id: Uuid,
    pub tools: &'a [WorkflowToolRegistration],
    pub targets: &'a [ToolTarget],
    pub banner: Option<&'a str>,
    pub error: Option<&'a str>,
}

/// `#workflow-pane` with the company's workflow tools.
pub fn workflow_tools_pane(view: &ToolsView<'_>) -> String {
    let company_id = view.company_id;
    let rows: String = view
        .tools
        .iter()
        .map(|tool| {
            let name = tool.name.as_str();
            format!(
                r##"<li class="py-3">
                    <div class="flex flex-wrap items-center gap-2"><span class="font-mono font-semibold">{name_text}</span>
                        <a class="link text-sm" href="/ui/workflows?company_id={company_id}&amp;workflow_id={workflow}&amp;tab=versions">{target}</a>
                        <button type="button" class="btn btn-error btn-xs ml-auto"
                            hx-post="/ui/workflows/tools/revoke?company_id={company_id}"
                            hx-vals='{{"name": "{name_attr}", "expected_revision": "{revision}"}}'
                            hx-confirm="Revoke {name_attr}? {warning}"
                            hx-target="#workflow-pane" hx-swap="outerHTML">Revoke</button></div>
                    <p class="font-mono text-[11px] opacity-60">registration revision {revision}</p>
                    <details class="mt-1"><summary class="cursor-pointer text-xs opacity-70">Static parameters</summary>{params}</details>
                </li>"##,
                name_text = escape_html_text(name),
                name_attr = escape_html_attr(name),
                workflow = tool.workflow.as_uuid(),
                target = escape_html_text(&target_label(view.targets, tool)),
                revision = tool.revision,
                warning = escape_html_attr(REVOKE_WARNING),
                params = json_block(&tool.params),
            )
        })
        .collect();
    let list = if view.tools.is_empty() {
        r#"<p class="text-sm opacity-70">No workflow tools yet. Register one from a workflow's Versions tab; an agent calls it only once the agent grants its name.</p>"#.to_owned()
    } else {
        format!(r#"<ul class="divide-y divide-base-300">{rows}</ul>"#)
    };
    format!(
        r#"<section id="workflow-pane" class="ui-pane-detail flex min-w-0 flex-1 flex-col bg-base-100">
            <div class="border-b border-base-300 px-4 py-4 sm:px-6"><h2 class="text-xl font-bold">Workflow tools</h2>
            <p class="mt-1 text-xs opacity-60">Published workflows agents may call as tools. A version that froze a tool keeps calling the version it froze; re-pinning a name changes only what later publications freeze.</p></div>
            <div class="flex-1 space-y-4 overflow-y-auto px-4 py-4 sm:px-6">{banner}{error}{list}</div>
        </section>"#,
        banner = view.banner.map(status_banner).unwrap_or_default(),
        error = form_error_banner(view.error),
    )
}

/// The registration a "Use as a workflow tool" submission found under its name.
pub struct ExistingTool<'a> {
    pub registration: &'a WorkflowToolRegistration,
    pub target: String,
    /// Whether the existing registration is for this same workflow.
    pub same_workflow: bool,
}

pub struct ToolFormView<'a> {
    pub company_id: Uuid,
    pub workflow: Uuid,
    /// Newest first.
    pub versions: &'a [VersionSummary],
    pub name: &'a str,
    pub version: Option<Uuid>,
    pub params: &'a str,
    /// Set when the name is registered already: the form asks before replacing it.
    pub existing: Option<ExistingTool<'a>>,
    pub banner: Option<&'a str>,
    pub error: Option<&'a str>,
}

pub fn workflow_tool_form(form: &ToolFormView<'_>) -> String {
    let (company_id, workflow) = (form.company_id, form.workflow);
    if form.versions.is_empty() {
        return String::new();
    }
    let versions: String = form
        .versions
        .iter()
        .enumerate()
        .map(|(index, version)| {
            let id = version.version.as_uuid();
            format!(
                r#"<option value="{id}"{selected}>Version {short} · draft revision {revision}{newest}</option>"#,
                selected = if Some(id) == form.version { " selected" } else { "" },
                short = short_id(id),
                revision = version.draft_revision,
                newest = if index == 0 { " (newest)" } else { "" },
            )
        })
        .collect();
    let (confirm, button) = match &form.existing {
        None => (String::new(), "Register tool"),
        Some(existing) => {
            let consequence = if existing.same_workflow {
                REPIN_NOTE
            } else {
                REPOINT_WARNING
            };
            (
                format!(
                    r#"<div role="alert" class="alert alert-warning text-sm"><div><p class="font-semibold">{name} is {target} (revision {revision}).</p><p>{consequence}</p></div></div>
                    <input type="hidden" name="expected_revision" value="{revision}">"#,
                    name = escape_html_text(existing.registration.name.as_str()),
                    target = escape_html_text(&existing.target),
                    revision = existing.registration.revision,
                    consequence = escape_html_text(consequence),
                ),
                if existing.same_workflow {
                    "Re-pin tool"
                } else {
                    "Point tool here"
                },
            )
        }
    };
    format!(
        r##"<section id="workflow-tool-form" class="space-y-3 rounded-box border border-base-300 p-4">
            <div><h3 class="font-semibold">Use as a workflow tool</h3>
            <p class="text-xs opacity-70">An agent that grants the name can call this version as a tool. The version needs an input and an output schema. Names starting with <span class="font-mono">message.</span> and the provisioning tools are reserved.</p></div>
            {banner}{error}
            <form hx-post="/ui/workflows/{workflow}/tools?company_id={company_id}" hx-target="#workflow-tool-form" hx-swap="outerHTML"
                hx-disabled-elt="find button[type='submit']" class="space-y-3">
                {confirm}
                <div class="grid gap-3 sm:grid-cols-2">
                    <label class="form-control w-full"><div class="label"><span class="text-xs">Tool name</span></div>
                        <input class="input w-full font-mono" name="name" required maxlength="128" value="{name}" placeholder="lookup_order"></label>
                    <label class="form-control w-full"><div class="label"><span class="text-xs">Version</span></div>
                        <select class="select w-full" name="version_id">{versions}</select></label>
                </div>
                <label class="form-control w-full"><div class="label"><span class="text-xs">Static parameters (JSON object, validated against the version's parameter schema)</span></div>
                    <textarea class="textarea w-full font-mono text-xs" rows="3" name="params_json">
{params}</textarea></label>
                <button type="submit" class="btn btn-outline btn-sm">{button}</button>
                <a class="link text-xs" href="/ui/workflows?company_id={company_id}&amp;tools=1">All workflow tools</a>
            </form>
        </section>"##,
        banner = form.banner.map(status_banner).unwrap_or_default(),
        error = form_error_banner(form.error),
        name = escape_html_attr(form.name),
        params = escape_html_text(form.params),
    )
}

/// Workflow bindings that reference this company agent.
pub struct AgentUsageCard<'a> {
    pub company_id: Uuid,
    /// The agent the card describes; its board is linked.
    pub agent: Uuid,
    pub bindings: &'a [BindingUse],
    /// Of the listed bindings, those on channels the viewer cannot see.
    pub hidden: usize,
    pub truncated: bool,
}

/// The agent pane's "Used by" card (F098.2): the bindings whose bound version froze this
/// agent, directly or through a child workflow it pins.
pub fn agent_usage_card(card: &AgentUsageCard<'_>) -> String {
    let company_id = card.company_id;
    let rows: String = card
        .bindings
        .iter()
        .map(|binding| {
            let summary = &binding.binding;
            format!(
                r#"<li class="py-2"><div class="flex flex-wrap items-center gap-2">
                    <a class="link font-semibold" href="/ui/workflows?company_id={company_id}&amp;workflow_id={workflow}&amp;tab=bindings">{title}</a>{state}</div>
                    <p class="text-xs opacity-70">{association}</p></li>"#,
                workflow = summary.workflow.as_uuid(),
                title = escape_html_text(if binding.workflow_title.is_empty() {
                    "(untitled workflow)"
                } else {
                    &binding.workflow_title
                }),
                state = if summary.active {
                    r#"<span class="badge badge-sm badge-success">active</span>"#
                } else {
                    r#"<span class="badge badge-sm badge-ghost">paused</span>"#
                },
                association = escape_html_text(&match summary.association {
                    RelatedAssociation::Company => "Company-wide".to_owned(),
                    RelatedAssociation::Channel(_) => format!(
                        "Channel {}",
                        binding.channel_name.as_deref().unwrap_or("(unnamed)")
                    ),
                    RelatedAssociation::Thread { .. } => format!(
                        "One thread of channel {}",
                        binding.channel_name.as_deref().unwrap_or("(unnamed)")
                    ),
                }),
            )
        })
        .collect();
    let list = if card.bindings.is_empty() && card.hidden == 0 {
        r#"<p class="text-xs opacity-70">No binding's version froze this agent. A workflow uses it only once a version that names it is published and bound.</p>"#.to_owned()
    } else {
        format!(r#"<ul class="divide-y divide-base-300">{rows}</ul>"#)
    };
    format!(
        r#"<section id="agent-workflow-usage" class="mt-6 space-y-2 rounded-box border border-base-300 p-4">
            <div class="flex flex-wrap items-start gap-2"><div class="min-w-0 flex-1"><h3 class="font-semibold">Used by</h3>
            <p class="text-sm opacity-70">Bindings whose published version froze this agent, directly or through a child workflow.</p></div>
            <a class="btn btn-ghost btn-xs" href="/ui/workflow-runs?company_id={company_id}&amp;view=board&amp;agent_id={agent}">Runs board</a></div>
            {list}{hidden}{cut}
        </section>"#,
        agent = card.agent,
        hidden = if card.hidden > 0 {
            format!(
                r#"<p class="text-xs opacity-70">{} more of the listed binding{} {} on channels you cannot see.</p>"#,
                card.hidden,
                if card.hidden == 1 { "" } else { "s" },
                if card.hidden == 1 { "is" } else { "are" },
            )
        } else {
            String::new()
        },
        cut = if card.truncated {
            r#"<p class="text-xs text-error">Only the first 50 bindings are listed.</p>"#
        } else {
            ""
        },
    )
}

/// A card the page loads after it renders, from a workflow route; a failed load says so.
pub(crate) fn lazy_card(url: &str, loading: &str, failed: &str) -> String {
    format!(
        r#"<div class="mt-6 rounded-box border border-dashed border-base-300 p-4 text-xs opacity-70" aria-busy="true"
            hx-get="{url}" hx-trigger="load" hx-swap="outerHTML" data-lazy-card="{failed}">{loading}</div>"#,
        url = escape_html_attr(url),
        failed = escape_html_attr(failed),
        loading = escape_html_text(loading),
    )
}

/// The agent pane's workflow cards: its tool grants and the bindings that run it.
pub(crate) fn workflow_agent_usage_card(company_id: Uuid, agent: Uuid) -> String {
    lazy_card(
        &format!("/ui/workflows/agents/{agent}/usage?company_id={company_id}"),
        "Loading where this agent is used…",
        "Where this agent is used could not be loaded.",
    )
}
