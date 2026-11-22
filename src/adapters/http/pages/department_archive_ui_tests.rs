//! Rendering of archived departments (Plan08 02E): the list hides them behind "Show archived",
//! the lifecycle card offers archive, cancel-and-archive and restore, an archived department
//! offers no configuration form, and pickers and filters leave archived departments out.
use super::*;
use crate::entities::department::{ArchivedUnits, DeskDepartments, UnitArchive};

fn archived(mut department: Department) -> Department {
    department.archived = Some(UnitArchive {
        at: chrono::DateTime::parse_from_rfc3339("2026-11-20T09:30:00Z")
            .unwrap()
            .with_timezone(&Utc),
        by: CreationProvenance {
            actor_name: "Olga <Owner>".into(),
            ..CreationProvenance::user(Uuid::new_v4())
        },
    });
    department
}

fn between<'h>(html: &'h str, start: &str, end: &str) -> &'h str {
    let from = html
        .find(start)
        .unwrap_or_else(|| panic!("{start} missing"));
    let rest = &html[from..];
    &rest[..rest.find(end).unwrap_or(rest.len())]
}

/// Revenue > Sales > Inside sales (archived), and Support.
fn with_archived_inside(tree: &Tree) -> Vec<Department> {
    vec![
        tree.support.clone(),
        archived(tree.inside.clone()),
        tree.sales.clone(),
        tree.revenue.clone(),
    ]
}

#[test]
fn unit_archive_ui_list_hides_archived_departments_behind_show_archived() {
    let tree = tree();
    let departments = with_archived_inside(&tree);
    let html = section(
        &tree,
        &departments,
        DepartmentDetailPane::None,
        &[],
        true,
        None,
    );
    let active = between(
        &html,
        r#"id="department-tree""#,
        r#"<details id="department-archived""#,
    );
    assert!(active.contains("Revenue") && active.contains("Support"));
    assert!(!active.contains("Inside sales"), "{active}");
    let hidden = between(&html, r#"<details id="department-archived""#, "</details>");
    assert!(hidden.contains("Show archived (1)"));
    assert!(hidden.contains("Inside sales"));
    assert!(
        !hidden.contains(" open>"),
        "closed until an archived one is selected"
    );
    // The create form's parent picker offers no archived department.
    let html = section(
        &tree,
        &departments,
        DepartmentDetailPane::Create(&DepartmentDraft::default()),
        &[],
        true,
        None,
    );
    let parent = between(&html, r#"name="parent_id""#, "</select>");
    assert!(parent.contains("Sales &lt;team&gt;"));
    assert!(!parent.contains("Inside sales"), "{parent}");
}

#[test]
fn unit_archive_ui_archived_department_says_when_and_offers_restore_and_delete_only() {
    let tree = tree();
    let departments = with_archived_inside(&tree);
    let inside = departments[1].clone();
    let member = principal(PrincipalKind::Person, "Mia", "mia@example.com");
    let view = plain_view(&inside, std::slice::from_ref(&member));
    let html = section(
        &tree,
        &departments,
        DepartmentDetailPane::Department {
            view: &view,
            draft: None,
        },
        std::slice::from_ref(&member),
        true,
        None,
    );
    assert!(html.contains(r#"<details id="department-archived" class="mt-2" open>"#));
    assert!(html.contains(r#"id="department-archived-badge""#));
    let card = between(&html, r#"id="department-lifecycle""#, "</section>");
    assert!(card.contains(r#"<time datetime="2026-11-20T09:30:00+00:00">2026-11-20 09:30 UTC</time> by Olga &lt;Owner&gt;"#), "{card}");
    assert!(card.contains(r#"id="department-restore""#));
    assert!(card.contains(&format!(
        r#"hx-post="/ui/companies/{}/departments/{}/restore""#,
        tree.company.id, inside.id
    )));
    assert!(card.contains("Delete department"));
    assert!(!card.contains(r#"id="department-archive""#));
    // No configuration form: edit, lead, members.
    for form in [
        r#"id="department-edit""#,
        r#"name="lead""#,
        r#"name="principal_id""#,
    ] {
        assert!(
            !html.contains(form),
            "{form} offered on an archived department"
        );
    }
    // Under an archived parent the restore waits for the parent.
    let both = vec![
        archived(tree.sales.clone()),
        inside.clone(),
        tree.revenue.clone(),
    ];
    let html = section(
        &tree,
        &both,
        DepartmentDetailPane::Department {
            view: &view,
            draft: None,
        },
        &[],
        true,
        None,
    );
    assert!(html.contains(r#"id="department-restore-blocked""#));
    assert!(!html.contains(r#"id="department-restore""#));
}

#[test]
fn unit_archive_ui_active_department_offers_archive_and_cancel_on_open_work() {
    let tree = tree();
    let departments = tree.all();
    let view = plain_view(&tree.sales, &[]);
    let pane = || DepartmentDetailPane::Department {
        view: &view,
        draft: None,
    };
    let html = section(&tree, &departments, pane(), &[], true, None);
    let card = between(&html, r#"id="department-lifecycle""#, "</section>");
    assert!(card.contains(&format!(
        r#"hx-post="/ui/companies/{}/departments/{}/archive""#,
        tree.company.id, tree.sales.id
    )));
    assert!(!card.contains("department-archive-cancel"));
    assert!(
        html.contains(r#"id="department-edit""#),
        "an active one stays editable"
    );
    let html = section(
        &tree,
        &departments,
        pane(),
        &[],
        true,
        Some((
            DepartmentForm::ArchiveOpenWork,
            "2 runs are still open here or below.",
        )),
    );
    let card = between(&html, r#"id="department-lifecycle""#, "</section>");
    assert!(card.contains("2 runs are still open here or below."));
    assert!(card.contains(r#"id="department-archive-cancel""#));
    assert!(card.contains(r#"hx-vals='{"cancel_open_runs":"true"}'"#));
    let html = section(
        &tree,
        &departments,
        pane(),
        &[],
        true,
        Some((
            DepartmentForm::Archive,
            "65 runs are still open, more than can be cancelled.",
        )),
    );
    let card = between(&html, r#"id="department-lifecycle""#, "</section>");
    assert!(card.contains("more than can be cancelled"));
    assert!(!card.contains("department-archive-cancel"));
    // A member who manages nothing sees no lifecycle card.
    let html = section(&tree, &departments, pane(), &[], false, None);
    assert!(!html.contains(r#"id="department-lifecycle""#));
}

#[test]
fn unit_archive_ui_filters_and_desk_owner_picker_leave_archived_departments_out() {
    let tree = tree();
    let departments = with_archived_inside(&tree);
    let desks = DeskDepartments {
        departments: departments.clone(),
        owners: Default::default(),
    };
    let filter = department_filter(&desks, None);
    assert!(
        filter.contains("Support") && !filter.contains("Inside sales"),
        "{filter}"
    );
    let filter = department_filter(&desks, Some(tree.inside.id));
    assert!(
        filter.contains("Inside sales (archived)"),
        "a set filter keeps its option"
    );
    // Desk headings and badges name an archived owner as such.
    assert_eq!(departments[1].label(), "Inside sales (archived)");
    assert!(ArchivedUnits::Include.included() && !ArchivedUnits::Exclude.included());
}

#[test]
fn unit_archive_ui_usage_page_offers_archived_rows_and_marks_them() {
    use crate::application::services::model_usage::{
        UnitBudgetStatus, UsageGrouping, UsageQuery, UsageReport, UsageScope,
    };
    let company = Uuid::new_v4();
    let end = Utc::now();
    let query = |archived| UsageQuery {
        archived,
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
    let row = |name: &str, archived| UnitBudgetStatus {
        unit: Uuid::new_v4(),
        name: name.into(),
        archived,
        period: "day".into(),
        cap_usd: Some("1.00".into()),
        spent_usd: "0.10".into(),
        remaining_usd: Some("0.90".into()),
        unpriced_tokens: 0,
    };
    let report = UsageReport {
        start: end - chrono::Duration::days(30),
        end,
        grouping: UsageGrouping::Department,
        groups: Vec::new(),
        scanned_calls: 0,
        truncated: false,
        prices: Vec::new(),
        prices_truncated: false,
        budgets: vec![row("Active", false), row("Dormant", true)],
        budgets_truncated: false,
        prices_editable: false,
    };
    let html = model_usage_page(company, &query(ArchivedUnits::Include), &report);
    assert!(
        html.contains(r#"name="include_archived" value="true" checked>"#),
        "{html}"
    );
    assert!(html.contains("<th scope=\"row\">Dormant (archived)</th>"));
    assert!(html.contains("<th scope=\"row\">Active</th>"));
    let html = model_usage_page(company, &query(ArchivedUnits::Exclude), &report);
    assert!(html.contains(r#"name="include_archived" value="true">"#));
}
