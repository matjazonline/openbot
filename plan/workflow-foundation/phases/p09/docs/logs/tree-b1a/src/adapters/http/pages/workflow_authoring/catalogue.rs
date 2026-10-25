//! The editor's reference panel: every step type this deployment runs with its contract and a
//! compiling example, and the company agents a step can name. Loaded the first time it opens.
use super::*;
use crate::application::workflow::authoring::CatalogueEntry;

/// The collapsed panel under the editor; its body loads on first open.
pub(super) fn catalogue_disclosure(company_id: Uuid) -> String {
    format!(
        r##"<details id="workflow-reference" class="mt-6 rounded-box border border-base-300 p-3"
            hx-get="/ui/workflows/catalogue?company_id={company_id}" hx-trigger="toggle once"
            hx-target="#workflow-reference-body" hx-swap="innerHTML">
            <summary class="cursor-pointer font-semibold">Step catalogue and agents reference</summary>
            <div id="workflow-reference-body" class="mt-3 text-sm" aria-busy="true">
                <span class="loading loading-spinner loading-sm"></span> Loading the reference…
            </div>
        </details>"##
    )
}

/// An agent a step's `agent` input may name: its slug is the key publication resolves.
pub struct AgentReference<'a> {
    pub slug: &'a str,
    pub name: &'a str,
    /// `provider / model`, or `None` when the agent has no explicit model, which workflows need.
    pub model: Option<String>,
}

pub fn workflow_catalogue(entries: &[CatalogueEntry], agents: &[AgentReference<'_>]) -> String {
    let types: String = entries
        .iter()
        .map(|entry| {
            let routes = if entry.declared_choices {
                format!(
                    "choices you declare (the example declares {})",
                    entry.routes.join(", ")
                )
            } else {
                entry.routes.join(", ")
            };
            let control = if entry.control.is_empty() {
                String::new()
            } else {
                format!(" · runs a {}", entry.control)
            };
            format!(
                r#"<li class="py-1"><details><summary class="cursor-pointer"><span class="font-mono font-semibold">{name}</span> <span class="text-xs opacity-70">routes: {routes}{control}</span></summary>
                    <div class="mt-2 space-y-2 pl-3">
                        <p>{notes}</p>
                        <p class="text-xs font-semibold">Inputs (<span class="font-mono">with</span>)</p>{input}
                        <p class="text-xs font-semibold">Output</p>{output}
                        <p class="text-xs font-semibold">Example</p><pre class="max-h-80 overflow-auto rounded-box bg-base-200 p-3 font-mono text-xs">{example}</pre>
                    </div></details></li>"#,
                name = escape_html_text(entry.type_name),
                routes = escape_html_text(&routes),
                control = escape_html_text(&control),
                notes = escape_html_text(entry.notes),
                input = json_block(&entry.input_schema),
                output = json_block(&entry.output_schema),
                example = escape_html_text(&entry.example),
            )
        })
        .collect();
    let agents = if agents.is_empty() {
        r#"<p class="text-xs opacity-70">This company has no agents yet; create one on the Agents page before naming it in an agent step.</p>"#.to_owned()
    } else {
        let rows: String = agents
            .iter()
            .map(|agent| {
                format!(
                    r#"<tr><td>{name}</td><td class="font-mono text-[11px] break-all">{slug}</td><td class="font-mono text-[11px]">{model}</td></tr>"#,
                    name = escape_html_text(agent.name),
                    slug = escape_html_text(agent.slug),
                    model = match &agent.model {
                        Some(model) => escape_html_text(model),
                        None => r#"<span class="text-error">no explicit model: workflows refuse this agent</span>"#.to_owned(),
                    },
                )
            })
            .collect();
        format!(
            r#"<p class="text-xs opacity-70">Name an agent by its handle in a step's <span class="font-mono">agent</span> input: <span class="font-mono">agent: {{literal: &lt;handle&gt;}}</span>.</p><div class="overflow-x-auto"><table class="table table-xs"><thead><tr><th>Agent</th><th>Handle</th><th>Model</th></tr></thead><tbody>{rows}</tbody></table></div>"#
        )
    };
    format!(
        r#"<div class="space-y-4">
            <section><h4 class="font-semibold">Step types</h4><ul class="divide-y divide-base-300">{types}</ul></section>
            <section class="space-y-1"><h4 class="font-semibold">Agents</h4>{agents}</section>
        </div>"#
    )
}
