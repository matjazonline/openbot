//! One server-rendered snapshot, copied into slots after SSE and partial-page swaps.
use crate::entities::task_counts::{TaskCountSnapshot, TaskCounts};
use std::fmt;
use uuid::Uuid;

pub const TASK_COUNTS_EVENT: &str = "task-counts";

#[derive(Clone, Copy)]
pub enum TaskCountKey {
    Company,
    Channel(Uuid),
    Agent(Uuid),
}

impl fmt::Display for TaskCountKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Company => formatter.write_str("company"),
            Self::Channel(id) => write!(formatter, "channel:{id}"),
            Self::Agent(id) => write!(formatter, "agent:{id}"),
        }
    }
}

pub fn task_counts_source(company_id: Uuid) -> String {
    format!(
        r#"<div hidden data-task-counts-source hx-ext="sse" sse-connect="/ui/task-counts/events?company_id={company_id}" sse-swap="{TASK_COUNTS_EVENT}" hx-swap="innerHTML"></div>"#
    )
}

pub fn task_counts_slot(key: TaskCountKey) -> String {
    format!(
        r#"<span class="inline-flex flex-wrap items-center gap-1" data-task-counts="{key}"></span>"#
    )
}

pub(super) fn task_counts_bar(company_id: Uuid) -> String {
    format!(
        r#"<div class="flex flex-wrap items-center gap-2 px-4 pb-3 text-xs"><span>Open tasks</span>{}{}</div>"#,
        task_counts_slot(TaskCountKey::Company),
        task_counts_source(company_id)
    )
}

pub fn task_count_badges(counts: &TaskCounts) -> String {
    if counts.is_empty() {
        return String::new();
    }
    let badges: String = [
        (counts.pending, "pending", "badge-ghost"),
        (counts.active, "active", "badge-info"),
        (counts.waiting, "waiting", "badge-warning"),
    ]
    .into_iter()
    .filter(|(count, _, _)| *count > 0)
    .map(|(count, label, badge_class)| {
        format!(r#"<span class="badge badge-sm shrink-0 whitespace-nowrap {badge_class}">{count} {label}</span>"#)
    })
    .collect();
    let label = format!(
        "{} pending, {} active, {} waiting",
        counts.pending, counts.active, counts.waiting
    );
    format!(
        r#"<span class="inline-flex flex-wrap gap-1" aria-label="{label}" title="{label}">{badges}</span>"#
    )
}

fn template(key: TaskCountKey, counts: &TaskCounts) -> String {
    let content = if matches!(key, TaskCountKey::Company) && counts.is_empty() {
        "No open tasks".to_owned()
    } else {
        task_count_badges(counts)
    };
    format!(r#"<template data-task-counts-key="{key}">{content}</template>"#)
}

pub fn task_counts_fragment(snapshot: &TaskCountSnapshot) -> String {
    let mut html = template(TaskCountKey::Company, &snapshot.company);
    for (id, counts) in &snapshot.channels {
        if !counts.is_empty() {
            html.push_str(&template(TaskCountKey::Channel(*id), counts));
        }
    }
    for (id, counts) in &snapshot.agents {
        if !counts.is_empty() {
            html.push_str(&template(TaskCountKey::Agent(*id), counts));
        }
    }
    html
}

pub(crate) const TASK_COUNTS_SCRIPT: &str = r#"
(function () {
    function applyTaskCounts(event) {
        var source = document.querySelector('[data-task-counts-source]');
        if (!source || !source.querySelector('template[data-task-counts-key="company"]')) return;
        var root = event.detail && event.detail.elt || event.target;
        if (!root || root === source || source.contains(root)) root = document;
        var templates = new Map();
        source.querySelectorAll('template[data-task-counts-key]').forEach(function (template) {
            templates.set(template.dataset.taskCountsKey, template);
        });
        function apply(slot) {
            var template = templates.get(slot.dataset.taskCounts);
            slot.replaceChildren(...(template ? [template.content.cloneNode(true)] : []));
        }
        if (root.matches && root.matches('[data-task-counts]')) apply(root);
        if (root.querySelectorAll) root.querySelectorAll('[data-task-counts]').forEach(apply);
    }
    document.addEventListener('htmx:afterSettle', applyTaskCounts);
    document.addEventListener('htmx:oobAfterSwap', applyTaskCounts);
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_protocol_renders_typed_keys_and_only_nonzero_badges() {
        let id = Uuid::new_v4();
        assert!(task_counts_slot(TaskCountKey::Agent(id)).contains(&format!("agent:{id}")));
        assert!(task_counts_slot(TaskCountKey::Channel(id)).contains(&format!("channel:{id}")));
        assert!(task_counts_fragment(&TaskCountSnapshot::default()).contains("No open tasks"));
        let badges = task_count_badges(&TaskCounts {
            active: 3,
            ..TaskCounts::default()
        });
        assert!(badges.contains("3 active</span>"));
        assert!(!badges.contains("0 pending</span>"));
        let source = task_counts_source(id);
        assert!(source.contains("sse-connect=\"/ui/task-counts/events?company_id="));
        assert!(source.contains("sse-swap=\"task-counts\""));
    }
}
