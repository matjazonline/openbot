use super::*;
use axum::body::Bytes;

const CONTENT_TYPE: &str = "multipart/form-data; boundary=skill-boundary";

async fn source(app: &Router, owner: &HeaderValue) -> (Skill, Bytes) {
    let mut input = payload("script-directory");
    let asset = SkillFile::from_bytes(
        "scripts/check.py".into(),
        b"#!/usr/bin/env python3\nprint('bundle')\n",
        true,
    )
    .unwrap();
    input["files"] = json!([asset]);
    let created = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/skill-library",
            owner,
            serde_json::to_vec(&input).unwrap(),
            "application/json",
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let source: Skill = serde_json::from_value(value(created).await).unwrap();
    let downloaded = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/ui/skill-library/{}/bundle", source.id),
            owner,
            vec![],
            "application/json",
        ))
        .await
        .unwrap();
    assert_eq!(downloaded.status(), StatusCode::OK);
    assert_eq!(
        downloaded.headers()[header::CONTENT_TYPE],
        "application/zip"
    );
    assert_eq!(
        downloaded.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    let archive = to_bytes(downloaded.into_body(), archive::MAX_ARCHIVE_BYTES)
        .await
        .unwrap();
    (source, archive)
}

async fn import(app: &Router, f: &Fixture, owner: &HeaderValue, archive: &[u8]) -> Uuid {
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/ui/companies/{}/skills/import", f.company),
            owner,
            multipart(archive),
            CONTENT_TYPE,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let html = String::from_utf8(
        to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains("Skill directory imported."));
    sqlx::query_scalar("SELECT id FROM skills WHERE company_id=$1 AND slug='script-directory'")
        .bind(f.company)
        .fetch_one(&f.database.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn skill_bundle_http_import_download_replace_and_asset_metadata() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (app, authority) = app(&f).await;
    let owner = cookie(&f, &authority, f.owner).await;
    let (source, archive) = source(&app, &owner).await;
    let id = import(&app, &f, &owner, &archive).await;
    let company_uri = format!("/ui/companies/{}/skills/{id}/bundle", f.company);
    let files = app
        .clone()
        .oneshot(request(
            "GET",
            &(company_uri.clone() + "/files"),
            &owner,
            vec![],
            "application/json",
        ))
        .await
        .unwrap();
    assert_eq!(files.status(), StatusCode::OK);
    let html = String::from_utf8(
        to_bytes(files.into_body(), 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains("scripts/check.py") && html.contains("executable"));
    let mut replacement = source.clone();
    replacement.files = vec![
        SkillFile::from_bytes("scripts/check.py".into(), b"print('replacement')\n", false).unwrap(),
    ];
    let data = archive::encode(&replacement).unwrap();
    for uri in [
        company_uri,
        format!("/ui/skill-library/{}/bundle", source.id),
    ] {
        let response = app
            .clone()
            .oneshot(request(
                "POST",
                &uri,
                &owner,
                multipart(&data),
                CONTENT_TYPE,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let downloaded = app
            .clone()
            .oneshot(request("GET", &uri, &owner, vec![], "application/json"))
            .await
            .unwrap();
        assert_eq!(downloaded.status(), StatusCode::OK);
        let data = to_bytes(downloaded.into_body(), archive::MAX_ARCHIVE_BYTES)
            .await
            .unwrap();
        assert_eq!(
            archive::decode(&data).unwrap().files,
            Some(replacement.files.clone())
        );
    }
}

async fn assert_stranger_denied(
    app: &Router,
    stranger: &HeaderValue,
    company_uri: &str,
    library_uri: &str,
    import_uri: &str,
) {
    // Both tenant membership and the operator workspace hide their existence from strangers.
    for uri in [company_uri, library_uri] {
        for suffix in ["", "/files"] {
            assert_eq!(
                app.clone()
                    .oneshot(request(
                        "GET",
                        &(uri.to_owned() + suffix),
                        stranger,
                        vec![],
                        "application/json"
                    ))
                    .await
                    .unwrap()
                    .status(),
                StatusCode::NOT_FOUND
            );
        }
        assert_eq!(
            app.clone()
                .oneshot(request(
                    "POST",
                    uri,
                    stranger,
                    multipart(b"invalid ZIP"),
                    CONTENT_TYPE
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        app.clone()
            .oneshot(request(
                "POST",
                import_uri,
                stranger,
                multipart(b"invalid ZIP"),
                CONTENT_TYPE
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn skill_bundle_http_tenant_operator_and_csrf_guards() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (app, authority) = app(&f).await;
    let owner = cookie(&f, &authority, f.owner).await;
    let stranger = cookie(&f, &authority, f.stranger).await;
    let (source, archive) = source(&app, &owner).await;
    let id = import(&app, &f, &owner, &archive).await;
    let import_uri = format!("/ui/companies/{}/skills/import", f.company);
    let company_uri = format!("/ui/companies/{}/skills/{id}/bundle", f.company);
    let library_uri = format!("/ui/skill-library/{}/bundle", source.id);
    assert_stranger_denied(&app, &stranger, &company_uri, &library_uri, &import_uri).await;
    let foreign_uri = format!("/ui/companies/{}/skills/{id}/bundle", f.foreign);
    assert_eq!(
        app.clone()
            .oneshot(request(
                "GET",
                &foreign_uri,
                &owner,
                vec![],
                "application/json"
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let mut csrf = request(
        "POST",
        &import_uri,
        &owner,
        multipart(&archive),
        CONTENT_TYPE,
    );
    csrf.headers_mut()
        .insert(header::ORIGIN, "https://foreign.example".parse().unwrap());
    assert_eq!(
        app.oneshot(csrf).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn skill_bundle_http_members_cannot_upload_and_admins_keep_read_access() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (app, authority) = app(&f).await;
    let owner = cookie(&f, &authority, f.owner).await;
    let (_, archive) = source(&app, &owner).await;
    let id = import(&app, &f, &owner, &archive).await;
    let uri = format!("/ui/companies/{}/skills/{id}/bundle", f.company);
    for (actor, read_status) in [(f.member, StatusCode::FORBIDDEN), (f.admin, StatusCode::OK)] {
        let actor = cookie(&f, &authority, actor).await;
        for suffix in ["", "/files"] {
            assert_eq!(
                app.clone()
                    .oneshot(request(
                        "GET",
                        &(uri.clone() + suffix),
                        &actor,
                        vec![],
                        "application/json"
                    ))
                    .await
                    .unwrap()
                    .status(),
                read_status
            );
        }
        for destination in [&uri, &format!("/ui/companies/{}/skills/import", f.company)] {
            assert_eq!(
                app.clone()
                    .oneshot(request(
                        "POST",
                        destination,
                        &actor,
                        multipart(b"invalid ZIP"),
                        CONTENT_TYPE
                    ))
                    .await
                    .unwrap()
                    .status(),
                StatusCode::FORBIDDEN,
                "company membership does not grant owner-only import"
            );
        }
    }
}

#[tokio::test]
async fn skill_bundle_http_failed_replacement_keeps_original_bytes_and_modes() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (app, authority) = app(&f).await;
    let owner = cookie(&f, &authority, f.owner).await;
    let (source, archive) = source(&app, &owner).await;
    let id = import(&app, &f, &owner, &archive).await;
    for uri in [
        format!("/ui/companies/{}/skills/{id}/bundle", f.company),
        format!("/ui/skill-library/{}/bundle", source.id),
    ] {
        let failed = app
            .clone()
            .oneshot(request(
                "POST",
                &uri,
                &owner,
                multipart(b"invalid ZIP"),
                CONTENT_TYPE,
            ))
            .await
            .unwrap();
        assert_eq!(failed.status(), StatusCode::OK);
        let html = String::from_utf8(
            to_bytes(failed.into_body(), 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(html.contains("Invalid skill ZIP"));
        let restored = app
            .clone()
            .oneshot(request("GET", &uri, &owner, vec![], "application/json"))
            .await
            .unwrap();
        let after = to_bytes(restored.into_body(), archive::MAX_ARCHIVE_BYTES)
            .await
            .unwrap();
        let imported = archive::decode(&after).unwrap();
        assert_eq!(imported.files, Some(source.files.clone()));
        assert_eq!(imported.instructions, source.instructions);
    }
}
