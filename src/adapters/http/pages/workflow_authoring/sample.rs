use super::*;
use crate::application::workflow::sample::{
    SampleReport, SampleResponse, SampleStatus, SampleWaitStage,
};

/// Raw author input is retained even when JSON or workflow validation fails.
pub struct SampleInputs {
    pub input: String,
    pub params: String,
    pub scripts: String,
    pub response_contract: String,
    pub response_approval_policy: String,
}
impl Default for SampleInputs {
    fn default() -> Self {
        Self {
            input: "{}".into(),
            params: "{}".into(),
            scripts: "[]".into(),
            response_contract: "null".into(),
            response_approval_policy: "null".into(),
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
        (
            "response_contract",
            "Sample response mapping (JSON or null)",
            &view.inputs.response_contract,
        ),
        (
            "response_approval_policy",
            "Sample final approval policy (JSON or null)",
            &view.inputs.response_approval_policy,
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
        <p class="text-sm">Samples use your scripted responses. Messages and writes are captured here; no provider, external service or production run is called. Response mappings and final approval policies apply only to this frozen sample. Without a mapping, automatic output is shown as unmapped business output. No human notification is created. Durability, retries and real service behavior are not tested. A sample cannot be resumed or promoted into a production run.</p>
        <p class="text-xs opacity-70">Draft revision {revision}. Save changes in Editor before sampling.</p>
        {error}{changed}
        <form id="workflow-sample-form" class="space-y-3" hx-post="/ui/workflows/{workflow}/sample?company_id={company}"
            hx-target="#workflow-pane" hx-swap="outerHTML" hx-disabled-elt="find button[type='submit']">
            <input type="hidden" name="expected_revision" value="{revision}">
            {fields}{selections}
            <details class="rounded-box border border-base-300 p-3"><summary class="cursor-pointer text-sm">Script format and limits</summary>
                <p class="mt-2 text-sm">Each external step consumes the next response for its workflow and step. Child workflows use their own workflow ID. Missing human responses show a waiting preview and stop the sample. Other missing or invalid responses stop the sample. Pure steps and workflow children execute directly.</p>
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
        <p class="text-xs opacity-70">Frozen configuration: <span class="font-mono">{configuration}</span></p>
        <p class="text-sm">Agent history is local to this sample. Repeated calls share history; workflow children keep private history. Model context token limits are not simulated.</p>
        {diagnostic}{response}<p class="text-sm">{unused} unused scripted responses. Execution paths below are sample identities.</p>{body}</section>"#,
        hash = escape_html_text(report.content_hash.as_str()),
        configuration = escape_html_text(report.configuration_hash.as_str()),
        diagnostic = form_error_banner(report.diagnostic.as_deref()),
        unused = report.unused_responses,
        response = report
            .response
            .as_ref()
            .map(response_preview)
            .unwrap_or_default(),
        body = json_block(&serde_json::to_value(report).expect("sample report is serializable"))
    )
}

fn response_preview(response: &SampleResponse) -> String {
    let (label, content) = match response {
        SampleResponse::AutomaticUnmapped => ("Automatic response: mapping required",
            "<p>Choose an explicit sample response mapping to preview the final text. Business output is shown below.</p>".into()),
        SampleResponse::Suppressed => ("Finish without reply",
            "<p>No automatic final reply. Business output and earlier intercepted messages remain in the report.</p>".into()),
        SampleResponse::Automatic { content, approval_required } => (
            if *approval_required { "Final response would wait for review" } else { "Automatic final response preview" },
            format!("<p class=\"font-semibold\">{}</p><pre class=\"whitespace-pre-wrap\">{}</pre>",
                escape_html_text(&content.subject),escape_html_text(&content.body))),
        SampleResponse::Waiting {stage,preview} => (match stage {
            SampleWaitStage::HumanReply => "Waiting for a human reply",
            SampleWaitStage::FormalReview => "Waiting for a reviewer",
            SampleWaitStage::QuestionApproval => "Question would wait for approval",
        },json_block(preview)),
    };
    format!(
        "<section aria-label=\"Intended sample response\" class=\"rounded-box border border-base-300 p-3\"><h4 class=\"font-semibold\">{label}</h4>{content}<p class=\"text-xs opacity-70\">Preview only; no delivery or reviewer notification is created.</p></section>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::workflow::sample::SampleResponseContent;

    #[test]
    fn channel_reply_authoring_sample_preview_escapes_content_and_explains_human_waits() {
        let html = response_preview(&SampleResponse::Automatic {
            content: SampleResponseContent {
                subject: "<img src=x onerror=alert(1)>".into(),
                body: "</pre><script>private text</script>".into(),
            },
            approval_required: true,
        });
        assert!(html.contains("Final response would wait for review"));
        assert!(html.contains("&lt;img") && html.contains("&lt;/pre&gt;&lt;script&gt;"));
        assert!(!html.contains("<script>") && !html.contains("<img"));
        for (stage, label) in [
            (SampleWaitStage::HumanReply, "Waiting for a human reply"),
            (SampleWaitStage::FormalReview, "Waiting for a reviewer"),
            (
                SampleWaitStage::QuestionApproval,
                "Question would wait for approval",
            ),
        ] {
            let html = response_preview(&SampleResponse::Waiting {
                stage,
                preview: serde_json::json!({"body":"</pre><img>"}),
            });
            assert!(html.contains(label) && !html.contains("<img>"));
            assert!(html.contains("no delivery or reviewer notification"));
        }
    }
}
