//! Administrative escalation decisions share the Decisions shell and current command service.
use super::*;
use crate::application::workflow::escalation::{EscalationSourceView, EscalationView};

pub fn workflow_escalation_page(view: &EscalationView) -> String {
    base_layout(
        "Escalation decision",
        &format!(
            r#"<main id="decision-shell" class="mx-auto w-full max-w-3xl px-4 py-6">{}</main>"#,
            workflow_escalation_section(view, None)
        ),
    )
}

pub fn workflow_escalation_section(
    view: &EscalationView,
    outcome: Option<Result<&str, &str>>,
) -> String {
    let banner = match outcome {
        Some(Ok(message)) => format!(
            r#"<div role="status" class="alert alert-success">{}</div>"#,
            escape_html_text(message)
        ),
        Some(Err(message)) => form_error_banner(Some(message)),
        None => String::new(),
    };
    let source = if view.payload_restricted {
        "Source content is restricted.".into()
    } else {
        match view.source {
            EscalationSourceView::Run { run } => run_chip(
                view.company.as_uuid(),
                run.as_uuid(),
                if view.inspect_run {
                    RunLinkAccess::Inspect
                } else {
                    RunLinkAccess::Label
                },
            ),
            EscalationSourceView::Admission { .. } => {
                "Workflow admission was refused; no run started.".into()
            }
        }
    };
    let recipients = view
        .recipients
        .iter()
        .map(|r| {
            format!(
                "<li>{} · {}</li>",
                escape_html_text(&r.label),
                escape_html_text(&format!("{:?}", r.delivery))
            )
        })
        .collect::<String>();
    let status = view
        .settlement
        .map(|s| format!("{:?}", s))
        .unwrap_or_else(|| format!("{:?}", view.state));
    let deadline = view
        .deadline
        .map(|date| format!("Acknowledgement deadline: {}", date.to_rfc3339()))
        .unwrap_or_else(|| "Manually assigned; the automatic ladder has stopped.".into());
    let current = if view.current_decision != view.decision {
        format!(
            r#"<p><a class="link" href="/ui/workflow-decisions/escalations/{}">View current assignment</a></p>"#,
            view.current_decision.as_uuid()
        )
    } else {
        String::new()
    };
    format!(
        r##"<section id="workflow-escalation" class="space-y-5">
        <header><p class="text-sm opacity-70">Decisions</p><h1 class="text-xl font-bold">Escalation decision</h1>
        <p>{status} · hop {hop}</p></header>{banner}<p>{source}</p>
        <p>Reason: {reason}{step}</p><p class="text-sm">{deadline}</p>
        <section><h2 class="font-semibold">Assigned recipients</h2><ul>{recipients}</ul></section>
        {audit}{current}{forms}</section>"##,
        status = escape_html_text(&status),
        hop = view.hop.get(),
        reason = escape_html_text(view.reason.code.as_str()),
        step = view
            .reason
            .step
            .as_ref()
            .map(|step| format!(" · step {}", escape_html_text(step.as_str())))
            .unwrap_or_default(),
        deadline = escape_html_text(&deadline),
        audit = command_audit(view),
        forms = forms(view)
    )
}

fn command_audit(view: &EscalationView) -> String {
    let Some(command) = &view.command else {
        return String::new();
    };
    let choice = match &command.choice {
        crate::application::workflow::escalation::EscalationChoice::Acknowledge => {
            "Acknowledged".into()
        }
        crate::application::workflow::escalation::EscalationChoice::Reassign { principal } => {
            format!("Reassigned to {principal}")
        }
        crate::application::workflow::escalation::EscalationChoice::Cancel { .. } => {
            "Run cancellation requested".into()
        }
    };
    format!(
        r#"<section aria-label="Command history"><h2 class="font-semibold">Command history</h2><p>{}</p><p class="text-sm">Account {} · principal {} · {}</p></section>"#,
        escape_html_text(&choice),
        command.actor_user,
        command.actor_principal,
        escape_html_text(&command.created_at.to_rfc3339()),
    )
}

fn form(view: &EscalationView, choice: &str, fields: &str, label: &str) -> String {
    format!(
        r##"<form hx-post="/ui/workflow-decisions/escalations/{decision}/submit"
        hx-target="#workflow-escalation" hx-swap="outerHTML" hx-sync="#decision-shell:queue all"
        hx-disabled-elt="#workflow-escalation button[type='submit']" class="space-y-2">
        <input type="hidden" name="command_key" value="{key}"><input type="hidden" name="choice" value="{choice}">
        {fields}<button type="submit" class="btn btn-neutral">{label}</button>
        <span class="hidden text-sm [.htmx-request_&]:inline" role="status">Saving…</span></form>"##,
        decision = view.decision.as_uuid(),
        key = Uuid::new_v4()
    )
}

fn forms(view: &EscalationView) -> String {
    let mut forms = String::new();
    if view.can_acknowledge {
        forms.push_str(&form(view, "acknowledge", "", "Acknowledge"));
    }
    if view.can_reassign {
        let options = view
            .assignment_options
            .iter()
            .map(|option| {
                format!(
                    r#"<option value="{}">{}</option>"#,
                    option.principal.as_uuid(),
                    escape_html_text(&option.label)
                )
            })
            .collect::<String>();
        if !options.is_empty() {
            let field = format!(
                r#"<label class="fieldset"><span class="fieldset-legend">Assign to</span><select name="principal" required class="select w-full">{options}</select></label>"#
            );
            forms.push_str(&form(view, "reassign", &field, "Reassign"));
        }
        if view.assignment_options_truncated {
            forms.push_str("<p>Showing the first 200 principals.</p>");
        }
    }
    if let Some(revision) = view.cancel_revision {
        forms.push_str(&form(
            view,
            "cancel",
            &format!(
                r#"<input type="hidden" name="expected_run_revision" value="{}">"#,
                revision.0
            ),
            "Cancel run",
        ));
    }
    if forms.is_empty() {
        forms.push_str("<p>You cannot submit a new command for this assignment.</p>");
    }
    forms
}
