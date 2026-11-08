use super::*;

struct NavigationReply {
    html: String,
    csp: String,
    target: String,
}
async fn begin_reply(router: &Router, f: &Fixture, body: &str) -> NavigationReply {
    let response = router
        .clone()
        .oneshot(request(
            "POST",
            "/ui/profile/vault/apps/gmail/connect",
            Some(&f.cookie),
            body,
            Some("http://localhost:3000"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get("location").is_none());
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(response.headers()["referrer-policy"], "no-referrer");
    let target = response.headers()["hx-redirect"]
        .to_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        url::Url::parse(&target).unwrap().host_str(),
        Some("accounts.google.com")
    );
    let csp = response.headers()["content-security-policy"]
        .to_str()
        .unwrap()
        .to_owned();
    let html = String::from_utf8(
        to_bytes(response.into_body(), 65536)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(
        html.contains("Continue to app consent")
            && html.contains("href=\"https://accounts.google.com/")
    );
    for secret in [
        "access-sentinel",
        "refresh-sentinel",
        "client-secret-sentinel",
        "enc:v2:",
    ] {
        assert!(!html.contains(secret));
    }
    NavigationReply { html, csp, target }
}

#[tokio::test]
async fn integration_connect_navigation_uses_registered_navigation_and_shared_progress() {
    let f = Box::pin(Fixture::new()).await;
    let first = f.begin(None).await;
    let connected = f.finish(&first).await;
    let account = f
        .gateway()
        .get_account(&f.owner, connected.account.unwrap())
        .await
        .unwrap();
    let router = app(&f);
    let connect = begin_reply(&router, &f, "").await;
    let reconnect = begin_reply(
        &router,
        &f,
        &format!(
            "account={}&authorization={}",
            account.id.as_uuid(),
            account.authorization_revision.get()
        ),
    )
    .await;
    let html = pages::base_layout(
        "Connected apps",
        &pages::vault::vault_content(
            &AccountPage {
                accounts: vec![account],
                next: None,
            },
            None,
            &f.gateway().connectable_apps().await.unwrap(),
        ),
    );
    assert_eq!(
        html.matches("hx-post=\"/ui/profile/vault/apps/gmail/connect\"")
            .count(),
        2
    );
    assert_eq!(html.matches("hx-disabled-elt=\"find button\"").count(), 2);
    if let Some(dir) = std::env::var_os("OPENBOT_BROWSER_FIXTURE_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("page.html"), html).unwrap();
        std::fs::write(dir.join("app.js"), pages::application_javascript()).unwrap();
        std::fs::write(dir.join("connect.html"), &connect.html).unwrap();
        std::fs::write(dir.join("reconnect.html"), &reconnect.html).unwrap();
        let metadata = serde_json::json!({"csp":connect.csp,"connect":connect.target,"reconnect":reconnect.target});
        std::fs::write(dir.join("navigation.json"), metadata.to_string()).unwrap();
    }
}
