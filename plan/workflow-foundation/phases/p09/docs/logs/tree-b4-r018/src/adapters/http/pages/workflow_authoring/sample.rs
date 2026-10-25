use super::*;
use crate::application::workflow::sample::{SampleReport, SampleStatus};

/// Raw author input is retained even when JSON or workflow validation fails.
pub struct SampleInputs {
    pub input: String,
    pub params: String,
    pub scripts: String,
}
impl Default for SampleInputs {
    fn default() -> Self {
        Self {
            input: "{}".into(),
            params: "{}".into(),
            scripts: "[]".into(),
        }
    }
}
pub struct SampleView<'a> {
    pub company: Uuid,
    pub workflow: Uuid,
    pub revision: u64,
    pub current_revision: u64,
    pub inputs: &'a SampleInputs,
    pub selections_html: &'a str,
    pub report: Option<&'a SampleReport>,
    pub error: Option<&'a str>,
}
pub fn workflow_sample(view: &SampleView<'_>) -> String {
    let mut fields = String::new();
    for (name, label, raw) in [
        ("input", "Workflow input (JSON)", &view.inputs.input),
        ("params", "Parameters (JSON)", &view.inputs.params),
        (
            "scripts",
            "Scripted responses (JSON array)",
            &view.inputs.scripts,
        ),
    ] {
        fields.push_str(&format!(r#"<label class="fieldset"><span class="fieldset-legend">{label}</span>
            <textarea name="{name}" class="textarea w-full font-mono text-xs" rows="6" required spellcheck="false">
{raw}</textarea></label>"#,
            raw = escape_html_text(raw)));
    }
    let changed = if view.revision != view.current_revision {
        format!(
            "The draft is now revision {}. This form and report refer to revision {}; reopen Sample to use the latest draft.",
            view.current_revision, view.revision
        )
    } else {
        String::new()
    };
    format!(
        r##"<div class="space-y-4">
        <h3 class="font-semibold">Try the saved draft</h3>
        <p class="text-sm">Samples use your scripted responses. Messages and writes are captured here; no provider, external service or production run is called. Durability, retries and real service behavior are not tested. A sample cannot be resumed or promoted into a production run.</p>
        <p class="text-xs opacity-70">Draft revision {revision}. Save changes in Editor before sampling.</p>
        {error}{changed}
        <form id="workflow-sample-form" class="space-y-3" hx-post="/ui/workflows/{workflow}/sample?company_id={company}"
            hx-target="#workflow-pane" hx-swap="outerHTML" hx-disabled-elt="find button[type='submit']">
            <input type="hidden" name="expected_revision" value="{revision}">
            {fields}{selections}
            <details class="rounded-box border border-base-300 p-3"><summary class="cursor-pointer text-sm">Script format and limits</summary>
                <p class="mt-2 text-sm">Each external step consumes the next response for its workflow and step. Child workflows use their own workflow ID. Missing or invalid responses stop the sample. Pure steps and workflow children execute directly.</p>
                {example}
                <p class="text-xs opacity-70">Replace STEP_ID and output with the step's declared result. Human output needs choice, data and feedback. Agent responses may include tools: [{{"name":"tool_name","input":{{}},"output":{{}}}}]. Omit a workflow tool's output: its pinned child runs inside the sample. Protected requests require approval: true. Token usage is synthetic. Limits: 256 KiB input, 128 activations/responses, 64 direct tools, 8 child levels, 1 MiB report; workflow budgets may be lower.</p>
            </details>
            <button class="btn btn-primary" type="submit"><span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span><span class="[.htmx-request_&]:hidden">Run sample</span><span class="hidden [.htmx-request_&]:inline">Running sample…</span></button>
        <p data-sample-status role="status" class="text-sm" aria-live="polite"></p></form>{report}</div>"##,
        revision = view.revision,
        workflow = view.workflow,
        company = view.company,
        error = form_error_banner(view.error),
        changed = form_error_banner((!changed.is_empty()).then_some(changed.as_str())),
        selections = view.selections_html,
        example = json_block(
            &serde_json::json!([{"workflow_id":view.workflow,"step":"STEP_ID","responses":[{"output":{},"tokens":0}]}])
        ),
        report = view.report.map(sample_report).unwrap_or_default()
    )
}
fn sample_report(report: &SampleReport) -> String {
    let status = match report.status {
        SampleStatus::Completed => "Sample completed",
        SampleStatus::Stopped => "Sample stopped",
    };
    format!(
        r#"<section id="sample-report" class="space-y-3" aria-label="Sample report"><h3 role="status" class="font-semibold">{status}</h3>
        <p class="text-xs opacity-70">Exact frozen content: <span class="font-mono">{hash}</span></p>
        {diagnostic}<p class="text-sm">{unused} unused scripted responses. Execution paths below are sample identities.</p>{body}</section>"#,
        hash = escape_html_text(report.content_hash.as_str()),
        diagnostic = form_error_banner(report.diagnostic.as_deref()),
        unused = report.unused_responses,
        body = json_block(&serde_json::to_value(report).expect("sample report is serializable"))
    )
}
