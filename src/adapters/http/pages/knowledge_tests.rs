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
    };
    let view = KnowledgeLibraryView {
        actor: LevelActor {
            membership: CompanyMembership::Member,
            access_level: AccessLevel::L5,
        },
        layers: vec![KnowledgeDocumentLayer {
            id: Uuid::from_u128(3),
            name: "<img onerror=alert(1)>.md".into(),
            access_level: AccessLevel::L0,
            revision: 1,
            text_bytes: 4,
        }],
    };
    let content = knowledge_library(company.id, &view, None);
    let page = ui_shell(&UiShell {
        title: "Knowledge",
        user: &user,
        company: Some(&company),
        section: UiSection::Knowledge,
        content: &content,
    });
    std::fs::write(root.join("knowledge.html"), page).unwrap();
    std::fs::write(root.join("app.js"), application_javascript()).unwrap();
    std::fs::write(root.join("theme-init.js"), theme_init_javascript()).unwrap();
}
