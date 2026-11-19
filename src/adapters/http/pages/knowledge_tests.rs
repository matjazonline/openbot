use super::*;
use crate::entities::access_level::{AccessLevel, LevelActor};
#[test]
#[ignore = "browser fixture: exact production library markup and JavaScript"]
fn dump_knowledge_browser_fixtures() {
    let root = std::path::PathBuf::from(
        std::env::var("KNOWLEDGE_BROWSER_FIXTURES").expect("KNOWLEDGE_BROWSER_FIXTURES"),
    );
    std::fs::create_dir_all(&root).unwrap();
    let user_id = Uuid::from_u128(1);
    let company = Company {
        id: Uuid::from_u128(2),
        user_id,
        name: "Library".into(),
        slug: "library".into(),
        channel_defaults: Default::default(),
        avatar_url: None,
        created_at: Utc::now(),
    };
    let email = EmailAddress::from("member@example.test");
    let user = MailboxUser {
        id: user_id,
        username: "Member",
        email: &email,
        avatar_url: None,
        is_operator: false,
        company_membership: CompanyMembership::Member,
        administers_units: CompanyMembership::Member.administers_company(),
    };
    let view = view();
    for (name, content) in [
        (
            "knowledge.html",
            knowledge_library(company.id, &view, None, None, None),
        ),
        (
            "knowledge-move.html",
            knowledge_move(
                company.id,
                &view.layers[0],
                &view,
                KnowledgeMoveState::default(),
            ),
        ),
    ] {
        let page = ui_shell(&UiShell {
            title: "Knowledge",
            user: &user,
            company: Some(&company),
            section: UiSection::Knowledge,
            content: &content,
        });
        std::fs::write(root.join(name), page).unwrap();
    }
    std::fs::write(root.join("app.js"), application_javascript()).unwrap();
    std::fs::write(root.join("theme-init.js"), theme_init_javascript()).unwrap();
}

fn view() -> KnowledgeLibraryView {
    KnowledgeLibraryView {
        actor: LevelActor {
            membership: CompanyMembership::Member,
            access_level: AccessLevel::L5,
        },
        layers: vec![KnowledgeDocumentLayer {
            id: Uuid::from_u128(3),
            unit_id: crate::entities::department::DepartmentId::new(Uuid::from_u128(4)),
            unit_name: "Root".into(),
            unit_is_root: true,
            name: "<img onerror=alert(1)>.md".into(),
            access_level: AccessLevel::L0,
            revision: 1,
            text_bytes: 4,
        }],
        upload_units: vec![
            crate::entities::knowledge::KnowledgeUnit {
                id: crate::entities::department::DepartmentId::new(Uuid::from_u128(4)),
                name: "Root".into(),
                is_root: true,
            },
            crate::entities::knowledge::KnowledgeUnit {
                id: crate::entities::department::DepartmentId::new(Uuid::from_u128(5)),
                name: "Child".into(),
                is_root: false,
            },
        ],
        writable_units: vec![crate::entities::department::DepartmentId::new(
            Uuid::from_u128(4),
        )],
        default_unit: Some(crate::entities::department::DepartmentId::new(
            Uuid::from_u128(4),
        )),
        units_truncated: false,
    }
}

#[test]
fn knowledge_scope_upload_maximum_library_keeps_destination_choices_bounded_for_every_document() {
    let mut view = view();
    view.upload_units = (0..256)
        .map(|index| crate::entities::knowledge::KnowledgeUnit {
            id: crate::entities::department::DepartmentId::new(Uuid::from_u128(100 + index)),
            name: format!("Unit {index:03}"),
            is_root: index == 0,
        })
        .collect();
    let root = view.upload_units[0].clone();
    view.default_unit = Some(root.id);
    view.writable_units = vec![root.id];
    view.layers = (0..4096)
        .map(|index| KnowledgeDocumentLayer {
            id: Uuid::from_u128(1000 + index),
            unit_id: root.id,
            unit_name: root.name.clone(),
            unit_is_root: true,
            name: format!("notes-{index}.md").into(),
            access_level: AccessLevel::PUBLIC,
            revision: 1,
            text_bytes: 0,
        })
        .collect();
    let html = knowledge_library(Uuid::from_u128(1), &view, None, None, None);
    assert!(
        html.matches("<option").count() <= 514,
        "destination choices must not multiply by the document count"
    );
    assert_eq!(html.matches("Move document</a>").count(), 4096);
    let moving = knowledge_move(
        Uuid::from_u128(1),
        &view.layers[0],
        &view,
        KnowledgeMoveState::default(),
    );
    assert_eq!(moving.matches(r#"name="target_unit_id""#).count(), 1);
    assert!(moving.matches("<option").count() <= 257);
}
