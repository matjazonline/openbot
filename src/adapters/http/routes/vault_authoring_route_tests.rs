use super::*;

async fn html(f: &Fixture, path: &str, cookie: &str) -> (StatusCode, String) {
    let mut request = f.request("GET", path, String::new(), None);
    request
        .headers_mut()
        .insert("cookie", cookie.parse().unwrap());
    let response = f.app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 200_000).await.unwrap();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

#[tokio::test]
async fn vault_authoring_picker_is_owner_scoped_and_only_exposes_approved_metadata() {
    let f = Fixture::new().await;
    let config = Arc::new(AppConfig::for_test());
    let other = browser_identity(&f.p, &config).await;
    sqlx::query("INSERT INTO company_members(id,company_id,user_id,role) VALUES(gen_random_uuid(),$1,$2,'admin')")
        .bind(f.company).bind(other.user).execute(f.p.pool()).await.unwrap();
    let mut tx = f.p.pool().begin().await.unwrap();
    crate::adapters::persistence::participant::create_person_principal_on(
        &mut tx,
        f.company,
        other.user,
        "other-administrator",
        "other-administrator@example.test",
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let source = format!(
        "/ui/companies/{}/connection-credentials/http/{}",
        f.company, f.connection
    );
    let (status, before) = html(&f, &source, &other.cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(before.contains("static-api-key") && before.contains("static-bearer"));
    assert!(
        !before.contains("&lt;private label&gt;") && !before.contains("sentinel-never-in-browser")
    );
    let (status, consent) = html(&f, &f.path(), &f.cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(consent.contains("&lt;private label&gt;") && consent.contains("hx-sync=\"this:drop\""));
    assert!(!consent.contains("sentinel-never-in-browser"));
    if let Some(directory) = std::env::var_os("VAULT_AUTHORING_BROWSER_FIXTURES") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        let (_, source) = html(&f, &source, &f.cookie).await;
        std::fs::write(directory.join("source.html"), source).unwrap();
        std::fs::write(directory.join("consent.html"), &consent).unwrap();
        std::fs::write(
            directory.join("app.js"),
            crate::adapters::http::pages::application_javascript(),
        )
        .unwrap();
    }
    let (status, denied) = html(&f, &f.path(), &other.cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!denied.contains("private label"));
    assert!(!denied.contains(&f.account.id.as_uuid().to_string()));
    let response = f
        .app
        .clone()
        .oneshot(f.request(
            "POST",
            &f.path(),
            format!("account_revision={}%3A1", f.account.id.as_uuid()),
            Some("http://localhost:3000"),
        ))
        .await
        .unwrap();
    assert!(response.status().is_redirection());
    let (status, configured) = html(&f, &source, &other.cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        configured.contains("&lt;private label&gt;")
            && configured.contains("Owner consent recorded.")
    );
    assert!(!configured.contains("sentinel-never-in-browser"));
    sqlx::query("DELETE FROM company_members WHERE company_id=$1 AND user_id=$2")
        .bind(f.company)
        .bind(other.user)
        .execute(f.p.pool())
        .await
        .unwrap();
    assert_ne!(html(&f, &source, &other.cookie).await.0, StatusCode::OK);
}

#[tokio::test]
async fn vault_authoring_foreign_company_cannot_inspect_a_connection_source() {
    let f = Fixture::new().await;
    let other = browser_identity(&f.p, &Arc::new(AppConfig::for_test())).await;
    let path = format!(
        "/ui/companies/{}/connection-credentials/http/{}",
        other.company, f.connection
    );
    let (status, body) = html(&f, &path, &f.cookie).await;
    assert_ne!(status, StatusCode::OK);
    assert!(!body.contains("private label") && !body.contains("sentinel-never-in-browser"));
}
