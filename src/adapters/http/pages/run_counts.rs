//! One server-rendered snapshot of open runs, copied into slots after SSE and partial-page swaps
//! (p08 D4.4).
use crate::entities::run_counts::{RunCountSnapshot, RunCounts};
use std::fmt;
use uuid::Uuid;

pub const RUN_COUNTS_EVENT: &str = "run-counts";

#[derive(Clone, Copy)]
pub enum RunCountKey {
    Company,
    Channel(Uuid),
}

impl fmt::Display for RunCountKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Company => formatter.write_str("company"),
            Self::Channel(id) => write!(formatter, "channel:{id}"),
        }
    }
}

pub fn run_counts_source(company_id: Uuid) -> String {
    format!(
        r#"<div hidden data-run-counts-source hx-ext="sse" sse-connect="/ui/run-counts/events?company_id={company_id}" sse-swap="{RUN_COUNTS_EVENT}" hx-swap="innerHTML"></div>"#
    )
}

pub fn run_counts_slot(key: RunCountKey) -> String {
    format!(
        r#"<span class="inline-flex flex-wrap items-center gap-1" data-run-counts="{key}"></span>"#
    )
}

pub(super) fn run_counts_bar(company_id: Uuid) -> String {
    format!(
        r#"<div class="flex flex-wrap items-center gap-2 px-4 pb-3 text-xs"><span>Open runs</span>{}{}</div>"#,
        run_counts_slot(RunCountKey::Company),
        run_counts_source(company_id)
    )
}

pub fn run_count_badges(counts: &RunCounts) -> String {
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

fn template(key: RunCountKey, counts: &RunCounts) -> String {
    let content = if matches!(key, RunCountKey::Company) && counts.is_empty() {
        "No open runs".to_owned()
    } else {
        run_count_badges(counts)
    };
    format!(r#"<template data-run-counts-key="{key}">{content}</template>"#)
}

pub fn run_counts_fragment(snapshot: &RunCountSnapshot) -> String {
    let mut html = template(RunCountKey::Company, &snapshot.company);
    for (id, counts) in &snapshot.channels {
        if !counts.is_empty() {
            html.push_str(&template(RunCountKey::Channel(*id), counts));
        }
    }
    html
}

pub(crate) const RUN_COUNTS_SCRIPT: &str = r#"
(function () {
    function applyRunCounts(event) {
        var source = document.querySelector('[data-run-counts-source]');
        if (!source || !source.querySelector('template[data-run-counts-key="company"]')) return;
        var root = event.detail && event.detail.elt || event.target;
        if (!root || root === source || source.contains(root)) root = document;
        var templates = new Map();
        source.querySelectorAll('template[data-run-counts-key]').forEach(function (template) {
            templates.set(template.dataset.runCountsKey, template);
        });
        function apply(slot) {
            var template = templates.get(slot.dataset.runCounts);
            slot.replaceChildren(...(template ? [template.content.cloneNode(true)] : []));
        }
        if (root.matches && root.matches('[data-run-counts]')) apply(root);
        if (root.querySelectorAll) root.querySelectorAll('[data-run-counts]').forEach(apply);
    }
    document.addEventListener('htmx:afterSettle', applyRunCounts);
    document.addEventListener('htmx:oobAfterSwap', applyRunCounts);
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_protocol_renders_typed_keys_and_only_nonzero_badges() {
        let id = Uuid::new_v4();
        assert!(run_counts_slot(RunCountKey::Channel(id)).contains(&format!("channel:{id}")));
        assert!(run_counts_fragment(&RunCountSnapshot::default()).contains("No open runs"));
        let badges = run_count_badges(&RunCounts {
            active: 3,
            ..RunCounts::default()
        });
        assert!(badges.contains("3 active</span>"));
        assert!(!badges.contains("0 pending</span>"));
        let source = run_counts_source(id);
        assert!(source.contains("sse-connect=\"/ui/run-counts/events?company_id="));
        assert!(source.contains("sse-swap=\"run-counts\""));
        assert!(run_counts_bar(id).contains("Open runs"));
    }
}
