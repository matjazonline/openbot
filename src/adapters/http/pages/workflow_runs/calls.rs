//! The "Desk calls" section and the "Called by" line (gate AWC-P05-D2 r2, D1): ids, phases,
//! codes and deadlines only. Every reference to another run, a desk or a responder is drawn
//! only when [`CallAccess`] says the viewer may open or see it.
use super::*;
use crate::application::workflow::channel_call_views::{
    CallPhase, ChannelCallItem, ChannelCallOrigin, ChannelCallPage,
};
use std::collections::HashSet;

/// What the viewer may follow from a call: runs they may open, desks they may see, desks whose
/// response settings (and so responder) they may read, and final-review decisions they may
/// open. Empty draws labels only.
#[derive(Debug, Default)]
pub struct CallAccess {
    pub runs: HashSet<Uuid>,
    pub desks: HashSet<Uuid>,
    pub responders: HashSet<Uuid>,
    pub decisions: HashSet<Uuid>,
}

fn desk(access: &CallAccess, desk: Uuid, name: &str) -> String {
    if access.desks.contains(&desk) && !name.is_empty() {
        format!(
            r#"desk <span class="font-semibold">{}</span>"#,
            escape_html_text(name)
        )
    } else {
        "a desk".into()
    }
}

fn phase(item: &ChannelCallItem, access: &CallAccess) -> String {
    let review = match &item.phase {
        // Linked only when the viewer may open the decision itself (AWC-P05-B2-F1); the
        // decision page reauthorizes.
        CallPhase::AwaitingFinalReview { decision } if access.decisions.contains(decision) => {
            format!(r#" · <a class="link" href="/ui/workflow-decisions/{decision}">review</a>"#)
        }
        _ => String::new(),
    };
    format!(
        r#"<span data-call-phase>{}</span>{review}"#,
        escape_html_text(&item.phase.label())
    )
}

fn item(company_id: Uuid, run: Uuid, item: &ChannelCallItem, access: &CallAccess) -> String {
    let responder = if access.responders.contains(&item.desk) {
        format!(
            r#" · responder <span class="font-mono">{}</span> revision {}"#,
            short_id(item.responder),
            item.responder_revision
        )
    } else {
        String::new()
    };
    format!(
        r#"<li class="py-1 text-sm" data-call="{call}">Call <span class="font-mono">{short}</span> to {desk} · {phase}
            <p class="text-xs opacity-70">Child {child} · own scope <span class="font-mono">{scope}</span> · <a class="link" href="/ui/workflow-runs/{run}/steps/{execution}?company_id={company_id}">step</a> · deadline {deadline}{past}{responder}</p></li>"#,
        call = item.call,
        short = short_id(item.call),
        desk = desk(access, item.desk, &item.desk_name),
        phase = phase(item, access),
        child = run_chip(
            company_id,
            item.child.as_uuid(),
            RunLinkAccess::Authorized(&access.runs)
        ),
        scope = short_id(item.scope_manifest),
        execution = item.execution,
        deadline = format_date_time(item.deadline),
        past = if item.past_deadline {
            r#" <span class="badge badge-sm badge-warning">past deadline</span>"#
        } else {
            ""
        },
    )
}

/// The calls a run (or one of its steps) made, newest first.
pub fn channel_calls(
    company_id: Uuid,
    run: Uuid,
    page: &ChannelCallPage,
    access: &CallAccess,
) -> String {
    if page.items.is_empty() {
        return String::new();
    }
    let rows: String = page
        .items
        .iter()
        .map(|call| item(company_id, run, call, access))
        .collect();
    format!(
        r#"<section class="space-y-2" aria-labelledby="run-channel-calls"><h3 id="run-channel-calls" class="font-semibold">Desk calls</h3><ul>{rows}</ul>{cut}</section>"#,
        cut = if page.truncated {
            r#"<p class="text-xs opacity-70">Only the newest 50 calls are listed.</p>"#
        } else {
            ""
        },
    )
}

/// The call this run answers, on a call child's page.
pub fn called_by(company_id: Uuid, origin: &ChannelCallOrigin, access: &CallAccess) -> String {
    format!(
        r#"<p class="text-sm" data-called-by>Called by {caller} through {desk} (call <span class="font-mono">{call}</span>). This run has its own dependency scope.</p>"#,
        caller = run_chip(
            company_id,
            origin.caller.as_uuid(),
            RunLinkAccess::Authorized(&access.runs)
        ),
        desk = desk(access, origin.desk, &origin.desk_name),
        call = short_id(origin.call),
    )
}

#[cfg(test)]
#[path = "calls_tests.rs"]
mod tests;
