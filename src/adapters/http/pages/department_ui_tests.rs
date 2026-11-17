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
        is_root: false,
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

fn section<'a>(
    tree: &'a Tree,
    departments: &'a [Department],
    detail: DepartmentDetailPane<'a>,
    choices: &'a [DepartmentPrincipal],
    editable: bool,
    error: Option<(DepartmentForm, &'a str)>,
) -> String {
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
        error,
        notice: None,
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
                charter: Some("Win and <keep> customers.".into()),
                description: Some("Closes <new> business.".into()),
                ..tree.sales.clone()
            },
            members: vec![DepartmentMember {
                principal_id: worker.principal_id,
                created_at: Utc::now(),
            }],
            roles: vec![DepartmentRoleHolder {
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
        error: None,
        notice: None,
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
        scope: UsageScope {
            company,
            actor: Uuid::new_v4(),
        },
        start: end - chrono::Duration::days(30),
        end,
        grouping: UsageGrouping::Department,
        agent: None,
        workflow: None,
    };
    let group = |department: UsageDepartment| UsageGroup {
        provider: None,
        model: None,
        workflow: None,
        department: Some(department),
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
