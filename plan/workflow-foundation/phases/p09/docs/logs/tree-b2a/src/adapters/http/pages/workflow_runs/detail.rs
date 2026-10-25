//! One run in `#run-pane`: what it is and where it came from (binding, version, triggering
//! message, lineage, rerun), its controls, its graph with progress, a chronological timeline,
//! its steps (each expanding to what it froze and produced), waits, actions and children.
use super::*;
use crate::application::workflow::RelatedAssociation;
use crate::application::workflow::run_views::RunView;
use chrono::{DateTime, Utc};

/// The most JSON text one block shows; a longer value says it was cut.
pub const MAX_JSON_PREVIEW_BYTES: usize = 32 * 1024;

/// A JSON value as an escaped block, cut at [`MAX_JSON_PREVIEW_BYTES`].
pub fn json_preview(value: &serde_json::Value) -> String {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_default();
    let cut = text.len() > MAX_JSON_PREVIEW_BYTES;
    if cut {
        let mut end = MAX_JSON_PREVIEW_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    format!(
        r#"<pre class="max-h-96 overflow-auto rounded-box bg-base-200 p-3 font-mono text-xs">{}</pre>{}"#,
        escape_html_text(&text),
        if cut {
            r#"<p class="text-xs opacity-70">Cut at 32 KiB for display.</p>"#
        } else {
            ""
        }
    )
}

/// What a control or annotation just did, above the pane it re-rendered.
pub enum RunBanner<'a> {
    Done(&'a str),
    Refused(&'a str),
}

pub struct RunPaneView<'a> {
    pub company_id: Uuid,
    pub run: &'a RunView,
    /// The graph with progress marks, rendered by the route from the run's own bundle.
    pub graph_html: &'a str,
    /// User names by id, for the actors of controls and history.
    pub people: &'a HashMap<Uuid, String>,
    pub banner: Option<RunBanner<'a>>,
}

fn person(people: &HashMap<Uuid, String>, user: Uuid) -> String {
    people
        .get(&user)
        .cloned()
        .unwrap_or_else(|| format!("user {}", short_id(user)))
}

fn run_link(company_id: Uuid, run: Uuid) -> String {
    format!(
        r##"<a class="link font-mono" href="/ui/workflow-runs?company_id={company_id}&amp;run_id={run}"
            hx-get="/ui/workflow-runs/{run}?company_id={company_id}" hx-target="#run-pane" hx-swap="outerHTML"
            hx-push-url="/ui/workflow-runs?company_id={company_id}&amp;run_id={run}">{short}</a>"##,
        short = short_id(run)
    )
}

/// Whether a run in `state` has ended: nothing in it runs again.
pub fn run_ended(state: &str) -> bool {
    matches!(state, "succeeded" | "failed" | "cancelled")
}

/// The pane: what a control just did, above the run's body. A wake-up reloads only the body, so
/// that answer stays until another run is opened.
pub fn run_pane(view: &RunPaneView<'_>) -> String {
    let banner = match &view.banner {
        Some(RunBanner::Done(message)) => status_banner(message),
        Some(RunBanner::Refused(message)) => format!(
            r#"<div role="alert" class="alert alert-error mb-4 text-sm">{}</div>"#,
            escape_html_text(message)
        ),
        None => String::new(),
    };
    let banner = if banner.is_empty() {
        banner
    } else {
        format!(r#"<div class="border-b border-base-300 px-4 pt-4 sm:px-6">{banner}</div>"#)
    };
    format!(
        r#"<section id="run-pane" class="ui-pane-detail flex min-w-0 flex-1 flex-col bg-base-100">{banner}{body}</section>"#,
        body = run_body(view),
    )
}

/// The run itself, reloaded in place on the stream's `run-<id>` and `resync` wake-ups and, while
/// the run is unfinished, on its periodic `tick` (step progress inside a run does not notify).
/// Finished steps keep their place, and an opened one stays open, across those reloads.
///
/// The reload is issued by an empty leaf syncing on `#run-body`, never by the body itself: htmx
/// inherits `hx-sync`, so on the body it would put the controls' writes in the reloads' replace
/// group, and a wake-up would abort an accepted write. Opening another run syncs on `#run-pane`,
/// so a wake-up never cancels it either.
pub fn run_body(view: &RunPaneView<'_>) -> String {
    let run = view.run;
    let summary = &run.detail.summary;
    let (company_id, id) = (view.company_id, summary.run.as_uuid());
    format!(
        r##"<div id="run-body" class="flex min-h-0 flex-1 flex-col">
            <span hidden hx-get="/ui/workflow-runs/{id}?company_id={company_id}&amp;part=body" hx-trigger="sse:run-{id}, sse:resync{tick}"
                hx-target="#run-body" hx-swap="outerHTML" hx-sync="#run-body:replace"></span>
            <div class="border-b border-base-300 px-4 py-4 sm:px-6">
                <div class="flex flex-wrap items-center gap-2"><h2 class="min-w-0 truncate text-xl font-bold">{title}</h2>{state}</div>
                <p class="mt-1 text-xs opacity-70">Run <span class="font-mono">{short}</span> · revision {revision} · started {created} · deadline {deadline}</p>
            </div>
            <div class="flex-1 space-y-6 overflow-y-auto px-4 py-4 sm:px-6">
                {facts}
                {controls}
                <section class="space-y-2"><h3 class="font-semibold">Graph</h3>{graph}</section>
                {timeline}
                {steps}
                {waits}
                {actions}
                {children}
                <section class="space-y-2"><details><summary class="cursor-pointer font-semibold">Input and parameters</summary>
                    <p class="mt-2 text-xs opacity-70">Input</p>{input}<p class="text-xs opacity-70">Parameters</p>{params}</details></section>
            </div>
        </div>"##,
        tick = if run_ended(&summary.state) {
            ""
        } else {
            ", sse:tick"
        },
        title = escape_html_text(&run.workflow_title),
        state = run_state_badge(&summary.state, summary.waiting_reason.as_deref()),
        short = short_id(id),
        revision = summary.revision,
        created = format_date_time(summary.created_at),
        deadline = format_date_time(summary.deadline),
        facts = facts(view),
        controls = controls(view),
        graph = view.graph_html,
        timeline = timeline(view),
        steps = steps(view),
        waits = waits(view),
        actions = actions(view),
        children = children(view),
        input = json_preview(&run.input),
        params = json_preview(&run.params),
    )
}

fn facts(view: &RunPaneView<'_>) -> String {
    let run = view.run;
    let summary = &run.detail.summary;
    let company_id = view.company_id;
    let workflow = summary.workflow.as_uuid();
    let association = match summary.association {
        RelatedAssociation::Company => "Company-wide".to_owned(),
        RelatedAssociation::Channel(_) => format!(
            "Channel {}",
            run.channel_name.as_deref().unwrap_or("(unnamed)")
        ),
        RelatedAssociation::Thread { .. } => format!(
            "A thread of channel {}",
            run.channel_name.as_deref().unwrap_or("(unnamed)")
        ),
    };
    let thread = match (summary.association, run.trigger_message) {
        (
            RelatedAssociation::Thread {
                channel_id,
                thread_id,
            },
            message,
        ) => format!(
            r#"<li>Conversation: <a class="link" href="/ui?company_id={company_id}&amp;channel_id={channel}&amp;thread_id={thread}">open the thread</a>{message}</li>"#,
            channel = channel_id.as_uuid(),
            thread = thread_id.as_uuid(),
            message = message
                .map(|message| format!(
                    r#" · started by message <span class="font-mono">{}</span>"#,
                    short_id(message)
                ))
                .unwrap_or_default(),
        ),
        (_, Some(message)) => format!(
            r#"<li>Started by message <span class="font-mono">{}</span></li>"#,
            short_id(message)
        ),
        _ => String::new(),
    };
    let lineage = match &run.lineage {
        None => String::new(),
        Some(lineage) => format!(
            r#"<li>Child of run {parent} (step <span class="font-mono">{step}</span>{round}{call}); it is controlled through its parent.</li>"#,
            parent = run_link(company_id, lineage.parent.as_uuid()),
            step = escape_html_text(lineage.parent_step.as_deref().unwrap_or("?")),
            round = lineage
                .round
                .map(|round| format!(", round {round}"))
                .unwrap_or_default(),
            call = lineage
                .tool_call
                .map(|call| format!(
                    r#", tool call <span class="font-mono">{}</span>"#,
                    short_id(call)
                ))
                .unwrap_or_default(),
        ),
    };
    let rerun = summary
        .rerun_of
        .map(|original| {
            format!(
                "<li>A rerun of {}: a new run with its own history, not a retry of that one.</li>",
                run_link(company_id, original.as_uuid())
            )
        })
        .unwrap_or_default();
    format!(
        r#"<ul class="space-y-1 text-sm">
            <li>Workflow <a class="link" href="/ui/workflows?company_id={company_id}&amp;workflow_id={workflow}&amp;tab=versions">{title}</a>, version <span class="font-mono">{version}</span> (draft revision {draft}), frozen when the run started.</li>
            <li>Binding <a class="link font-mono" href="/ui/workflows?company_id={company_id}&amp;workflow_id={workflow}&amp;tab=bindings">{binding}</a> · {association} · started by {actor}</li>
            {thread}{lineage}{rerun}
        </ul>"#,
        title = escape_html_text(&run.workflow_title),
        version = short_id(summary.version.as_uuid()),
        draft = run.draft_revision,
        binding = short_id(summary.binding.as_uuid()),
        association = escape_html_text(&association),
        actor = escape_html_text(&person(view.people, summary.actor)),
    )
}

fn controls(view: &RunPaneView<'_>) -> String {
    let run = view.run;
    let summary = &run.detail.summary;
    let (company_id, id) = (view.company_id, summary.run.as_uuid());
    if let Some(lineage) = &run.lineage {
        return format!(
            r#"<section class="space-y-2"><h3 class="font-semibold">Controls</h3><p class="text-sm opacity-70">A child run is cancelled or retried through its parent, run {}.</p></section>"#,
            run_link(company_id, lineage.parent.as_uuid())
        );
    }
    let accepted = run
        .detail
        .actions
        .iter()
        .filter(|action| action.has_receipt)
        .count();
    let form = |action: &str, label: &str, class: &str, confirm: Option<String>| {
        format!(
            r##"<form hx-post="/ui/workflow-runs/{id}/{action}?company_id={company_id}" hx-target="#run-pane" hx-swap="outerHTML"
                hx-disabled-elt="find button"{confirm}>
                <input type="hidden" name="expected_revision" value="{revision}">
                <input type="hidden" name="command_key" value="{key}">
                <button type="submit" class="btn btn-sm {class}">{label}</button></form>"##,
            confirm = confirm
                .map(|text| format!(r#" hx-confirm="{}""#, escape_html_attr(&text)))
                .unwrap_or_default(),
            revision = summary.revision,
            key = Uuid::new_v4(),
        )
    };
    let mut buttons = Vec::new();
    let mut notes = Vec::new();
    if !run_ended(&summary.state) {
        let effects = if accepted == 0 {
            "No effect of this run has been accepted yet.".to_owned()
        } else {
            format!(
                "{accepted} action{} of this run {} already accepted (listed under Actions); cancelling does not undo {}.",
                if accepted == 1 { "" } else { "s" },
                if accepted == 1 { "was" } else { "were" },
                if accepted == 1 { "it" } else { "them" },
            )
        };
        buttons.push(form(
            "cancel",
            "Cancel run",
            "btn-error",
            Some(format!(
                "Cancel this run? Steps not yet started will not run. {effects}"
            )),
        ));
        notes.push(effects);
    }
    if summary.state == "failed" {
        buttons.push(form("retry", "Retry failed step", "btn-outline", None));
        notes.push(
            "Retry resumes this run at its failed step, only when repeating that step is known to be safe; it is the same run, not a new revision."
                .to_owned(),
        );
    }
    if run_ended(&summary.state) {
        buttons.push(form("rerun", "Rerun", "btn-outline", None));
        notes.push("Rerun starts a new run with this run's input, on the same binding.".to_owned());
    }
    if summary.state == "waiting" && summary.waiting_reason.as_deref() == Some("reconciliation") {
        notes.push(
            "This run waits on reconciliation. Recording what happened is an annotation: it never resumes the run. To recover, cancel it and rerun."
                .to_owned(),
        );
    }
    if !buttons.is_empty() {
        notes.push(
            "If a request gets no answer, press it again: it repeats the same request, which is safe."
                .to_owned(),
        );
    }
    let notes: String = notes
        .iter()
        .map(|note| format!(r#"<li>{}</li>"#, escape_html_text(note)))
        .collect();
    format!(
        r#"<section class="space-y-2"><h3 class="font-semibold">Controls</h3>
            <div class="flex flex-wrap gap-2">{buttons}</div>
            <ul class="list-disc space-y-1 pl-5 text-xs opacity-80">{notes}</ul>
        </section>"#,
        buttons = buttons.join(""),
    )
}

/// One line of the timeline.
struct Entry {
    at: DateTime<Utc>,
    text: String,
}

fn timeline(view: &RunPaneView<'_>) -> String {
    let run = view.run;
    let summary = &run.detail.summary;
    let mut entries = vec![Entry {
        at: summary.created_at,
        text: "Run created".to_owned(),
    }];
    for execution in &run.detail.executions {
        if let Some(at) = execution.activated_at {
            entries.push(Entry {
                at,
                text: format!(
                    "Step {} started (activation {})",
                    execution.step, execution.activation
                ),
            });
        }
        if let Some(at) = execution.completed_at {
            entries.push(Entry {
                at,
                text: format!(
                    "Step {} finished{}",
                    execution.step,
                    execution
                        .route
                        .as_deref()
                        .map(|route| format!(" → {route}"))
                        .unwrap_or_default()
                ),
            });
        }
    }
    for action in &run.detail.actions {
        entries.push(Entry {
            at: action.created_at,
            text: format!(
                "{} action dispatched (attempt {}){}",
                action.effect_kind,
                action.attempt,
                if action.has_receipt { "; accepted" } else { "" }
            ),
        });
    }
    for control in &run.controls {
        entries.push(Entry {
            at: control.created_at,
            text: format!(
                "{} by {}: {} (revision {} → {})",
                control.operation,
                person(view.people, control.actor),
                control.result,
                control.expected_revision,
                control.result_revision
            ),
        });
    }
    for reconciliation in &run.reconciliations {
        entries.push(Entry {
            at: reconciliation.observed_at,
            text: format!(
                "Reconciliation needed for a {} action ({})",
                reconciliation.effect_kind, reconciliation.failure_code
            ),
        });
    }
    for event in &run.events {
        entries.push(Entry {
            at: event.created_at,
            text: format!(
                "{}{} by {}",
                event.kind.replace('_', " "),
                event
                    .terminal_state
                    .as_deref()
                    .map(|state| format!(" ({state})"))
                    .unwrap_or_default(),
                person(view.people, event.actor)
            ),
        });
    }
    entries.sort_by_key(|entry| entry.at);
    let rows: String = entries
        .iter()
        .map(|entry| {
            format!(
                r#"<li class="flex gap-3 py-1"><span class="w-40 shrink-0 text-xs opacity-70">{at}</span><span>{text}</span></li>"#,
                at = format_date_time(entry.at),
                text = escape_html_text(&entry.text),
            )
        })
        .collect();
    let cut = run.detail.executions_truncated
        || run.detail.actions_truncated
        || run.controls_truncated
        || run.reconciliations_truncated
        || run.events_truncated;
    format!(
        r#"<section class="space-y-2"><h3 class="font-semibold">Timeline</h3><ol class="text-sm">{rows}</ol>{cut}</section>"#,
        cut = if cut {
            r#"<p class="text-xs opacity-70">Only the newest 200 of each kind of record are included.</p>"#
        } else {
            ""
        },
    )
}

fn steps(view: &RunPaneView<'_>) -> String {
    let run = view.run;
    let summary = &run.detail.summary;
    let (company_id, id) = (view.company_id, summary.run.as_uuid());
    if run.detail.executions.is_empty() {
        return r#"<section class="space-y-2"><h3 class="font-semibold">Steps</h3><p class="text-sm opacity-70">No step has started yet.</p></section>"#.to_owned();
    }
    let rows: String = run
        .detail
        .executions
        .iter()
        .map(|execution| {
            let state = match (execution.activated_at, execution.completed_at) {
                (_, Some(_)) => "finished".to_owned(),
                _ if run_ended(&summary.state) => {
                    format!("stopped, run {}", escape_html_text(&summary.state))
                }
                (Some(_), None) => "running or waiting".to_owned(),
                (None, None) => "queued".to_owned(),
            };
            // A finished step no longer changes, so a reload keeps the one already shown.
            let preserved = if execution.completed_at.is_some() {
                format!(r#" id="run-step-{}" hx-preserve="true""#, execution.execution)
            } else {
                String::new()
            };
            format!(
                r##"<li><details{preserved} class="rounded-box border border-base-300 p-2">
                    <summary class="cursor-pointer text-sm"><span class="font-mono font-semibold">{step}</span> · activation {activation} · {state}{route}</summary>
                    <div class="mt-2" hx-get="/ui/workflow-runs/{id}/steps/{execution}?company_id={company_id}"
                        hx-trigger="toggle from:closest details once" hx-swap="innerHTML"><p class="text-xs opacity-70">Loading…</p></div>
                </details></li>"##,
                step = escape_html_text(&execution.step),
                activation = execution.activation,
                route = execution
                    .route
                    .as_deref()
                    .map(|route| format!(" → {}", escape_html_text(route)))
                    .unwrap_or_default(),
                execution = execution.execution,
            )
        })
        .collect();
    format!(
        r#"<section class="space-y-2"><h3 class="font-semibold">Steps</h3><p class="text-xs opacity-70">Newest first. Open a step for what it froze, what it produced, its model calls, receipts and children.</p><ul class="space-y-2">{rows}</ul></section>"#
    )
}

fn waits(view: &RunPaneView<'_>) -> String {
    let run = view.run;
    if run.detail.waits.is_empty() {
        return String::new();
    }
    let company_id = view.company_id;
    let rows: String = run
        .detail
        .waits
        .iter()
        .map(|wait| {
            let subject = match (wait.decision, wait.child_run) {
                (Some(decision), _) => format!(
                    r#" · <a class="link" href="/ui/workflow-decisions/{decision}">decision {}</a>"#,
                    short_id(decision)
                ),
                (None, Some(child)) => format!(" · child run {}", run_link(company_id, child.as_uuid())),
                (None, None) => String::new(),
            };
            format!(
                r#"<li class="py-1 text-sm">{reason} · {state} · deadline {deadline}{subject}</li>"#,
                reason = escape_html_text(&wait.reason),
                state = escape_html_text(&wait.state),
                deadline = format_date_time(wait.deadline),
            )
        })
        .collect();
    format!(
        r#"<section class="space-y-2"><h3 class="font-semibold">Waits</h3><ul>{rows}</ul></section>"#
    )
}

fn actions(view: &RunPaneView<'_>) -> String {
    let run = view.run;
    if run.detail.actions.is_empty() {
        return String::new();
    }
    let summary = &run.detail.summary;
    let (company_id, id) = (view.company_id, summary.run.as_uuid());
    let rows: String = run
        .detail
        .actions
        .iter()
        .map(|action| {
            let status = match (action.has_receipt, action.evidence_conflict) {
                (_, true) => "evidence conflicts",
                (true, false) => "accepted",
                (false, false) => "no receipt yet",
            };
            let destinations: String = action
                .destinations
                .iter()
                .map(|destination| {
                    format!(
                        r#"<li>destination {ordinal}: {status}{parts}{form}</li>"#,
                        ordinal = destination.ordinal,
                        status = escape_html_text(&destination.status.replace('_', " ")),
                        parts = if destination.parts > 1 {
                            format!(" ({} parts)", destination.parts)
                        } else {
                            String::new()
                        },
                        form = if destination.reconcilable() && summary.parent.is_none() {
                            reconcile_form(
                                company_id,
                                id,
                                summary.revision,
                                action,
                                Some((destination.ordinal, destination.delivery)),
                            )
                        } else {
                            String::new()
                        },
                    )
                })
                .collect();
            let form = if action.reconcilable(summary) && summary.parent.is_none() {
                reconcile_form(company_id, id, summary.revision, action, None)
            } else {
                String::new()
            };
            format!(
                r#"<li class="py-2 text-sm"><span class="font-semibold">{kind}</span> action · attempt {attempt} · {status} · {at}{form}<ul class="ml-4 text-xs">{destinations}</ul></li>"#,
                kind = escape_html_text(&action.effect_kind),
                attempt = action.attempt,
                at = format_date_time(action.created_at),
            )
        })
        .collect();
    format!(
        r#"<section class="space-y-2"><h3 class="font-semibold">Actions</h3><p class="text-xs opacity-70">Remote calls and messages this run dispatched. Accepted effects stay accepted whatever happens to the run.</p><ul class="divide-y divide-base-300">{rows}</ul></section>"#
    )
}

/// "Record what happened": the reconciliation annotation for one action (or one delivery of a
/// message action). It records an unknown note; it never resumes the run (WF-C005).
fn reconcile_form(
    company_id: Uuid,
    run: Uuid,
    revision: u64,
    action: &crate::application::workflow::reads::ActionSummary,
    delivery: Option<(i16, Uuid)>,
) -> String {
    let target = match delivery {
        None => r#"<input type="hidden" name="target" value="remote">"#.to_owned(),
        Some((ordinal, delivery)) => format!(
            r#"<input type="hidden" name="target" value="message_delivery"><input type="hidden" name="ordinal" value="{ordinal}"><input type="hidden" name="delivery_id" value="{delivery}">"#
        ),
    };
    format!(
        r##"<details class="mt-2"><summary class="cursor-pointer text-xs font-semibold">Record what happened (an annotation, not a resume)</summary>
            <form class="mt-2 space-y-2" hx-post="/ui/workflow-runs/{run}/reconcile?company_id={company_id}" hx-target="#run-pane"
                hx-swap="outerHTML" hx-disabled-elt="find button">
                <input type="hidden" name="execution_id" value="{execution}">
                <input type="hidden" name="invocation_id" value="{invocation}">
                <input type="hidden" name="argument_digest" value="{digest}">
                <input type="hidden" name="marker_id" value="{marker}">
                <input type="hidden" name="expected_revision" value="{revision}">
                <input type="hidden" name="command_key" value="{key}">
                {target}
                <label class="form-control w-full"><div class="label"><span class="text-xs">What you know about it (kept with the run)</span></div>
                    <textarea class="textarea w-full text-xs" name="note" rows="2" maxlength="2000" required></textarea></label>
                <p class="text-xs opacity-80">This records the outcome as unknown. The run does not resume from it; to recover, cancel the run and rerun it.</p>
                <button type="submit" class="btn btn-sm btn-outline">Record</button>
            </form></details>"##,
        execution = action.execution,
        invocation = action.invocation,
        digest = escape_html_attr(&action.argument_digest),
        marker = action.marker,
        key = Uuid::new_v4(),
    )
}

fn children(view: &RunPaneView<'_>) -> String {
    let run = view.run;
    if run.children.is_empty() {
        return String::new();
    }
    let company_id = view.company_id;
    let rows: String = run
        .children
        .iter()
        .map(|child| {
            format!(
                r#"<li class="py-1 text-sm">{link} {state} · step <span class="font-mono">{step}</span>{round}{call}</li>"#,
                link = run_link(company_id, child.run.as_uuid()),
                state = run_state_badge(&child.state, child.waiting_reason.as_deref()),
                step = escape_html_text(child.step.as_deref().unwrap_or("?")),
                round = child
                    .round
                    .map(|round| format!(" · round {round}"))
                    .unwrap_or_default(),
                call = child
                    .tool_call
                    .map(|call| format!(
                        r#" · tool call <span class="font-mono">{}</span>"#,
                        short_id(call)
                    ))
                    .unwrap_or_default(),
            )
        })
        .collect();
    format!(
        r#"<section class="space-y-2"><h3 class="font-semibold">Child runs</h3><ul>{rows}</ul>{cut}</section>"#,
        cut = if run.children_truncated {
            r#"<p class="text-xs opacity-70">Only the newest 200 children are listed.</p>"#
        } else {
            ""
        },
    )
}
