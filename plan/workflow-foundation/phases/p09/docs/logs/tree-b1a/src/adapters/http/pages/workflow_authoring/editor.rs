//! The editor tab: title, description and YAML source saved against the revision the author
//! opened, the publication selections the draft asks for, and what the last save or validation
//! said. A refused save keeps every byte the author typed.
use super::*;
use crate::application::workflow::authoring::{PublicationNeeds, line_diff};
use crate::application::workflow::compiler::Diagnostic;
use crate::application::workflow::reads::{OperationOption, PublicationOptions};
use std::collections::BTreeMap;

/// The connection chosen for each publication need, as the editor's selects send them:
/// `mcp:<slot>:<tool>` and `http:<tool>` fields naming a connection id (empty: none).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkflowSelections {
    pub mcp: BTreeMap<(String, String), Uuid>,
    pub http: BTreeMap<String, Uuid>,
}

impl WorkflowSelections {
    /// The selections among a form's fields; anything else is ignored, and a field that names
    /// no connection selects none.
    pub fn from_fields<'a>(fields: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut selections = Self::default();
        for (name, value) in fields {
            let Ok(connection) = value.trim().parse::<Uuid>() else {
                continue;
            };
            if let Some(rest) = name.strip_prefix("mcp:") {
                if let Some((slot, tool)) = rest.split_once(':') {
                    selections
                        .mcp
                        .insert((slot.to_owned(), tool.to_owned()), connection);
                }
            } else if let Some(tool) = name.strip_prefix("http:") {
                selections.http.insert(tool.to_owned(), connection);
            }
        }
        selections
    }

    /// The same selections as hidden fields, so a publish sends exactly what was validated.
    pub fn hidden_fields(&self) -> String {
        let mcp = self.mcp.iter().map(|((slot, tool), connection)| {
            format!(
                r#"<input type="hidden" name="mcp:{}:{}" value="{connection}">"#,
                escape_html_attr(slot),
                escape_html_attr(tool)
            )
        });
        let http = self.http.iter().map(|(tool, connection)| {
            format!(
                r#"<input type="hidden" name="http:{}" value="{connection}">"#,
                escape_html_attr(tool)
            )
        });
        mcp.chain(http).collect()
    }
}

/// What the editor says above the form.
pub enum EditorBanner<'a> {
    Saved(&'a str),
    /// Someone saved `current` after the author opened the draft. The form keeps the author's
    /// text and now sends `current`, so the next save deliberately replaces it.
    Conflict {
        current: u64,
        theirs: &'a str,
    },
    Refused(&'a str),
}

/// The draft as the form shows it: stored, or as last typed when a save was refused.
pub struct EditorDraft<'a> {
    pub title: &'a str,
    pub description: &'a str,
    pub source: &'a str,
    /// The revision the form sends as `expected_revision`.
    pub expected_revision: u64,
}

pub struct EditorView<'a> {
    pub company_id: Uuid,
    pub workflow: Uuid,
    pub archived: bool,
    pub draft: EditorDraft<'a>,
    pub banner: Option<EditorBanner<'a>>,
    /// The last validation's refusal, located in the saved source.
    pub diagnostic: Option<&'a Diagnostic>,
    /// The last validation's success, pre-rendered.
    pub preview_html: Option<&'a str>,
    /// What the draft asks to select, or why its steps could not be read.
    pub needs: Result<&'a PublicationNeeds, &'a Diagnostic>,
    pub options: &'a PublicationOptions,
    pub selections: &'a WorkflowSelections,
}

pub fn workflow_editor(view: &EditorView<'_>) -> String {
    let (company_id, workflow) = (view.company_id, view.workflow);
    let banner = match &view.banner {
        None => String::new(),
        Some(EditorBanner::Saved(message)) => status_banner(message),
        Some(EditorBanner::Refused(message)) => form_error_banner(Some(message)),
        Some(EditorBanner::Conflict { current, theirs }) => {
            conflict_banner(*current, theirs, view.draft.source)
        }
    };
    let diagnostic = view
        .diagnostic
        .map(|diagnostic| diagnostic_panel(diagnostic, view.draft.source))
        .unwrap_or_default();
    let disabled = if view.archived { " disabled" } else { "" };
    let archived_note = if view.archived {
        r#"<div class="alert alert-info mb-4 text-sm">This workflow is archived: its versions stay readable and keep serving the bindings that use them, but the draft can no longer be saved or published.</div>"#
    } else {
        ""
    };
    format!(
        r##"{archived_note}{banner}{diagnostic}
        <form id="workflow-editor" class="space-y-4"
            hx-post="/ui/workflows/{workflow}/save?company_id={company_id}"
            hx-target="#workflow-pane" hx-swap="outerHTML"
            hx-disabled-elt="find button[type='submit']">
            <input type="hidden" name="expected_revision" value="{expected}">
            <label class="fieldset">
                <span class="fieldset-legend">Title</span>
                <input class="input w-full" name="title" required maxlength="256" value="{title}"{disabled}>
            </label>
            <label class="fieldset">
                <span class="fieldset-legend">Description</span>
                <textarea class="textarea w-full" name="description" rows="2" maxlength="4096"{disabled}>{description}</textarea>
            </label>
            <label class="fieldset">
                <span class="fieldset-legend">Source (YAML)</span>
                <textarea id="workflow-source" class="textarea w-full font-mono text-xs" name="source" rows="28"
                    spellcheck="false" autocapitalize="off" autocomplete="off" wrap="off"
                    aria-describedby="workflow-source-help"{disabled}>{source}</textarea>
                <span id="workflow-source-help" class="text-xs opacity-60">Saving keeps whatever you type, valid or not. Validation compiles the saved revision exactly as publishing would.</span>
            </label>
            {selections}
            <div class="flex flex-wrap items-center gap-2 border-t border-base-300 pt-4">
                <button type="submit" name="intent" value="validate" class="btn btn-primary"{disabled}>
                    <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                    <span class="[.htmx-request_&]:hidden">Save and validate</span>
                    <span class="hidden [.htmx-request_&]:inline">Validating…</span>
                </button>
                <button type="submit" name="intent" value="save" class="btn btn-outline"{disabled}>Save draft</button>
                <button type="button" class="btn btn-ghost"
                    hx-get="/ui/workflows/{workflow}?company_id={company_id}"
                    hx-target="#workflow-pane" hx-swap="outerHTML" hx-sync="#workflow-pane:replace"
                    hx-confirm="Discard unsaved changes and reload the saved draft?">Reload saved draft</button>
            </div>
        </form>
        {preview}
        {catalogue}
        {archive}"##,
        expected = view.draft.expected_revision,
        title = escape_html_attr(view.draft.title),
        description = escape_html_text(view.draft.description),
        source = escape_html_text(view.draft.source),
        selections = selection_fieldset(&view.needs, view.options, view.selections, disabled),
        preview = view.preview_html.unwrap_or_default(),
        catalogue = catalogue_disclosure(company_id),
        archive = archive_form(view),
    )
}

/// Archiving stops new publications of the draft; versions and their bindings are untouched.
fn archive_form(view: &EditorView<'_>) -> String {
    if view.archived {
        return String::new();
    }
    format!(
        r##"<form class="mt-6 border-t border-base-300 pt-4"
            hx-post="/ui/workflows/{workflow}/archive?company_id={company_id}"
            hx-target="#workflow-pane" hx-swap="outerHTML"
            hx-confirm="Archive this workflow? Its draft can no longer be saved or published; published versions keep serving their bindings."
            hx-disabled-elt="find button[type='submit']">
            <input type="hidden" name="expected_revision" value="{expected}">
            <button type="submit" class="btn btn-sm btn-error btn-outline">
                <span class="loading loading-spinner loading-xs hidden [.htmx-request_&]:inline-block"></span>
                <span class="[.htmx-request_&]:hidden">Archive workflow</span>
                <span class="hidden [.htmx-request_&]:inline">Archiving…</span>
            </button>
        </form>"##,
        workflow = view.workflow,
        company_id = view.company_id,
        expected = view.draft.expected_revision,
    )
}

fn conflict_banner(current: u64, theirs: &str, yours: &str) -> String {
    format!(
        r##"<div role="alert" class="alert alert-warning mb-4 flex-col items-start text-sm">
            <p><strong>Someone else saved revision {current} while you were editing.</strong> Your text is still in the form below and nothing of yours was saved. Save again to replace revision {current} with your text, or reload the saved draft to discard it.</p>
            <details class="w-full"><summary class="cursor-pointer">What revision {current} has, against your text</summary>{diff}</details>
        </div>"##,
        diff = diff_block(&line_diff(theirs, yours), "revision", "yours"),
    )
}

/// Lines around a diagnostic's location, the offending line marked, and a control that puts
/// the cursor there in the source field.
fn diagnostic_panel(diagnostic: &Diagnostic, source: &str) -> String {
    let line = diagnostic.span.line;
    let excerpt: String = source
        .lines()
        .enumerate()
        .map(|(index, text)| (index + 1, text))
        .filter(|(number, _)| line > 0 && number.abs_diff(line) <= 2)
        .map(|(number, text)| {
            format!(
                r#"<span class="block {class}">{number:>5} │ {text}</span>"#,
                class = if number == line {
                    "bg-error/10 font-semibold"
                } else {
                    "opacity-70"
                },
                text = escape_html_text(text),
            )
        })
        .collect();
    let location = if line > 0 {
        format!(
            r##"line {line}, column {column}
            <button type="button" class="btn btn-xs btn-outline ml-2" data-action="workflow-source-goto"
                data-line="{line}">Go to line {line}</button>"##,
            column = diagnostic.span.column,
        )
    } else {
        "the whole source".to_owned()
    };
    let schema = match (&diagnostic.instance_path, &diagnostic.schema_path) {
        (None, None) => String::new(),
        (instance, schema) => format!(
            r#"<p class="font-mono text-xs opacity-70">value {} · schema {}</p>"#,
            escape_html_text(instance.as_deref().unwrap_or("-")),
            escape_html_text(schema.as_deref().unwrap_or("-")),
        ),
    };
    format!(
        r##"<div role="alert" id="workflow-diagnostic" class="alert alert-error mb-4 flex-col items-start text-sm">
            <p><strong>Not valid:</strong> {message}</p>
            <p class="text-xs">At {location} · field <span class="font-mono">{field}</span> · <span class="font-mono">{code}</span></p>
            {schema}
            {excerpt}
        </div>"##,
        message = escape_html_text(&diagnostic.message),
        field = escape_html_text(if diagnostic.field_path.is_empty() {
            "/"
        } else {
            &diagnostic.field_path
        }),
        code = escape_html_text(diagnostic.code),
        excerpt = if excerpt.is_empty() {
            String::new()
        } else {
            format!(
                r#"<pre class="w-full overflow-x-auto rounded-box bg-base-100 p-2 font-mono text-xs">{excerpt}</pre>"#
            )
        },
    )
}

fn option_label(option: &OperationOption) -> String {
    let state = if option.connection_enabled {
        ""
    } else {
        " (disabled)"
    };
    let approval = if option.approval_required {
        " · approval required"
    } else {
        ""
    };
    format!("{}{state}{approval}", option.connection_label)
}

fn selection_select(
    name: &str,
    label: &str,
    choices: &[&OperationOption],
    chosen: Option<Uuid>,
    disabled: &str,
) -> String {
    let options: String = choices
        .iter()
        .map(|option| {
            format!(
                r#"<option value="{id}"{selected}>{label}</option>"#,
                id = option.connection,
                selected = if chosen == Some(option.connection) {
                    " selected"
                } else {
                    ""
                },
                label = escape_html_text(&option_label(option)),
            )
        })
        .collect();
    // Every candidate's contract, so the author can compare before choosing.
    let schema: String = choices
        .iter()
        .map(|option| frozen_contract(option))
        .collect();
    format!(
        r##"<div class="space-y-1">
            <label class="flex flex-wrap items-center gap-2 text-sm">
                <span class="min-w-0 flex-1">{label}</span>
                <select class="select select-sm" name="{name}"{disabled}><option value="">Not selected</option>{options}</select>
            </label>
            {schema}
        </div>"##,
        name = escape_html_attr(name),
        label = label,
    )
}

/// The selected operation's frozen contract and policy, as publication will capture it.
fn frozen_contract(option: &OperationOption) -> String {
    let contract = &option.snapshot["contract"];
    let policy = &option.snapshot["policy"];
    format!(
        r##"<details class="ml-2 text-xs">
            <summary class="cursor-pointer opacity-70">Frozen contract of {name} on {connection} · {effect}, recovery {recovery}</summary>
            <p class="mt-1 font-semibold">Arguments</p>{input}
            <p class="mt-1 font-semibold">Result</p>{output}
        </details>"##,
        name = escape_html_text(&option.name),
        connection = escape_html_text(&option.connection_label),
        effect = escape_html_text(policy["effect"].as_str().unwrap_or("unknown effect")),
        recovery = escape_html_text(policy["recovery"].as_str().unwrap_or("unknown")),
        input = json_block(&contract["input_schema"]),
        output = json_block(&contract["output_schema"]),
    )
}

fn selection_fieldset(
    needs: &Result<&PublicationNeeds, &Diagnostic>,
    options: &PublicationOptions,
    chosen: &WorkflowSelections,
    disabled: &str,
) -> String {
    let needs = match needs {
        Ok(needs) => needs,
        // The diagnostic panel (after a validation) or the compiler says why; nothing to pick.
        Err(_) => return String::new(),
    };
    let mut rows = Vec::new();
    for need in &needs.mcp {
        let choices: Vec<&OperationOption> = options
            .mcp
            .iter()
            .filter(|option| option.name == need.tool.as_str())
            .collect();
        let label = format!(
            r#"MCP tool <span class="font-mono">{tool}</span> for slot <span class="font-mono">{slot}</span> <span class="opacity-60">(steps {steps})</span>"#,
            tool = escape_html_text(need.tool.as_str()),
            slot = escape_html_text(need.slot.as_str()),
            steps = escape_html_text(&steps(&need.steps)),
        );
        if choices.is_empty() {
            rows.push(format!(
                r#"<p class="text-sm">{label}: <span class="text-error">no company MCP connection has this tool approved.</span> Approve it on an MCP server first.</p>"#
            ));
            continue;
        }
        rows.push(selection_select(
            &format!("mcp:{}:{}", need.slot.as_str(), need.tool.as_str()),
            &label,
            &choices,
            chosen
                .mcp
                .get(&(need.slot.as_str().to_owned(), need.tool.as_str().to_owned()))
                .copied(),
            disabled,
        ));
    }
    for need in &needs.tools {
        let choices: Vec<&OperationOption> = options
            .http
            .iter()
            .filter(|option| option.name == need.tool.as_str())
            .collect();
        if choices.is_empty() {
            // A workflow tool or a built-in tool: publication resolves it by name.
            continue;
        }
        let label = format!(
            r#"HTTP operation <span class="font-mono">{tool}</span> <span class="opacity-60">(steps {steps})</span>"#,
            tool = escape_html_text(need.tool.as_str()),
            steps = escape_html_text(&steps(&need.steps)),
        );
        rows.push(selection_select(
            &format!("http:{}", need.tool.as_str()),
            &label,
            &choices,
            chosen.http.get(need.tool.as_str()).copied(),
            disabled,
        ));
    }
    if rows.is_empty() {
        return String::new();
    }
    let cut = if options.truncated {
        r#"<p class="text-xs text-error">The company has more approved operations than this list shows (200).</p>"#
    } else {
        ""
    };
    format!(
        r#"<fieldset class="fieldset rounded-box border border-base-300 p-3">
            <legend class="fieldset-legend">Connections for the draft's tools</legend>
            <p class="text-xs opacity-70">Validation and publishing freeze each selected tool's approved contract. Credentials stay with the connection; they are never part of the source or this preview.</p>
            {rows}{cut}
        </fieldset>"#,
        rows = rows.join(""),
    )
}

fn steps(ids: &[crate::domain::workflow::StepId]) -> String {
    ids.iter()
        .map(|id| id.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The editor's client behaviour: "Go to line" selects that line of the source field and
/// scrolls it into view, from the keyboard as well as the mouse (it is a button).
pub const WORKFLOW_EDITOR_SCRIPT: &str = r##"
document.addEventListener('click', function (event) {
    var control = event.target.closest('[data-action="workflow-source-goto"]');
    if (!control) return;
    var field = document.getElementById('workflow-source');
    if (!field) return;
    var wanted = parseInt(control.dataset.line, 10);
    var lines = field.value.split('\n');
    if (!(wanted >= 1 && wanted <= lines.length)) return;
    var start = 0;
    for (var index = 0; index < wanted - 1; index++) start += lines[index].length + 1;
    field.focus();
    field.setSelectionRange(start, start + lines[wanted - 1].length);
    var lineHeight = parseFloat(getComputedStyle(field).lineHeight) || 16;
    field.scrollTop = Math.max(0, (wanted - 3) * lineHeight);
});
"##;
