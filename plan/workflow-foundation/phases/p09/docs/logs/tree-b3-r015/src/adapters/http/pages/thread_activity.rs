//! A thread's run activity in the mailbox (p08 D4.6): a mark on its row and a strip under it.
use super::*;

/// A compact activity mark for a thread row.
pub fn thread_activity_mark(activity: Option<ThreadActivity>) -> String {
    match activity {
        None => String::new(),
        Some(activity) => format!(
            r##"<span class="shrink-0 leading-none {tint}{pulse}" title="{label}">{mark}</span>"##,
            tint = if activity.needs_someone() {
                "text-error"
            } else {
                "opacity-60"
            },
            pulse = if activity.is_running() {
                " animate-pulse"
            } else {
                ""
            },
            label = escape_html_text(activity.label()),
            mark = icon(activity_icon(activity), BUTTON_ICON),
        ),
    }
}

fn activity_icon(activity: ThreadActivity) -> Icon {
    match activity {
        ThreadActivity::Queued | ThreadActivity::Working => Icon::DotFill,
        ThreadActivity::AwaitingReview | ThreadActivity::Waiting => Icon::Hourglass,
        ThreadActivity::NeedsAttention | ThreadActivity::Failed => Icon::Alert,
    }
}

/// The badge colour for an activity, shared by the strip and nothing else.
fn activity_style(activity: ThreadActivity) -> &'static str {
    match activity {
        ThreadActivity::Queued => "badge-warning",
        ThreadActivity::Working => "badge-info animate-pulse",
        ThreadActivity::AwaitingReview => "badge-accent",
        ThreadActivity::NeedsAttention => "badge-error badge-outline",
        ThreadActivity::Waiting => "badge-info badge-outline",
        ThreadActivity::Failed => "badge-error",
    }
}

/// An independently replaceable activity mark within a live thread row.
pub fn thread_activity_slot(thread_id: Uuid, activity: Option<ThreadActivity>) -> String {
    format!(
        r##"<span class="thread-activity" sse-swap="{event}" hx-target="this" hx-swap="innerHTML">{mark}</span>"##,
        event = thread_activity_event(thread_id),
        mark = thread_activity_mark(activity),
    )
}

/// The SSE event name carrying one thread's activity.
pub fn thread_activity_event(thread_id: Uuid) -> String {
    format!("activity-{thread_id}")
}

/// Every unfinished or failed run remains independently identifiable. The live stream replaces
/// only this strip, leaving its stream owner and the message/composer state mounted.
pub fn thread_activity_strip(
    runs: &crate::application::thread_activity::ThreadRuns,
    company_id: Uuid,
    access: RunLinkAccess<'_>,
) -> String {
    if runs.items.is_empty() {
        return String::new();
    }
    let entries: String = runs.items.iter().map(|run| {
        let spinner = if run.activity.is_running() {
            r#"<span class="loading loading-dots loading-sm" aria-hidden="true"></span>"#
        } else { "" };
        format!(r#"<li class="flex flex-wrap items-center gap-2" data-run-id="{id}">{spinner}{chip}<span>{title}</span><span class="badge badge-sm {style}">{label}</span></li>"#,
            id=run.run_id, chip=run_chip(company_id, run.run_id, access),
            title=escape_html_text(&run.title), style=activity_style(run.activity), label=escape_html_text(run.activity.label()))
    }).collect();
    let more = if runs.has_more {
        "<li>Showing the newest 50 active or failed runs. Older runs are not shown.</li>"
    } else {
        ""
    };
    format!(
        r#"<ul aria-label="Thread runs" class="space-y-1 border-t border-base-300 px-6 py-2 text-xs">{entries}{more}</ul>"#
    )
}

/// The live DOM retains the same newest-N window as initial reads and attribution reconciliation.
/// Trimming only message bubbles preserves the stream owner and unsent composer/correction state
/// on retained messages. Keep a retained anchor stationary even with browser scroll anchoring.
pub const THREAD_HISTORY_SCRIPT: &str = r#"
document.addEventListener('htmx:sseMessage', function (event) {
    var pane = event.target;
    if (!pane || pane.id !== 'message-scroll') return;
    var limit = Number(pane.dataset.historyLimit);
    if (!Number.isInteger(limit) || limit < 1) return;
    var bubbles = pane.querySelectorAll(':scope > .chat[data-message-id]');
    var excess = bubbles.length - limit;
    if (excess <= 0) return;
    var anchor = bubbles[excess];
    var before = anchor.getBoundingClientRect().top;
    for (var index = 0; index < excess; index++) bubbles[index].remove();
    pane.scrollTop += anchor.getBoundingClientRect().top - before;
});
"#;
