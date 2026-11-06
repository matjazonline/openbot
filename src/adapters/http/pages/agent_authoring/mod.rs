//! Escaped, credential-free authoring panels. Saved publication actions are separate from draft input.
use super::*;
use crate::application::agent::*;

mod catalog;
mod publication;
mod script;

pub use catalog::agent_tool_catalog;
pub use publication::{agent_publication_panel, agent_revision_history};
pub use script::AGENT_AUTHORING_SCRIPT;

pub fn agent_tools_editor(
    company: Uuid,
    state: &AgentAuthoringState,
    submitted: Option<&str>,
) -> String {
    let agent = state.agent.id;
    let stored = state
        .tools
        .iter()
        .map(|tool| tool.assignment.clone())
        .collect::<Vec<_>>();
    // Serialization of validated metadata cannot fail. Retain an invalid submitted payload verbatim
    // so a refused save never turns into an unintended empty assignment on a second submit.
    let serialized = serde_json::to_string(&stored).expect("agent assignment metadata serializes");
    let raw = submitted.unwrap_or(&serialized);
    let parsed = (raw.len() <= 65_536)
        .then(|| serde_json::from_str::<Vec<AgentToolAssignment>>(raw).ok())
        .flatten()
        .filter(|rows| rows.len() <= 32);
    let rows = match parsed {
        Some(assignments) => assignments
            .iter()
            .enumerate()
            .map(|(index, assignment)| tool_row(index, assignment))
            .collect(),
        None => format!(
            r#"{}<details><summary>Recover submitted tool choices</summary><textarea readonly class="textarea w-full font-mono">{}</textarea></details>"#,
            error_alert(
                "The submitted tool selection could not be read. Reload the draft to restore its saved tools."
            ),
            escape_html_text(raw)
        ),
    };
    format!(
        r##"<section class="space-y-3 rounded-box border border-base-300 p-4" data-agent-tools-editor>
        <h3 class="font-semibold">Tools for this agent</h3>
        <p class="text-xs opacity-70">Aliases are the names the agent sees. Each tool has an explicit target. Selecting a tool does not grant a connection or approve its operation. Builder provisioning tools can only be kept or removed.</p>
        <input type="hidden" name="tool_assignments" value="{raw}">
        <div data-agent-tool-rows>{rows}</div><p data-agent-tool-error role="alert"></p>
        <div id="agent-tool-catalog-{agent}" hx-get="/ui/agents/{agent}/tool-catalog?company_id={company}" hx-trigger="load" hx-params="none" hx-target="this" hx-swap="outerHTML" hx-sync="this:replace"><span class="loading loading-spinner loading-sm"></span> Loading tool catalog</div>
        </section>"##,
        raw = escape_html_attr(raw)
    )
}

fn tool_row(index: usize, assignment: &AgentToolAssignment) -> String {
    format!(
        r##"<div class="my-2 rounded border border-base-300 p-3" data-agent-tool-row data-assignment="{assignment}">
        <label class="form-control">Agent-visible alias<input class="input input-sm" data-agent-tool-alias value="{alias}" maxlength="64" required aria-label="Tool {number} alias"></label>
        <p class="break-all text-xs opacity-70" data-agent-tool-target>{target}</p>
        <p class="break-all font-mono text-xs">Binding: {binding}</p>
        <button type="button" class="btn btn-ghost btn-xs" data-agent-tool-remove>Remove from draft</button></div>"##,
        assignment = escape_html_attr(
            &serde_json::to_string(assignment).expect("assignment metadata serializes")
        ),
        alias = escape_html_attr(assignment.alias.as_str()),
        number = index + 1,
        target = escape_html_text(&assignment.choice.target_label()),
        binding = assignment
            .binding
            .map(|id| id.as_uuid().to_string())
            .unwrap_or_else(|| "assigned on save".into())
    )
}

pub(super) fn availability_label(availability: AgentToolAvailability) -> &'static str {
    match availability {
        AgentToolAvailability::Configured => "Configured; publication validates current policy",
        AgentToolAvailability::ConnectionUnavailable => {
            "Connection unavailable: enable or repair it"
        }
        AgentToolAvailability::GrantRequired => "Connection permission required",
        AgentToolAvailability::ReviewChanged => {
            "Reviewed contract changed: review and select it again"
        }
        AgentToolAvailability::OperationUnavailable => {
            "Operation unavailable: configure or review it"
        }
        AgentToolAvailability::HistoricalPin => {
            "Retained workflow version; registration currently differs"
        }
    }
}

pub(super) fn access_label(access: AgentGrantState) -> &'static str {
    match access {
        AgentGrantState::Unpublished => "Not published",
        AgentGrantState::Live => "Live access",
        AgentGrantState::Revoked => "Access revoked",
    }
}

fn kind_name(kind: AgentToolKind) -> &'static str {
    match kind {
        AgentToolKind::Builtin => "builtin",
        AgentToolKind::Http => "http",
        AgentToolKind::Mcp => "mcp",
        AgentToolKind::Workflow => "workflow",
    }
}
