//! The Departments tab's routes (01-departments 04A), mirroring `channel_authorization_tests.rs`:
//! a member or stranger gets `404` on every department write and a member sees only their own
//! departments; an owner's writes render, and a refusal comes back on the form that made it.
//! `department_ui_`.
use super::super::authorization_test_support::{Fixture, denied};
use super::*;
use axum::http::StatusCode;

fn workspace(f: &Fixture, user: &crate::entities::user::User) -> Workspace {
    Workspace {
        company_use_cases: f.companies.clone(),
        department_use_cases: f.departments(),
        config: f.config.clone(),
        user_id: user.id,
    }
}

async fn body(response: Response) -> (StatusCode, Option<String>, String) {
    let status = response.status();
    let pushed = response
        .headers()
        .get("HX-Push-Url")
        .map(|value| value.to_str().unwrap().to_owned());
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    (status, pushed, String::from_utf8(bytes.to_vec()).unwrap())
}

async fn person(f: &Fixture, user: &crate::entities::user::User) -> Uuid {
    sqlx::query_scalar(
        "SELECT id FROM principals WHERE company_id = $1 AND user_id = $2 AND kind = 'person'",
    )
    .bind(f.company.id)
    .bind(user.id)
    .fetch_one(f.persistence.pool())
    .await
    .unwrap()
}

fn form(name: &str, slug: &str) -> DepartmentFormData {
    DepartmentFormData {
        name: name.into(),
        slug: Some(slug.into()),
        ..DepartmentFormData::default()
    }
}

async fn create(f: &Fixture, name: &str, slug: &str) -> DepartmentId {
    f.departments()
        .create_department(
            f.owner.id,
            f.company.id,
            DepartmentWrite {
                name: name.into(),
                slug: slug.into(),
                ..DepartmentWrite::default()
            },
            None,
        )
        .await
        .unwrap()
        .id
}

#[tokio::test]
async fn department_ui_writes_answer_404_to_members_strangers_and_foreign_companies() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let department = create(&f, "Hidden", "hidden").await.as_uuid();
    let principal = person(&f, &f.owner).await;
    let cases = [
        (&f.owner, f.foreign.id, f.foreign.name.as_str()),
        (&f.member, f.company.id, f.company.name.as_str()),
        (&f.stranger, f.company.id, f.company.name.as_str()),
    ];
    for (user, company, hidden) in cases {
        let ws = || workspace(&f, user);
        let principal_form = || {
            Form(PrincipalFormData {
                principal_id: Some(principal.to_string()),
            })
        };
        denied(
            create_department(ws(), Path(company), Form(form("New", "new"))).await,
            StatusCode::NOT_FOUND,
            hidden,
        )
        .await;
        denied(
            new_department(ws(), Path(company)).await,
            StatusCode::NOT_FOUND,
            hidden,
        )
        .await;
        denied(
            update_department(
                ws(),
                Path((company, department)),
                Form(form("Renamed", "renamed")),
            )
            .await,
            StatusCode::NOT_FOUND,
            hidden,
        )
        .await;
        denied(
            delete_department(ws(), Path((company, department))).await,
            StatusCode::NOT_FOUND,
            hidden,
        )
        .await;
        denied(
            add_member(ws(), Path((company, department)), principal_form()).await,
            StatusCode::NOT_FOUND,
            hidden,
        )
        .await;
        denied(
            remove_member(ws(), Path((company, department, principal))).await,
            StatusCode::NOT_FOUND,
            hidden,
        )
        .await;
        denied(
            assign_lead(ws(), Path((company, department)), principal_form()).await,
            StatusCode::NOT_FOUND,
            hidden,
        )
        .await;
        denied(
            clear_lead(ws(), Path((company, department, principal))).await,
            StatusCode::NOT_FOUND,
            hidden,
        )
        .await;
    }
    // Nothing changed: the department is still there under its name, with no members or lead.
    let detail = f
        .departments()
        .visible_department(f.owner.id, f.company.id, DepartmentId::new(department))
        .await
        .unwrap();
    assert_eq!(detail.department.name, "Hidden");
    assert!(detail.members.is_empty() && detail.roles.is_empty());
    assert_eq!(
        f.departments()
            .list_departments(f.owner.id, f.company.id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn department_ui_member_sees_only_their_departments() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let mine = create(&f, "Mine", "mine").await;
    let theirs = create(&f, "Theirs", "theirs").await;
    let member = person(&f, &f.member).await;
    f.departments()
        .add_member(f.owner.id, f.company.id, mine, PrincipalId::new(member))
        .await
        .unwrap();

    let section = close_department(workspace(&f, &f.member), Path(f.company.id))
        .await
        .unwrap()
        .0;
    assert!(section.contains(">Mine<"), "{section}");
    assert!(!section.contains(">Theirs<"));
    assert!(!section.contains("New department"));

    let shown = show_department(
        workspace(&f, &f.member),
        Path((f.company.id, mine.as_uuid())),
    )
    .await
    .unwrap()
    .0;
    assert!(shown.contains("mine@"));
    assert!(!shown.contains("hx-put="), "a member gets no forms");
    denied(
        show_department(
            workspace(&f, &f.member),
            Path((f.company.id, theirs.as_uuid())),
        )
        .await,
        StatusCode::NOT_FOUND,
        &f.company.name,
    )
    .await;
    // A stranger reads nothing at all.
    denied(
        close_department(workspace(&f, &f.stranger), Path(f.company.id)).await,
        StatusCode::NOT_FOUND,
        &f.company.name,
    )
    .await;
}

#[tokio::test]
async fn department_ui_owner_writes_render_and_refusals_land_on_their_form() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let ws = || workspace(&f, &f.owner);
    let company = f.company.id;

    // Create, with a blank slug following the name.
    let created = create_department(
        ws(),
        Path(company),
        Form(DepartmentFormData {
            name: "Sales Team".into(),
            charter: Some("Win customers.".into()),
            ..DepartmentFormData::default()
        }),
    )
    .await
    .unwrap();
    let (status, pushed, html) = body(created).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Department created."));
    assert!(html.contains("sales-team@"), "{html}");
    assert!(html.contains("Win customers."));
    let sales = f
        .departments()
        .list_departments(f.owner.id, company)
        .await
        .unwrap()
        .into_iter()
        .find(|department| department.slug.as_str() == "sales-team")
        .unwrap();
    assert_eq!(
        pushed.as_deref(),
        Some(
            format!(
                "/ui/companies?company_id={company}&tab=departments&department_id={}",
                sales.id
            )
            .as_str()
        )
    );
    // The department's own desk is listed as one it owns.
    assert!(html.contains(r#"id="department-desks""#));

    // A taken slug is refused on the create form, which keeps what was typed.
    let (status, pushed, html) = body(
        create_department(ws(), Path(company), Form(form("Sales again", "sales-team")))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pushed, None);
    let create_form = &html[html.find(r#"id="department-create""#).unwrap()..];
    let create_form = &create_form[..create_form.find("</form>").unwrap()];
    assert!(
        create_form.contains("Failed to create department"),
        "{html}"
    );
    assert!(create_form.contains(r#"value="Sales again""#));

    // A child, then a cycle: making Sales a child of its own child is refused on the edit form.
    let child = create(&f, "Inside", "inside").await;
    f.departments()
        .update_department(
            f.owner.id,
            company,
            child,
            crate::application::ports::department::DepartmentEdit {
                parent_id: Some(Some(sales.id)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let (_, _, html) = body(
        update_department(
            ws(),
            Path((company, sales.id.as_uuid())),
            Form(DepartmentFormData {
                parent_id: Some(child.to_string()),
                ..form("Sales Team", "sales-team")
            }),
        )
        .await
        .unwrap(),
    )
    .await;
    let edit_form = &html[html.find(r#"id="department-edit""#).unwrap()..];
    let edit_form = &edit_form[..edit_form.find("</form>").unwrap()];
    assert!(edit_form.contains("Failed to save department"), "{html}");
    let stored = f
        .departments()
        .visible_department(f.owner.id, company, sales.id)
        .await
        .unwrap();
    assert_eq!(stored.department.parent_id, None);

    // A slug edit does not move the department's desk: the header keeps the desk's address (F1).
    let (_, _, html) = body(
        update_department(
            ws(),
            Path((company, sales.id.as_uuid())),
            Form(form("Sales Team", "revenue")),
        )
        .await
        .unwrap(),
    )
    .await;
    assert!(html.contains("Department saved."), "{html}");
    let header = &html[html.find(r#"id="department-address""#).unwrap()..];
    let header = &header[..header.find("</p>").unwrap()];
    assert!(header.contains("sales-team@"), "{header}");
    assert!(!header.contains("revenue@"), "{header}");

    // A department that still owns its desk is not deleted; the refusal is on the delete card.
    let (_, _, html) = body(
        delete_department(ws(), Path((company, sales.id.as_uuid())))
            .await
            .unwrap(),
    )
    .await;
    let delete_card = &html[html.find(r#"id="department-delete""#).unwrap()..];
    assert!(
        delete_card[..delete_card.find("Delete department</button>").unwrap()]
            .contains("Failed to delete department"),
        "{html}"
    );

    // Members and the lead.
    let member = person(&f, &f.member).await;
    let (_, _, html) = body(
        add_member(
            ws(),
            Path((company, sales.id.as_uuid())),
            Form(PrincipalFormData {
                principal_id: Some(member.to_string()),
            }),
        )
        .await
        .unwrap(),
    )
    .await;
    assert!(html.contains("Member added."));
    assert!(html.contains(&format!("/members/{member}")));
    let (_, _, html) = body(
        assign_lead(
            ws(),
            Path((company, sales.id.as_uuid())),
            Form(PrincipalFormData {
                principal_id: Some(member.to_string()),
            }),
        )
        .await
        .unwrap(),
    )
    .await;
    assert!(html.contains("Lead assigned."));
    assert!(html.contains(&format!("/lead/{member}")));
    let (_, _, html) = body(
        clear_lead(ws(), Path((company, sales.id.as_uuid(), member)))
            .await
            .unwrap(),
    )
    .await;
    assert!(html.contains("Lead cleared."));
    assert!(html.contains("No lead."));
    let (_, _, html) = body(
        remove_member(ws(), Path((company, sales.id.as_uuid(), member)))
            .await
            .unwrap(),
    )
    .await;
    assert!(html.contains("Member removed."));
    assert!(html.contains("No members yet."));

    // An empty choice is refused on the members form, not as an error page.
    let (status, _, html) = body(
        add_member(
            ws(),
            Path((company, sales.id.as_uuid())),
            Form(PrincipalFormData { principal_id: None }),
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Choose a person or agent."));

    // A department with no desk and no children is deleted, and the tab shows nothing selected.
    let leaf = child;
    let leaf_desk: Uuid =
        sqlx::query_scalar("SELECT id FROM channels WHERE company_id = $1 AND department_id = $2")
            .bind(company)
            .bind(leaf.as_uuid())
            .fetch_one(f.persistence.pool())
            .await
            .unwrap();
    sqlx::query("UPDATE channels SET department_id = NULL, members_only = false WHERE id = $1")
        .bind(leaf_desk)
        .execute(f.persistence.pool())
        .await
        .unwrap();
    let (_, pushed, html) = body(
        delete_department(ws(), Path((company, leaf.as_uuid())))
            .await
            .unwrap(),
    )
    .await;
    assert!(html.contains("Department deleted."), "{html}");
    assert!(!html.contains(">Inside<"));
    assert_eq!(
        pushed.as_deref(),
        Some(format!("/ui/companies?company_id={company}&tab=departments").as_str())
    );
}

/// `src/adapters/http/AGENTS.md`: another company's valid ids under the caller's own company path
/// are not found, or refused on the form, and never change the other company (F3).
#[tokio::test]
async fn department_ui_foreign_ids_under_the_owners_path_change_nothing() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let ws = || workspace(&f, &f.owner);
    let company = f.company.id;
    let mine = create(&f, "Mine", "mine").await;
    // The foreign company is the stranger's: its department, with its owner as member and lead.
    let foreign = f.departments();
    let theirs = foreign
        .create_department(
            f.stranger.id,
            f.foreign.id,
            DepartmentWrite {
                name: "Theirs".into(),
                slug: "theirs".into(),
                ..DepartmentWrite::default()
            },
            None,
        )
        .await
        .unwrap()
        .id;
    let their_person: Uuid = sqlx::query_scalar(
        "SELECT id FROM principals WHERE company_id = $1 AND user_id = $2 AND kind = 'person'",
    )
    .bind(f.foreign.id)
    .bind(f.stranger.id)
    .fetch_one(f.persistence.pool())
    .await
    .unwrap();
    foreign
        .add_member(
            f.stranger.id,
            f.foreign.id,
            theirs,
            PrincipalId::new(their_person),
        )
        .await
        .unwrap();
    foreign
        .assign_role(
            f.stranger.id,
            f.foreign.id,
            theirs,
            DepartmentRole::Lead,
            PrincipalId::new(their_person),
        )
        .await
        .unwrap();
    let their_id = theirs.as_uuid();
    let principal_form = |id: Uuid| {
        Form(PrincipalFormData {
            principal_id: Some(id.to_string()),
        })
    };

    // Their department under my company's path: not found, whatever the request.
    let hidden = f.foreign.name.as_str();
    denied(
        show_department(ws(), Path((company, their_id))).await,
        StatusCode::NOT_FOUND,
        hidden,
    )
    .await;
    denied(
        update_department(
            ws(),
            Path((company, their_id)),
            Form(form("Taken", "taken")),
        )
        .await,
        StatusCode::NOT_FOUND,
        hidden,
    )
    .await;
    denied(
        delete_department(ws(), Path((company, their_id))).await,
        StatusCode::NOT_FOUND,
        hidden,
    )
    .await;
    denied(
        add_member(
            ws(),
            Path((company, their_id)),
            principal_form(their_person),
        )
        .await,
        StatusCode::NOT_FOUND,
        hidden,
    )
    .await;
    denied(
        remove_member(ws(), Path((company, their_id, their_person))).await,
        StatusCode::NOT_FOUND,
        hidden,
    )
    .await;
    denied(
        assign_lead(
            ws(),
            Path((company, their_id)),
            principal_form(their_person),
        )
        .await,
        StatusCode::NOT_FOUND,
        hidden,
    )
    .await;
    denied(
        clear_lead(ws(), Path((company, their_id, their_person))).await,
        StatusCode::NOT_FOUND,
        hidden,
    )
    .await;
    // Their department as my new department's parent is refused on the create form.
    let (status, _, html) = body(
        create_department(
            ws(),
            Path(company),
            Form(DepartmentFormData {
                parent_id: Some(their_id.to_string()),
                ..form("Child", "child")
            }),
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Failed to create department"), "{html}");
    assert!(!html.contains(hidden));

    // Their person in my department's forms and paths: refused on the form, nothing added.
    let mine_id = mine.as_uuid();
    for (label, response) in [
        (
            "add member",
            add_member(ws(), Path((company, mine_id)), principal_form(their_person))
                .await
                .unwrap(),
        ),
        (
            "assign lead",
            assign_lead(ws(), Path((company, mine_id)), principal_form(their_person))
                .await
                .unwrap(),
        ),
        (
            "remove member",
            remove_member(ws(), Path((company, mine_id, their_person)))
                .await
                .unwrap(),
        ),
        (
            "clear lead",
            clear_lead(ws(), Path((company, mine_id, their_person)))
                .await
                .unwrap(),
        ),
    ] {
        let (status, _, html) = body(response).await;
        assert_eq!(status, StatusCode::OK, "{label}");
        assert!(html.contains("Failed to"), "{label}: {html}");
        assert!(!html.contains(hidden), "{label}");
    }
    let detail = f
        .departments()
        .visible_department(f.owner.id, company, mine)
        .await
        .unwrap();
    assert!(detail.members.is_empty() && detail.roles.is_empty());
    assert!(
        !f.departments()
            .list_departments(f.owner.id, company)
            .await
            .unwrap()
            .iter()
            .any(|department| department.slug.as_str() == "child")
    );

    // Their department is untouched: name, member and lead as they were.
    let untouched = foreign
        .visible_department(f.stranger.id, f.foreign.id, theirs)
        .await
        .unwrap();
    assert_eq!(untouched.department.name, "Theirs");
    assert_eq!(untouched.members.len(), 1);
    assert_eq!(untouched.roles.len(), 1);
    assert_eq!(untouched.members[0].principal_id.as_uuid(), their_person);
}
