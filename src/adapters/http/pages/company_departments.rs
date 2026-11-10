//! The company pane's Departments tab (01-departments 04A): the department tree beside the
//! selected department, and for an owner or admin the forms that change it.
//!
//! Like the Skills tab, the tab's content is one section, `#department-section`, that every
//! department request re-renders whole, so a write's refusal comes back on the form that made it
//! and the tree always shows what was just saved.

use super::*;
use crate::entities::department::{
    Department, DepartmentDesk, DepartmentId, DepartmentPrincipal, DepartmentRole, department_tree,
};
use crate::entities::participant::PrincipalKind;
use crate::use_cases::department::DepartmentView;

/// What a department form was last submitted with, so a refused submit comes back filled in.
#[derive(Debug, Clone, Default)]
pub struct DepartmentDraft {
    pub name: String,
    pub slug: String,
    pub description: String,
    pub charter: String,
    pub parent_id: Option<Uuid>,
    /// Create only: who leads the new department.
    pub lead: Option<Uuid>,
}

impl DepartmentDraft {
    pub fn stored(department: &Department) -> Self {
        Self {
            name: department.name.clone(),
            slug: department.slug.as_str().to_owned(),
            description: department.description.clone().unwrap_or_default(),
            charter: department.charter.clone().unwrap_or_default(),
            parent_id: department.parent_id.map(DepartmentId::as_uuid),
            lead: None,
        }
    }
}

/// Which form a refusal belongs to, so it renders on that form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepartmentForm {
    Create,
    Edit,
    Lead,
    Members,
    Delete,
}

/// The right-hand column: a department, the create form, or nothing picked yet.
pub enum DepartmentDetailPane<'a> {
    None,
    Create(&'a DepartmentDraft),
    /// The selected department, with the edit form's draft when an edit was refused.
    Department {
        view: &'a DepartmentView,
        draft: Option<&'a DepartmentDraft>,
    },
}

pub struct DepartmentSection<'a> {
    pub company: &'a Company,
    pub app_domain_name: &'a str,
    /// The departments the viewer may see: all of them for an owner or admin, otherwise their own.
    pub departments: &'a [Department],
    pub detail: DepartmentDetailPane<'a>,
    /// The people and agents the forms offer; empty unless `editable`.
    pub choices: &'a [DepartmentPrincipal],
    /// The company has more people and agents than `choices` lists.
    pub choices_cut: bool,
    /// The viewer manages the company (owner or admin).
    pub editable: bool,
    pub error: Option<(DepartmentForm, &'a str)>,
    pub notice: Option<&'a str>,
}

impl DepartmentSection<'_> {
    fn error_for(&self, form: DepartmentForm) -> String {
        form_error_banner(
            self.error
                .and_then(|(failed, message)| (failed == form).then_some(message)),
        )
    }

    fn selected(&self) -> Option<DepartmentId> {
        match self.detail {
            DepartmentDetailPane::Department { view, .. } => Some(view.detail.department.id),
            _ => None,
        }
    }
}

/// The shareable URL of the tab, with a department open or not.
pub fn departments_url(company_id: Uuid, department: Option<DepartmentId>) -> String {
    match department {
        Some(department) => format!(
            "/ui/companies?company_id={company_id}&amp;tab=departments&amp;department_id={department}"
        ),
        None => format!("/ui/companies?company_id={company_id}&amp;tab=departments"),
    }
}

/// Where the section's own requests go.
fn section_endpoint(company_id: Uuid, rest: &str) -> String {
    format!("/ui/companies/{company_id}/departments{rest}")
}

pub fn company_departments_section(section: &DepartmentSection<'_>) -> String {
    let company_id = section.company.id;
    let new_button = if section.editable {
        format!(
            r##"<button type="button" class="btn btn-primary btn-sm"
                hx-get="{new_endpoint}" hx-target="#department-section" hx-swap="outerHTML"
                hx-sync="#department-section:replace"
                hx-push-url="/ui/companies?company_id={company_id}&amp;tab=departments&amp;new=1">{plus} New department</button>"##,
            new_endpoint = section_endpoint(company_id, "/new"),
            plus = icon(Icon::Plus, BUTTON_ICON),
        )
    } else {
        String::new()
    };
    let intro = if section.editable {
        "Departments group desks and the people and agents who work them."
    } else {
        "The departments you belong to or lead."
    };
    format!(
        r##"<div id="department-section" class="flex-1 overflow-y-auto px-4 py-4 sm:px-6">
            {notice}
            <div class="grid grid-cols-1 gap-5 xl:grid-cols-[minmax(0,1fr)_minmax(24rem,1.5fr)]">
                <section class="space-y-3">
                    <div class="flex items-center justify-between gap-3">
                        <div><h3 class="font-semibold">Departments</h3><p class="text-xs opacity-60">{intro}</p></div>
                        {new_button}
                    </div>
                    {tree}
                </section>
                <section>{detail}</section>
            </div>
        </div>"##,
        notice = section
            .notice
            .map(|message| format!(
                r#"<div role="status" class="alert alert-success mb-4 text-sm">{}</div>"#,
                escape_html_text(message)
            ))
            .unwrap_or_default(),
        tree = department_tree_list(section),
        detail = detail_column(section),
    )
}

/// The tree, as nested menus: a child department sits indented under its parent.
fn department_tree_list(section: &DepartmentSection<'_>) -> String {
    if section.departments.is_empty() {
        let message = if section.editable {
            "No departments yet."
        } else {
            "You are not in any department."
        };
        return format!(
            r#"<p class="rounded-box border border-dashed border-base-300 p-5 text-sm opacity-60">{message}</p>"#
        );
    }
    let company_id = section.company.id;
    let selected = section.selected();
    let mut html = String::new();
    let mut open: Option<usize> = None;
    for (depth, department) in department_tree(section.departments) {
        match open {
            None => {}
            Some(last) if depth > last => html.push_str("<ul>"),
            Some(last) => {
                html.push_str("</li>");
                for _ in depth..last {
                    html.push_str("</ul></li>");
                }
            }
        }
        html.push_str(&format!(
            r##"<li data-department-depth="{depth}"><a class="flex flex-col items-start gap-0.5 {active}"
                hx-get="{endpoint}" hx-target="#department-section" hx-swap="outerHTML"
                hx-sync="#department-section:replace" hx-push-url="{url}">
                <span class="truncate">{name}</span>
                <span class="font-mono text-[11px] opacity-60">{slug}</span></a>"##,
            active = if selected == Some(department.id) {
                "menu-active"
            } else {
                ""
            },
            endpoint = section_endpoint(company_id, &format!("/{}", department.id)),
            url = departments_url(company_id, Some(department.id)),
            name = escape_html_text(&department.name),
            slug = escape_html_text(department.slug.as_str()),
        ));
        open = Some(depth);
    }
    if let Some(last) = open {
        html.push_str("</li>");
        for _ in 0..last {
            html.push_str("</ul></li>");
        }
    }
    format!(
        r#"<ul id="department-tree" class="menu w-full rounded-box bg-base-200 p-2">{html}</ul>"#
    )
}

fn detail_column(section: &DepartmentSection<'_>) -> String {
    match section.detail {
        DepartmentDetailPane::None => r#"<div class="rounded-box border border-dashed border-base-300 p-5 text-sm opacity-60">Select a department to see its lead, members and desks.</div>"#.to_string(),
        DepartmentDetailPane::Create(draft) => create_form(section, draft),
        DepartmentDetailPane::Department { view, draft } => department_detail(section, view, draft),
    }
}

fn create_form(section: &DepartmentSection<'_>, draft: &DepartmentDraft) -> String {
    let company_id = section.company.id;
    format!(
        r##"<form id="department-create" class="space-y-4 rounded-box border border-base-300 bg-base-200 p-4"
            hx-post="{endpoint}" hx-target="#department-section" hx-swap="outerHTML"
            hx-disabled-elt="find button[type='submit']">
            <h3 class="font-semibold">New department</h3>
            {error}
            {fields}
            <label class="form-control"><span class="label text-xs opacity-70">Lead</span>
                <select class="select w-full" name="lead">{lead_options}</select>
                <span class="label text-xs opacity-60">An agent lead answers the department's desk; a person lead or none leaves it a plain team desk.</span>{cut}</label>
            <div class="flex items-center gap-3 border-t border-base-300 pt-4">
                {submit}
                <button type="button" class="btn btn-ghost"
                    hx-get="{cancel}" hx-target="#department-section" hx-swap="outerHTML"
                    hx-sync="#department-section:replace"
                    hx-push-url="{cancel_url}">Cancel</button>
            </div>
        </form>"##,
        endpoint = section_endpoint(company_id, ""),
        error = section.error_for(DepartmentForm::Create),
        fields = department_fields(section, draft, None),
        lead_options = principal_options(section.choices, draft.lead, "No lead"),
        cut = choices_cut_note(section),
        submit = busy_submit_button("Create department"),
        cancel = section_endpoint(company_id, ""),
        cancel_url = departments_url(company_id, None),
    )
}

/// Name, address slug, description, charter and parent: what both the create and the edit form
/// set. `editing` is the department being edited, which cannot be its own parent.
fn department_fields(
    section: &DepartmentSection<'_>,
    draft: &DepartmentDraft,
    editing: Option<DepartmentId>,
) -> String {
    let parents: String = department_tree(section.departments)
        .into_iter()
        .filter(|(_, department)| Some(department.id) != editing)
        .map(|(depth, department)| {
            let id = department.id.as_uuid();
            format!(
                r#"<option value="{id}"{selected}>{indent}{name}</option>"#,
                selected = if draft.parent_id == Some(id) {
                    " selected"
                } else {
                    ""
                },
                indent = "&nbsp;&nbsp;".repeat(depth),
                name = escape_html_text(&department.name),
            )
        })
        .collect();
    format!(
        r##"<div class="grid grid-cols-1 gap-3 md:grid-cols-2">
                <label class="form-control"><span class="label text-xs opacity-70">Name</span>
                    <input class="input w-full" name="name" required maxlength="120" value="{name}"{slugify}></label>
                <label class="form-control"><span class="label text-xs opacity-70">Slug</span>
                    <input class="input w-full font-mono" name="slug" maxlength="64" value="{slug}" placeholder="sales"></label>
            </div>
            <p class="text-xs opacity-60">{slug_help}</p>
            <label class="form-control"><span class="label text-xs opacity-70">Description</span>
                <textarea class="textarea w-full" name="description" maxlength="500">{description}</textarea></label>
            <label class="form-control"><span class="label text-xs opacity-70">Charter</span>
                <textarea class="textarea min-h-32 w-full" name="charter" maxlength="{max_charter}">{charter}</textarea>
                <span class="label text-xs opacity-60">What the department is for. Its agents read it when they answer on its desks. At most {max_charter} bytes.</span></label>
            <label class="form-control"><span class="label text-xs opacity-70">Parent department</span>
                <select class="select w-full" name="parent_id"><option value="">None (top level)</option>{parents}</select></label>"##,
        // The new department's desk is created at its slug. Editing the slug later does not move
        // the desk (01DEP-P03-D8), so the edit form says where its address is changed instead.
        slug_help = match editing {
            None => format!(
                r#"The department's desk is created at this slug: <span class="font-mono">slug@{}.{}</span>"#,
                escape_html_text(&section.company.slug),
                escape_html_text(section.app_domain_name),
            ),
            Some(_) => "Changing the slug does not move the department's desk; change the desk's address in its own settings.".to_owned(),
        },
        // Only a new department's slug follows its name: renaming one keeps its desk's address.
        slugify = if editing.is_none() {
            r#" data-input="slugify""#
        } else {
            ""
        },
        name = escape_html_attr(&draft.name),
        slug = escape_html_attr(&draft.slug),
        description = escape_html_text(&draft.description),
        charter = escape_html_text(&draft.charter),
        max_charter = crate::entities::department::MAX_DEPARTMENT_CHARTER_BYTES,
    )
}

/// `<option>`s for a principal picker, agents and people labelled, with a leading blank choice.
fn principal_options(
    choices: &[DepartmentPrincipal],
    selected: Option<Uuid>,
    blank: &str,
) -> String {
    let mut options = format!(r#"<option value="">{}</option>"#, escape_html_text(blank));
    for choice in choices {
        let id = choice.principal_id.as_uuid();
        options.push_str(&format!(
            r#"<option value="{id}"{selected}>{label}</option>"#,
            selected = if selected == Some(id) {
                " selected"
            } else {
                ""
            },
            label = escape_html_text(&principal_label(choice)),
        ));
    }
    options
}

fn principal_label(principal: &DepartmentPrincipal) -> String {
    let kind = match principal.kind {
        PrincipalKind::Agent => "agent",
        PrincipalKind::Person => "person",
        PrincipalKind::External | PrincipalKind::System => "other",
    };
    if principal.detail.is_empty() {
        format!("{} ({kind})", principal.name)
    } else {
        format!("{} ({kind}, {})", principal.name, principal.detail)
    }
}

/// A principal the department names, or a placeholder when it is no longer the company's.
fn named_principal(view: &DepartmentView, id: crate::entities::transport::PrincipalId) -> String {
    match view.principal(id) {
        Some(principal) => format!(
            r#"<span class="badge badge-ghost badge-sm">{kind}</span> <span>{name}</span> <span class="font-mono text-xs opacity-60">{detail}</span>"#,
            kind = match principal.kind {
                PrincipalKind::Agent => "Agent",
                PrincipalKind::Person => "Person",
                PrincipalKind::External | PrincipalKind::System => "Other",
            },
            name = escape_html_text(&principal.name),
            detail = escape_html_text(&principal.detail),
        ),
        None => r#"<span class="opacity-60">Unknown principal</span>"#.to_string(),
    }
}

fn department_detail(
    section: &DepartmentSection<'_>,
    view: &DepartmentView,
    draft: Option<&DepartmentDraft>,
) -> String {
    let department = &view.detail.department;
    let parent = department
        .parent_id
        .map(|parent| {
            section
                .departments
                .iter()
                .find(|candidate| candidate.id == parent)
                .map_or_else(
                    || "A department you are not in".to_owned(),
                    |parent| escape_html_text(&parent.name),
                )
        })
        .unwrap_or_else(|| "None (top level)".to_owned());
    let description = department
        .description
        .as_deref()
        .map(|text| {
            format!(
                r#"<p class="mt-2 text-sm opacity-80">{}</p>"#,
                escape_html_text(text)
            )
        })
        .unwrap_or_default();
    let charter = match department.charter.as_deref() {
        Some(charter) => format!(
            r#"<p class="whitespace-pre-wrap text-sm">{}</p>"#,
            escape_html_text(charter)
        ),
        None => r#"<p class="text-sm opacity-60">No charter yet.</p>"#.to_string(),
    };
    // The department's own desk, wherever its address now is; the department's slug is only
    // where it was created.
    let address = match view.desks.iter().find(|desk| desk.own) {
        Some(desk) => format!(
            r#"<p id="department-address" class="font-mono text-xs opacity-60">{}</p>"#,
            escape_html_text(&format!(
                "{}@{}.{}",
                desk.slug, section.company.slug, section.app_domain_name
            ))
        ),
        None => r#"<p id="department-address" class="text-xs opacity-60">No department desk.</p>"#
            .to_owned(),
    };
    let edit = if section.editable {
        edit_form(
            section,
            department.id,
            draft
                .cloned()
                .unwrap_or_else(|| DepartmentDraft::stored(department)),
        )
    } else {
        String::new()
    };
    format!(
        r##"<div id="department-detail" class="space-y-5" data-department-id="{id}">
            <div class="rounded-box border border-base-300 bg-base-200 p-4">
                <h3 class="text-lg font-semibold">{name}</h3>
                {address}
                <dl class="mt-3 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
                    <dt class="opacity-60">Slug</dt><dd class="font-mono">{slug}</dd>
                    <dt class="opacity-60">Parent</dt><dd>{parent}</dd>
                </dl>
                {description}
                <h4 class="mt-4 text-xs font-semibold uppercase tracking-wider opacity-60">Charter</h4>
                {charter}
            </div>
            {lead}
            {members}
            {desks}
            {edit}
        </div>"##,
        id = department.id,
        name = escape_html_text(&department.name),
        slug = escape_html_text(department.slug.as_str()),
        lead = lead_card(section, view),
        members = members_card(section, view),
        desks = desks_card(section, &view.desks),
    )
}

fn lead_card(section: &DepartmentSection<'_>, view: &DepartmentView) -> String {
    let company_id = section.company.id;
    let department = view.detail.department.id;
    let holder = view
        .detail
        .roles
        .iter()
        .find(|holder| holder.role == DepartmentRole::Lead);
    let current = match holder {
        Some(holder) => {
            let clear = if section.editable {
                format!(
                    r##"<button type="button" class="btn btn-ghost btn-xs"
                        hx-delete="{endpoint}" hx-target="#department-section" hx-swap="outerHTML"
                        hx-confirm="Clear this department's lead?" hx-disabled-elt="this">Clear lead</button>"##,
                    endpoint = section_endpoint(
                        company_id,
                        &format!("/{department}/lead/{}", holder.principal_id)
                    ),
                )
            } else {
                String::new()
            };
            format!(
                r#"<div class="flex flex-wrap items-center justify-between gap-2"><div id="department-lead" class="flex flex-wrap items-center gap-1">{}</div>{clear}</div>"#,
                named_principal(view, holder.principal_id)
            )
        }
        None => r#"<p id="department-lead" class="text-sm opacity-60">No lead.</p>"#.to_string(),
    };
    let form = if section.editable {
        format!(
            r##"<form class="mt-3 flex flex-wrap items-end gap-2"
                hx-put="{endpoint}" hx-target="#department-section" hx-swap="outerHTML"
                hx-disabled-elt="find button[type='submit']">
                <label class="form-control min-w-0 flex-1"><span class="label text-xs opacity-70">Lead</span>
                    <select class="select select-sm w-full" name="principal_id" required>{options}</select></label>
                <button type="submit" class="btn btn-sm">{label}</button>
            </form>{cut}"##,
            cut = choices_cut_note(section),
            endpoint = section_endpoint(company_id, &format!("/{department}/lead")),
            options = principal_options(
                section.choices,
                holder.map(|holder| holder.principal_id.as_uuid()),
                "Choose a person or agent",
            ),
            label = if holder.is_some() {
                "Change lead"
            } else {
                "Assign lead"
            },
        )
    } else {
        String::new()
    };
    format!(
        r#"<div class="rounded-box border border-base-300 p-4">
            <h4 class="mb-2 text-xs font-semibold uppercase tracking-wider opacity-60">Lead</h4>
            {error}{current}{form}
        </div>"#,
        error = section.error_for(DepartmentForm::Lead),
    )
}

fn members_card(section: &DepartmentSection<'_>, view: &DepartmentView) -> String {
    let company_id = section.company.id;
    let department = view.detail.department.id;
    let rows: String = view
        .detail
        .members
        .iter()
        .map(|member| {
            let remove = if section.editable {
                format!(
                    r##"<button type="button" class="btn btn-ghost btn-xs"
                        hx-delete="{endpoint}" hx-target="#department-section" hx-swap="outerHTML"
                        hx-confirm="Remove this member from the department?" hx-disabled-elt="this">Remove</button>"##,
                    endpoint = section_endpoint(
                        company_id,
                        &format!("/{department}/members/{}", member.principal_id)
                    ),
                )
            } else {
                String::new()
            };
            format!(
                r#"<li class="flex flex-wrap items-center justify-between gap-2 py-1"><div class="flex flex-wrap items-center gap-1">{}</div>{remove}</li>"#,
                named_principal(view, member.principal_id)
            )
        })
        .collect();
    let list = if rows.is_empty() {
        r#"<p class="text-sm opacity-60">No members yet.</p>"#.to_string()
    } else {
        format!(r#"<ul id="department-members" class="divide-y divide-base-300">{rows}</ul>"#)
    };
    let form = if section.editable {
        let candidates: Vec<DepartmentPrincipal> = section
            .choices
            .iter()
            .filter(|choice| {
                !view
                    .detail
                    .members
                    .iter()
                    .any(|member| member.principal_id == choice.principal_id)
            })
            .cloned()
            .collect();
        format!(
            r##"<form class="mt-3 flex flex-wrap items-end gap-2"
                hx-post="{endpoint}" hx-target="#department-section" hx-swap="outerHTML"
                hx-disabled-elt="find button[type='submit']">
                <label class="form-control min-w-0 flex-1"><span class="label text-xs opacity-70">Add member</span>
                    <select class="select select-sm w-full" name="principal_id" required>{options}</select></label>
                <button type="submit" class="btn btn-sm">Add member</button>
            </form>{cut}"##,
            cut = choices_cut_note(section),
            endpoint = section_endpoint(company_id, &format!("/{department}/members")),
            options = principal_options(&candidates, None, "Choose a person or agent"),
        )
    } else {
        String::new()
    };
    format!(
        r#"<div class="rounded-box border border-base-300 p-4">
            <h4 class="mb-2 text-xs font-semibold uppercase tracking-wider opacity-60">Members</h4>
            {error}{list}{form}
        </div>"#,
        error = section.error_for(DepartmentForm::Members),
    )
}

fn desks_card(section: &DepartmentSection<'_>, desks: &[DepartmentDesk]) -> String {
    let company_id = section.company.id;
    let rows: String = desks
        .iter()
        .map(|desk| {
            let name = escape_html_text(&desk.name);
            // Desk settings are for owners and admins; a member reads the address only.
            let name = if section.editable {
                format!(
                    r#"<a class="link-hover" href="/ui/channels?company_id={company_id}&amp;channel_id={id}">{name}</a>"#,
                    id = desk.id,
                )
            } else {
                format!("<span>{name}</span>")
            };
            format!(
                r#"<li class="py-1">{name}{own}
                    <span class="block font-mono text-xs opacity-60">{slug}@{company_slug}.{domain}</span></li>"#,
                own = if desk.own {
                    r#" <span class="badge badge-ghost badge-sm">Department desk</span>"#
                } else {
                    ""
                },
                slug = escape_html_text(&desk.slug),
                company_slug = escape_html_text(&section.company.slug),
                domain = escape_html_text(section.app_domain_name),
            )
        })
        .collect();
    let list = if rows.is_empty() {
        r#"<p class="text-sm opacity-60">This department owns no desks.</p>"#.to_string()
    } else {
        format!(r#"<ul id="department-desks" class="divide-y divide-base-300">{rows}</ul>"#)
    };
    format!(
        r#"<div class="rounded-box border border-base-300 p-4">
            <h4 class="mb-2 text-xs font-semibold uppercase tracking-wider opacity-60">Desks</h4>
            {list}
        </div>"#
    )
}

/// Said under a picker that does not list everyone.
fn choices_cut_note(section: &DepartmentSection<'_>) -> String {
    if section.choices_cut {
        format!(
            r#"<p class="mt-1 text-xs opacity-60">Only the first {} people and agents are listed.</p>"#,
            crate::entities::department::MAX_DEPARTMENT_PRINCIPAL_CHOICES
        )
    } else {
        String::new()
    }
}

fn edit_form(
    section: &DepartmentSection<'_>,
    department: DepartmentId,
    draft: DepartmentDraft,
) -> String {
    let company_id = section.company.id;
    let endpoint = section_endpoint(company_id, &format!("/{department}"));
    format!(
        r##"<form id="department-edit" class="space-y-4 rounded-box border border-base-300 bg-base-200 p-4"
            hx-put="{endpoint}" hx-target="#department-section" hx-swap="outerHTML"
            hx-disabled-elt="find button[type='submit']">
            <h4 class="font-semibold">Edit department</h4>
            {error}
            {fields}
            <div class="flex items-center gap-3 border-t border-base-300 pt-4">
                {submit}
            </div>
        </form>
        <div id="department-delete" class="rounded-box border border-error/40 p-4">
            {delete_error}
            <div class="flex flex-wrap items-center justify-between gap-3">
                <p class="text-sm opacity-70">A department is deleted only when it owns no desks and has no child departments.</p>
                <button type="button" class="btn btn-error btn-outline btn-sm"
                    hx-delete="{endpoint}" hx-target="#department-section" hx-swap="outerHTML"
                    hx-confirm="Delete department '{name}'? Its members and roles go with it."
                    hx-disabled-elt="this">Delete department</button>
            </div>
        </div>"##,
        error = section.error_for(DepartmentForm::Edit),
        fields = department_fields(section, &draft, Some(department)),
        submit = busy_submit_button("Save department"),
        delete_error = section.error_for(DepartmentForm::Delete),
        name = escape_html_attr(&draft.name),
    )
}
