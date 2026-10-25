//! The editor tab: title, description and YAML source saved against the revision the author
//! opened, the publication selections the draft asks for, and what the last save or validation
//! said. A refused save keeps every byte the author typed, and leaving the pane with unsaved
//! text asks first (`WORKFLOW_EDITOR_SCRIPT`).
use super::*;
use crate::application::workflow::authoring::{PublicationNeeds, line_diff};
use crate::application::workflow::compiler::Diagnostic;
use crate::application::workflow::reads::{OperationOption, PublicationOptions};
use crate::domain::workflow::{ResourceName, TypeName};
use std::collections::BTreeMap;

/// An `mcp.call` selection's key: the declared slot and the tool reached through it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct McpSelectionKey {
    pub slot: ResourceName,
    pub tool: TypeName,
}

impl McpSelectionKey {
    /// The form field that carries this selection.
    pub fn field(&self) -> String {
        format!("mcp:{}:{}", self.slot.as_str(), self.tool.as_str())
    }
}

/// The connection chosen for each publication need, as the editor's selects send them:
/// `mcp:<slot>:<tool>` and `http:<tool>` fields naming a connection id (empty: none).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkflowSelections {
    pub mcp: BTreeMap<McpSelectionKey, Uuid>,
    pub http: BTreeMap<TypeName, Uuid>,
}

impl WorkflowSelections {
    /// The selections among a form's fields. Anything else, a field whose slot or tool is not a
    /// valid name, and a field that names no connection select nothing.
    pub fn from_fields<'a>(fields: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut selections = Self::default();
        for (name, value) in fields {
            let Ok(connection) = value.trim().parse::<Uuid>() else {
                continue;
            };
            if let Some(rest) = name.strip_prefix("mcp:") {
                let Some((slot, tool)) = rest.split_once(':') else {
                    continue;
                };
                if let (Ok(slot), Ok(tool)) = (ResourceName::parse(slot), TypeName::parse(tool)) {
                    selections
                        .mcp
                        .insert(McpSelectionKey { slot, tool }, connection);
                }
            } else if let Some(Ok(tool)) = name.strip_prefix("http:").map(TypeName::parse) {
                selections.http.insert(tool, connection);
            }
        }
        selections
    }

    /// The same selections as hidden fields, so a publish sends exactly what was validated.
    pub fn hidden_fields(&self) -> String {
        let mcp = self.mcp.iter().map(|(key, connection)| {
            format!(
                r#"<input type="hidden" name="{}" value="{connection}">"#,
                escape_html_attr(&key.field())
            )
        });
        let http = self.http.iter().map(|(tool, connection)| {
            format!(
                r#"<input type="hidden" name="http:{}" value="{connection}">"#,
                escape_html_attr(tool.as_str())
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
    /// The author's save landed as `saved`, then someone saved `current` before it could be
    /// validated. The form keeps the author's text and sends `current`.
    MovedAfterSave {
        saved: u64,
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
    /// The form shows text the server echoed back unsaved (a refused or conflicting save), so
    /// leaving the pane must ask as if the author had just typed it.
    pub unsaved: bool,
    /// The draft names more tools of one kind than connections are offered for.
    pub too_many_tools: bool,
}

pub fn workflow_editor(view: &EditorView<'_>) -> String {
    let (company_id, workflow) = (view.company_id, view.workflow);
    let banner = match &view.banner {
        None => String::new(),
        Some(EditorBanner::Saved(message)) => status_banner(message),
        Some(EditorBanner::Refused(message)) => form_error_banner(Some(message)),
        Some(EditorBanner::Conflict { current, theirs }) => conflict_banner(
            &format!(
                "<strong>Someone else saved revision {current} while you were editing.</strong> Your text is still in the form below and nothing of yours was saved. Save again to replace revision {current} with your text, or reload the saved draft to discard it."
            ),
            *current,
            theirs,
            view.draft.source,
        ),
        Some(EditorBanner::MovedAfterSave {
            saved,
            current,
            theirs,
        }) => conflict_banner(
            &format!(
                "<strong>Saved revision {saved}, but someone else saved revision {current} before it could be validated.</strong> Your text is still in the form below. Save again to replace revision {current} with it, or reload the saved draft to keep theirs."
            ),
            *current,
            theirs,
            view.draft.source,
        ),
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
    // The newline after each <textarea> start tag is consumed by the HTML parser, so a value
    // that itself starts with a newline keeps it (F005).
    format!(
        r##"{archived_note}{banner}{diagnostic}
        <form id="workflow-editor" class="space-y-4" data-workflow-editor{dirty}
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
                <textarea class="textarea w-full" name="description" rows="2" maxlength="4096"{disabled}>
{description}</textarea>
            </label>
            <label class="fieldset">
                <span class="fieldset-legend">Source (YAML)</span>
                <textarea id="workflow-source" class="textarea w-full font-mono text-xs" name="source" rows="28"
                    spellcheck="false" autocapitalize="off" autocomplete="off" wrap="off"
                    aria-describedby="workflow-source-help"{disabled}>
{source}</textarea>
                <span id="workflow-source-help" class="text-xs opacity-60">Saving keeps whatever you type, valid or not. Validation compiles the saved revision exactly as publishing would.</span>
            </label>
            {selections}
            <div class="flex flex-wrap items-center gap-2 border-t border-base-300 pt-4">
                <button type="submit" name="intent" value="validate" class="btn btn-primary"{disabled}>Save and validate</button>
                <button type="submit" name="intent" value="save" class="btn btn-outline"{disabled}>Save draft</button>
                <button type="button" class="btn btn-ghost" data-editor-reload
                    hx-get="/ui/workflows/{workflow}?company_id={company_id}"
                    hx-target="#workflow-pane" hx-swap="outerHTML" hx-sync="#workflow-pane:replace"
                    hx-confirm="Discard unsaved changes and reload the saved draft?">Reload saved draft</button>
                <span id="workflow-editor-status" class="text-sm" role="status" aria-live="polite"></span>
            </div>
        </form>
        {preview}
        {catalogue}
        {archive}"##,
        dirty = if view.unsaved { " data-dirty" } else { "" },
        expected = view.draft.expected_revision,
        title = escape_html_attr(view.draft.title),
        description = escape_html_text(view.draft.description),
        source = escape_html_text(view.draft.source),
        selections = workflow_selection_fieldset(
            view.needs,
            view.options,
            view.selections,
            view.too_many_tools,
            disabled
        ),
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
            <button type="submit" class="btn btn-sm btn-error">
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

/// `lead` is trusted markup built here; the two sources are escaped by the diff.
fn conflict_banner(lead: &str, current: u64, theirs: &str, yours: &str) -> String {
    format!(
        r##"<div role="alert" class="alert alert-warning mb-4 flex-col items-start text-sm">
            <p>{lead}</p>
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
                r#"<pre class="w-full overflow-x-auto rounded-box bg-base-100 p-2 font-mono text-xs text-base-content">{excerpt}</pre>"#
            )
        },
    )
}

pub(super) fn option_label(option: &OperationOption) -> String {
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

/// One select over the connections offering a needed tool.
struct SelectionRow<'a> {
    /// The form field (`mcp:<slot>:<tool>` or `http:<tool>`).
    field: String,
    /// Trusted markup naming the need.
    label: String,
    choices: Vec<&'a OperationOption>,
    chosen: Option<Uuid>,
    /// More connections offer the tool than were listed.
    cut: bool,
}

fn selection_select(row: &SelectionRow<'_>, disabled: &str) -> String {
    let options: String = row
        .choices
        .iter()
        .map(|option| {
            format!(
                r#"<option value="{id}"{selected}>{label}</option>"#,
                id = option.connection,
                selected = if row.chosen == Some(option.connection) {
                    " selected"
                } else {
                    ""
                },
                label = escape_html_text(&option_label(option)),
            )
        })
        .collect();
    // Every candidate's contract, so the author can compare before choosing.
    let schema: String = row
        .choices
        .iter()
        .map(|option| frozen_contract(option))
        .collect();
    let cut = if row.cut {
        format!(
            r#"<p class="text-xs text-error">More connections offer this tool than the {MAX} listed here.</p>"#,
            MAX = crate::application::workflow::reads::MAX_CONNECTIONS_PER_TOOL
        )
    } else {
        String::new()
    };
    format!(
        r##"<div class="space-y-1">
            <label class="flex flex-wrap items-center gap-2 text-sm">
                <span class="min-w-0 flex-1">{label}</span>
                <select class="select select-sm" name="{name}"{disabled}><option value="">Not selected</option>{options}</select>
            </label>
            {cut}{schema}
        </div>"##,
        name = escape_html_attr(&row.field),
        label = row.label,
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

fn offered<'a>(options: &'a [OperationOption], tool: &TypeName) -> Vec<&'a OperationOption> {
    options
        .iter()
        .filter(|option| option.name == tool.as_str())
        .collect()
}

pub fn workflow_selection_fieldset(
    needs: Result<&PublicationNeeds, &Diagnostic>,
    options: &PublicationOptions,
    chosen: &WorkflowSelections,
    too_many_tools: bool,
    disabled: &str,
) -> String {
    let needs = match needs {
        Ok(needs) => needs,
        // The diagnostic panel (after a validation) or the compiler says why; nothing to pick.
        Err(_) => return String::new(),
    };
    let mut rows = Vec::new();
    if too_many_tools {
        rows.push(format!(
            r#"<p class="text-sm text-error">This draft names more than {MAX} tools of one kind; connections are offered for the first {MAX} only, and publication refuses more selections than that.</p>"#,
            MAX = crate::application::workflow::reads::MAX_OPTION_NAMES
        ));
    }
    for need in &needs.mcp {
        let label = format!(
            r#"MCP tool <span class="font-mono">{tool}</span> for slot <span class="font-mono">{slot}</span> <span class="opacity-60">(steps {steps})</span>"#,
            tool = escape_html_text(need.tool.as_str()),
            slot = escape_html_text(need.slot.as_str()),
            steps = escape_html_text(&steps(&need.steps)),
        );
        let choices = offered(&options.mcp, &need.tool);
        if choices.is_empty() {
            rows.push(format!(
                r#"<p class="text-sm">{label}: <span class="text-error">no company MCP connection has this tool approved.</span> Approve it on an MCP server first.</p>"#
            ));
            continue;
        }
        let key = McpSelectionKey {
            slot: need.slot.clone(),
            tool: need.tool.clone(),
        };
        rows.push(selection_select(
            &SelectionRow {
                field: key.field(),
                label,
                cut: options.mcp_cut.contains(need.tool.as_str()),
                chosen: chosen.mcp.get(&key).copied(),
                choices,
            },
            disabled,
        ));
    }
    for need in &needs.tools {
        let choices = offered(&options.http, &need.tool);
        if choices.is_empty() {
            // No company HTTP operation has the name: a workflow tool or a built-in tool, which
            // publication resolves by name.
            continue;
        }
        let label = format!(
            r#"HTTP operation <span class="font-mono">{tool}</span> <span class="opacity-60">(steps {steps})</span>"#,
            tool = escape_html_text(need.tool.as_str()),
            steps = escape_html_text(&steps(&need.steps)),
        );
        rows.push(selection_select(
            &SelectionRow {
                field: format!("http:{}", need.tool.as_str()),
                label,
                cut: options.http_cut.contains(need.tool.as_str()),
                chosen: chosen.http.get(&need.tool).copied(),
                choices,
            },
            disabled,
        ));
    }
    if rows.is_empty() {
        return String::new();
    }
    format!(
        r#"<fieldset class="fieldset rounded-box border border-base-300 p-3">
            <legend class="fieldset-legend">Connections for the draft's tools</legend>
            <p class="text-xs opacity-70">Validation and publishing freeze each selected tool's approved contract. Credentials stay with the connection; they are never part of the source or this preview.</p>
            {rows}
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

/// The editor's client behaviour:
/// - "Go to line" selects that line of the source field and scrolls it into view (a button, so
///   the keyboard reaches it too);
/// - the editor form's own submit says what it is doing ("Saving…" or "Saving and
///   validating…") in its status region, and clears it when a request fails in place;
/// - typing marks the editor dirty, and any other request that would replace the pane (tabs,
///   sidebar, New, publish, archive) or a page navigation asks before discarding the text;
/// - the reference panel clears its busy state and says so when its load fails.
pub const WORKFLOW_EDITOR_SCRIPT: &str = r##"
// A sample is read-only but may finish after navigation. A detached requesting form must
// not replace a newly selected workflow pane. Keep errors beside the author's raw inputs.
document.body.addEventListener('htmx:beforeRequest', function (event) {
    var form = event.detail.elt;
    if (!form || form.id !== 'workflow-sample-form') return;
    form.setAttribute('aria-busy', 'true');
    var status = form.querySelector('[data-sample-status]');
    if (status) status.textContent = 'Running sample…';
    form.addEventListener('htmx:beforeOnLoad', function (response) {
        if (!form.isConnected) response.preventDefault();
    }, { once: true });
});
document.body.addEventListener('htmx:afterRequest', function (event) {
    var form = event.detail.elt;
    if (!form || form.id !== 'workflow-sample-form') return;
    form.removeAttribute('aria-busy');
    if (!event.detail.successful) {
        var status = form.querySelector('[data-sample-status]');
        if (status) status.textContent = 'Sample request failed. Your input is still here; try again.';
    }
});
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
function workflowEditorDirty() {
    return document.querySelector('form[data-workflow-editor][data-dirty]');
}
document.addEventListener('input', function (event) {
    var form = event.target.closest && event.target.closest('form[data-workflow-editor]');
    if (form) form.setAttribute('data-dirty', '');
});
document.addEventListener('change', function (event) {
    var form = event.target.closest && event.target.closest('form[data-workflow-editor]');
    if (form) form.setAttribute('data-dirty', '');
});
document.body.addEventListener('htmx:beforeRequest', function (event) {
    var elt = event.detail.elt;
    if (!(elt && elt.matches && elt.matches('form[data-workflow-editor]'))) return;
    var submitter = event.detail.requestConfig && event.detail.requestConfig.triggeringEvent && event.detail.requestConfig.triggeringEvent.submitter;
    var status = document.getElementById('workflow-editor-status');
    if (status) status.textContent = submitter && submitter.value === 'save' ? 'Saving…' : 'Saving and validating…';
    elt.setAttribute('aria-busy', 'true');
});
// `htmx:confirm` precedes `htmx:beforeRequest`, so a cancelled request never reaches the shared
// busy-button handler, which only clears its spinner on `htmx:afterRequest` (F014).
document.body.addEventListener('htmx:confirm', function (event) {
    var elt = event.detail.elt;
    if (elt && elt.matches && elt.matches('form[data-workflow-editor]')) return;
    // Only a request that replaces the whole pane discards the editor; one that fills a region
    // of it (the reference, a compare) does not.
    var dirty = workflowEditorDirty();
    var target = event.detail.target;
    if (!dirty || !target || target.id !== 'workflow-pane') return;
    if (elt && elt.hasAttribute && elt.hasAttribute('data-editor-reload')) return;
    // Accepting keeps the mark: the swap removes the form, and a request that is then refused
    // (an archive confirm cancelled, a failed request) leaves the text still guarded.
    if (!window.confirm('The editor has changes that are not saved. Discard them?')) {
        event.preventDefault();
    }
});
document.body.addEventListener('htmx:afterRequest', function (event) {
    var elt = event.detail.elt;
    if (elt && elt.matches && elt.matches('form[data-workflow-editor]') && !event.detail.successful) {
        var status = document.getElementById('workflow-editor-status');
        if (status) status.textContent = 'Not saved: the request failed. Your text is still here.';
        elt.removeAttribute('aria-busy');
    }
    var body = document.getElementById('workflow-reference-body');
    if (body && elt && elt.id === 'workflow-reference' && !event.detail.successful) {
        body.removeAttribute('aria-busy');
        body.textContent = 'The reference could not be loaded. Close and reopen this panel to try again.';
        elt.setAttribute('hx-trigger', 'toggle');
        if (window.htmx) window.htmx.process(elt);
    }
});
// A card loaded after the page (`pages::lazy_card`) says so when its load fails.
document.body.addEventListener('htmx:afterRequest', function (event) {
    var elt = event.detail.elt;
    if (!(elt && elt.hasAttribute && elt.hasAttribute('data-lazy-card')) || event.detail.successful) return;
    elt.removeAttribute('aria-busy');
    elt.textContent = elt.getAttribute('data-lazy-card');
});
window.addEventListener('beforeunload', function (event) {
    if (!workflowEditorDirty()) return;
    event.preventDefault();
    event.returnValue = '';
});
"##;
