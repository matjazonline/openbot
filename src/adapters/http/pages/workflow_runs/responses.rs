use super::*;
use crate::application::workflow::channel_reply::{
    ChannelResponseOutcome, FinalPublicationState, ResponseDelivery,
};
use crate::entities::message::CanonicalMessageId;
use crate::entities::transport::DeliveryStatus;

fn message_link(
    outcome: &ChannelResponseOutcome,
    company: Uuid,
    message: CanonicalMessageId,
    label: &str,
) -> String {
    format!(
        r#"<a class="link" href="/ui/message?company_id={company}&amp;channel_id={channel}&amp;thread_id={thread}&amp;message_id={message}">{label}</a>"#,
        channel = outcome.channel,
        thread = outcome.thread,
        message = message.as_uuid(),
        label = escape_html_text(label)
    )
}
fn delivery_link(
    delivery: &ResponseDelivery,
    company: Uuid,
    access: RunLinkAccess<'_>,
    run: Uuid,
) -> String {
    let state = escape_html_text(delivery.state.label());
    let caution = match delivery.state {
        DeliveryStatus::OutcomeUnknown => " Outcome unconfirmed; do not resend blindly.",
        DeliveryStatus::Delivered => " Provider acceptance does not confirm that a person read it.",
        DeliveryStatus::Retryable => {
            " Safe retries reuse this message; completed workflow work is not rerun."
        }
        DeliveryStatus::DeadLetter => {
            " Delivery needs attention; earlier accepted parts remain in history."
        }
        DeliveryStatus::Pending | DeliveryStatus::Sending => "",
    };
    let may_inspect = match access {
        RunLinkAccess::Authorized(ids) => ids.contains(&run),
        RunLinkAccess::Inspect => true,
        RunLinkAccess::Label => false,
    };
    let link = if may_inspect {
        format!(
            r#" <a class="link" href="/ui/deliveries?company_id={company}&amp;entry_id={id}">Inspect delivery</a>"#,
            id = delivery.id.as_uuid()
        )
    } else {
        String::new()
    };
    format!("{state}{}{link}", escape_html_text(caution))
}

pub fn channel_response_panel(
    outcome: &ChannelResponseOutcome,
    company: Uuid,
    access: RunLinkAccess<'_>,
) -> String {
    let run = outcome.run.as_uuid();
    let final_response = outcome
        .final_response
        .as_ref()
        .map(|response| {
            let state = match response.publication {
                FinalPublicationState::Suppressed => "Intentionally suppressed",
                FinalPublicationState::Pending | FinalPublicationState::Claimed => {
                    "Publication pending"
                }
                FinalPublicationState::Failed => "Publication needs attention",
                FinalPublicationState::Published => "Published",
            };
            let message = response
                .message
                .map(|message| message_link(outcome, company, message, "Final response"))
                .unwrap_or_default();
            let delivery = response
                .delivery
                .as_ref()
                .map(|delivery| delivery_link(delivery, company, access, run))
                .unwrap_or_else(|| {
                    if response.publication == FinalPublicationState::Published {
                        "Posted locally".into()
                    } else {
                        String::new()
                    }
                });
            format!("<p>Final response: {state} {message} {delivery}</p>")
        })
        .unwrap_or_default();
    let prompt = outcome
        .prompt
        .as_ref()
        .map(|prompt| {
            let state = if prompt.waiting {
                "Human input pending"
            } else {
                "Human wait closed"
            };
            let link = message_link(outcome, company, prompt.message, "Open question");
            let delivery = prompt
                .delivery
                .as_ref()
                .map(|delivery| delivery_link(delivery, company, access, run))
                .unwrap_or_else(|| "Posted locally".into());
            format!(
                "<p>{state}: {link} · {delivery} · deadline {}</p>",
                format_date_time(prompt.deadline)
            )
        })
        .unwrap_or_default();
    format!(
        r#"<section class="space-y-1 rounded-box border border-base-300 p-3 text-sm" data-response-run="{run}" aria-label="Response outcome">
        <p class="font-semibold">{label}</p><p>Execution: {execution} · {chip} · {source}</p>{prompt}{final_response}</section>"#,
        label = escape_html_text(outcome.label()),
        execution = outcome.execution.as_str(),
        chip = run_chip(company, run, access),
        source = message_link(outcome, company, outcome.source, "Original message")
    )
}

pub fn thread_response_outcomes(
    runs: &crate::application::thread_activity::ThreadRuns,
    company: Uuid,
    access: RunLinkAccess<'_>,
) -> String {
    let rows: String = runs
        .response_outcomes
        .iter()
        .map(|outcome| channel_response_panel(outcome, company, access))
        .collect();
    let more = if runs.responses_truncated {
        "<p>Showing the newest 50 response outcomes.</p>"
    } else {
        ""
    };
    if rows.is_empty() && more.is_empty() {
        return String::new();
    }
    format!(
        r#"<div class="space-y-2 px-6 py-2" aria-label="Conversation responses">{rows}{more}</div>"#
    )
}
