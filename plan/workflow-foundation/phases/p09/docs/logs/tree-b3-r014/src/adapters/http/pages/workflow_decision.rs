//! Exact human decision identities, artifacts and discussion over the production commands.
use super::*;
use crate::application::workflow::decisions::{DecisionSubject, DecisionView};
use std::collections::BTreeMap;

mod forms;

pub enum DecisionOutcome<'a> {
    Done(&'a str),
    Refused(&'a str),
}

/// Raw form values survive validation errors without being coerced through a JSON value first.
#[derive(Debug, Clone, Default)]
pub struct DecisionInputs {
    pub values: BTreeMap<String, String>,
}
pub enum DecisionFormInputs {
    Submission(DecisionInputs),
    Comment(DecisionInputs),
}
impl DecisionInputs {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }
}

pub fn workflow_decision_page(view: &DecisionView) -> String {
    base_layout(
        view.subject.label(),
        &format!(
            r#"<main id="decision-shell" class="mx-auto w-full max-w-3xl px-4 py-6">{}</main>"#,
            workflow_decision_section(view, None, None)
        ),
    )
}

pub fn workflow_decision_section(
    view: &DecisionView,
    outcome: Option<DecisionOutcome<'_>>,
    inputs: Option<&DecisionFormInputs>,
) -> String {
    let submission = inputs.and_then(|inputs| match inputs {
        DecisionFormInputs::Submission(inputs) => Some(inputs),
        _ => None,
    });
    let comment = inputs.and_then(|inputs| match inputs {
        DecisionFormInputs::Comment(inputs) => Some(inputs),
        _ => None,
    });
    let banner = match outcome {
        None => String::new(),
        Some(DecisionOutcome::Done(message)) => format!(
            r#"<div role="status" class="alert alert-success text-sm">{}</div>"#,
            escape_html_text(message)
        ),
        Some(DecisionOutcome::Refused(message)) => form_error_banner(Some(message)),
    };
    let decision = view.decision.0;
    let open =
        view.state == "waiting" && view.submission.is_none() && view.deadline > chrono::Utc::now();
    format!(
        r##"<section id="workflow-decision" class="space-y-6">
        <header class="space-y-1"><h1 class="text-xl font-bold">{subject}</h1>
        <p class="text-sm">{title} · step {step}</p><p class="text-xs opacity-70">Decision {decision} · deadline {deadline}</p></header>
        {banner}{context}
        <section class="space-y-2"><h2 class="font-semibold">Reviewed artifact</h2>
            <p class="break-all font-mono text-xs">SHA-256 {digest}</p>
            <pre class="overflow-x-auto whitespace-pre-wrap rounded-box bg-base-200 p-3 text-sm">{proposal}</pre></section>
        {form}{history}{discussion}
        </section>"##,
        subject = view.subject.label(),
        title = escape_html_text(&view.context.workflow_title),
        step = escape_html_text(view.step.as_str()),
        deadline = escape_html_text(&view.deadline.to_rfc3339()),
        context = context(view),
        digest = escape_html_text(&view.artifact_digest),
        proposal = pretty(&view.proposal),
        form = if view.can_submit && open {
            forms::submission(view, submission)
        } else {
            settled(view, submission, open)
        },
        history = history(view),
        discussion = discussion(view, open, comment),
    )
}

fn pretty(value: &serde_json::Value) -> String {
    escape_html_text(&serde_json::to_string_pretty(value).unwrap_or_default())
}

fn context(view: &DecisionView) -> String {
    let company = view.context.company.as_uuid();
    let conversation = match (view.context.channel, view.context.thread) {
        (Some(channel), Some(thread)) => {
            let (href, label) = match view.context.trigger_message {
                Some(message) => (
                    format!(
                        "/ui/message?company_id={company}&channel_id={channel}&thread_id={thread}&message_id={message}"
                    ),
                    "Source message",
                ),
                None => (
                    format!("/ui?company_id={company}&channel_id={channel}&thread_id={thread}"),
                    "Conversation",
                ),
            };
            format!(
                r#"<a class="link" href="{}">{label}</a>"#,
                escape_html_attr(&href)
            )
        }
        _ => String::new(),
    };
    format!(
        r#"<section class="space-y-1 text-xs"><h2 class="font-semibold">Context</h2><p>{run} · {conversation}</p>
        <p class="break-all">Published version {version} · execution {execution} · decision revision {revision}</p></section>"#,
        run = run_chip(
            company,
            view.run.as_uuid(),
            if view.context.inspect_run {
                RunLinkAccess::Inspect
            } else {
                RunLinkAccess::Label
            }
        ),
        version = view.context.version.as_uuid(),
        execution = view.execution.as_uuid(),
        revision = view.revision
    )
}

fn settled(view: &DecisionView, inputs: Option<&DecisionInputs>, open: bool) -> String {
    let message = if open {
        "You can read and discuss this decision; only its assigned reviewers may submit it."
    } else {
        "This exact decision is closed or its deadline has passed. This link cannot submit to a later round."
    };
    let outcome=view.submission.as_ref().map(|submission|format!(r#"<p>Decided: <strong>{choice}</strong> at {at}.</p>
        <p class="whitespace-pre-wrap">{feedback}</p><details><summary class="link">Submitted data</summary><pre class="whitespace-pre-wrap">{data}</pre></details>"#,
        choice=escape_html_text(&submission.choice),at=escape_html_text(&submission.submitted_at.to_rfc3339()),
        feedback=escape_html_text(&submission.feedback),data=pretty(&submission.data))).unwrap_or_default();
    let unsent=inputs.map(|inputs|format!(r#"<details open><summary>Your unsubmitted input</summary><pre class="whitespace-pre-wrap text-xs">{}</pre></details>"#,
        pretty(&serde_json::to_value(inputs.values.iter().filter(|(key, _)| matches!(key.as_str(), "choice" | "feedback" | "data") || key.starts_with("data:")).collect::<BTreeMap<_,_>>()).unwrap_or_default()))).unwrap_or_default();
    format!(r#"<section class="space-y-2 text-sm"><p>{message}</p>{outcome}{unsent}</section>"#)
}

fn history(view: &DecisionView) -> String {
    let rows:String=view.history.iter().map(|entry|{
        let round=entry.round.map(|round|format!(" · round {round}")).unwrap_or_default();
        let choice=entry.choice.as_deref().unwrap_or(&entry.state);
        format!(r#"<li class="space-y-1 rounded-box border border-base-300 p-3 text-xs">
            <a class="link" href="/ui/workflow-decisions/{id}">{subject} · {step}{round}{current}</a>
            <p>{choice} · {at}</p><p class="break-all font-mono">Artifact {digest}</p><p class="whitespace-pre-wrap">{feedback}</p></li>"#,
            id=entry.decision.0,subject=entry.subject.label(),step=escape_html_text(entry.step.as_str()),
            current=if entry.decision==view.decision {" (this decision)"}else{""},choice=escape_html_text(choice),
            at=escape_html_text(&entry.created_at.to_rfc3339()),digest=escape_html_text(&entry.artifact_digest),
            feedback=escape_html_text(entry.feedback.as_deref().unwrap_or("")))
    }).collect();
    format!(
        r#"<section class="space-y-2"><h2 class="font-semibold">Draft and decision history</h2>
        <p class="text-xs opacity-70">Exact artifacts and feedback in this run and its sibling revision rounds. Open a decision to inspect its draft.</p>
        <ol class="space-y-2">{rows}</ol>{more}</section>"#,
        more = if view.history_truncated {
            "<p class=\"text-xs\">Showing the newest 40 decisions from up to 20 recent rounds. Older history is omitted.</p>"
        } else {
            ""
        }
    )
}

fn discussion(view: &DecisionView, open: bool, inputs: Option<&DecisionInputs>) -> String {
    let comments:String=view.comments.iter().map(|comment|format!(r#"<li class="rounded-box border border-base-300 p-3 text-sm">
        <p class="whitespace-pre-wrap">{body}</p><p class="text-xs opacity-60">{at} · author {author}</p></li>"#,
        body=escape_html_text(&comment.body),at=escape_html_text(&comment.created_at.to_rfc3339()),author=comment.author_principal)).collect();
    let form = if open {
        format!(
            r##"<form hx-post="/ui/workflow-decisions/{decision}/comments" hx-target="#workflow-decision" hx-swap="outerHTML"
        hx-sync="#decision-shell:queue all" hx-disabled-elt="#workflow-decision button[type='submit']" class="space-y-2">
        <input type="hidden" name="command_key" value="{key}"><label class="fieldset"><span class="fieldset-legend">Comment</span>
        <textarea name="body" rows="2" required maxlength="16384" class="textarea w-full">
{body}</textarea></label>
        <p class="text-xs opacity-70">Add comment leaves the decision open. It does not approve, reject or request a revision.</p>
        <button type="submit" class="btn btn-sm">Add comment</button><span class="hidden text-sm [.htmx-request_&]:inline" role="status">Posting…</span></form>"##,
            decision = view.decision.0,
            key = escape_html_attr(
                inputs
                    .and_then(|inputs| inputs.get("command_key"))
                    .unwrap_or(&Uuid::new_v4().to_string())
            ),
            body = escape_html_text(inputs.and_then(|inputs| inputs.get("body")).unwrap_or(""))
        )
    } else {
        inputs
            .and_then(|inputs| inputs.get("body"))
            .map(|body| {
                format!(
                    r#"<p>Your unposted comment:</p><p class="whitespace-pre-wrap">{}</p>"#,
                    escape_html_text(body)
                )
            })
            .unwrap_or_default()
    };
    format!(
        r#"<section class="space-y-2"><h2 class="font-semibold">Discussion</h2><ul class="space-y-2">{comments}</ul>{form}</section>"#
    )
}

/// A single exact context message; no live-window owner or newest-message scrolling applies.
pub fn workflow_source_message_page(
    message: &crate::entities::message_view::ThreadMessageView,
    viewer_email: &crate::entities::value_objects::EmailAddress,
    scope: MessageScope,
    run_access: RunLinkAccess<'_>,
) -> String {
    base_layout(
        "Source message",
        &format!(
            r#"<main class="mx-auto w-full max-w-3xl space-y-4">
        <h1 class="text-xl font-bold">Source message</h1>
        <p class="break-all text-xs">Message {id}</p>
        <article id="source-message" tabindex="-1" autofocus>{bubble}</article>
        <a class="link" href="/ui?company_id={company}&amp;channel_id={channel}&amp;thread_id={thread}">Open latest conversation</a>
        </main>"#,
            id = message.canonical_id.as_uuid(),
            bubble = message_bubble_chat(message, Some(viewer_email), scope, run_access),
            company = scope.company_id,
            channel = scope.channel_id,
            thread = message.thread_id
        ),
    )
}

#[cfg(test)]
mod tests;
