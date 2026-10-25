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

/// The full-width activity strip shown below an open thread.
pub fn thread_activity_strip(activity: Option<ThreadActivity>) -> String {
    let Some(activity) = activity else {
        return String::new();
    };
    let spinner = if activity.is_running() {
        r##"<span class="loading loading-dots loading-sm"></span>"##
    } else {
        ""
    };
    format!(
        r##"<div class="flex flex-wrap items-center gap-2 border-t border-base-300 px-6 py-2 text-xs opacity-70">{spinner}<span class="badge badge-sm shrink-0 {style}">{label}</span></div>"##,
        style = activity_style(activity),
        label = escape_html_text(activity.label()),
    )
}
