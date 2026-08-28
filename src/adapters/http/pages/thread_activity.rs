use super::*;

/// A compact activity mark for a thread row.
pub fn thread_activity_mark(activity: Option<ThreadActivity>) -> String {
    match activity {
        None => String::new(),
        Some(activity) => format!(
            r##"<span class="shrink-0 leading-none {tint}{pulse}" title="{label}">{mark}</span>"##,
            tint = if activity == ThreadActivity::Failed {
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
        ThreadActivity::WaitingApproval => Icon::Hourglass,
        ThreadActivity::WaitingReply => Icon::Mail,
        ThreadActivity::Failed => Icon::Alert,
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

    let spinner = if activity == ThreadActivity::Working {
        r##"<span class="loading loading-dots loading-sm"></span>"##
    } else {
        ""
    };

    format!(
        r##"<div class="flex items-center gap-2 border-t border-base-300 px-6 py-2 text-xs opacity-70">{spinner}<span class="badge badge-sm shrink-0 {style}">{label}</span></div>"##,
        style = task_status_style(activity.task_status()),
        label = escape_html_text(activity.label()),
    )
}

/// Text badge used by schedule-run rows, including the completed/idle state.
pub fn schedule_run_activity_badge(activity: Option<ThreadActivity>) -> String {
    let Some(activity) = activity else {
        return r#"<span class="badge badge-ghost badge-xs opacity-60">Done</span>"#.into();
    };

    let spinner = if activity == ThreadActivity::Working {
        r#"<span class="loading loading-spinner loading-xs"></span>"#
    } else {
        ""
    };
    let label = match activity {
        ThreadActivity::Working => "Running",
        ThreadActivity::Queued => "Queued",
        ThreadActivity::WaitingApproval => "Waiting for approval",
        ThreadActivity::WaitingReply => "Waiting for reply",
        ThreadActivity::Failed => "Failed",
    };

    format!(
        r#"<span class="badge badge-xs flex items-center gap-1 {style}">{spinner}{label}</span>"#,
        style = task_status_style(activity.task_status()),
    )
}
