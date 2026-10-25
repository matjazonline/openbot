//! A passed validation: what publishing this revision would freeze — the graph, every step,
//! the resource slots, the schemas, the agents' frozen capabilities and tools — and the form
//! that publishes exactly this revision with exactly these selections.
use super::*;
use crate::application::workflow::authoring::workflow_graph;
use crate::application::workflow::publication::{PublishedBundle, ToolKind};
use crate::application::workflow::registry::ModelStep;
use crate::domain::workflow::{Binding, Routes, TransitionTarget};
use std::collections::BTreeMap;

pub struct PreviewView<'a> {
    pub company_id: Uuid,
    pub workflow: Uuid,
    /// The revision that validated, which the publish form sends.
    pub revision: u64,
    pub bundle: &'a PublishedBundle,
    pub selections: &'a WorkflowSelections,
    /// Company agent ids to names, for the capability list.
    pub agent_names: &'a BTreeMap<Uuid, String>,
    /// The editor's connection choices, to name the selected ones.
    pub options: &'a crate::application::workflow::reads::PublicationOptions,
}

fn routes_text(routes: &Routes) -> String {
    let target = |target: &TransitionTarget| match target {
        TransitionTarget::Step(step) => step.as_str().to_owned(),
        TransitionTarget::End => "end".to_owned(),
    };
    match routes {
        Routes::Success(next) => format!("success → {}", target(next)),
        Routes::Choices(choices) => choices
            .iter()
            .map(|(choice, next)| format!("{} → {}", choice.as_str(), target(next)))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

fn schema_section(label: &str, schema: Option<&serde_json::Value>) -> String {
    match schema {
        None => format!(
            r#"<details class="text-sm"><summary class="cursor-pointer">{label}: not declared</summary></details>"#
        ),
        Some(schema) => format!(
            r#"<details class="text-sm"><summary class="cursor-pointer">{label}</summary>{}</details>"#,
            json_block(schema)
        ),
    }
}

/// What publication captured besides the source: the frozen agents, skills and tools and the
/// pinned children. A preview and the version published from the same revision differ in their
/// content hash (it covers the version identity) but agree here unless a dependency changed.
pub fn dependency_digest(bundle: &PublishedBundle) -> String {
    use sha2::{Digest, Sha256};
    let children: Vec<Uuid> = bundle
        .children()
        .keys()
        .map(|version| version.as_uuid())
        .collect();
    let captured = serde_json::to_vec(&(bundle.snapshots(), children)).unwrap_or_default();
    format!("{:x}", Sha256::digest(captured))
}

fn names(list: &[crate::domain::workflow::TypeName], none: &str) -> String {
    if list.is_empty() {
        none.to_owned()
    } else {
        list.iter()
            .map(|name| name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Every agent this version freezes, once (06-D5): its model and exactly its saved tools and
/// skills. A step names one of them literally or through a `/params` enum.
fn frozen_agents(view: &PreviewView<'_>) -> String {
    view.bundle
        .snapshots()
        .agents
        .iter()
        .map(|agent| {
            format!(
                r#"<li class="py-2"><span class="font-semibold">{name}</span> <span class="font-mono text-xs opacity-70">@{handle}</span> · <span class="font-mono text-xs">{provider} / {model}</span>
                    <p class="text-xs">Uses the agent's tools and skills <span class="opacity-60">(source: the agent, frozen at publication)</span></p>
                    <p class="font-mono text-xs">tools: {tools}</p><p class="font-mono text-xs">skills: {skills}</p></li>"#,
                name = escape_html_text(
                    view.agent_names
                        .get(&agent.agent_id)
                        .map(String::as_str)
                        .unwrap_or(agent.key.as_str())
                ),
                handle = escape_html_text(agent.key.as_str()),
                provider = escape_html_text(agent.model.provider.as_str()),
                model = escape_html_text(&agent.model.model),
                tools = escape_html_text(&names(&agent.tools, "no tools")),
                skills = escape_html_text(&names(&agent.skills, "no skills")),
            )
        })
        .collect()
}

/// Which frozen agent each model step runs, and whether its kind uses the agent's tools.
fn model_steps(view: &PreviewView<'_>) -> String {
    let definition = view.bundle.compiled().graph().definition();
    let frozen = &view.bundle.snapshots().agents;
    definition
        .steps
        .iter()
        .filter_map(|(id, step)| {
            let model = ModelStep::of(step.step_type.as_str())?;
            let runs = if model.uses_capabilities() {
                "with the agent's tools and skills listed above"
            } else {
                "with the agent's model and instructions only: this step kind runs no tools or skills"
            };
            let agent = match step.inputs.get("agent") {
                Some(Binding::Literal(serde_json::Value::String(key))) => {
                    if frozen.iter().any(|agent| agent.key.as_str() == key) {
                        format!("agent <span class=\"font-mono\">@{}</span>", escape_html_text(key))
                    } else {
                        format!(
                            "<span class=\"text-error\">agent @{} is not among the frozen agents</span>",
                            escape_html_text(key)
                        )
                    }
                }
                Some(Binding::Reference(reference)) => format!(
                    "the agent named by <span class=\"font-mono\">{}</span> at run time, one of {}",
                    escape_html_text(reference.as_str()),
                    escape_html_text(&parameter_choices(definition, reference)),
                ),
                _ => "an agent chosen at run time from the frozen agents".to_owned(),
            };
            Some(format!(
                r#"<li class="py-1"><span class="font-semibold">{step}</span> <span class="font-mono text-xs opacity-70">{kind}</span> · {agent}, {runs}.</li>"#,
                step = escape_html_text(id.as_str()),
                kind = escape_html_text(step.step_type.as_str()),
            ))
        })
        .collect()
}

/// The handles a `/params/<name>` agent selector may take: that parameter's `enum`, the same
/// finite set publication froze for it (`agent_universe`), not every frozen agent (F016).
fn parameter_choices(
    definition: &crate::domain::workflow::WorkflowDefinition,
    reference: &crate::domain::workflow::ContextReference,
) -> String {
    let choices = (reference.root_name() == "params" && reference.tail().len() == 1)
        .then_some(definition.parameter_schema.as_ref())
        .flatten()
        .and_then(|schema| schema["properties"][&reference.tail()[0]]["enum"].as_array());
    match choices {
        Some(choices) => choices
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(|handle| format!("@{handle}"))
            .collect::<Vec<_>>()
            .join(", "),
        None => "the frozen agents".to_owned(),
    }
}

fn capabilities(view: &PreviewView<'_>) -> String {
    let agents = frozen_agents(view);
    let steps = model_steps(view);
    if agents.is_empty() && steps.is_empty() {
        return String::new();
    }
    format!(
        r#"<div class="space-y-1"><h4 class="font-semibold">Agents and capabilities</h4>
            <p class="text-xs opacity-70">Revoking a tool or skill later still stops it at run time; it never adds one.</p>
            <ul class="divide-y divide-base-300">{agents}</ul>
            <ul class="text-sm">{steps}</ul></div>"#
    )
}

/// The connections the publish form's selections name, as the editor labelled them.
fn selected_connections(view: &PreviewView<'_>) -> String {
    let label = |options: &[crate::application::workflow::reads::OperationOption],
                 tool: &str,
                 connection: Uuid| {
        options
            .iter()
            .find(|option| option.name == tool && option.connection == connection)
            .map(super::editor::option_label)
            .unwrap_or_else(|| format!("connection {}", short_id(connection)))
    };
    let rows: Vec<String> = view
        .selections
        .mcp
        .iter()
        .map(|(key, connection)| {
            format!(
                r#"<li>MCP tool <span class="font-mono">{tool}</span> through slot <span class="font-mono">{slot}</span> on {connection}</li>"#,
                tool = escape_html_text(key.tool.as_str()),
                slot = escape_html_text(key.slot.as_str()),
                connection = escape_html_text(&label(&view.options.mcp, key.tool.as_str(), *connection)),
            )
        })
        .chain(view.selections.http.iter().map(|(tool, connection)| {
            format!(
                r#"<li>HTTP operation <span class="font-mono">{tool}</span> on {connection}</li>"#,
                tool = escape_html_text(tool.as_str()),
                connection = escape_html_text(&label(&view.options.http, tool.as_str(), *connection)),
            )
        }))
        .collect();
    if rows.is_empty() {
        return String::new();
    }
    format!(
        r#"<div class="space-y-1"><h4 class="font-semibold">Connections this publish freezes</h4><ul class="list-disc pl-5 text-sm">{}</ul></div>"#,
        rows.join("")
    )
}

fn frozen_tools(bundle: &PublishedBundle) -> String {
    let rows: String = bundle
        .snapshots()
        .tools
        .iter()
        .map(|tool| {
            let kind = match tool.kind() {
                ToolKind::Message => "built-in message tool".to_owned(),
                ToolKind::Provisioning => "built-in provisioning tool".to_owned(),
                ToolKind::Slot(slot) => format!("through slot {}", slot.as_str()),
                ToolKind::Http => "company HTTP operation".to_owned(),
                ToolKind::Workflow(pin) => format!(
                    "workflow tool → version {}",
                    short_id(pin.version_id.as_uuid())
                ),
            };
            format!(
                r#"<li class="py-1"><details><summary class="cursor-pointer"><span class="font-mono">{name}</span> · {kind} · {effect}, recovery {recovery}</summary><p class="mt-1 text-xs font-semibold">Arguments</p>{input}<p class="text-xs font-semibold">Result</p>{output}</details></li>"#,
                name = escape_html_text(tool.contract.name.as_str()),
                kind = escape_html_text(&kind),
                effect = escape_html_text(&format!("{:?}", tool.policy.effect).to_lowercase()),
                recovery = escape_html_text(&format!("{:?}", tool.policy.recovery)),
                input = json_block(&tool.contract.input_schema),
                output = json_block(&tool.contract.output_schema),
            )
        })
        .collect();
    if rows.is_empty() {
        return String::new();
    }
    format!(
        r#"<div class="space-y-1"><h4 class="font-semibold">Frozen tools</h4><ul class="divide-y divide-base-300 text-sm">{rows}</ul></div>"#
    )
}

pub fn workflow_preview(view: &PreviewView<'_>) -> String {
    let compiled = view.bundle.compiled();
    let definition = compiled.graph().definition();
    let graph = workflow_graph(definition);
    let steps: String = definition
        .steps
        .iter()
        .map(|(id, step)| {
            format!(
                r#"<tr><td class="font-mono">{id}</td><td class="font-mono">{kind}</td><td>{routes}</td><td>{error}</td></tr>"#,
                id = escape_html_text(id.as_str()),
                kind = escape_html_text(step.step_type.as_str()),
                routes = escape_html_text(&routes_text(&step.routes)),
                error = step
                    .final_error
                    .as_ref()
                    .map(|target| escape_html_text(&routes_text(&Routes::Success(target.clone())).replace("success", "error")))
                    .unwrap_or_default(),
            )
        })
        .collect();
    let resources = if definition.resources.is_empty() {
        r#"<p class="text-sm opacity-70">No resource slots: bindings need no resources.</p>"#
            .to_owned()
    } else {
        let rows: String = definition
            .resources
            .iter()
            .map(|resource| {
                format!(
                    r#"<tr><td class="font-mono">{slot}</td><td class="font-mono">{kind}</td><td class="font-mono">{contract}</td></tr>"#,
                    slot = escape_html_text(resource.slot.as_str()),
                    kind = escape_html_text(resource.kind.as_str()),
                    contract = escape_html_text(resource.contract.as_ref().map(|c| c.as_str()).unwrap_or("-")),
                )
            })
            .collect();
        format!(
            r#"<table class="table table-xs"><thead><tr><th>Slot</th><th>Kind</th><th>Contract</th></tr></thead><tbody>{rows}</tbody></table><p class="text-xs opacity-70">Each binding selects a company resource for every slot before it can be activated.</p>"#
        )
    };
    format!(
        r##"<section id="workflow-preview" class="mt-6 space-y-5 rounded-box border border-base-300 p-4" aria-labelledby="workflow-preview-title">
            <div class="flex flex-wrap items-center justify-between gap-3">
                <div>
                    <h3 id="workflow-preview-title" class="text-lg font-semibold">Revision {revision} is valid</h3>
                    <p class="font-mono text-xs opacity-60">content {hash}</p>
                </div>
                <form hx-post="/ui/workflows/{workflow}/publish?company_id={company_id}"
                    hx-target="#workflow-pane" hx-swap="outerHTML"
                    hx-disabled-elt="find button[type='submit']">
                    <input type="hidden" name="expected_revision" value="{revision}">
                    <input type="hidden" name="command_key" value="{key}">
                    <input type="hidden" name="previewed_dependencies" value="{dependencies}">
                    {selections}
                    <button type="submit" class="btn btn-primary">
                        <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                        <span class="[.htmx-request_&]:hidden">Publish revision {revision}</span>
                        <span class="hidden [.htmx-request_&]:inline">Publishing…</span>
                    </button>
                </form>
            </div>
            <p class="text-xs opacity-70">Publishing creates an immutable version. It does not activate it or move any binding: bindings keep their versions until you reconfigure them.</p>
            <div class="space-y-2"><h4 class="font-semibold">Graph</h4>{graph}</div>
            <div class="space-y-2"><h4 class="font-semibold">Steps</h4>
                <div class="overflow-x-auto"><table class="table table-xs"><thead><tr><th>Step</th><th>Type</th><th>Routes</th><th>After retries</th></tr></thead><tbody>{steps}</tbody></table></div></div>
            <div class="space-y-2"><h4 class="font-semibold">Resource slots</h4>{resources}</div>
            <div class="space-y-1"><h4 class="font-semibold">Schemas</h4>{input}{params}{output}</div>
            {capabilities}
            {connections}
            {tools}
        </section>"##,
        company_id = view.company_id,
        workflow = view.workflow,
        revision = view.revision,
        hash = escape_html_text(view.bundle.content_hash().as_str()),
        key = Uuid::new_v4(),
        dependencies = dependency_digest(view.bundle),
        selections = view.selections.hidden_fields(),
        graph = workflow_graph_view(&graph, &BTreeMap::new()),
        input = schema_section("Input", definition.input_schema.as_ref()),
        params = schema_section("Binding parameters", definition.parameter_schema.as_ref()),
        output = schema_section("Output", definition.output_schema.as_ref()),
        capabilities = capabilities(view),
        connections = selected_connections(view),
        tools = frozen_tools(view.bundle),
    )
}
