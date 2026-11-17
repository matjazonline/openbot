use super::*;
use crate::entities::department::DepartmentRoleHolder;

pub(super) fn sponsors_card(section: &DepartmentSection<'_>, view: &DepartmentView) -> String {
    let department = view.detail.department.id;
    let sponsors: Vec<_> = view
        .detail
        .roles
        .iter()
        .filter(|holder| holder.role == DepartmentRole::Sponsor)
        .collect();
    let endpoint = section_endpoint(section.company.id, &format!("/{department}/sponsors"));
    let mut rows = String::new();
    for holder in &sponsors {
        let mark = if holder.decider {
            "decides"
        } else {
            "informed"
        };
        let controls = sponsor_controls(section, view, holder, &sponsors, &endpoint);
        rows.push_str(&format!(r#"<li class="flex flex-wrap items-center justify-between gap-2 py-2"><div>{person} <span class="badge badge-sm">{mark}</span></div>{controls}</li>"#,person=named_principal(view,holder.principal_id)));
    }
    if sponsors.is_empty() {
        rows.push_str(r#"<li class="text-sm opacity-60">No sponsors.</li>"#);
    }
    let warning = if !sponsors.is_empty() && !sponsors.iter().any(|holder| holder.decider) {
        r#"<p class="text-sm text-warning">No sponsor is marked decides. Choose a decider before sponsor work can start.</p>"#
    } else {
        ""
    };
    let add = if section.editable {
        format!(
            r##"<form class="mt-3 flex flex-wrap items-end gap-2" hx-post="{endpoint}" hx-target="#department-section" hx-swap="outerHTML" data-submit="busy-once" data-pending-label="Saving…">
            <label class="form-control flex-1"><span class="label text-xs">Sponsor</span><select class="select select-sm w-full" name="principal_id" required>{options}</select></label>
            <label class="label"><input class="checkbox checkbox-sm" type="checkbox" name="decider" value="true">Decides</label>
            <button type="submit" class="btn btn-sm"><span data-label>Add sponsor</span></button></form>{cut}"##,
            options = principal_options(section.choices, None, "Choose a person or agent"),
            cut = choices_cut_note(section)
        )
    } else {
        String::new()
    };
    format!(
        r#"<section id="department-sponsors" class="rounded-box border border-base-300 p-4"><h4 class="text-sm font-semibold">Sponsors</h4>{error}{warning}<ul>{rows}</ul>{add}</section>"#,
        error = section.error_for(DepartmentForm::Sponsors)
    )
}

pub(super) fn escalation_card(section: &DepartmentSection<'_>, view: &DepartmentView) -> String {
    let department = &view.detail.department;
    let inherited = escalation_summary(section, department);
    let form = escalation_form(section, department);
    format!(
        r#"<section id="department-escalation" class="rounded-box border border-base-300 p-4"><h4 class="text-sm font-semibold">When work stalls, tell</h4>{error}<p class="text-sm">{inherited}</p>{form}</section>"#,
        error = section.error_for(DepartmentForm::Escalation),
        inherited = escape_html_text(&inherited)
    )
}

fn sponsor_controls(
    section: &DepartmentSection<'_>,
    view: &DepartmentView,
    holder: &DepartmentRoleHolder,
    sponsors: &[&DepartmentRoleHolder],
    endpoint: &str,
) -> String {
    if section.editable {
        let promote = if holder.decider {
            String::new()
        } else {
            format!(
                r##"<form hx-post="{endpoint}" hx-target="#department-section" hx-swap="outerHTML" data-submit="busy-once" data-pending-label="Saving…">
                    <input type="hidden" name="principal_id" value="{principal}"><input type="hidden" name="decider" value="true">
                    <button type="submit" class="btn btn-ghost btn-xs"><span data-label>Mark decides</span></button></form>"##,
                principal = holder.principal_id
            )
        };
        let successors = if holder.decider && sponsors.len() > 1 {
            let choices = sponsors
                .iter()
                .filter(|other| other.principal_id != holder.principal_id)
                .map(|other| {
                    let label = view.principal(other.principal_id).map_or_else(
                        || other.principal_id.to_string(),
                        |person| person.name.clone(),
                    );
                    format!(
                        r#"<option value="{}">{}</option>"#,
                        other.principal_id,
                        escape_html_text(&label)
                    )
                })
                .collect::<String>();
            format!(
                r#"<label class="form-control"><span class="label text-xs">Successor</span><select class="select select-sm" name="successor" required><option value="">Choose who decides</option>{choices}</select></label>"#
            )
        } else {
            String::new()
        };
        format!(
            r##"<div class="flex flex-wrap items-end gap-2">{promote}
                <form class="flex flex-wrap items-end gap-2" hx-post="{endpoint}/remove" hx-target="#department-section" hx-swap="outerHTML" data-submit="busy-once" data-pending-label="Removing…">
                    <input type="hidden" name="principal_id" value="{principal}">{successors}
                    <button type="submit" class="btn btn-ghost btn-xs"><span data-label>Remove sponsor</span></button></form></div>"##,
            principal = holder.principal_id
        )
    } else {
        String::new()
    }
}

fn escalation_form(section: &DepartmentSection<'_>, department: &Department) -> String {
    if section.editable {
        let roles = DepartmentRole::ALL
            .into_iter()
            .map(|role| {
                format!(
                    r#"<option value="{role}" {selected}>{role}</option>"#,
                    selected = if department
                        .escalation
                        .is_some_and(|policy| policy.notify == role)
                    {
                        "selected"
                    } else {
                        ""
                    }
                )
            })
            .collect::<String>();
        let hops = department.escalation.map_or(2, |policy| policy.hops.get());
        let endpoint = section_endpoint(
            section.company.id,
            &format!("/{}/escalation", department.id),
        );
        format!(
            r##"<form class="mt-3 flex flex-wrap items-end gap-2" hx-post="{endpoint}" hx-target="#department-section" hx-swap="outerHTML" data-submit="busy-once" data-pending-label="Saving…">
            <label class="form-control"><span class="label text-xs">Role</span><select class="select select-sm" name="notify"><option value="">Inherit default</option>{roles}</select></label>
            <label class="form-control"><span class="label text-xs">Hops</span><select class="select select-sm" name="hops"><option value="1" {one}>1 · this unit</option><option value="2" {two}>2 · then parent lead</option></select></label>
            <button type="submit" class="btn btn-sm"><span data-label>Save escalation</span></button></form>"##,
            one = if hops == 1 { "selected" } else { "" },
            two = if hops == 2 { "selected" } else { "" }
        )
    } else {
        String::new()
    }
}

fn escalation_summary(section: &DepartmentSection<'_>, department: &Department) -> String {
    let mut current = department;
    for _ in 0..crate::entities::department::MAX_DEPARTMENT_DEPTH {
        if let Some(policy) = current.escalation {
            return if current.id == department.id {
                format!("Tell {} · {} hop(s)", policy.notify, policy.hops.get())
            } else {
                format!(
                    "Inherited from {}: tell {} · {} hop(s)",
                    current.name,
                    policy.notify,
                    policy.hops.get()
                )
            };
        }
        let Some(parent) = current.parent_id else {
            return "No escalation default.".into();
        };
        let Some(parent) = section.departments.iter().find(|unit| unit.id == parent) else {
            return "No local default; an ancestor may supply one.".into();
        };
        current = parent;
    }
    "Escalation ancestry is unavailable.".into()
}
