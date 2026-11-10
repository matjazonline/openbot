use super::*;
use crate::{
    adapters::http::pages,
    entities::{company_member::CompanyMembership, value_objects::EmailAddress},
};

async fn owner_page(f: &Journey) -> String {
    let accounts = f
        .vault
        .list(&f.account.owner, PageRequest::new(None, 50).unwrap())
        .await
        .unwrap();
    let entry = f.vault.registry.get(&Case::Google.app()).unwrap();
    let apps = vec![ConnectApp {
        provider: entry.provider.id.clone(),
        id: entry.app.id.clone(),
        config: entry.config.id.clone(),
        capabilities: entry.app.capabilities.clone(),
    }];
    let content = pages::vault::vault_content(&accounts, None, &apps);
    let email = EmailAddress::from("workflow@example.test");
    let user = pages::MailboxUser {
        id: f.setup.fixture.target.actor.user_id(),
        username: "workflow_owner",
        email: &email,
        avatar_url: None,
        is_operator: false,
        company_membership: CompanyMembership::None,
    };
    pages::profile_page(&pages::ProfilePage {
        user: &user,
        company: None,
        pane_html: &content,
    })
}
#[tokio::test]
async fn vault_authoring_owner_forms_and_reconnect_states_emit_real_browser_markup() {
    let f = Box::pin(Journey::new(Case::Google)).await;
    let mut input = write("owner-static", "owner-form-secret-sentinel");
    input.metadata = AccountMetadata::new("<owner label>".into(), None).unwrap();
    f.vault.create(&f.account.owner, input).await.unwrap();
    let owner = owner_page(&f).await;
    *f.token.state.status.lock().unwrap() = axum::http::StatusCode::BAD_REQUEST;
    *f.token.state.reply.lock().unwrap() =
        Reply::Json(json!({"error":"invalid_grant","error_description":"refresh-sentinel"}));
    assert!(
        Box::pin(f.runtime.run(&f.setup.kind, &f.setup.claim))
            .await
            .completed_result()
            .is_err()
    );
    assert_eq!(
        f.vault
            .get(&f.account.owner, f.account.id)
            .await
            .unwrap()
            .status,
        AccountStatus::ReconnectRequired
    );
    assert_eq!(f.peer.calls(), 0);
    let reconnect = owner_page(&f).await;
    assert!(reconnect.contains("Reconnect required") && reconnect.contains("Reconnect gmail"));
    assert!(owner.contains("&lt;owner label&gt;"));
    for html in [&owner, &reconnect] {
        for secret in [
            "owner-form-secret-sentinel",
            "refresh-sentinel",
            "old-access-sentinel",
            "enc:v2",
        ] {
            assert!(!html.contains(secret));
        }
    }
    if let Some(directory) = std::env::var_os("VAULT_AUTHORING_BROWSER_FIXTURES") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("owner.html"), owner).unwrap();
        std::fs::write(directory.join("reconnect.html"), reconnect).unwrap();
        std::fs::write(directory.join("app.js"), pages::application_javascript()).unwrap();
    }
    f.peer.close().await;
}
