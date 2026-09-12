//! The delegation control forms, shared by the Tasks workspace pane and the mailbox thread pane.
//!
//! Two surfaces offer these controls and they resolve a different authority server-side: the Tasks
//! workspace admits company owners and admins only, while the mailbox pane also reaches a task's
//! current human owner. What they must *not* differ in is the wording. Every button here names a
//! consequence the operator cannot take back — an external message already delivered stays
//! delivered, and nothing recalls email — so a label that drifted on one surface would be a label
//! that lies on one surface. The labels, the warnings and the form shape live here once; a caller
//! supplies only its [`DelegationSurface`].

use super::*;
use crate::entities::collaboration::CollaborationTarget;

/// Where one surface's delegation forms post, and what they swap when they answer.
pub(crate) struct DelegationSurface<'a> {
    /// What every form's `hx-post` carries before the operation segment, e.g.
    /// `/ui/tasks/{task_id}/delegation`.
    pub action_prefix: &'a str,
    /// What follows the operation segment, query string included. Empty on a route that names its
    /// scope in hidden fields instead.
    pub action_suffix: &'a str,
    /// The element every form on this surface replaces, e.g. `#task-pane`.
    pub target: &'a str,
    /// Hidden fields naming the scope the submission came from, repeated in every form because
    /// each one posts on its own.
    pub scope_fields: &'a str,
    /// Internal channels a mis-routed target may be moved to. Empty offers no reassignment.
    pub reassign_channels: &'a [Channel],
}

/// One surface's controls for one outreach, or nothing when there is nothing left to act on.
pub(crate) fn delegation_control_forms(
    summary: &CollaborationSummary,
    surface: &DelegationSurface<'_>,
) -> String {
    let (Some(outreach_id), Some(version)) = (summary.outreach_id, summary.outreach_version) else {
        return String::new();
    };
    if matches!(
        summary.status,
        OutreachBusinessStatus::Completed | OutreachBusinessStatus::Cancelled
    ) {
        return String::new();
    }
    let forms = DelegationForms {
        surface,
        outreach_id,
        version,
    };
    let waiting_may_change = matches!(
        summary.status,
        OutreachBusinessStatus::Waiting
            | OutreachBusinessStatus::NeedsDecision
            | OutreachBusinessStatus::Failed
    );
    let target_controls: String = summary
        .children
        .iter()
        .filter(|target| waiting_may_change && target_is_open(target.status))
        .map(|target| target_control(&forms, target))
        .collect();
    let extend = if waiting_may_change {
        format!(
            r##"{open}<input class="input input-xs w-28" type="number" min="1" max="720" name="deadline_hours" value="96" aria-label="New response window in hours"><button class="btn btn-xs">Extend deadline</button></form>"##,
            open = forms.open("extend", "deadline_changed", r#" class="flex gap-2""#),
        )
    } else {
        String::new()
    };
    let proceed_partial = if waiting_may_change {
        format!(
            r##"{open}<button class="btn btn-primary btn-xs">Proceed with partial</button></form>"##,
            open = forms.open("proceed-partial", "partial_results_accepted", ""),
        )
    } else {
        String::new()
    };
    format!(
        r##"<section class="space-y-3 rounded-box border border-warning/30 bg-warning/5 p-4"><div><h3 class="text-xs font-bold uppercase opacity-60">Delegation controls</h3><p class="text-[11px] opacity-70">These actions change waiting or execution state. They do not recall email.</p></div>{extend}<div class="space-y-2">{target_controls}</div><div class="flex flex-wrap gap-2">{proceed_partial}{cancel_outreach}<button class="btn btn-warning btn-xs">Cancel outreach waiting</button></form>{stop_task}<button class="btn btn-error btn-xs">Stop task</button></form></div></section>"##,
        cancel_outreach = forms.open("cancel-outreach", "no_longer_needed", ""),
        stop_task = forms.open("stop-task", "task_stopped", ""),
    )
}

/// A target still worth a control: one that has neither answered nor already been taken out of the
/// wait.
fn target_is_open(status: TargetBusinessStatus) -> bool {
    !matches!(
        status,
        TargetBusinessStatus::Responded
            | TargetBusinessStatus::Cancelled
            | TargetBusinessStatus::Superseded
            | TargetBusinessStatus::Expired
    )
}

/// One outreach's forms on one surface: everything a form tag and its command envelope need.
struct DelegationForms<'a> {
    surface: &'a DelegationSurface<'a>,
    outreach_id: Uuid,
    version: u64,
}

impl DelegationForms<'_> {
    /// A form tag for one operation, already carrying the command envelope: a fresh idempotency
    /// key, the outreach version the reader is acting on, which outreach, and why.
    ///
    /// Every control posts on its own, so each repeats the envelope rather than sharing one form.
    fn open(&self, operation: &str, reason: &str, form_attributes: &str) -> String {
        format!(
            r##"<form hx-post="{prefix}/{operation}{suffix}" hx-target="{target}" hx-swap="outerHTML"{form_attributes}><input type="hidden" name="command_id" value="{command_id}"><input type="hidden" name="expected_version" value="{version}"><input type="hidden" name="outreach_id" value="{outreach_id}"><input type="hidden" name="reason" value="{reason}">{scope}"##,
            prefix = self.surface.action_prefix,
            suffix = self.surface.action_suffix,
            target = self.surface.target,
            command_id = Uuid::new_v4(),
            version = self.version,
            outreach_id = self.outreach_id,
            scope = self.surface.scope_fields,
        )
    }
}

/// The controls for one waiting target, with the consequence of using them spelled out.
fn target_control(forms: &DelegationForms<'_>, target: &CollaborationTargetSummary) -> String {
    let target_field = format!(
        r#"<input type="hidden" name="target_id" value="{}">"#,
        target.id
    );
    let warning = match target.target {
        CollaborationTarget::External { .. } => {
            "Cancels an unsent delivery when possible; otherwise only stops waiting. The external message may already have been received."
        }
        _ => "Stops waiting and revokes unfinished internal work. A completed result wins.",
    };
    format!(
        r##"<div class="rounded-box border border-base-300 p-2"><p class="text-xs font-semibold">{label}</p><p class="mb-2 text-[11px] opacity-70">{warning}</p><div class="flex flex-wrap gap-2">{cancel}{target_field}<button class="btn btn-warning btn-xs">Cancel waiting</button></form>{reassign}</div></div>"##,
        label = escape_html_text(&target.label),
        cancel = forms.open("cancel-target", "target_unavailable", ""),
        reassign = reassign_control(forms, target, &target_field),
    )
}

/// Moving an internal request to another channel, offered only where there is somewhere to move it.
fn reassign_control(
    forms: &DelegationForms<'_>,
    target: &CollaborationTargetSummary,
    target_field: &str,
) -> String {
    let CollaborationTarget::InternalChannel {
        channel_id: current_channel_id,
    } = target.target
    else {
        return String::new();
    };
    let options: String = forms
        .surface
        .reassign_channels
        .iter()
        .filter(|channel| channel.enabled && channel.id != current_channel_id)
        .map(|channel| {
            format!(
                r#"<option value="{}">{}</option>"#,
                channel.id,
                escape_html_text(&channel.name)
            )
        })
        .collect();
    if options.is_empty() {
        return String::new();
    }
    format!(
        r##"{open}{target_field}<select name="new_channel_id" class="select select-xs grow" required><option value="">Reassign to…</option>{options}</select><button class="btn btn-xs">Reassign</button></form>"##,
        open = forms.open("reassign", "incorrect_target", r#" class="flex gap-2""#),
    )
}
