//! Runtime selection and shared output settings. No script rewrites submitted configuration.
use super::{AgentDraft, escape_html_attr, escape_html_text};
use crate::entities::harness::HarnessKind;

pub(super) fn fields(draft: &AgentDraft<'_>) -> String {
    let selected = draft
        .harness_kind_raw
        .unwrap_or_else(|| draft.harness_kind.map_or("", HarnessKind::as_str));
    let mut options = HarnessKind::ALL
        .into_iter()
        .map(|kind| {
            format!(
                "<option value=\"{}\"{}>{}</option>",
                kind.as_str(),
                if selected == kind.as_str() {
                    " selected"
                } else {
                    ""
                },
                kind.label()
            )
        })
        .collect::<String>();
    if !selected.is_empty() && HarnessKind::parse(selected).is_none() {
        options.push_str(&format!(
            "<option selected value=\"{}\">Unavailable harness: {}</option>",
            escape_html_attr(selected),
            escape_html_text(selected)
        ));
    }
    format!(
        r#"<section class="space-y-4" data-agent-runtime>
        <label class="form-control"><span class="label">Runtime harness</span>
        <select name="harness_kind" class="select w-full" data-action="agent-runtime">
        <option value=""{default}>Deployment default (preserve on edit)</option>{options}</select></label>
        <p class="text-xs opacity-70">Switching requires explicit target configuration. Review attached tools, skills, MCP servers and the response contract for compatibility. Company model credentials are reused.</p>
        <label class="form-control"><span class="label">Advanced harness configuration</span>
        <textarea name="config_json" rows="4" class="textarea w-full font-mono text-xs">{config}</textarea></label>
        <div data-harness-help="rig"{rig_hidden}><p>Rig V1 accepts version and max_turns (1–16), subject to run budgets.</p><code>{{"version":1,"max_turns":8}}</code></div>
        <div data-harness-help="ai_agents"{ai_hidden}><p>ai-agents V1 accepts bounded reasoning, reflection and disambiguation settings.</p><code>{{"version":1,"reasoning":{{"mode":"react","max_iterations":8}},"reflection":{{"enabled":"auto","max_retries":2}},"disambiguation":{{"enabled":true}}}}</code></div>
        <noscript><p>Rig accepts version and max_turns (1–16). ai-agents accepts version, reasoning, reflection and disambiguation. Enter the target settings before saving a harness switch.</p></noscript>
        <p class="text-xs opacity-60">Models, credentials, tool grants, skills, approvals and response contracts are configured separately; they are rejected in advanced JSON.</p>
        <fieldset class="space-y-2"><legend>Response format</legend>
        <select name="response_format" class="select w-full" aria-label="Response format">
            <option value="text"{text}>Ordinary text</option><option value="json_schema"{json}>JSON Schema</option>
        </select>
        <p>With JSON Schema, recipients receive validated JSON. Rig permits at most two repair calls, subject to run budgets. ai-agents does not support response contracts; select ordinary text explicitly to clear one.</p>
        <label class="form-control"><span class="label">Response JSON Schema</span>
        <textarea name="response_schema" rows="6" class="textarea w-full font-mono text-xs" placeholder='{{"type":"object"}}'>{schema}</textarea></label>
        </fieldset>
    </section>"#,
        default = if selected.is_empty() { " selected" } else { "" },
        config = escape_html_text(draft.config_json),
        rig_hidden = if selected == "ai_agents" {
            " hidden"
        } else {
            ""
        },
        ai_hidden = if selected == "rig" { " hidden" } else { "" },
        text = if draft.response_format == "text" {
            " selected"
        } else {
            ""
        },
        json = if draft.response_format == "json_schema" {
            " selected"
        } else {
            ""
        },
        schema = escape_html_text(&draft.response_schema),
    )
}

/// Project reviewed runtime settings only; historical payloads have no implied new harness.
pub(super) fn execution_summary(payload: &serde_json::Value) -> String {
    let Some(params) = payload.get("execution_parameters") else {
        return String::new();
    };
    let Some(kind) = params
        .get("harness_kind")
        .and_then(|v| v.as_str())
        .and_then(HarnessKind::parse)
    else {
        return "<p class=\"text-xs opacity-60\">Historical execution: harness was not recorded.</p>".into();
    };
    let config = crate::entities::harness::HarnessConfig::parse(kind, params.get("config"))
        .and_then(|config| config.to_json())
        .map(|value| value.to_string())
        .unwrap_or_else(|_| "Reviewed settings unavailable".into());
    format!(
        "<section><h4>Runtime: {}</h4><p>Response format: {}</p><pre>{}</pre></section>",
        kind.label(),
        if params
            .get("response_contract")
            .is_some_and(|value| !value.is_null())
        {
            "JSON Schema (validated JSON; at most two repairs within run budgets)"
        } else {
            "Ordinary text"
        },
        escape_html_text(&config)
    )
}

#[cfg(test)]
#[path = "agent_runtime_settings_tests.rs"]
mod tests;
