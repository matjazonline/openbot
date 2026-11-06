use super::*;
use crate::application::agent::*;
use crate::application::workflow::compiler::AgentReferenceStep;
use crate::domain::workflow::StepId;

pub struct WorkflowAgentPicker<'a> {
    pub company: Uuid,
    pub workflow: Uuid,
    pub steps: &'a [AgentReferenceStep],
    pub selected_step: Option<&'a AgentReferenceStep>,
    pub next_step: Option<&'a StepId>,
    pub agents: &'a AgentDirectoryPage,
    pub selected_agent: Option<&'a AgentAuthoringState>,
}

pub(super) fn workflow_agent_picker_loader(company: Uuid, workflow: Uuid) -> String {
    format!(
        r##"<section id="workflow-agent-picker" data-workflow-agent-picker class="rounded-box border border-base-300 p-4" hx-post="/ui/workflows/{workflow}/agent-picker?company_id={company}" hx-trigger="load" hx-include="#workflow-source" hx-params="source" hx-target="this" hx-swap="outerHTML" hx-sync="this:replace">Loading agent choices…</section>"##
    )
}

pub fn workflow_agent_picker_error(company: Uuid, workflow: Uuid, message: &str) -> String {
    format!(
        r##"<section id="workflow-agent-picker" data-workflow-agent-picker class="rounded-box border border-base-300 p-4"><h3 class="font-semibold">Choose an agent for a model step</h3>{error}<button type="button" class="btn btn-ghost btn-sm" hx-post="/ui/workflows/{workflow}/agent-picker?company_id={company}" hx-include="#workflow-source" hx-params="source" hx-target="#workflow-agent-picker" hx-swap="outerHTML" hx-sync="#workflow-agent-picker:replace">Refresh from current source</button></section>"##,
        error = error_alert(message)
    )
}

pub fn workflow_agent_picker(view: &WorkflowAgentPicker<'_>) -> String {
    let company = view.company;
    let workflow = view.workflow;
    let steps = step_options(view);
    let agents = agent_options(view);
    let more_steps = view
        .next_step
        .map(|after| {
            more_choices(
                company,
                workflow,
                "after_step",
                after.as_str(),
                "More model steps",
            )
        })
        .unwrap_or_default();
    let more_agents = view
        .agents
        .next
        .as_ref()
        .map(|after| {
            more_choices(
                company,
                workflow,
                "after_agent",
                after.as_str(),
                "More agents",
            )
        })
        .unwrap_or_default();
    let disabled = if view.selected_step.is_none() || view.selected_agent.is_none() {
        " disabled"
    } else {
        ""
    };
    format!(
        r##"<section id="workflow-agent-picker" data-workflow-agent-picker class="space-y-3 rounded-box border border-base-300 p-4">
        <h3 class="font-semibold">Choose an agent for a model step</h3>
        <p class="text-xs opacity-70">This edits the agent reference in your current source. Save and validate the workflow afterward. No connection slots or duplicated agent configuration are added.</p>
        <form hx-post="/ui/workflows/{workflow}/agent-picker?company_id={company}" hx-include="#workflow-source" hx-target="#workflow-agent-picker" hx-swap="outerHTML" hx-sync="#workflow-agent-picker:replace" class="space-y-3">
        <label class="form-control">Model step<select class="select" name="step" hx-post="/ui/workflows/{workflow}/agent-picker?company_id={company}" hx-params="source,step"><option value="" disabled{no_step}>Select a model step</option>{steps}</select></label>{more_steps}
        <label class="form-control">Company agent<select class="select" name="agent" hx-post="/ui/workflows/{workflow}/agent-picker?company_id={company}"><option value="" disabled{no_agent}>Select an agent</option>{agents}</select></label>{more_agents}
        <p class="text-xs opacity-60">Up to 50 model steps and 50 agents per page. The current selection remains available outside its page.</p>
        <div class="flex flex-wrap gap-2"><button type="submit" class="btn btn-ghost btn-sm">Refresh choices</button><button type="button" class="btn btn-primary btn-sm" hx-post="/ui/workflows/{workflow}/agent-reference?company_id={company}" hx-target="#workflow-agent-reference-result" hx-swap="innerHTML"{disabled}>Use selected agent in source</button></div></form>
        <div id="workflow-agent-reference-result" role="status" aria-live="polite"></div>{capabilities}</section>"##,
        no_step = if view.selected_step.is_none() {
            " selected"
        } else {
            ""
        },
        no_agent = if view.selected_agent.is_none() {
            " selected"
        } else {
            ""
        },
        capabilities = current_capabilities(view)
    )
}

fn step_options(view: &WorkflowAgentPicker<'_>) -> String {
    view.steps
        .iter()
        .map(|step| {
            format!(
                r#"<option value="{id}"{selected}>{id} · {kind}</option>"#,
                id = escape_html_attr(step.id.as_str()),
                kind = step.model_step.kind(),
                selected = if view
                    .selected_step
                    .is_some_and(|selected| selected.id == step.id)
                {
                    " selected"
                } else {
                    ""
                }
            )
        })
        .collect::<String>()
}

fn agent_options(view: &WorkflowAgentPicker<'_>) -> String {
    view.agents
        .items
        .iter()
        .map(|agent| {
            format!(
                r#"<option value="{id}"{selected}>{name} · @{key} · {status}</option>"#,
                id = agent.id.as_uuid(),
                name = escape_html_text(&agent.name),
                key = escape_html_text(agent.key.as_str()),
                status = if agent.active_revision.is_some() {
                    "published"
                } else {
                    "inactive"
                },
                selected = if view
                    .selected_agent
                    .is_some_and(|selected| selected.agent.id == agent.id.as_uuid())
                {
                    " selected"
                } else {
                    ""
                }
            )
        })
        .collect::<String>()
}

fn more_choices(company: Uuid, workflow: Uuid, field: &str, after: &str, label: &str) -> String {
    let values = escape_html_attr(&serde_json::json!({(field):after}).to_string());
    format!(
        r##"<button type="button" class="btn btn-ghost btn-xs" hx-post="/ui/workflows/{workflow}/agent-picker?company_id={company}" hx-vals="{values}">{label} · choices continue</button>"##
    )
}

fn current_capabilities(view: &WorkflowAgentPicker<'_>) -> String {
    let Some(state) = view.selected_agent else {
        return "<p class=\"text-sm opacity-70\">Select an agent to see its current publication.</p>".into();
    };
    let company = view.company;
    let agent = state.agent.id;
    let active = state
        .active
        .as_ref()
        .map(|active| {
            format!(
                "{} · {} / {}",
                active.id.as_uuid(),
                active.model.provider.as_str(),
                active.model.model
            )
        })
        .unwrap_or_else(|| {
            "Inactive. Publish this agent before publishing a workflow that uses it.".into()
        });
    let capabilities = if view
        .selected_step
        .is_some_and(|step| !step.model_step.uses_capabilities())
    {
        "<p class=\"text-sm\">This model-only step uses model settings. Tools and skills are unavailable.</p>".into()
    } else {
        active_capabilities(state)
    };
    format!(
        r#"<div class="space-y-2 border-t border-base-300 pt-3"><h4 class="font-semibold">Current agent information</h4><p class="break-all text-xs">Agent {agent} · active {active}</p><p class="text-xs opacity-70">New runs choose the agent's active revision at admission. Running work and retries retain their selections. These current capabilities do not guarantee what a future run will use, or its access.</p>{capabilities}<a class="link text-sm" href="/ui/agents?company_id={company}&amp;agent_id={agent}">Configure and publish this agent</a></div>"#,
        active = escape_html_text(&active)
    )
}

fn active_capabilities(state: &AgentAuthoringState) -> String {
    let Some(active) = &state.active else {
        return String::new();
    };
    let tools = active
        .tools
        .iter()
        .map(|tool| {
            format!(
                "<li>{} → {} · {} · {}</li>",
                escape_html_text(tool.assignment.alias.as_str()),
                escape_html_text(&tool.assignment.choice.target_label()),
                super::super::agent_authoring::access_label(tool.access),
                super::super::agent_authoring::availability_label(tool.availability)
            )
        })
        .collect::<String>();
    let skills = active
        .skills
        .iter()
        .map(|skill| {
            format!(
                "<li>{} · {}</li>",
                escape_html_text(skill.name.as_str()),
                super::super::agent_authoring::access_label(skill.access)
            )
        })
        .collect::<String>();
    format!(
        "<div class=\"text-xs\"><p>Published tools</p><ul>{tools}</ul><p>Published skills</p><ul>{skills}</ul></div>"
    )
}

pub fn workflow_agent_reference_applied(source: &str) -> String {
    format!(
        r#"<p>Agent reference applied to the editor. Save and validate the workflow when ready.</p><span hidden data-workflow-agent-source="{}"></span>"#,
        escape_html_attr(source)
    )
}

pub const WORKFLOW_AGENT_PICKER_SCRIPT: &str = r#"
var workflowAgentRequests = new WeakMap();
document.addEventListener('htmx:beforeRequest', function (event) {
    var element = event.detail.elt;
    if (!element.closest('[data-workflow-agent-picker]')) return;
    var source = document.getElementById('workflow-source');
    if (!source) return;
    var context = { field: source, value: source.value };
    var request = event.detail.xhr;
    workflowAgentRequests.set(request, context);
    element.addEventListener('htmx:beforeOnLoad', function (answer) {
        if (answer.detail.xhr !== request) return;
        if (!context.field.isConnected || document.getElementById('workflow-source') !== context.field || context.field.value !== context.value) {
            answer.preventDefault();
            var status = document.getElementById('workflow-agent-reference-result');
            if (status && context.field.isConnected) status.textContent = 'The source changed. Refresh the agent choices before applying a reference.';
        }
    }, { once: true });
}, true);
document.addEventListener('htmx:afterSwap', function (event) {
    var context = workflowAgentRequests.get(event.detail.xhr);
    var target = event.detail.target;
    var marker = target && target.querySelector('[data-workflow-agent-source]');
    if (!context || !marker || !context.field.isConnected || context.field.value !== context.value) return;
    context.field.value = marker.dataset.workflowAgentSource;
    context.field.dispatchEvent(new Event('input', { bubbles: true }));
    context.field.focus();
    marker.remove();
});
"#;
