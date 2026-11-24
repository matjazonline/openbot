//! The company pane's Departments tab (01-departments 04A): the department tree beside the
//! selected department, and for an owner or admin the forms that change it.
//!
//! Like the Skills tab, the tab's content is one section, `#department-section`, that every
//! department request re-renders whole, so a write's refusal comes back on the form that made it
//! and the tree always shows what was just saved.

mod roles;
use super::*;
use crate::entities::company_member::CompanyMembership;
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
    /// `standing` or `goal` (Plan08 03A), and a goal's target date as typed (`YYYY-MM-DD`).
    pub kind: String,
    pub target_date: String,
    /// Create only: a new goal's deciding sponsor.
    pub sponsor: Option<Uuid>,
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
            kind: department.kind.as_str().to_owned(),
            target_date: department
                .target_date
                .map(|date| date.to_string())
                .unwrap_or_default(),
            sponsor: None,
        }
    }
}

/// Which form a refusal belongs to, so it renders on that form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepartmentForm {
    Create,
    Edit,
    Lead,
    Sponsors,
    Escalation,
    Members,
    Delete,
    /// Archiving or restoring (Plan08 02E.1).
    Archive,
    /// An archive refused for open runs it may cancel: the card offers to cancel and archive.
    ArchiveOpenWork,
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

#[derive(Clone, Copy)]
pub enum DepartmentKnowledge<'a> {
    Documents(&'a [crate::entities::knowledge::KnowledgeDocumentLayer]),
    ScopeLimit,
}

pub struct DepartmentSection<'a> {
    pub company: &'a Company,
    pub app_domain_name: &'a str,
    /// The departments the viewer may see: all of them for an owner or admin, otherwise their own.
    pub departments: &'a [Department],
    pub detail: DepartmentDetailPane<'a>,
    /// The people and agents the forms offer; empty unless the viewer manages a unit.
    pub choices: &'a [DepartmentPrincipal],
    /// The company has more people and agents than `choices` lists.
    pub choices_cut: bool,
    pub membership: CompanyMembership,
    /// The departments the viewer manages (Plan06): each control follows the server's rule for
    /// its target, so the pane offers nothing the viewer would be refused.
    pub managed: &'a std::collections::HashSet<DepartmentId>,
    /// The selected department's recent administration, loaded only for its managers.
    pub history: &'a [crate::entities::department::DepartmentAdministrationEntry],
    pub error: Option<(DepartmentForm, &'a str)>,
    pub notice: Option<&'a str>,
    pub knowledge: DepartmentKnowledge<'a>,
    /// The selected department's own model connections, loaded only for its managers.
    pub models: Option<&'a [crate::entities::company::CompanyModelConnection]>,
    /// The selected department's spend caps and counted spend, loaded only for its managers.
    pub budgets: Option<&'a [crate::application::ports::unit_resources::UnitBudgetState]>,
    /// The selected goal's status, for those who may see it (Plan08 03D.2).
    pub goal: Option<&'a crate::application::ports::department::GoalRollup>,
}

impl DepartmentSection<'_> {
    /// Manages at least one department, so may create one under it.
    fn administers(&self) -> bool {
        !self.managed.is_empty()
    }

    fn manages(&self, department: DepartmentId) -> bool {
        self.managed.contains(&department)
    }

    fn manages_root(&self) -> bool {
        self.membership.administers_company()
            || self
                .departments
                .iter()
                .find(|department| department.is_root)
                .is_some_and(|root| self.manages(root.id))
    }

    /// Editing or moving a department needs its parent; the root needs root management. An
    /// archived department takes no configuration (Plan08 02B.4), so offers no form.
    fn manages_placement(&self, department: &Department) -> bool {
        !department.is_archived()
            && match department.parent_id {
                Some(parent) => self.manages(parent),
                None => self.manages(department.id),
            }
    }

    /// Archiving, restoring and deleting a department needs its parent's manager; the root has
    /// none of the three.
    fn manages_lifecycle(&self, department: &Department) -> bool {
        department
            .parent_id
            .is_some_and(|parent| self.manages(parent))
    }

    /// The lead of a department is assigned by its parent's manager; the root's roles by the owner.
    fn assigns_roles(&self, department: &Department) -> bool {
        !department.is_archived()
            && match department.parent_id {
                Some(parent) => self.manages(parent),
                None => self.membership.is_owner(),
            }
    }

    /// Members follow the department's own manager. The root's membership is the company's
    /// team: only company administrators add or remove root agents (persons: the owner).
    fn manages_members(&self, department: &Department) -> bool {
        if department.is_archived() {
            false
        } else if department.is_root {
            self.membership.administers_company()
        } else {
            self.manages(department.id)
        }
    }

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
    let new_button = if section.administers() {
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
    let intro = if section.membership.administers_company() {
        "Departments group desks and the people and agents who work them."
    } else if section.administers() {
        "The departments you belong to, lead or manage."
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
        let message = if section.administers() {
            "No departments yet."
        } else {
            "You are not in any department."
        };
        return format!(
            r#"<p class="rounded-box border border-dashed border-base-300 p-5 text-sm opacity-60">{message}</p>"#
        );
    }
    let (archived, active): (Vec<Department>, Vec<Department>) = section
        .departments
        .iter()
        .cloned()
        .partition(Department::is_archived);
    let hidden = if archived.is_empty() {
        String::new()
    } else {
        // Open when the selected department is one of them, so the selection stays in view.
        let open = section
            .selected()
            .is_some_and(|selected| archived.iter().any(|unit| unit.id == selected));
        format!(
            r#"<details id="department-archived" class="mt-2"{open}>
                <summary class="cursor-pointer text-sm opacity-70">Show archived ({count})</summary>
                <ul class="menu w-full rounded-box bg-base-200 p-2 mt-2">{tree}</ul>
            </details>"#,
            open = if open { " open" } else { "" },
            count = archived.len(),
            tree = tree_items(section, &archived),
        )
    };
    let tree = if active.is_empty() {
        String::new()
    } else {
        format!(
            r#"<ul id="department-tree" class="menu w-full rounded-box bg-base-200 p-2">{}</ul>"#,
            tree_items(section, &active)
        )
    };
    format!("{tree}{hidden}")
}

/// One tree's items, nested: a child department sits indented under its parent.
fn tree_items(section: &DepartmentSection<'_>, departments: &[Department]) -> String {
    let company_id = section.company.id;
    let selected = section.selected();
    let mut html = String::new();
    let mut open: Option<usize> = None;
    for (depth, department) in department_tree(departments) {
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
    html
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
            <label class="form-control"><span class="label text-xs opacity-70">Deciding sponsor</span>
                <select class="select w-full" name="sponsor">{sponsor_options}</select>
                <span class="label text-xs opacity-60">For a goal: who approves its work. Add more sponsors on the goal once it exists.</span></label>
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
        sponsor_options = principal_options(section.choices, draft.sponsor, "No sponsor"),
        cut = choices_cut_note(section),
        submit = busy_submit_button("Create department"),
        cancel = section_endpoint(company_id, ""),
        cancel_url = departments_url(company_id, None),
    )
}

/// Name, address slug, description, charter and parent: what both the create and the edit form
/// set. `editing` is the department being edited, which cannot be its own parent.
fn parent_options(
    section: &DepartmentSection<'_>,
    draft: &DepartmentDraft,
    editing: Option<DepartmentId>,
) -> String {
    department_tree(section.departments)
        .into_iter()
        .filter(|(_, department)| {
            Some(department.id) != editing
                && !department.is_root
                && !department.is_archived()
                && section.manages(department.id)
        })
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
        .collect()
}

fn department_fields(
    section: &DepartmentSection<'_>,
    draft: &DepartmentDraft,
    editing: Option<DepartmentId>,
) -> String {
    let is_root = editing
        .and_then(|id| section.departments.iter().find(|entry| entry.id == id))
        .is_some_and(|entry| entry.is_root);
    let parents = parent_options(section, draft, editing);
    format!(
        r##"<div class="grid grid-cols-1 gap-3 md:grid-cols-2">
                <label class="form-control"><span class="label text-xs opacity-70">Name</span>
                    <input class="input w-full" name="name" required maxlength="120" value="{name}"{slugify}{readonly}></label>
                <label class="form-control"><span class="label text-xs opacity-70">Slug</span>
                    <input class="input w-full font-mono" name="slug" maxlength="64" value="{slug}" placeholder="sales"{readonly}></label>
            </div>
            <p class="text-xs opacity-60">{slug_help}</p>
            <label class="form-control"><span class="label text-xs opacity-70">Description</span>
                <textarea class="textarea w-full" name="description" maxlength="500">{description}</textarea></label>
            <label class="form-control"><span class="label text-xs opacity-70">Charter</span>
                <textarea class="textarea min-h-32 w-full" name="charter" maxlength="{max_charter}">{charter}</textarea>
                <span class="label text-xs opacity-60">What the department is for. Its agents read it when they answer on its desks. At most {max_charter} bytes.</span></label>
            {parent_field}{kind_field}"##,
        // A goal is a unit with an end (Plan08 03A); the root is always standing.
        kind_field = if is_root {
            String::new()
        } else {
            format!(
                r#"<div class="grid grid-cols-1 gap-3 md:grid-cols-2">
                <label class="form-control"><span class="label text-xs opacity-70">Kind</span>
                    <select class="select w-full" name="kind"><option value="standing"{standing}>Standing department</option><option value="goal"{goal}>Goal</option></select></label>
                <label class="form-control"><span class="label text-xs opacity-70">Target date</span>
                    <input class="input w-full" type="date" name="target_date" value="{target_date}"></label>
            </div>
            <p class="text-xs opacity-60">A goal records every run done for it, in any department. Its charter says when it is done; the target date applies to a goal only.</p>"#,
                standing = if draft.kind == "goal" { "" } else { " selected" },
                goal = if draft.kind == "goal" { " selected" } else { "" },
                target_date = escape_html_attr(&draft.target_date),
            )
        },
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
        readonly = if is_root { " readonly" } else { "" },
        parent_field = if is_root {
            r#"<input type="hidden" name="parent_id" value=""><p class="text-xs opacity-60">The company is the root department. Its name and slug follow company settings.</p>"#.to_owned()
        } else {
            format!(r#"<label class="form-control"><span class="label text-xs opacity-70">Parent department</span><select class="select w-full" name="parent_id"{required}>{root}{parents}</select></label>"#,
                root = if section.manages_root() { r#"<option value="">Company root</option>"# } else { "" },
                required = if section.manages_root() { "" } else { " required" })
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

fn department_header(section: &DepartmentSection<'_>, view: &DepartmentView) -> String {
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
    format!(
        r#"            <div class="rounded-box border border-base-300 bg-base-200 p-4">
                <h3 class="text-lg font-semibold">{name}</h3>{root_badge}
                {address}
                <dl class="mt-3 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
                    <dt class="opacity-60">Slug</dt><dd class="font-mono">{slug}</dd>
                    <dt class="opacity-60">Parent</dt><dd>{parent}</dd>
                </dl>
                {description}
                <h4 class="mt-4 text-xs font-semibold uppercase tracking-wider opacity-60">Charter</h4>
                {charter}
            </div>"#,
        name = escape_html_text(&department.name),
        slug = escape_html_text(department.slug.as_str()),
        root_badge = if department.is_root {
            r#"<span class="badge badge-ghost badge-sm">Company root</span>"#
        } else if department.is_archived() {
            r#"<span id="department-archived-badge" class="badge badge-warning badge-sm">Archived</span>"#
        } else {
            ""
        },
    )
}

fn department_detail(
    section: &DepartmentSection<'_>,
    view: &DepartmentView,
    draft: Option<&DepartmentDraft>,
) -> String {
    let department = &view.detail.department;
    let edit = if section.manages_placement(department) {
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
            {header}
            {lead}
            {sponsors}
            {escalation}
            {roles_cut}
            {goal}
            {members}
            {desks}
            {knowledge}
            {models}
            {budgets}
            {history}
            {edit}
            {lifecycle}
        </div>"##,
        id = department.id,
        lifecycle = lifecycle_card(section, department),
        history = if section.manages(department.id) {
            history_card(section)
        } else {
            String::new()
        },
        header = department_header(section, view),
        lead = lead_card(section, view),
        sponsors = roles::sponsors_card(section, view),
        escalation = roles::escalation_card(section, view),
        roles_cut = if view.detail.department.is_root && view.detail.roles_truncated {
            "<p class=\"text-xs opacity-60\">Some role badges are omitted.</p>"
        } else {
            ""
        },
        goal = section.goal.map_or(String::new(), |rollup| goal_card(
            section.company.id,
            rollup
        )),
        members = members_card(section, view),
        desks = desks_card(section, view.detail.department.id, &view.desks),
        knowledge = knowledge_card(section, department),
        models = models_card(section, department),
        budgets = budgets_card(section, department),
    )
}

/// A goal's status (Plan08 03D.2): runs by department, calls, waits, spend and time. Counts only;
/// each run link keeps the run page's own check.
/// A goal's status card (Plan08 03D.2): on the goal's department page for its members and
/// managers, and on the Goals tab for anyone who may see the goal.
pub fn goal_card(
    company: Uuid,
    rollup: &crate::application::ports::department::GoalRollup,
) -> String {
    let usd = crate::services::model_usage::usd;
    let time = |at: Option<chrono::DateTime<chrono::Utc>>| {
        at.map_or("—".to_owned(), |at| {
            at.format("%Y-%m-%d %H:%M UTC").to_string()
        })
    };
    let rows: String = rollup
        .units
        .iter()
        .map(|unit| {
            format!(
                "<tr><th scope=\"row\">{name}</th><td>{open}</td><td>{waiting}</td><td>{succeeded}</td><td>{failed}</td><td>{cancelled}</td><td>{spent}</td></tr>",
                name = unit
                    .name
                    .as_deref()
                    .map_or("A deleted department".to_owned(), escape_html_text),
                open = unit.open(),
                waiting = unit.waiting,
                succeeded = unit.succeeded,
                failed = unit.failed,
                cancelled = unit.cancelled,
                spent = usd(unit.spent_micros),
            )
        })
        .collect();
    let waits: String = rollup
        .waiting
        .iter()
        .map(|wait| {
            format!(
                r#"<li><a class="link" href="/ui/workflow-runs?company_id={company}&amp;run_id={run}">Run {short}</a> in {unit}, waiting on {who} since {since}</li>"#,
                run = wait.run_id,
                short = &wait.run_id.to_string()[..8],
                unit = wait
                    .unit_name
                    .as_deref()
                    .map_or("a deleted department".to_owned(), escape_html_text),
                who = escape_html_text(&wait.reviewers.join(", ")),
                since = wait.since.format("%Y-%m-%d %H:%M UTC"),
            )
        })
        .collect();
    let waits = if rollup.waiting.is_empty() {
        "<p class=\"text-sm opacity-70\">Nothing is waiting on a person.</p>".to_owned()
    } else {
        format!(
            r#"<p class="text-sm">{total} decision(s) waiting on people{shown}:</p><ul class="list-disc pl-5 text-sm space-y-1">{waits}</ul>"#,
            total = rollup.waiting_total,
            shown = if rollup.waiting_total > rollup.waiting.len() as i64 {
                format!(", the oldest {} shown", rollup.waiting.len())
            } else {
                String::new()
            },
        )
    };
    let calls = &rollup.calls;
    format!(
        r##"<section id="goal-status" class="card bg-base-200 p-4 space-y-3">
            <h3 class="font-semibold">Goal status</h3>
            <p class="text-xs opacity-70">Work done for this goal in every department: counts, states and spend only. Opening a run still needs access to its desk.{cut}</p>
            <p class="text-sm">Spend {spent}{cap}. Calls: {done} done, {denied} denied, {handed_off} handed off, {no_reply} without reply, {error} failed, {open} open. Open escalations: {escalations}.</p>
            <p class="text-sm">First run {first}; last finished {last}; last activity {activity}; time waiting on people {waited} h.</p>
            <table class="table table-sm"><thead><tr><th scope="col">Department</th><th scope="col">Open</th><th scope="col">Waiting</th><th scope="col">Succeeded</th><th scope="col">Failed</th><th scope="col">Cancelled</th><th scope="col">Spend (USD)</th></tr></thead><tbody>{rows}</tbody></table>
            {waits}
            <a class="link text-sm" href="/ui/workflow-runs?company_id={company}&amp;goal_id={goal}">Runs for this goal</a>
        </section>"##,
        cut = if rollup.truncated {
            format!(" Counts cover the newest {} runs.", rollup.runs_counted)
        } else {
            String::new()
        },
        spent = usd(rollup.spend.spent_micros),
        cap = rollup
            .spend
            .cap_micros
            .map_or(" (no total cap)".to_owned(), |cap| format!(
                " of a {} total cap",
                usd(cap)
            )),
        done = calls.done,
        denied = calls.denied,
        handed_off = calls.handed_off,
        no_reply = calls.no_reply,
        error = calls.error,
        open = calls.open,
        escalations = rollup.escalations_open,
        first = time(rollup.first_run_at),
        last = time(rollup.last_finished_at),
        activity = time(rollup.last_activity_at),
        waited = rollup.waiting_on_people_seconds / 3600,
        goal = rollup.goal,
    )
}

/// The department's spend caps (Plan07 02B/02C), for its managers: counted spend this UTC day and
/// month and a form to set or clear each cap. A cap stops new runs here and below once reached.
fn budgets_card(section: &DepartmentSection<'_>, department: &Department) -> String {
    let Some(states) = section.budgets else {
        return String::new();
    };
    // An archived department keeps its caps and spend for reading; they change with a restore.
    let form = if department.is_archived() {
        String::new()
    } else {
        format!(
            r##"<form class="flex flex-wrap gap-3 items-end" hx-post="/ui/companies/{company}/departments/{unit}/budgets" hx-target="#department-section" hx-swap="outerHTML">
                <label class="form-control"><span class="text-xs opacity-70">Daily cap (USD)</span><input name="day_cap" value="{day}" inputmode="decimal" class="input input-sm"></label>
                <label class="form-control"><span class="text-xs opacity-70">Monthly cap (USD)</span><input name="month_cap" value="{month}" inputmode="decimal" class="input input-sm"></label>{total}
                <button type="submit" class="btn btn-sm btn-primary">Save spend caps</button>
            </form>"##,
            company = section.company.id,
            unit = department.id,
            day = escape_html_attr(&cap(states, "day")),
            month = escape_html_attr(&cap(states, "month")),
            total = if department.kind == crate::entities::department::DepartmentKind::Goal {
                format!(
                    r#"
                <label class="form-control"><span class="text-xs opacity-70">Goal total cap (USD)</span><input name="total_cap" value="{}" inputmode="decimal" class="input input-sm"></label>"#,
                    escape_html_attr(&cap(states, "total"))
                )
            } else {
                String::new()
            },
        )
    };
    let usd = crate::services::model_usage::usd;
    let rows: String = states
        .iter()
        .map(|state| {
            format!(
                "<tr><th scope=\"row\">{}</th><td>{}</td><td>{}</td><td>{}</td></tr>",
                match state.period {
                    "day" => "Today (UTC)",
                    "month" => "This month (UTC)",
                    _ => "Goal total, every department",
                },
                state.cap_micros.map_or("No cap".to_owned(), usd),
                usd(state.spent_micros),
                if state.unpriced_tokens > 0 {
                    format!("{} unpriced tokens not counted", state.unpriced_tokens)
                } else {
                    String::new()
                },
            )
        })
        .collect();
    format!(
        r##"<section id="department-budgets" class="card bg-base-200 p-4 space-y-3">
            <h3 class="font-semibold">Spend caps</h3>
            <p class="text-xs opacity-70">Counted spend is each settled model call's priced cost, for this department and every one below it. Once a cap is reached, new runs here and below are refused until the next UTC day or month; runs already started finish.{goal_note}</p>
            <table class="table table-sm"><thead><tr><th scope="col">Period</th><th scope="col">Cap (USD)</th><th scope="col">Counted (USD)</th><th scope="col"></th></tr></thead><tbody>{rows}</tbody></table>
            {form}
        </section>"##,
        goal_note = if department.kind == crate::entities::department::DepartmentKind::Goal {
            " A goal's total cap counts the work done for it in every department, for as long as it runs; once reached, new work for the goal is refused until the cap is raised."
        } else {
            ""
        },
    )
}

/// The cap set for `period`, as the form shows it.
fn cap(
    states: &[crate::application::ports::unit_resources::UnitBudgetState],
    period: &str,
) -> String {
    states
        .iter()
        .find(|state| state.period == period)
        .and_then(|state| state.cap_micros)
        .map_or(String::new(), crate::services::model_usage::usd)
}

/// Archive or restore, and delete, for the parent's manager (Plan08 02E.1). An archive refused
/// for open runs it may cancel offers to cancel them and archive; an archived department says
/// when and by whom, and is restored only under an active parent.
fn lifecycle_card(section: &DepartmentSection<'_>, department: &Department) -> String {
    if !section.manages_lifecycle(department) {
        return String::new();
    }
    let endpoint = section_endpoint(section.company.id, &format!("/{}", department.id));
    let name = escape_html_attr(&department.name);
    let lifecycle = match &department.archived {
        None => {
            let cancel = match section.error {
                Some((DepartmentForm::ArchiveOpenWork, _)) => format!(
                    r##"<button type="button" id="department-archive-cancel" class="btn btn-warning btn-sm"
                    hx-post="{endpoint}/archive" hx-vals='{{"cancel_open_runs":"true"}}'
                    hx-target="#department-section" hx-swap="outerHTML"
                    hx-confirm="Cancel the open runs of '{name}' and the departments below it, then archive?"
                    hx-disabled-elt="this">Cancel open runs and archive</button>"##
                ),
                _ => String::new(),
            };
            format!(
                r##"{error}{open_work}<div class="flex flex-wrap items-center justify-between gap-3">
                <p class="text-sm opacity-70">Archiving stops all work in this department and every one below it. What it holds stays readable, and a restore brings it back as it was.</p>
                <div class="flex gap-2">{cancel}<button type="button" id="department-archive" class="btn btn-outline btn-sm"
                    hx-post="{endpoint}/archive" hx-target="#department-section" hx-swap="outerHTML"
                    hx-confirm="Archive department '{name}' and every department below it?"
                    hx-disabled-elt="this">Archive department</button></div>
            </div>"##,
                error = section.error_for(DepartmentForm::Archive),
                open_work = section.error_for(DepartmentForm::ArchiveOpenWork),
            )
        }
        Some(archive) => {
            let parent_archived = department.parent_id.is_some_and(|parent| {
                section
                    .departments
                    .iter()
                    .any(|unit| unit.id == parent && unit.is_archived())
            });
            let restore = if parent_archived {
                r#"<p id="department-restore-blocked" class="text-sm opacity-70">Restore the department above it first.</p>"#.to_owned()
            } else {
                format!(
                    r##"<button type="button" id="department-restore" class="btn btn-primary btn-sm"
                    hx-post="{endpoint}/restore" hx-target="#department-section" hx-swap="outerHTML"
                    hx-disabled-elt="this">Restore department</button>"##
                )
            };
            format!(
                r##"{error}<div class="flex flex-wrap items-center justify-between gap-3">
                <p id="department-archived-at" class="text-sm">Archived <time datetime="{at}">{shown}</time> by {by}. Departments below it stay archived until each is restored.</p>
                {restore}
            </div>"##,
                error = section.error_for(DepartmentForm::Archive),
                at = archive.at.to_rfc3339(),
                shown = archive.at.format("%Y-%m-%d %H:%M UTC"),
                by = escape_html_text(&archive.by.actor_name),
            )
        }
    };
    format!(
        r##"<section id="department-lifecycle" class="space-y-3">
        <div class="rounded-box border border-base-300 p-4 space-y-2">{lifecycle}</div>
        <div id="department-delete" class="rounded-box border border-error/40 p-4">
            {error}<div class="flex flex-wrap items-center justify-between gap-3">
                <p class="text-sm opacity-70">A department is deleted only when it owns no desks and has no child departments.</p>
                <button type="button" class="btn btn-error btn-outline btn-sm"
                    hx-delete="{endpoint}" hx-target="#department-section" hx-swap="outerHTML"
                    hx-confirm="Delete department '{name}'? Its members and roles go with it."
                    hx-disabled-elt="this">Delete department</button>
            </div></div>
        </section>"##,
        error = section.error_for(DepartmentForm::Delete),
    )
}

/// The department's own model connections (Plan07 02A), for its managers: a unit marks at most
/// one as its default, which agents placed in it or below pin when they name no model.
fn models_card(section: &DepartmentSection<'_>, department: &Department) -> String {
    let Some(connections) = section.models else {
        return String::new();
    };
    format!(
        r##"<section id="department-models" class="card bg-base-200 p-4 space-y-3">
            <h3 class="font-semibold">Model connections</h3>
            <p class="text-xs opacity-70">Agents placed here or below pin this department's default when they name no model; without one, the nearest department above decides.</p>
            <form class="space-y-3" hx-post="/ui/companies/{company}/departments/{unit}/models" hx-target="#department-section" hx-swap="outerHTML">
                {fields}
                <button type="submit" class="btn btn-sm btn-primary">Save model connections</button>
            </form>
        </section>"##,
        company = section.company.id,
        unit = department.id,
        fields = super::company_settings::unit_model_connection_fields(
            &super::company_settings::model_connection_drafts(connections)
        ),
    )
}

/// The last administration events of the department, newest first (Plan06 02B.2), with the
/// labels and role they were written with.
fn history_card(section: &DepartmentSection<'_>) -> String {
    let rows: String = section
        .history
        .iter()
        .map(|entry| {
            let actor = entry.actor.as_deref().unwrap_or("System");
            let subject = entry
                .subject
                .as_deref()
                .map(|subject| format!(": {}", escape_html_text(subject)))
                .unwrap_or_default();
            let role = entry
                .role
                .map(|role| format!(" ({})", role.as_str()))
                .unwrap_or_default();
            let acting = entry
                .acting_role
                .as_deref()
                .map(|acting| {
                    format!(
                        r#" <span class="badge badge-ghost badge-sm" data-acting-role="{}">as {}</span>"#,
                        escape_html_attr(acting),
                        escape_html_text(&acting_role_label(section, acting))
                    )
                })
                .unwrap_or_default();
            format!(
                r#"<li class="py-1 text-sm"><span class="font-medium">{actor}</span> {action}{subject}{role}{acting}
                    <span class="block text-xs opacity-60">{at}</span></li>"#,
                actor = escape_html_text(actor),
                action = entry.action.label(),
                at = super::format_date_time(entry.created_at),
            )
        })
        .collect();
    let list = if rows.is_empty() {
        r#"<p class="text-sm opacity-60">No administration recorded yet.</p>"#.to_owned()
    } else {
        format!(r#"<ul class="divide-y divide-base-300">{rows}</ul>"#)
    };
    format!(
        r#"<section id="department-history" class="card bg-base-200 p-4 space-y-2"><h3 class="font-semibold">Recent administration</h3>{list}</section>"#
    )
}

/// `owner`, `admin`, `lead:<id>` or `sponsor:<id>` as a reader names it: the lead or deciding
/// sponsor of which department.
fn acting_role_label(section: &DepartmentSection<'_>, acting: &str) -> String {
    let (role, unit) = match (
        acting.strip_prefix("lead:"),
        acting.strip_prefix("sponsor:"),
    ) {
        (Some(unit), _) => ("lead", unit),
        (_, Some(unit)) => ("sponsor", unit),
        _ => return acting.to_owned(),
    };
    section
        .departments
        .iter()
        .find(|department| department.id.as_uuid().to_string() == unit)
        .map_or_else(
            || format!("a department {role}"),
            |department| format!("{role} of {}", department.name),
        )
}

fn knowledge_card(section: &DepartmentSection<'_>, department: &Department) -> String {
    let DepartmentKnowledge::Documents(layers) = section.knowledge else {
        return r#"<section id="department-knowledge" class="card bg-base-200 p-4 space-y-2"><h3 class="font-semibold">Knowledge</h3><p data-knowledge-unavailable="scope-limit">Knowledge is unavailable because your readable departments exceed the search limit. Ask an owner or administrator for help.</p></section>"#.into();
    };
    let rows = layers
        .iter()
        .map(|layer| format!("<li>{}</li>", escape_html_text(&layer.file_name())))
        .collect::<String>();
    let rows = if rows.is_empty() {
        "<li>No documents at your access level.</li>".into()
    } else {
        rows
    };
    let guidance = if department.is_root {
        ""
    } else {
        "<p>Move or delete this department’s documents before deleting the department. Reparenting leaves the documents in this department.</p>"
    };
    format!(
        r#"<section id="department-knowledge" class="card bg-base-200 p-4 space-y-2"><h3 class="font-semibold">Knowledge</h3><ul>{rows}</ul>{guidance}<a class="link" href="/ui/knowledge?company_id={company}&amp;unit_id={unit}">Open department knowledge</a></section>"#,
        company = section.company.id,
        unit = department.id
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
            let clear = if section.assigns_roles(&view.detail.department) {
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
    let form = if section.assigns_roles(&view.detail.department) {
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

fn removable_member(
    section: &DepartmentSection<'_>,
    view: &DepartmentView,
    principal: crate::entities::transport::PrincipalId,
) -> bool {
    if !section.manages_members(&view.detail.department) {
        return false;
    }
    if !view.detail.department.is_root {
        return true;
    }
    if view
        .detail
        .roles
        .iter()
        .any(|holder| holder.role == DepartmentRole::Owner && holder.principal_id == principal)
    {
        return false;
    }
    match view.principal(principal).map(|entry| entry.kind) {
        Some(PrincipalKind::Person) => section.membership.is_owner(),
        Some(PrincipalKind::Agent) => true,
        _ => false,
    }
}

fn root_authority_badge(
    view: &DepartmentView,
    principal: crate::entities::transport::PrincipalId,
) -> &'static str {
    if !view.detail.department.is_root {
        return "";
    }
    if view
        .detail
        .roles
        .iter()
        .any(|holder| holder.principal_id == principal && holder.role == DepartmentRole::Owner)
    {
        return r#"<span class="badge badge-ghost badge-sm">Owner</span>"#;
    }
    if view
        .detail
        .roles
        .iter()
        .any(|holder| holder.principal_id == principal && holder.role == DepartmentRole::Admin)
    {
        return r#"<span class="badge badge-ghost badge-sm">Admin</span>"#;
    }
    ""
}

fn members_card(section: &DepartmentSection<'_>, view: &DepartmentView) -> String {
    let company_id = section.company.id;
    let department = view.detail.department.id;
    let rows: String = view
        .detail
        .members
        .iter()
        .map(|member| {
            let remove = if removable_member(section, view, member.principal_id) {
                format!(
                    r##"<button type="button" class="btn btn-ghost btn-xs"
                        hx-delete="{endpoint}" hx-target="#department-section" hx-swap="outerHTML"
                        hx-confirm="{confirmation}" hx-disabled-elt="this">Remove</button>"##,
                    confirmation = if view.detail.department.is_root && view.principal(member.principal_id).is_some_and(|entry| entry.kind == PrincipalKind::Person) {
                        "Remove this person from the company?"
                    } else { "Remove this member from the department?" },
                    endpoint = section_endpoint(
                        company_id,
                        &format!("/{department}/members/{}", member.principal_id)
                    ),
                )
            } else {
                String::new()
            };
            format!(
                r#"<li class="flex flex-wrap items-center justify-between gap-2 py-1"><div class="flex flex-wrap items-center gap-1">{} {authority}</div>{remove}</li>"#,
                named_principal(view, member.principal_id),
                authority = root_authority_badge(view, member.principal_id),
            )
        })
        .collect();
    let list = if rows.is_empty() {
        r#"<p class="text-sm opacity-60">No members yet.</p>"#.to_string()
    } else {
        format!(r#"<ul id="department-members" class="divide-y divide-base-300">{rows}</ul>"#)
    };
    let form = if section.manages_members(&view.detail.department) {
        let candidates: Vec<DepartmentPrincipal> = section
            .choices
            .iter()
            .filter(|choice| {
                (!view.detail.department.is_root || choice.kind == PrincipalKind::Agent)
                    && !view
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
            {error}{list}{cut}{form}
        </div>"#,
        error = section.error_for(DepartmentForm::Members),
        cut = if view.detail.members_truncated {
            format!(
                "<p class=\"mt-2 text-xs opacity-60\">Showing the first {} members.</p>",
                crate::application::ports::department::MAX_DEPARTMENT_ROSTER_ENTRIES
            )
        } else {
            String::new()
        },
    )
}

fn desks_card(
    section: &DepartmentSection<'_>,
    department: DepartmentId,
    desks: &[DepartmentDesk],
) -> String {
    let company_id = section.company.id;
    let rows: String = desks
        .iter()
        .map(|desk| {
            let name = escape_html_text(&desk.name);
            // Desk settings are for the department's managers; anyone else reads the address only.
            let name = if section.manages(department) {
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
        </form>"##,
        error = section.error_for(DepartmentForm::Edit),
        fields = department_fields(section, &draft, Some(department)),
        submit = busy_submit_button("Save department"),
    )
}

/// The Goals tab (Plan08 03D.2): the goals the viewer may see, each with its lead, deciding
/// sponsor, target date, open, waiting and failed runs and spend against its total cap. A goal
/// opens here with its status card (`goal_id`), for anyone who may see it, including a viewer of
/// one of its desks who could not open its department page (08G-P03-B2-F2).
pub fn goals_list(
    company_id: Uuid,
    goals: &[crate::application::ports::department::GoalSummary],
    cut: bool,
    archived: bool,
    selected: Option<&crate::application::ports::department::GoalRollup>,
) -> String {
    let usd = crate::services::model_usage::usd;
    let rows: String = goals
        .iter()
        .map(|goal| {
            let cut = if goal.truncated { "+" } else { "" };
            format!(
                r#"<tr><th scope="row"><a class="link" href="/ui/companies?company_id={company_id}&amp;tab=goals&amp;goal_id={id}{archived_query}">{name}</a></th><td>{lead}</td><td>{decider}</td><td>{target}</td><td>{open}{cut}</td><td>{waiting}{cut}</td><td>{failed}{cut}</td><td>{spend}</td></tr>"#,
                id = goal.goal,
                archived_query = if archived { "&amp;archived=true" } else { "" },
                name = escape_html_text(&goal.name),
                lead = goal.lead.as_deref().map_or("—".to_owned(), escape_html_text),
                decider = goal.decider.as_deref().map_or("—".to_owned(), escape_html_text),
                target = goal
                    .target_date
                    .map_or("—".to_owned(), |date| date.format("%Y-%m-%d").to_string()),
                open = goal.open,
                waiting = goal.waiting,
                failed = goal.failed,
                spend = match goal.spend.cap_micros {
                    Some(cap) => format!("{} of {}", usd(goal.spend.spent_micros), usd(cap)),
                    None => usd(goal.spend.spent_micros),
                },
            )
        })
        .collect();
    let table = if goals.is_empty() {
        format!(
            r#"<p class="text-sm opacity-70">{}</p>"#,
            if archived {
                "No archived goals you can see."
            } else {
                "No active goals you can see. A goal is a department of the Goal kind; create one on the Departments tab."
            }
        )
    } else {
        format!(
            r#"<table class="table table-sm" id="goals-table"><thead><tr><th scope="col">Goal</th><th scope="col">Lead</th><th scope="col">Deciding sponsor</th><th scope="col">Target date</th><th scope="col">Open</th><th scope="col">Waiting</th><th scope="col">Failed</th><th scope="col">Spend (USD)</th></tr></thead><tbody>{rows}</tbody></table>
            <p class="text-xs opacity-60">Counts cover each goal's newest {limit} runs; "+" marks a goal with more.</p>{more}"#,
            limit = crate::application::ports::department::GOALS_LIST_RUN_LIMIT,
            more = if cut {
                format!(
                    r#"<p class="text-xs opacity-60" id="goals-cut">Showing the newest {} goals; more exist. Open an older goal from its department.</p>"#,
                    crate::application::ports::department::GOALS_LIST_LIMIT
                )
            } else {
                String::new()
            },
        )
    };
    format!(
        r##"<div id="goals-section" class="flex-1 overflow-y-auto px-4 py-4 sm:px-6 space-y-4">
            <div class="flex items-center gap-3">
                <h3 class="font-semibold">{heading}</h3>
                <a class="link text-sm" href="/ui/companies?company_id={company_id}&amp;tab=goals{toggle}">{toggle_label}</a>
            </div>
            {selected}
            {table}
        </div>"##,
        selected = selected.map_or(String::new(), |rollup| goal_card(company_id, rollup)),
        heading = if archived {
            "Archived goals"
        } else {
            "Active goals"
        },
        toggle = if archived { "" } else { "&amp;archived=true" },
        toggle_label = if archived {
            "Show active goals"
        } else {
            "Show archived goals"
        },
    )
}
