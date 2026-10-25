//! The workflow decision page review notices link to (p08 B3b-1): the proposal, the deadline,
//! a submission form over the decision's own choices, and the discussion.
use super::*;
use crate::application::workflow::decisions::DecisionView;

/// What the last submission or comment from this page did, for the section to say.
pub enum DecisionOutcome<'a> {
    Done(&'a str),
    Refused(&'a str),
}

pub fn workflow_decision_page(view: &DecisionView) -> String {
    base_layout(
        "Workflow decision",
        &format!(
            r#"<main class="mx-auto w-full max-w-3xl px-4 py-6">{}</main>"#,
            workflow_decision_section(view, None)
        ),
    )
}

/// The page's live region; its forms swap it in place.
pub fn workflow_decision_section(
    view: &DecisionView,
    outcome: Option<DecisionOutcome<'_>>,
) -> String {
    let decision = view.decision.0;
    let banner = match outcome {
        None => String::new(),
        Some(DecisionOutcome::Done(message)) => format!(
            r#"<div role="status" class="alert alert-success text-sm">{}</div>"#,
            escape_html_text(message)
        ),
        Some(DecisionOutcome::Refused(message)) => form_error_banner(Some(message)),
    };
    let proposal = serde_json::to_string_pretty(&view.proposal).unwrap_or_default();
    let open = view.state == "waiting" && view.submission.is_none();
    let form = if open {
        let buttons = view
            .choices
            .iter()
            .map(|choice| {
                format!(
                    r#"<button type="submit" name="choice" value="{value}" class="btn btn-outline">{label}</button>"#,
                    value = escape_html_attr(choice),
                    label = escape_html_text(choice),
                )
            })
            .collect::<String>();
        format!(
            r##"<form hx-post="/ui/workflow-decisions/{decision}/submit"
                    hx-target="#workflow-decision" hx-swap="outerHTML"
                    hx-disabled-elt="find button[type='submit']" class="space-y-3">
                    <input type="hidden" name="command_key" value="{command_key}">
                    <input type="hidden" name="expected_revision" value="{revision}">
                    <input type="hidden" name="expected_artifact" value="{artifact}">
                    <fieldset class="fieldset">
                        <legend class="fieldset-legend">Data (JSON, edit before approving)</legend>
                        <textarea name="data" rows="8" class="textarea w-full font-mono text-sm">{proposal}</textarea>
                    </fieldset>
                    <fieldset class="fieldset">
                        <legend class="fieldset-legend">Feedback</legend>
                        <textarea name="feedback" rows="3" class="textarea w-full"></textarea>
                    </fieldset>
                    <div class="flex flex-wrap items-center gap-2">
                        {buttons}
                        <span class="hidden text-sm [.htmx-request_&]:inline" role="status">
                            <span class="loading loading-spinner loading-sm" aria-hidden="true"></span> Submitting…
                        </span>
                    </div>
                </form>"##,
            command_key = Uuid::new_v4(),
            revision = view.revision,
            artifact = escape_html_attr(&view.artifact_digest),
            proposal = escape_html_text(&proposal),
        )
    } else {
        match &view.submission {
            Some(submission) => format!(
                r#"<p class="text-sm">Decided: <strong>{choice}</strong> at {at}.</p>{feedback}"#,
                choice = escape_html_text(&submission.choice),
                at = escape_html_text(&submission.submitted_at.to_rfc3339()),
                feedback = if submission.feedback.is_empty() {
                    String::new()
                } else {
                    format!(
                        r#"<p class="text-sm opacity-80">{}</p>"#,
                        escape_html_text(&submission.feedback)
                    )
                },
            ),
            None => format!(
                r#"<p class="text-sm">This decision is closed ({}).</p>"#,
                escape_html_text(&view.state)
            ),
        }
    };
    let comments = view
        .comments
        .iter()
        .map(|comment| {
            format!(
                r#"<li class="rounded-box border border-base-300 p-3 text-sm"><p class="whitespace-pre-wrap">{body}</p><p class="mt-1 text-xs opacity-60">{at}</p></li>"#,
                body = escape_html_text(&comment.body),
                at = escape_html_text(&comment.created_at.to_rfc3339()),
            )
        })
        .collect::<String>();
    format!(
        r##"<section id="workflow-decision" class="space-y-6">
            <header class="space-y-1">
                <h1 class="text-xl font-bold">Workflow decision</h1>
                <p class="text-sm opacity-70">Step {step} · deadline {deadline}</p>
            </header>
            {banner}
            <div class="space-y-2">
                <h2 class="font-semibold">Proposal</h2>
                <pre class="overflow-x-auto rounded-box bg-base-200 p-3 text-sm">{proposal}</pre>
            </div>
            {form}
            <div class="space-y-2">
                <h2 class="font-semibold">Discussion</h2>
                <ul class="space-y-2">{comments}</ul>
                <form hx-post="/ui/workflow-decisions/{decision}/comments"
                    hx-target="#workflow-decision" hx-swap="outerHTML"
                    hx-disabled-elt="find button[type='submit']" class="space-y-2">
                    <input type="hidden" name="command_key" value="{comment_key}">
                    <textarea name="body" rows="2" required maxlength="16384" class="textarea w-full" aria-label="Comment"></textarea>
                    <button type="submit" class="btn btn-sm">
                        <span class="loading loading-spinner loading-xs hidden [.htmx-request_&]:inline-block"></span>
                        <span class="[.htmx-request_&]:hidden">Comment</span>
                        <span class="hidden [.htmx-request_&]:inline">Posting…</span>
                    </button>
                </form>
            </div>
        </section>"##,
        step = escape_html_text(view.step.as_str()),
        deadline = escape_html_text(&view.deadline.to_rfc3339()),
        proposal = escape_html_text(&proposal),
        comment_key = Uuid::new_v4(),
    )
}
