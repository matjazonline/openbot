//! Rendering of the department UI (01-departments 04A, 04B.1–3): the Departments tab, the desk
//! form's owner fields, desk lists grouped by department, and the agent's desk and departments:
//! `department_ui_`.
use super::*;
use crate::application::agent::{AgentAddress, AgentDirectoryEntry, AgentKey, DepartmentRef};
use crate::entities::agent_capability::AgentId;
use crate::entities::creation::CreationProvenance;
use crate::entities::department::{
    Department, DepartmentDesk, DepartmentId, DepartmentMember, DepartmentPrincipal,
    DepartmentRole, DepartmentRoleHolder, department_tree,
};
use crate::entities::participant::PrincipalKind;
use crate::entities::transport::PrincipalId;
use crate::use_cases::department::{DepartmentDetail, DepartmentView};

fn department(company_id: Uuid, name: &str, slug: &str, parent: Option<&Department>) -> Department {
    Department {
        kind: Default::default(),
        target_date: None,
        is_root: false,
        escalation: None,
        archived: None,
        id: DepartmentId::new(Uuid::new_v4()),
        company_id,
        name: name.into(),
        slug: slug.to_owned().into(),
        description: None,
        charter: None,
        parent_id: parent.map(|parent| parent.id),
        created_by: CreationProvenance::system(),
        created_at: Utc::now(),
    }
}

fn principal(kind: PrincipalKind, name: &str, detail: &str) -> DepartmentPrincipal {
    DepartmentPrincipal {
        principal_id: PrincipalId::new(Uuid::new_v4()),
        kind,
        name: name.into(),
        detail: detail.into(),
    }
}

/// Revenue > Sales > Inside sales, and a second root, Support.
struct Tree {
    company: Company,
    revenue: Department,
    sales: Department,
    inside: Department,
    support: Department,
}

fn tree() -> Tree {
    let company = mailbox_company();
    let revenue = department(company.id, "Revenue", "revenue", None);
    let sales = department(company.id, "Sales <team>", "sales", Some(&revenue));
    let inside = department(company.id, "Inside sales", "inside-sales", Some(&sales));
    let support = department(company.id, "Support", "support", None);
    Tree {
        company,
        revenue,
        sales,
        inside,
        support,
    }
}

impl Tree {
    fn all(&self) -> Vec<Department> {
        vec![
            self.support.clone(),
            self.inside.clone(),
            self.sales.clone(),
            self.revenue.clone(),
        ]
    }
}

/// An owner manages every listed department and their parents; a member manages none.
fn managed_when(
    editable: bool,
    departments: &[Department],
) -> std::collections::HashSet<DepartmentId> {
    if !editable {
        return Default::default();
    }
    departments
        .iter()
        .flat_map(|department| std::iter::once(department.id).chain(department.parent_id))
        .collect()
}

fn section<'a>(
    tree: &'a Tree,
    departments: &'a [Department],
    detail: DepartmentDetailPane<'a>,
    choices: &'a [DepartmentPrincipal],
    editable: bool,
    error: Option<(DepartmentForm, &'a str)>,
) -> String {
    let managed = managed_when(editable, departments);
    company_departments_section(&DepartmentSection {
        company: &tree.company,
        app_domain_name: "example.com",
        departments,
        detail,
        choices,
        choices_cut: false,
        membership: if editable {
            crate::entities::company_member::CompanyMembership::Owner
        } else {
            crate::entities::company_member::CompanyMembership::Member
        },
        managed: &managed,
        history: &[],
        error,
        notice: None,
        knowledge: DepartmentKnowledge::Documents(&[]),
        models: None,
        budgets: None,
    })
}

#[test]
fn department_ui_tree_orders_each_root_before_its_children() {
    let tree = tree();
    let all = tree.all();
    let ordered: Vec<(usize, &str)> = department_tree(&all)
        .into_iter()
        .map(|(depth, department)| (depth, department.slug.as_str()))
        .collect();
    assert_eq!(
        ordered,
        [
            (0, "revenue"),
            (1, "sales"),
            (2, "inside-sales"),
            (0, "support")
        ]
    );
    // A member sees only their own department: its unseen parent does not drop it.
    let only_inside = vec![tree.inside.clone()];
    assert_eq!(department_tree(&only_inside).len(), 1);
    assert_eq!(department_tree(&only_inside)[0].0, 0);
}

#[test]
fn department_ui_pane_indents_the_tree_and_shows_the_selected_department() {
    let tree = tree();
    let all = tree.all();
    let lead = principal(PrincipalKind::Agent, "Closer", "closer");
    let worker = principal(PrincipalKind::Person, "dana", "dana@example.com");
    // The department's slug was edited after its desk was created: the desk did not move.
    let desk = DepartmentDesk {
        id: Uuid::new_v4(),
        name: "Sales desk".into(),
        slug: "sales-desk".into(),
        own: true,
    };
    let view = DepartmentView {
        detail: DepartmentDetail {
            members_truncated: false,
            roles_truncated: false,
            department: Department {
                escalation: None,
                charter: Some("Win and <keep> customers.".into()),
                description: Some("Closes <new> business.".into()),
                ..tree.sales.clone()
            },
            members: vec![DepartmentMember {
                principal_id: worker.principal_id,
                created_at: Utc::now(),
            }],
            roles: vec![DepartmentRoleHolder {
                decider: false,
                role: DepartmentRole::Lead,
                principal_id: lead.principal_id,
            }],
        },
        principals: vec![lead.clone(), worker.clone()],
        desks: vec![desk.clone()],
    };
    let choices = vec![lead.clone(), worker.clone()];
    let html = section(
        &tree,
        &all,
        DepartmentDetailPane::Department {
            view: &view,
            draft: None,
        },
        &choices,
        true,
        None,
    );

    // The tree nests: Inside sales sits two lists deep, under Sales under Revenue.
    let revenue = html.find(">Revenue<").unwrap();
    let sales = html.find(">Sales &lt;team&gt;<").unwrap();
    let inside = html.find(">Inside sales<").unwrap();
    let support = html.find(">Support<").unwrap();
    assert!(
        revenue < sales && sales < inside && inside < support,
        "{html}"
    );
    assert!(html.contains(r#"data-department-depth="2""#));
    let nested = &html[revenue..inside];
    assert_eq!(nested.matches("<ul>").count(), 2, "{nested}");
    assert!(!html.contains("Sales <team>"), "names are escaped");
    // The selected department is lit and its page is shareable.
    assert!(html.contains("menu-active"));
    assert!(html.contains(&format!(
        "hx-push-url=\"/ui/companies?company_id={}&amp;tab=departments&amp;department_id={}\"",
        tree.company.id, tree.sales.id
    )));

    // Name, slug, parent, charter, lead, members and owned desks.
    // The header gives the address the department's desk answers on, not slug@company (F1).
    assert!(html.contains(
        r#"<p id="department-address" class="font-mono text-xs opacity-60">sales-desk@acme.example.com</p>"#
    ));
    assert!(!html.contains("sales@acme.example.com"));
    assert!(html.contains(r#"<span class="badge badge-ghost badge-sm">Department desk</span>"#));
    assert!(html.contains("<dd>Revenue</dd>"));
    assert!(html.contains("Win and &lt;keep&gt; customers."));
    assert!(html.contains("Closes &lt;new&gt; business."));
    let lead_block = &html[html.find(r#"id="department-lead""#).unwrap()..];
    assert!(lead_block.starts_with(r#"id="department-lead" class="flex"#));
    assert!(lead_block[..400].contains("Closer"));
    let members = &html[html.find(r#"id="department-members""#).unwrap()..];
    assert!(members[..600].contains("dana@example.com"));
    assert!(html.contains(&format!(
        r#"href="/ui/channels?company_id={}&amp;channel_id={}">Sales desk</a>"#,
        tree.company.id, desk.id
    )));

    // An owner or admin gets every form, posting to the department's own routes.
    let base = format!(
        "/ui/companies/{}/departments/{}",
        tree.company.id, tree.sales.id
    );
    assert!(html.contains(&format!(r#"hx-put="{base}""#)));
    assert!(html.contains(&format!(r#"hx-delete="{base}""#)));
    assert!(html.contains(&format!(r#"hx-post="{base}/members""#)));
    assert!(html.contains(&format!(r#"hx-put="{base}/lead""#)));
    assert!(html.contains(&format!(r#"hx-delete="{base}/lead/{}""#, lead.principal_id)));
    assert!(html.contains(&format!(
        r#"hx-delete="{base}/members/{}""#,
        worker.principal_id
    )));
    // The member picker leaves out who is already a member; the lead picker offers everyone.
    let add_member = &html[html.find(&format!(r#"hx-post="{base}/members""#)).unwrap()..];
    let add_member = &add_member[..add_member.find("</form>").unwrap()];
    assert!(add_member.contains("Closer (agent, closer)"));
    assert!(!add_member.contains("dana"));
    // The edit form cannot make a department its own parent.
    let edit = &html[html.find(r#"id="department-edit""#).unwrap()..];
    let edit = &edit[..edit.find("</form>").unwrap()];
    assert!(!edit.contains(&format!(r#"value="{}""#, tree.sales.id)));
    assert!(edit.contains(&format!(r#"value="{}" selected"#, tree.revenue.id)));
    // Renaming a department does not rewrite its slug, and the edit form says a slug edit does
    // not move the desk.
    assert!(!edit.contains("slugify"));
    assert!(edit.contains("Changing the slug does not move the department's desk"));
    assert!(!edit.contains("is created at this slug"));
    // Nothing is cut, so no picker says so.
    assert!(!html.contains("Only the first"));
}

#[test]
fn department_ui_member_sees_their_departments_without_forms() {
    let tree = tree();
    let mine = vec![tree.inside.clone()];
    let view = DepartmentView {
        detail: DepartmentDetail {
            members_truncated: false,
            roles_truncated: false,
            department: tree.inside.clone(),
            members: Vec::new(),
            roles: Vec::new(),
        },
        principals: Vec::new(),
        desks: Vec::new(),
    };
    let html = section(
        &tree,
        &mine,
        DepartmentDetailPane::Department {
            view: &view,
            draft: None,
        },
        &[],
        false,
        None,
    );
    assert!(html.contains("The departments you belong to or lead."));
    assert!(html.contains(">Inside sales<"));
    assert!(!html.contains(">Revenue<"));
    // Its parent is one the member is not in, so it is not named.
    assert!(html.contains("A department you are not in"));
    assert!(html.contains("No local default; an ancestor may supply one."));
    for write in ["hx-put=", "hx-post=", "hx-delete=", "New department"] {
        assert!(!html.contains(write), "{write} in {html}");
    }
    assert!(html.contains("No lead."));
    assert!(html.contains("This department owns no desks."));
    assert!(html.contains("No department desk."));

    // A member reads a desk's address, without a link to the admins' desk settings (F5).
    let with_desk = DepartmentView {
        desks: vec![DepartmentDesk {
            id: Uuid::new_v4(),
            name: "Inside desk".into(),
            slug: "inside-sales".into(),
            own: true,
        }],
        ..view.clone()
    };
    let html = section(
        &tree,
        &mine,
        DepartmentDetailPane::Department {
            view: &with_desk,
            draft: None,
        },
        &[],
        false,
        None,
    );
    assert!(html.contains("<span>Inside desk</span>"));
    assert!(!html.contains("/ui/channels?"));
    assert!(html.contains("inside-sales@acme.example.com"));

    let none = section(&tree, &[], DepartmentDetailPane::None, &[], false, None);
    assert!(none.contains("You are not in any department."));
}

#[test]
fn department_ui_refusal_renders_on_the_form_that_made_it() {
    let tree = tree();
    let all = tree.all();
    let draft = DepartmentDraft {
        name: "Sales again".into(),
        slug: "sales".into(),
        ..DepartmentDraft::default()
    };
    let choices = vec![principal(PrincipalKind::Agent, "Closer", "closer")];
    let html = section(
        &tree,
        &all,
        DepartmentDetailPane::Create(&draft),
        &choices,
        true,
        Some((
            DepartmentForm::Create,
            "Failed to create department: Slug 'sales' is taken",
        )),
    );
    let form = &html[html.find(r#"id="department-create""#).unwrap()..];
    assert!(
        form.contains("Slug &#39;sales&#39; is taken") || form.contains("Slug 'sales' is taken")
    );
    assert!(form.contains(r#"value="Sales again""#));
    assert!(form.contains(r#"name="lead""#));
    assert!(form.contains("Closer (agent, closer)"));
    // A new department's slug follows its name as it is typed, and its desk is created there.
    assert!(form.contains(r#"data-input="slugify""#));
    assert!(form.contains("The department's desk is created at this slug"));

    // A company with more people and agents than the pickers hold says so (F4).
    let cut = company_departments_section(&DepartmentSection {
        company: &tree.company,
        app_domain_name: "example.com",
        departments: &all,
        detail: DepartmentDetailPane::Create(&draft),
        choices: &choices,
        choices_cut: true,
        membership: crate::entities::company_member::CompanyMembership::Owner,
        managed: &managed_when(true, &all),
        history: &[],
        error: None,
        notice: None,
        knowledge: DepartmentKnowledge::Documents(&[]),
        models: None,
        budgets: None,
    });
    assert!(cut.contains("Only the first 500 people and agents are listed."));

    // A refused lead change shows on the lead card, not on the edit form.
    let view = DepartmentView {
        detail: DepartmentDetail {
            members_truncated: false,
            roles_truncated: false,
            department: tree.sales.clone(),
            members: Vec::new(),
            roles: Vec::new(),
        },
        principals: Vec::new(),
        desks: Vec::new(),
    };
    let html = section(
        &tree,
        &all,
        DepartmentDetailPane::Department {
            view: &view,
            draft: None,
        },
        &choices,
        true,
        Some((
            DepartmentForm::Lead,
            "Failed to assign the lead: not publishable",
        )),
    );
    let refusal = html.find("not publishable").unwrap();
    let lead = html.find(r#"id="department-lead""#).unwrap();
    let edit = html.find(r#"id="department-edit""#).unwrap();
    assert!(refusal < lead && lead < edit, "{html}");
    assert_eq!(html.matches("not publishable").count(), 1);
}

#[test]
fn department_ui_company_pane_has_a_departments_tab_and_count() {
    let company = settings_company("Acme", "acme");
    let html = company_edit_pane(&CompanyEditPane {
        company: &company,
        model_connections: &[],
        app_domain_name: "example.com",
        counts: CompanyCounts {
            channels: 3,
            departments: 2,
            agents: 1,
            skills: 0,
        },
        draft: None,
        error: None,
        editable: true,
        resend_api_section: "",
        body: CompanyPaneBody::Settings,
    });
    assert!(html.contains(&format!(
        r#"href="/ui/companies?company_id={}&amp;tab=departments">Departments</a>"#,
        company.id
    )));
    let count = html.find(r#"<span class="text-[11px] font-normal opacity-60">Departments</span>"#);
    assert!(count.is_some(), "{html}");
    assert!(
        html[..count.unwrap()]
            .ends_with("<span class=\"text-lg font-bold\">2</span>\n                        ")
    );
    // Desks first, then departments.
    assert!(html.find(">Desks</span>").unwrap() < count.unwrap());

    let tab = company_edit_pane(&CompanyEditPane {
        company: &company,
        model_connections: &[],
        app_domain_name: "example.com",
        counts: CompanyCounts::default(),
        draft: None,
        error: None,
        editable: true,
        resend_api_section: "",
        body: CompanyPaneBody::Departments("<div id=\"department-section\"></div>"),
    });
    assert!(tab.contains(r#"class="tab tab-active" href="/ui/companies?company_id="#));
    assert!(tab.contains("department-section"));
    assert_eq!(
        CompanyTab::from_query(Some("departments")),
        CompanyTab::Departments
    );
}

#[test]
fn access_mode_ui_desk_form_requires_owner_defaults_root_and_offers_public() {
    let tree = tree();
    let mut root = tree.sales.clone();
    root.id = crate::entities::department::DepartmentId::random();
    root.parent_id = None;
    root.is_root = true;
    root.name = "Company root".into();
    let mut sales = tree.sales.clone();
    sales.parent_id = Some(root.id);
    let all = vec![root.clone(), sales.clone()];
    let plain = channel_create_pane(&ChannelCreatePane {
        company: &tree.company,
        app_domain_name: "example.com",
        spam_scan_enabled: true,
        draft: &ChannelDraft::default(),
        error: None,
        departments: &all,
    });
    assert!(plain.contains(r#"<select name="department_id" required class="select w-full">"#));
    assert!(plain.contains(&format!(
        r#"<option value="{}" selected>Company root</option>"#,
        root.id
    )));
    assert!(!plain.contains("No department"));
    assert!(!plain.contains("members_only"));
    assert!(plain.contains("Open to anyone"));
    assert!(plain.contains("Also allow these addresses"));
    let owned = Channel {
        department_id: sales.id,
        ..mailbox_channel(tree.company.id)
    };
    let html = channel_edit_pane(&ChannelEditPane {
        company: &tree.company,
        app_domain_name: "example.com",
        channel: &owned,
        schedules: &[],
        schedule_bindings: &[],
        spam_scan_enabled: true,
        draft: None,
        error: None,
        departments: &all,
    });
    assert!(html.contains(&format!(
        r#"<option value="{}" selected>&nbsp;&nbsp;Sales &lt;team&gt;</option>"#,
        sales.id
    )));
    assert!(html.contains("its parent Lead and direct child Leads"));
    assert!(html.contains("Other agents need an explicit grant"));
    assert!(!CHANNEL_SETTINGS_SCRIPT.contains("toggleDeskMembersOnly"));
}

#[test]
fn department_ui_desk_lists_group_by_department_with_a_badge() {
    let tree = tree();
    let all = tree.all();
    let desk = |name: &str, slug: &str, owner: Option<&Department>| Channel {
        id: Uuid::new_v4(),
        name: name.into(),
        slug: slug.into(),
        department_id: owner.map_or_else(
            crate::entities::department::DepartmentId::random,
            |department| department.id,
        ),
        ..mailbox_channel(tree.company.id)
    };
    let channels = vec![
        desk("Loose", "loose", None),
        desk("Support desk", "support", Some(&tree.support)),
        desk("Sales desk", "sales", Some(&tree.sales)),
        desk("Revenue desk", "revenue", Some(&tree.revenue)),
    ];
    let list = channel_settings_list(
        &ChannelSettingsList {
            company: &tree.company,
            app_domain_name: "example.com",
            channels: &channels,
            read_only: &Default::default(),
            departments: &all,
            selected_channel_id: None,
        },
        FragmentSwap::Inline,
    );
    let heading = |name: &str| {
        list.find(&format!(
            r#"<li class="menu-title desk-group pt-3 text-[11px] uppercase tracking-wider">{name}</li>"#
        ))
        .unwrap_or_else(|| panic!("{name} heading in {list}"))
    };
    let (revenue, sales, support, none) = (
        heading("Revenue"),
        heading("Sales &lt;team&gt;"),
        heading("Support"),
        heading("Other desks"),
    );
    assert!(
        revenue < sales && sales < support && support < none,
        "{list}"
    );
    assert!(list.find(">Sales desk<").unwrap() > sales);
    assert!(list.find(">Loose<").unwrap() > none);
    // Inside sales owns no desk, so it has no heading.
    assert!(!list.contains("Inside sales"));

    // A separately read list without the permitted desk owner keeps that desk visible.
    let flat = channel_settings_list(
        &ChannelSettingsList {
            company: &tree.company,
            app_domain_name: "example.com",
            channels: &channels[..1],
            read_only: &Default::default(),
            departments: &all,
            selected_channel_id: None,
        },
        FragmentSwap::Inline,
    );
    assert!(!flat.contains("desk-group"));

    // The classic desks page groups the same way and badges each owned desk.
    let page = channel_list_fragment(&tree.company, "example.com", &channels, &all);
    let none = page.find(">Other desks</h4>").unwrap();
    assert!(page.find(">Revenue</h4>").unwrap() < none);
    let row = channel_row_fragment(
        &tree.company,
        "example.com",
        &channels[2],
        Some("Sales <team>"),
    );
    assert!(row.contains(
        r#"<span class="desk-department badge badge-outline badge-sm">Sales &lt;team&gt;</span>"#
    ));
    let unowned = channel_row_fragment(&tree.company, "example.com", &channels[0], None);
    assert!(!unowned.contains("desk-department"));
}

#[test]
fn department_ui_agent_settings_show_its_direct_address_and_departments() {
    let tree = tree();
    let agent = settings_agent(tree.company.id, "Closer", "closer");
    let desk_id = Uuid::new_v4();
    let entry = AgentDirectoryEntry {
        id: AgentId::new(agent.id),
        key: AgentKey::parse("closer").unwrap(),
        name: "Closer".into(),
        active_revision: None,
        departments: vec![DepartmentRef {
            id: tree.sales.id.as_uuid(),
            name: "Sales".into(),
            slug: "sales".into(),
            charter: None,
            roles: vec![DepartmentRole::Lead],
        }],
        address: Some(AgentAddress {
            principal_id: crate::entities::transport::PrincipalId::new(desk_id),
            address: "closer@acme.example.com".into(),
        }),
    };
    let pane = |directory| {
        agent_edit_pane(&AgentEditPane {
            company: &tree.company,
            app_domain_name: "example.com",
            model_connections: &[],
            agent: &agent,
            draft: None,
            error: None,
            capability_options: AgentCapabilityOptions::default(),
            authoring: None,
            directory,
        })
    };
    let html = pane(Some(&entry));
    assert!(html.contains(
        r#"<code id="agent-direct-address" class="font-mono">closer@acme.example.com</code>"#
    ));
    assert!(html.contains(&format!(
        r#"href="/ui?company_id={}&amp;principal_id={desk_id}">Open direct inbox</a>"#,
        tree.company.id
    )));
    assert!(html.contains(&format!(
        r#"<a class="link-hover" href="/ui/companies?company_id={}&amp;tab=departments&amp;department_id={}">Sales</a>"#,
        tree.company.id, tree.sales.id
    )));
    assert!(html.contains(r#"<span class="badge badge-outline badge-sm">lead</span>"#));
    // Membership is not edited here.
    let card = &html[html.find(r#"id="agent-placement-card""#).unwrap()..];
    let card = &card[..card.find("</section>").unwrap()];
    assert!(!card.contains("<form"));
    // The assistant desk action now makes an additional desk.
    assert!(html.contains("The agent has its own direct address."));
    assert!(html.contains(">Create additional desk</button>"));

    let gone = AgentDirectoryEntry {
        address: None,
        departments: Vec::new(),
        ..entry.clone()
    };
    let html = pane(Some(&gone));
    assert!(html.contains("This agent's direct address is unavailable."));
    assert!(html.contains("Not in any department."));
}

/// 01-departments 04B.4: the runs workspace filters by department beside agent and binding, the
/// view switch offers "Waiting on a person", and that view lists the queue by department with
/// a department filter.
#[test]
fn department_views_run_board_filters_by_department_and_waits_on_people() {
    let tree = tree();
    let company_id = tree.company.id;
    let options = ScopeOptions {
        departments: vec![
            ScopeOption {
                id: tree.revenue.id.as_uuid(),
                label: "Revenue".into(),
            },
            ScopeOption {
                id: tree.sales.id.as_uuid(),
                label: "\u{a0}\u{a0}Sales".into(),
            },
        ],
        ..ScopeOptions::default()
    };
    let filter = RunFilterView {
        department: Some(tree.sales.id.as_uuid()),
        ..RunFilterView::default()
    };
    let controls = scope_controls(company_id, filter, &options, RunsView::List);
    assert!(controls.contains(r#"name="department_id" aria-label="Filter by department""#));
    assert!(controls.contains(r#"<option value="">Any department</option>"#));
    assert!(controls.contains(&format!(r#"<option value="{}" selected>"#, tree.sales.id)));
    // A company without departments gets no department select.
    let plain = scope_controls(
        company_id,
        RunFilterView::default(),
        &ScopeOptions::default(),
        RunsView::List,
    );
    assert!(!plain.contains("department_id"));
    assert_eq!(RunsView::parse(Some("waiting")), RunsView::Waiting);

    let toggle = runs_view_toggle(company_id, filter.scope(), RunsView::Waiting);
    assert!(toggle.contains(&format!(
        r#"href="/ui/workflow-runs?company_id={company_id}&amp;department_id={}&amp;view=board""#,
        tree.sales.id
    )));
    assert!(toggle.contains(">Waiting on a person</a>"));
    assert!(toggle.contains(r#"class="tab tab-active""#));

    let all = tree.all();
    let departments = crate::entities::department::DeskDepartments {
        departments: all.clone(),
        owners: Default::default(),
    };
    let page = crate::entities::attention::AttentionPage {
        items: Vec::new(),
        next_cursor: None,
        as_of: Utc::now(),
        working_set_size: 0,
        truncated: false,
    };
    let waiting = runs_waiting(&RunsWaitingView {
        company_id,
        scope: filter.scope(),
        departments: &departments,
        department: Some(tree.sales.id),
        page: &page,
    });
    assert!(waiting.contains(r#"<input type="hidden" name="view" value="waiting">"#));
    assert!(waiting.contains(&format!(
        r#"<option value="{}" selected>&nbsp;&nbsp;Sales &lt;team&gt;</option>"#,
        tree.sales.id
    )));
    assert!(waiting.contains("Decisions waiting on a person and run problems escalated to one"));
    assert!(waiting.contains("No work items in this view."));
}

/// 01-departments 04C.2 (B2-F4): the usage page offers grouping by department, keeps it selected,
/// and labels each row with its department, "No department" included.
#[test]
fn department_views_usage_page_groups_by_department() {
    use crate::application::services::model_usage::{
        UsageDepartment, UsageGroup, UsageGrouping, UsageQuery, UsageReport, UsageScope,
    };
    let company = Uuid::new_v4();
    let end = Utc::now();
    let query = UsageQuery {
        archived: crate::entities::department::ArchivedUnits::Include,
        scope: UsageScope {
            company,
            actor: Uuid::new_v4(),
        },
        start: end - chrono::Duration::days(30),
        end,
        grouping: UsageGrouping::Department,
        agent: None,
        workflow: None,
        unit: None,
    };
    let group = |department: UsageDepartment| UsageGroup {
        provider: None,
        model: None,
        workflow: None,
        department: Some(department),
        desk: None,
        calls: 2,
        completed_calls: 2,
        unconfirmed_calls: 0,
        reported_calls: 2,
        estimated_calls: 0,
        mixed_calls: 0,
        input_tokens: 10,
        output_tokens: 5,
        priced_subtotal_usd: None,
        unpriced_calls: 2,
    };
    let report = UsageReport {
        start: query.start,
        end: query.end,
        grouping: UsageGrouping::Department,
        groups: vec![
            group(UsageDepartment {
                id: Some(Uuid::new_v4()),
                name: "Sales <team>".into(),
            }),
            group(UsageDepartment {
                id: None,
                name: "No department".into(),
            }),
        ],
        scanned_calls: 4,
        truncated: false,
        prices: Vec::new(),
        prices_truncated: false,
        budgets: Vec::new(),
        budgets_truncated: false,
        prices_editable: true,
    };
    let html = model_usage_page(company, &query, &report);
    assert!(
        html.contains(r#"<option value="department" selected>Department</option>"#),
        "{html}"
    );
    let sales = html
        .find("Sales &lt;team&gt;")
        .expect("the department's row");
    let none = html.find("No department").expect("the unowned row");
    assert!(sales < none);
    assert!(!html.contains("Sales <team>"));
}

fn desk_browser_workspace(company: &Company, content: &str) -> String {
    let email = crate::entities::value_objects::EmailAddress::from("owner@example.com");
    let user = mailbox_user(&email);
    ui_shell(&UiShell {
        title: "Desk",
        user: &user,
        company: Some(company),
        section: UiSection::Channels,
        content,
    })
}

/// Real controls from both live page producers, also used by the shipped-JavaScript browser gate.
pub(super) fn desk_access_browser_fixtures() -> [(&'static str, String); 4] {
    let mut company = mailbox_company();
    company.id = Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap();
    let mut root = department(company.id, "Company root", "company", None);
    root.is_root = true;
    let child = department(company.id, "Dispatch", "dispatch", Some(&root));
    let departments = [root, child.clone()];
    let mut channel = mailbox_channel(company.id);
    channel.department_id = child.id;
    channel.access_mode = crate::entities::channel::ChannelAccessMode::Public;
    channel.participant_emails = Some(vec!["@public".into(), "listed@example.test".into()]);
    let classic_create = channels_page(&ChannelsPage {
        company: &company,
        app_domain_name: "example.test",
        channels: &[],
        departments: &departments,
        spam_scan_enabled: false,
        focus: ChannelsPageFocus {
            create_form_open: true,
            editing_channel_id: None,
        },
    });
    let classic_edit =
        channel_edit_fragment(&company, "example.test", &channel, &departments, false);
    let modern_create = channel_create_pane(&ChannelCreatePane {
        company: &company,
        app_domain_name: "example.test",
        departments: &departments,
        spam_scan_enabled: false,
        draft: &ChannelDraft::default(),
        error: None,
    });
    let modern_edit = channel_edit_pane(&ChannelEditPane {
        company: &company,
        app_domain_name: "example.test",
        channel: &channel,
        departments: &departments,
        schedules: &[],
        schedule_bindings: &[],
        spam_scan_enabled: false,
        draft: None,
        error: None,
    });
    [
        ("desk-classic-create.html", classic_create),
        (
            "desk-classic-edit.html",
            base_layout("Edit desk", &classic_edit),
        ),
        (
            "desk-modern-create.html",
            desk_browser_workspace(&company, &modern_create),
        ),
        (
            "desk-modern-edit.html",
            desk_browser_workspace(&company, &modern_edit),
        ),
    ]
}

fn plain_view(department: &Department, members: &[DepartmentPrincipal]) -> DepartmentView {
    DepartmentView {
        detail: DepartmentDetail {
            members_truncated: false,
            roles_truncated: false,
            department: department.clone(),
            members: members
                .iter()
                .map(|principal| DepartmentMember {
                    principal_id: principal.principal_id,
                    created_at: Utc::now(),
                })
                .collect(),
            roles: Vec::new(),
        },
        principals: members.to_vec(),
        desks: vec![DepartmentDesk {
            id: Uuid::new_v4(),
            name: "Unit desk".into(),
            slug: "unit-desk".into(),
            own: true,
        }],
    }
}

fn led_section(
    tree: &Tree,
    departments: &[Department],
    view: &DepartmentView,
    choices: &[DepartmentPrincipal],
    managed: &std::collections::HashSet<DepartmentId>,
) -> String {
    company_departments_section(&DepartmentSection {
        company: &tree.company,
        app_domain_name: "example.com",
        departments,
        detail: DepartmentDetailPane::Department { view, draft: None },
        choices,
        choices_cut: false,
        membership: crate::entities::company_member::CompanyMembership::Member,
        managed,
        history: &[],
        error: None,
        notice: None,
        knowledge: DepartmentKnowledge::Documents(&[]),
        models: None,
        budgets: None,
    })
}

/// Plan06 02A.2/D1 r2-8: each control follows the server rule for its own target. The lead of
/// Sales manages Sales and Inside sales: it edits and leads Inside (its parent is Sales), manages
/// Sales' members, but neither edits Sales nor assigns its lead (Revenue is Sales' parent).
#[test]
fn delegated_admin_ui_pane_offers_each_control_only_where_its_server_rule_passes() {
    let tree = tree();
    let all = tree.all();
    let worker = principal(PrincipalKind::Person, "dana", "dana@example.com");
    let choices = vec![worker.clone()];
    let managed: std::collections::HashSet<DepartmentId> =
        [tree.sales.id, tree.inside.id].into_iter().collect();
    let endpoint = |department: &Department, rest: &str| {
        format!(
            "/ui/companies/{}/departments/{}{rest}",
            tree.company.id, department.id
        )
    };

    let inside = led_section(
        &tree,
        &all,
        &plain_view(&tree.inside, std::slice::from_ref(&worker)),
        &choices,
        &managed,
    );
    assert!(inside.contains(r#"id="department-edit""#), "{inside}");
    assert!(inside.contains(&endpoint(&tree.inside, "/lead")));
    assert!(inside.contains(&endpoint(&tree.inside, "/members")));
    assert!(inside.contains("New department"));
    assert!(
        inside.contains("/ui/channels?company_id="),
        "the managed unit's desk links to settings"
    );
    // Destinations are managed units only; the company root is not offered to a unit lead.
    assert!(!inside.contains(r#"<option value="">Company root</option>"#));
    assert!(!inside.contains(&format!(r#"<option value="{}""#, tree.revenue.id.as_uuid())));
    assert!(!inside.contains(&format!(r#"<option value="{}""#, tree.support.id.as_uuid())));

    let sales = led_section(
        &tree,
        &all,
        &plain_view(&tree.sales, std::slice::from_ref(&worker)),
        &choices,
        &managed,
    );
    assert!(!sales.contains(r#"id="department-edit""#), "{sales}");
    assert!(!sales.contains(&endpoint(&tree.sales, "/lead")));
    assert!(sales.contains(&endpoint(&tree.sales, "/members")));

    let support = led_section(
        &tree,
        &all,
        &plain_view(&tree.support, std::slice::from_ref(&worker)),
        &choices,
        &managed,
    );
    for absent in [
        r#"id="department-edit""#.to_owned(),
        endpoint(&tree.support, "/lead"),
        endpoint(&tree.support, "/members"),
        "/ui/channels?company_id=".to_owned(),
    ] {
        assert!(!support.contains(&absent), "{absent}: {support}");
    }

    // F9: a person who leads the root manages every unit, yet root membership stays the
    // company's: no root member removal or add control, and root roles need the owner.
    let mut root = department(tree.company.id, "Acme", "acme", None);
    root.is_root = true;
    let agent = principal(PrincipalKind::Agent, "Helper", "helper");
    let with_root: Vec<Department> = all.iter().cloned().chain([root.clone()]).collect();
    let everything: std::collections::HashSet<DepartmentId> =
        with_root.iter().map(|department| department.id).collect();
    let root_html = led_section(
        &tree,
        &with_root,
        &plain_view(&root, &[worker.clone(), agent.clone()]),
        &[worker, agent],
        &everything,
    );
    assert!(
        !root_html.contains(&endpoint(&root, "/members")),
        "{root_html}"
    );
    assert!(!root_html.contains(&endpoint(&root, "/lead")));
    assert!(
        !root_html.contains("hx-delete"),
        "no root member removal for a root lead"
    );
}

/// Plan06 02B.2: a department's managers see its last administration events with the role that
/// admitted each, escaped; anyone else sees no history card.
#[test]
fn administration_audit_pane_shows_recent_history_to_managers_only() {
    use crate::entities::department::{
        DepartmentAdministrationAction, DepartmentAdministrationEntry,
    };
    let tree = tree();
    let all = tree.all();
    let history = vec![
        DepartmentAdministrationEntry {
            action: DepartmentAdministrationAction::MemberAdded,
            actor: Some("Dana <lead>".into()),
            acting_role: Some(format!("lead:{}", tree.sales.id)),
            subject: Some("Closer <agent>".into()),
            role: None,
            created_at: Utc::now(),
        },
        DepartmentAdministrationEntry {
            action: DepartmentAdministrationAction::Demoted,
            actor: None,
            acting_role: None,
            subject: Some("Former".into()),
            role: Some(DepartmentRole::Lead),
            created_at: Utc::now(),
        },
    ];
    let view = plain_view(&tree.inside, &[]);
    let render = |managed: &std::collections::HashSet<DepartmentId>| {
        company_departments_section(&DepartmentSection {
            company: &tree.company,
            app_domain_name: "example.com",
            departments: &all,
            detail: DepartmentDetailPane::Department {
                view: &view,
                draft: None,
            },
            choices: &[],
            choices_cut: false,
            membership: crate::entities::company_member::CompanyMembership::Member,
            managed,
            history: &history,
            error: None,
            notice: None,
            knowledge: DepartmentKnowledge::Documents(&[]),
            models: None,
            budgets: None,
        })
    };
    let managing: std::collections::HashSet<DepartmentId> =
        [tree.sales.id, tree.inside.id].into_iter().collect();
    let html = render(&managing);
    assert!(html.contains(r#"id="department-history""#), "{html}");
    assert!(html.contains("Dana &lt;lead&gt;") && html.contains("Closer &lt;agent&gt;"));
    assert!(!html.contains("Dana <lead>"));
    assert!(html.contains("as lead of Sales &lt;team&gt;"), "{html}");
    assert!(html.contains("System") && html.contains("(lead)"));
    assert!(html.contains(&format!(r#"data-acting-role="lead:{}""#, tree.sales.id)));
    let outsider = render(&Default::default());
    assert!(!outsider.contains(r#"id="department-history""#));
}

#[test]
fn department_sponsor_ui_lists_decider_controls_and_named_inherited_escalation() {
    let mut tree = tree();
    tree.revenue.escalation = Some(crate::entities::workflow_escalation::EscalationPolicy {
        notify: DepartmentRole::Sponsor,
        hops: crate::entities::workflow_escalation::EscalationHops::try_from(2).unwrap(),
    });
    let deciding = principal(
        PrincipalKind::Person,
        "Sponsor <decides>",
        "sponsor@example.test",
    );
    let informed = principal(PrincipalKind::Agent, "Agent <informed>", "agent");
    let view = DepartmentView {
        detail: DepartmentDetail {
            department: tree.inside.clone(),
            members: Vec::new(),
            members_truncated: false,
            roles: vec![
                DepartmentRoleHolder {
                    role: DepartmentRole::Sponsor,
                    principal_id: deciding.principal_id,
                    decider: true,
                },
                DepartmentRoleHolder {
                    role: DepartmentRole::Sponsor,
                    principal_id: informed.principal_id,
                    decider: false,
                },
            ],
            roles_truncated: false,
        },
        principals: vec![deciding.clone(), informed.clone()],
        desks: Vec::new(),
    };
    let all = tree.all();
    let html = section(
        &tree,
        &all,
        DepartmentDetailPane::Department {
            view: &view,
            draft: None,
        },
        &[deciding, informed],
        true,
        None,
    );
    assert!(html.contains("Sponsor &lt;decides&gt;"));
    assert!(html.contains(">decides</span>"));
    assert!(html.contains(">informed</span>"));
    assert!(html.contains("Mark decides"));
    assert!(html.contains("Remove sponsor"));
    assert!(html.contains("name=\"successor\""));
    assert!(html.contains("When work stalls, tell"));
    assert!(html.contains("Inherited from Revenue: tell sponsor"));
    assert!(html.contains("data-pending-label=\"Saving…\""));
    assert!(!html.contains("Sponsor <decides>"));
    let readonly = section(
        &tree,
        &all,
        DepartmentDetailPane::Department {
            view: &view,
            draft: None,
        },
        &[],
        false,
        None,
    );
    assert!(!readonly.contains("Mark decides"));
    assert!(!readonly.contains("Remove sponsor"));
}

#[path = "department_archive_ui_tests.rs"]
mod archive;

/// Plan08 03A: the create form offers the kind, a target date and a deciding sponsor, and keeps
/// what was typed when a goal create is refused.
#[test]
fn goal_unit_create_form_offers_kind_target_date_and_sponsor() {
    let tree = tree();
    let all = tree.all();
    let closer = principal(PrincipalKind::Agent, "Closer", "closer");
    let draft = DepartmentDraft {
        name: "Launch X".into(),
        slug: "launch-x".into(),
        kind: "goal".into(),
        target_date: "2027-03-31".into(),
        sponsor: Some(closer.principal_id.as_uuid()),
        ..DepartmentDraft::default()
    };
    let choices = vec![closer];
    let html = section(
        &tree,
        &all,
        DepartmentDetailPane::Create(&draft),
        &choices,
        true,
        None,
    );
    let form = &html[html.find(r#"id="department-create""#).unwrap()..];
    assert!(
        form.contains(r#"<select class="select w-full" name="kind">"#),
        "{form}"
    );
    assert!(form.contains(r#"<option value="goal" selected>Goal</option>"#));
    assert!(form.contains(r#"type="date" name="target_date" value="2027-03-31""#));
    assert!(form.contains(r#"name="sponsor""#));
    assert!(form.contains("Deciding sponsor"));
    let standing = DepartmentDraft {
        name: "Sales".into(),
        ..DepartmentDraft::default()
    };
    let html = section(
        &tree,
        &all,
        DepartmentDetailPane::Create(&standing),
        &[],
        true,
        None,
    );
    assert!(html.contains(r#"<option value="standing" selected>Standing department</option>"#));
}
