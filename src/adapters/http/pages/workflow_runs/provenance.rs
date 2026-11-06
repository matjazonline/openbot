use super::*;
use crate::application::workflow::run_views::{
    CallProvenancePage, CallRecordKind, RunView, SelectedAgentSummary,
};

pub(super) fn dependencies(run: &RunView) -> String {
    if run.payload_restricted {
        return String::new();
    }
    let Some(selected) = &run.dependencies else {
        return String::new();
    };
    let agents = selected
        .agents
        .iter()
        .map(agent_revision)
        .collect::<String>();
    let workflows = selected
        .workflows
        .iter()
        .map(|workflow| {
            format!(
                "<li class=\"break-all text-xs\">Workflow {} · version {}</li>",
                workflow.workflow_id.as_uuid(),
                workflow.version_id.as_uuid()
            )
        })
        .collect::<String>();
    format!(
        r#"<section class="space-y-2"><h3 class="font-semibold">Captured dependencies</h3>
        <p class="break-all text-xs">Manifest {} · root run {}</p>
        <p class="text-xs opacity-70">Selected at root admission and retained by retries and children. Published definitions describe available capabilities; model-only steps use no tools or skills.</p>
        <details><summary class="cursor-pointer">Agent revisions and workflow versions</summary>{agents}<ul>{workflows}</ul>{cut}</details></section>{calls}"#,
        selected.manifest.as_uuid(),
        selected.root.as_uuid(),
        cut = truncation(
            selected.agents_truncated || selected.workflows_truncated,
            "Dependency lists show at most 50 agents and 50 workflow versions; additional selections are omitted."
        ),
        calls = call_provenance(&run.calls),
    )
}

pub(super) fn agent_revision(agent: &SelectedAgentSummary) -> String {
    let bindings = agent
        .bindings
        .iter()
        .map(|tool| {
            format!(
                "<li class=\"break-all text-xs\">{} · binding {} · {}</li>",
                escape_html_text(tool.alias.as_str()),
                tool.binding.as_uuid(),
                escape_html_text(&tool.target.target_label())
            )
        })
        .collect::<String>();
    let assignments = agent
        .assignments
        .iter()
        .map(|skill| {
            format!(
                "<li class=\"break-all text-xs\">{} · skill {} · assignment {}</li>",
                escape_html_text(&skill.name),
                skill.skill,
                skill.assignment.as_uuid()
            )
        })
        .collect::<String>();
    let list = |values: &[String]| {
        if values.is_empty() {
            "none".to_owned()
        } else {
            values
                .iter()
                .map(|value| escape_html_text(value))
                .collect::<Vec<_>>()
                .join(", ")
        }
    };
    format!(
        r#"<div class="space-y-1 py-2"><p class="text-sm">@{} · {}</p>
        <p class="break-all text-xs">Agent {} · revision {} · manifest {}</p>
        <p class="text-xs">tools {}; skills {}</p><ul>{bindings}{assignments}</ul></div>"#,
        escape_html_text(&agent.key),
        escape_html_text(&agent.model),
        agent.agent_id.as_uuid(),
        agent.revision_id.as_uuid(),
        agent.manifest.as_uuid(),
        list(&agent.tools),
        list(&agent.skills)
    )
}

pub(super) fn call_provenance(calls: &CallProvenancePage) -> String {
    if calls.items.is_empty() && !calls.truncated {
        return String::new();
    }
    let rows = calls
        .items
        .iter()
        .map(|call| {
            let kind = match call.kind {
                CallRecordKind::ActionInvocation => "Action invocation",
                CallRecordKind::ChildToolCall => "Child tool call",
            };
            format!(
                r#"<li class="break-all text-xs space-y-1"><p>{kind} {} · {}</p>
            <p>Agent {} · revision {} · manifest {}</p><p>Binding {} · {}</p></li>"#,
                call.record,
                escape_html_text(call.tool.alias.as_str()),
                call.origin.agent.as_uuid(),
                call.origin.revision.as_uuid(),
                call.origin.manifest.as_uuid(),
                call.origin.binding.as_uuid(),
                escape_html_text(&call.tool.target.target_label())
            )
        })
        .collect::<String>();
    format!(
        r#"<section class="space-y-2"><h3 class="font-semibold">Agent tool-call provenance</h3>
        <p class="text-xs opacity-70">Immutable origins for the action invocations and child tool calls listed in this view.</p>
        <ul class="space-y-2">{rows}</ul>{}</section>"#,
        truncation(
            calls.truncated,
            "Showing at most 50 call origins; additional listed calls are omitted from this projection."
        )
    )
}

fn truncation(truncated: bool, message: &str) -> String {
    if truncated {
        format!("<p class=\"text-xs\">{}</p>", escape_html_text(message))
    } else {
        String::new()
    }
}
