use super::*;
use crate::application::workflow::compiler::ReviewerReferenceStep;
use crate::application::workflow::reads::{ChoiceOption, ReviewerChoices};
use crate::domain::workflow::StepId;
use crate::entities::department::DepartmentRole;

pub struct ReviewerRolePreview {
    pub role: DepartmentRole,
    pub holders: Vec<String>,
    pub refusal: Option<&'static str>,
}

pub struct WorkflowReviewerPicker<'a> {
    pub company: Uuid,
    pub workflow: Uuid,
    pub steps: &'a [ReviewerReferenceStep],
    pub step: Option<&'a ReviewerReferenceStep>,
    pub people: &'a ReviewerChoices,
    pub selected: Option<&'a str>,
    pub desks: &'a [ChoiceOption],
    pub desk: Option<Uuid>,
    pub preview: &'a [ReviewerRolePreview],
    pub next_step: Option<&'a StepId>,
    pub next_desk: Option<Uuid>,
    pub desks_cut: bool,
}

pub(super) fn workflow_reviewer_picker_loader(company: Uuid, workflow: Uuid) -> String {
    format!(
        r##"<section id="workflow-reviewer-picker" data-workflow-reviewer-picker class="rounded-box border border-base-300 p-4" hx-post="/ui/workflows/{workflow}/reviewer-picker?company_id={company}" hx-trigger="load" hx-include="#workflow-source" hx-params="source" hx-target="this" hx-swap="outerHTML" hx-sync="this:replace">Loading reviewer choices…</section>"##
    )
}

pub fn workflow_reviewer_picker(view: &WorkflowReviewerPicker<'_>) -> String {
    let PickerOptions {
        steps,
        people,
        roles,
        desks,
    } = picker_options(view);
    let preview = holder_preview(view);
    let controlled = view.step.is_some_and(|step| step.selectors.is_none());
    let disabled = if controlled || view.step.is_none() || view.selected.is_none() {
        " disabled"
    } else {
        ""
    };
    let current = if controlled {
        "These reviewers come from parameters. Edit the parameter binding in source."
    } else {
        "Use replaces the reviewer list; Add keeps its existing literal selections."
    };
    let more = more_button(
        view,
        "after_person",
        view.people.next.map(|id| id.as_uuid().to_string()),
        "More people",
    ) + &more_button(
        view,
        "after_step",
        view.next_step.map(ToString::to_string),
        "More reviewer steps",
    ) + &more_button(
        view,
        "after_desk",
        view.next_desk.map(|id| id.to_string()),
        "More desks",
    );
    format!(
        r##"<section id="workflow-reviewer-picker" data-workflow-reviewer-picker class="space-y-3 rounded-box border border-base-300 p-4">
        <h3 class="font-semibold">Choose reviewers</h3><p class="text-xs opacity-70">{current}</p>
        <form hx-post="/ui/workflows/{workflow}/reviewer-picker?company_id={company}" hx-include="#workflow-source" hx-target="#workflow-reviewer-picker" hx-swap="outerHTML" hx-sync="#workflow-reviewer-picker:replace" hx-indicator="#workflow-reviewer-progress" hx-disabled-elt="#workflow-reviewer-picker button" class="space-y-3">
        <label class="form-control">Review step<select name="step" class="select" hx-post="/ui/workflows/{workflow}/reviewer-picker?company_id={company}" hx-params="source,step,desk">{steps}</select></label>
        <label class="form-control">Reviewer<select name="reviewer" class="select" hx-post="/ui/workflows/{workflow}/reviewer-picker?company_id={company}"><option value="" disabled{placeholder}>Choose a person or role</option><optgroup label="People">{people}</optgroup><optgroup label="Role">{roles}</optgroup></select></label>
        <label class="form-control">Preview desk<select name="desk" class="select" hx-post="/ui/workflows/{workflow}/reviewer-picker?company_id={company}"><option value="">Select a desk</option>{desks}</select></label>
        <p class="text-xs opacity-70">Today's holders — read-only preview. Actual reviewers are frozen when a run starts.</p><ul class="text-sm">{preview}</ul>
        {more}<p class="text-xs opacity-60">50 choices per page.{cut}</p>
        <div class="flex flex-wrap gap-2"><button class="btn btn-ghost btn-sm" type="submit">Refresh choices</button><button class="btn btn-primary btn-sm" type="button" hx-post="/ui/workflows/{workflow}/reviewer-reference?company_id={company}" hx-target="#workflow-reviewer-result" hx-swap="innerHTML"{disabled}>Use reviewer</button><button class="btn btn-ghost btn-sm" type="button" hx-post="/ui/workflows/{workflow}/reviewer-reference?company_id={company}" hx-vals='{{"edit":"add"}}' hx-target="#workflow-reviewer-result" hx-swap="innerHTML"{disabled}>Add reviewer</button></div>
        <span id="workflow-reviewer-progress" class="htmx-indicator text-sm" role="status">Updating reviewer selection…</span></form><div id="workflow-reviewer-result" role="status" aria-live="polite"></div></section>"##,
        workflow = view.workflow,
        company = view.company,
        placeholder = if view.selected.is_none() {
            " selected"
        } else {
            ""
        },
        cut = if view.desks_cut {
            " More desks exist beyond the binding directory limit."
        } else {
            ""
        }
    )
}

struct PickerOptions {
    steps: String,
    people: String,
    roles: String,
    desks: String,
}

fn picker_options(view: &WorkflowReviewerPicker<'_>) -> PickerOptions {
    let steps = view
        .steps
        .iter()
        .map(|step| {
            option(
                step.id.as_str(),
                step.id.as_str(),
                view.step.is_some_and(|selected| selected.id == step.id),
            )
        })
        .collect::<String>();
    let people = view
        .people
        .items
        .iter()
        .map(|person| {
            option(
                &person.principal.as_uuid().to_string(),
                &person.label,
                view.selected == Some(&person.principal.as_uuid().to_string()),
            )
        })
        .collect::<String>();
    let roles = DepartmentRole::ALL
        .into_iter()
        .map(|role| {
            option(
                &format!("role:{role}"),
                &format!("{role} · role"),
                view.selected == Some(&format!("role:{role}")),
            )
        })
        .collect::<String>();
    let desks = view
        .desks
        .iter()
        .map(|desk| {
            option(
                &desk.id.to_string(),
                &desk.label,
                view.desk == Some(desk.id),
            )
        })
        .collect::<String>();
    PickerOptions {
        steps,
        people,
        roles,
        desks,
    }
}

fn holder_preview(view: &WorkflowReviewerPicker<'_>) -> String {
    view.preview
        .iter()
        .map(|preview| {
            format!(
                "<li>{role}: {holders}</li>",
                role = preview.role,
                holders = preview
                    .refusal
                    .map(escape_html_text)
                    .unwrap_or_else(|| preview
                        .holders
                        .iter()
                        .map(|label| escape_html_text(label))
                        .collect::<Vec<_>>()
                        .join(", "))
            )
        })
        .collect::<String>()
}

fn option(value: &str, label: &str, selected: bool) -> String {
    format!(
        r#"<option value="{}"{}>{}</option>"#,
        escape_html_attr(value),
        if selected { " selected" } else { "" },
        escape_html_text(label)
    )
}
fn more_button(
    view: &WorkflowReviewerPicker<'_>,
    field: &str,
    after: Option<String>,
    label: &str,
) -> String {
    after.map(|after| format!(r##"<button class="btn btn-ghost btn-xs" type="button" hx-post="/ui/workflows/{workflow}/reviewer-picker?company_id={company}" hx-vals="{values}">{label}</button>"##,
        workflow=view.workflow,company=view.company,values=escape_html_attr(&serde_json::json!({(field):after}).to_string()))).unwrap_or_default()
}
