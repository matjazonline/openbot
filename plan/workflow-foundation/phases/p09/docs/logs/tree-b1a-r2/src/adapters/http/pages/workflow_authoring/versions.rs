//! The versions tab: every immutable version, a line diff between any two of them or the
//! draft, and where each version is in use — which bindings (with their channels and
//! schedules) stay on an older version after a publish.
use super::*;
use crate::application::workflow::RelatedAssociation;
use crate::application::workflow::authoring::{DiffLine, LineDiff};
use crate::application::workflow::reads::{BindingUse, VersionSummary};

/// Unchanged lines kept around each change; longer unchanged runs fold.
const CONTEXT_LINES: usize = 3;

pub struct VersionsView<'a> {
    pub company_id: Uuid,
    pub workflow: Uuid,
    pub draft_revision: u64,
    /// Newest first.
    pub versions: &'a [VersionSummary],
    pub versions_truncated: bool,
    /// The bindings the viewer may see.
    pub usage: &'a [BindingUse],
    pub usage_truncated: bool,
    /// Bindings left out because they are on channels the viewer cannot see.
    pub hidden_bindings: usize,
    pub banner: Option<&'a str>,
    /// A compare already rendered into `#workflow-compare`.
    pub compare_html: Option<&'a str>,
}

fn version_label(versions: &[VersionSummary], version: Uuid) -> String {
    match versions.iter().position(|v| v.version.as_uuid() == version) {
        Some(0) => format!("{} (newest)", short_id(version)),
        Some(_) => format!("{} (older)", short_id(version)),
        None => short_id(version),
    }
}

fn compare_form(view: &VersionsView<'_>) -> String {
    let (company_id, workflow) = (view.company_id, view.workflow);
    let options = |selected: &str| {
        let mut options = format!(
            r#"<option value="draft"{}>Draft (revision {})</option>"#,
            if selected == "draft" { " selected" } else { "" },
            view.draft_revision
        );
        for version in view.versions {
            let id = version.version.as_uuid().to_string();
            options.push_str(&format!(
                r#"<option value="{id}"{selected}>Version {short} · draft revision {revision}</option>"#,
                selected = if selected == id { " selected" } else { "" },
                short = short_id(version.version.as_uuid()),
                revision = version.draft_revision,
            ));
        }
        options
    };
    let newest = view
        .versions
        .first()
        .map(|version| version.version.as_uuid().to_string())
        .unwrap_or_else(|| "draft".to_owned());
    format!(
        r##"<form class="flex flex-wrap items-end gap-2" hx-get="/ui/workflows/{workflow}/compare"
            hx-target="#workflow-compare" hx-swap="innerHTML" hx-sync="#workflow-compare:replace"
            hx-indicator="#workflow-compare-busy">
            <input type="hidden" name="company_id" value="{company_id}">
            <label class="fieldset"><span class="fieldset-legend">From</span><select class="select select-sm" name="from">{from}</select></label>
            <label class="fieldset"><span class="fieldset-legend">To</span><select class="select select-sm" name="to">{to}</select></label>
            <button type="submit" class="btn btn-sm">Compare</button>
            <span id="workflow-compare-busy" class="htmx-indicator text-sm" role="status"><span class="loading loading-spinner loading-xs"></span> Comparing…</span>
        </form>"##,
        from = options(&newest),
        to = options("draft"),
    )
}

fn association_label(binding: &BindingUse) -> String {
    match binding.binding.association {
        RelatedAssociation::Company => "Company-wide (manual starts and schedules)".to_owned(),
        RelatedAssociation::Channel(_) => format!(
            "Channel {}",
            binding.channel_name.as_deref().unwrap_or("(unnamed)")
        ),
        RelatedAssociation::Thread { .. } => format!(
            "One thread of channel {}",
            binding.channel_name.as_deref().unwrap_or("(unnamed)")
        ),
    }
}

fn usage_list(view: &VersionsView<'_>) -> String {
    if view.usage.is_empty() && view.hidden_bindings == 0 {
        return r#"<p class="text-sm opacity-70">No binding uses this workflow yet. A published version runs only once a binding selects it and is activated.</p>"#.to_owned();
    }
    let newest = view.versions.first().map(|version| version.version);
    let rows: String = view
        .usage
        .iter()
        .map(|binding| {
            let on_newest = Some(binding.binding.version) == newest;
            let version = if on_newest {
                r#"<span class="badge badge-sm badge-success">newest version</span>"#.to_owned()
            } else {
                format!(
                    r#"<span class="badge badge-sm badge-warning">stays on version {}</span>"#,
                    escape_html_text(&version_label(view.versions, binding.binding.version.as_uuid()))
                )
            };
            let state = if binding.binding.active {
                r#"<span class="badge badge-sm badge-outline">active</span>"#
            } else {
                r#"<span class="badge badge-sm badge-ghost">paused: admits nothing</span>"#
            };
            let schedules = if binding.schedules.is_empty() {
                String::new()
            } else {
                let names: Vec<String> = binding
                    .schedules
                    .iter()
                    .map(|schedule| {
                        format!(
                            "{}{}",
                            escape_html_text(&schedule.name),
                            if schedule.enabled { "" } else { " (disabled)" }
                        )
                    })
                    .collect();
                format!(
                    r#"<p class="text-xs opacity-70">Schedules: {}{}</p>"#,
                    names.join(", "),
                    if binding.schedules_truncated { ", …" } else { "" }
                )
            };
            format!(
                r#"<li class="py-2"><div class="flex flex-wrap items-center gap-2"><span class="font-semibold">{association}</span>{state}{version}</div><p class="font-mono text-[11px] opacity-60">binding {binding}</p>{schedules}</li>"#,
                association = escape_html_text(&association_label(binding)),
                binding = short_id(binding.binding.binding.as_uuid()),
            )
        })
        .collect();
    let hidden = if view.hidden_bindings > 0 {
        format!(
            r#"<p class="text-xs opacity-70">{} more binding{} on channels you cannot see.</p>"#,
            view.hidden_bindings,
            if view.hidden_bindings == 1 { "" } else { "s" }
        )
    } else {
        String::new()
    };
    let cut = if view.usage_truncated {
        r#"<p class="text-xs text-error">Only the first 200 bindings are listed.</p>"#
    } else {
        ""
    };
    format!(r#"<ul class="divide-y divide-base-300">{rows}</ul>{hidden}{cut}"#)
}

pub fn workflow_versions(view: &VersionsView<'_>) -> String {
    let rows: String = view
        .versions
        .iter()
        .enumerate()
        .map(|(index, version)| {
            format!(
                r#"<tr><td class="font-mono">{id}{newest}</td><td>{revision}</td><td>{created}</td><td class="font-mono text-[11px]">{hash}</td></tr>"#,
                id = short_id(version.version.as_uuid()),
                newest = if index == 0 {
                    r#" <span class="badge badge-xs badge-success">newest</span>"#
                } else {
                    ""
                },
                revision = version.draft_revision,
                created = format_date_time(version.created_at),
                hash = escape_html_text(&version.content_hash[..16.min(version.content_hash.len())]),
            )
        })
        .collect();
    let versions = if view.versions.is_empty() {
        r#"<p class="text-sm opacity-70">Nothing published yet. Validate the draft in the editor, then publish it from the validation preview.</p>"#.to_owned()
    } else {
        format!(
            r#"<div class="overflow-x-auto"><table class="table table-xs"><thead><tr><th>Version</th><th>From draft revision</th><th>Published</th><th>Content</th></tr></thead><tbody>{rows}</tbody></table></div>{cut}"#,
            cut = if view.versions_truncated {
                r#"<p class="text-xs text-error">Only the newest 200 versions are listed.</p>"#
            } else {
                ""
            },
        )
    };
    format!(
        r##"{banner}
        <div class="space-y-6">
            <section class="space-y-2"><h3 class="font-semibold">Published versions</h3>{versions}</section>
            <section class="space-y-2"><h3 class="font-semibold">Compare</h3>{form}<div id="workflow-compare" class="mt-2">{compare}</div></section>
            <section class="space-y-2"><h3 class="font-semibold">Where it runs</h3>
                <p class="text-xs opacity-70">Publishing never moves a binding. A binding keeps the version it was configured with until someone reconfigures it.</p>
                {usage}</section>
        </div>"##,
        banner = view.banner.map(status_banner).unwrap_or_default(),
        form = compare_form(view),
        compare = view.compare_html.unwrap_or_default(),
        usage = usage_list(view),
    )
}

/// A diff as numbered lines, unchanged runs folded to [`CONTEXT_LINES`] around each change.
pub fn diff_block(diff: &LineDiff<'_>, old_label: &str, new_label: &str) -> String {
    if !diff.changed() {
        return r#"<p class="text-sm opacity-70">No differences.</p>"#.to_owned();
    }
    let near_change: Vec<bool> = (0..diff.lines.len())
        .map(|index| {
            let start = index.saturating_sub(CONTEXT_LINES);
            let end = (index + CONTEXT_LINES + 1).min(diff.lines.len());
            diff.lines[start..end]
                .iter()
                .any(|line| !matches!(line, DiffLine::Same(_)))
        })
        .collect();
    let mut body = String::new();
    let mut folded = 0;
    for (line, keep) in diff.lines.iter().zip(&near_change) {
        if !keep {
            folded += 1;
            continue;
        }
        if folded > 0 {
            body.push_str(&format!(
                "<span class=\"block opacity-50\">  … {folded} unchanged line{}</span>",
                if folded == 1 { "" } else { "s" }
            ));
            folded = 0;
        }
        let (class, mark, text) = match line {
            DiffLine::Same(text) => ("", ' ', text),
            DiffLine::Removed(text) => ("bg-error/10", '-', text),
            DiffLine::Added(text) => ("bg-success/10", '+', text),
        };
        body.push_str(&format!(
            "<span class=\"block {class}\">{mark} {}</span>",
            escape_html_text(text)
        ));
    }
    if folded > 0 {
        body.push_str(&format!(
            "<span class=\"block opacity-50\">  … {folded} unchanged line{}</span>",
            if folded == 1 { "" } else { "s" }
        ));
    }
    let approximate = if diff.exact {
        ""
    } else {
        r#"<p class="text-xs opacity-70">These sources differ too much to align line by line; the changed middle is shown as one removed and one added block.</p>"#
    };
    format!(
        r#"<p class="text-xs opacity-70"><span class="font-mono">-</span> {old} · <span class="font-mono">+</span> {new}</p>{approximate}<pre class="max-h-96 overflow-auto rounded-box bg-base-200 p-3 font-mono text-xs">{body}</pre>"#,
        old = escape_html_text(old_label),
        new = escape_html_text(new_label),
    )
}
