//! Rendering of the department UI (01-departments 04A, 04B.1–3): the Departments tab, the desk
//! form's owner fields, desk lists grouped by department, and the agent's desk and departments:
//! `department_ui_`.
use super::*;
use crate::application::agent::{AgentDesk, AgentDirectoryEntry, AgentKey, DepartmentRef};
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
        editable,
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
        editable: true,
        error: None,
        notice: None,
    });
    assert!(cut.contains("Only the first 500 people and agents are listed."));

    // A refused lead change shows on the lead card, not on the edit form.
    let view = DepartmentView {
        detail: DepartmentDetail {
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
fn department_ui_desk_form_offers_the_department_and_members_only() {
    let tree = tree();
    let all = tree.all();
    let channel = mailbox_channel(tree.company.id);
    let plain = channel_create_pane(&ChannelCreatePane {
        company: &tree.company,
        app_domain_name: "example.com",
        spam_scan_enabled: true,
        draft: &ChannelDraft::default(),
        error: None,
        departments: &all,
    });
    assert!(plain.contains(
        r#"<select name="department_id" class="select w-full" data-input="desk-members-only">"#
    ));
    assert!(plain.contains(r#"<option value="">No department</option>"#));
    // Without a department the members-only box is hidden and unticked.
    assert!(plain.contains(r#"class="desk-members-only form-control w-full hidden""#));
    assert!(!plain.contains(
        r#"name="members_only" value="true" class="checkbox checkbox-sm mt-0.5" checked"#
    ));

    let owned = Channel {
        department_id: Some(tree.sales.id),
        members_only: true,
        ..channel
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
        tree.sales.id
    )));
    assert!(html.contains(r#"class="desk-members-only form-control w-full""#));
    assert!(html.contains(
        r#"name="members_only" value="true" class="checkbox checkbox-sm mt-0.5" checked"#
    ));
    // Who gets access, one step up and down (COORDINATION C002), and that relays stay (C001).
    // Participant Emails still admit their addresses on a members-only desk (F2).
    assert!(html.contains("Only the department's members, its lead, the lead of its parent department, the leads of its direct child departments and the addresses in Participant Emails can use this desk."));
    assert!(html.contains("Other desks in the company can still hand work to it."));

    // A company without departments sees no owner fields at all.
    let none = channel_create_pane(&ChannelCreatePane {
        company: &tree.company,
        app_domain_name: "example.com",
        spam_scan_enabled: true,
        draft: &ChannelDraft::default(),
        error: None,
        departments: &[],
    });
    assert!(!none.contains("department_id"));
    // The script that shows the box is in the shared layout's handler.
    assert!(CHANNEL_SETTINGS_SCRIPT.contains("function toggleDeskMembersOnly(select)"));
}

#[test]
fn department_ui_desk_lists_group_by_department_with_a_badge() {
    let tree = tree();
    let all = tree.all();
    let desk = |name: &str, slug: &str, owner: Option<&Department>| Channel {
        id: Uuid::new_v4(),
        name: name.into(),
        slug: slug.into(),
        department_id: owner.map(|department| department.id),
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
        heading("No department"),
    );
    assert!(
        revenue < sales && sales < support && support < none,
        "{list}"
    );
    assert!(list.find(">Sales desk<").unwrap() > sales);
    assert!(list.find(">Loose<").unwrap() > none);
    // Inside sales owns no desk, so it has no heading.
    assert!(!list.contains("Inside sales"));

    // A company whose desks have no department keeps a flat list.
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
    let none = page.find(">No department</h4>").unwrap();
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
fn department_ui_agent_settings_show_its_desk_and_departments() {
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
        desk: Some(AgentDesk {
            id: desk_id,
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
        r#"<code id="agent-default-desk" class="font-mono">closer@acme.example.com</code>"#
    ));
    assert!(html.contains(&format!(
        r#"href="/ui/channels?company_id={}&amp;channel_id={desk_id}">Open desk settings</a>"#,
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
    assert!(html.contains("The agent already has its own desk."));
    assert!(html.contains(">Create additional desk</button>"));

    let gone = AgentDirectoryEntry {
        desk: None,
        departments: Vec::new(),
        ..entry.clone()
    };
    let html = pane(Some(&gone));
    assert!(html.contains(
        "This agent's own desk was deleted. Create an additional desk below to give it an address again."
    ));
    assert!(html.contains("Not in any department."));
}
