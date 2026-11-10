use super::*;
use crate::application::session::VerifiedSession;
use serde_json::{Value, json};

#[path = "operation_test_support.rs"]
mod support;
use support::*;

#[tokio::test]
async fn doop_connection_foreign_paths_cannot_read_update_rotate_refresh_or_delete() {
    use crate::use_cases::mcp::McpCredentialPersistence;
    let f = Box::pin(Fixture::new(&["get_profile"])).await;
    let definition = json!({"slug":"doop","endpoint_url":f.right.endpoint,"transport":"streamable_http",
        "enabled":true,"auth":{"type":"bearer"},"tool_grants":[]});
    for request in [
        f.connection_request("GET", f.company, f.right.id, "", Value::Null),
        f.connection_request(
            "PUT",
            f.company,
            f.right.id,
            "",
            json!({"expected_revision":f.right.revision,"definition":definition}),
        ),
        f.connection_request(
            "DELETE",
            f.company,
            f.right.id,
            "",
            json!({"expected_revision":f.right.revision}),
        ),
        f.connection_request(
            "PUT",
            f.company,
            f.right.id,
            "/credential",
            json!({"expected_revision":f.right.revision,"token":"attempted-cross-company"}),
        ),
        f.connection_request(
            "POST",
            f.company,
            f.right.id,
            "/refresh",
            json!({"expected_revision":f.right.revision}),
        ),
    ] {
        let response = f.send(request).await;
        assert_eq!(response.status, StatusCode::NOT_FOUND, "{}", response.body);
    }
    assert!(
        f.persistence
            .mcp_token(f.company, f.right.id, f.right.revision)
            .await
            .is_err()
    );
    let unchanged = f.state.mcp.get(f.user, f.other, f.right.id).await.unwrap();
    assert_eq!(unchanged.revision, f.right.revision);
    assert_eq!(unchanged.tool_grants, f.right.tool_grants);
}

#[tokio::test]
async fn doop_connection_same_slug_uses_separate_credentials_and_rotation() {
    use crate::use_cases::mcp::McpCredentialPersistence;
    use secrecy::ExposeSecret;
    let f = Box::pin(Fixture::new(&["get_profile"])).await;
    assert_eq!(f.left.slug, f.right.slug);
    for (company, connection) in [(f.company, &f.left), (f.other, &f.right)] {
        let response = f
            .send(f.connection_request("GET", company, connection.id, "", Value::Null))
            .await;
        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(response.body["connection"]["id"], json!(connection.id));
        assert!(!response.body.to_string().contains("left-key"));
        assert!(!response.body.to_string().contains("right-key"));
    }
    let response = f
        .send(f.connection_request(
            "PUT",
            f.company,
            f.left.id,
            "/credential",
            json!({"expected_revision":f.left.revision,"token":"left-rotated"}),
        ))
        .await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.body);
    let revision = response.body["revision"].as_i64().unwrap();
    assert!(revision > f.left.revision);
    assert_eq!(
        f.persistence
            .mcp_token(f.company, f.left.id, revision)
            .await
            .unwrap()
            .unwrap()
            .expose_secret(),
        "left-rotated"
    );
    assert_eq!(
        f.persistence
            .mcp_token(f.other, f.right.id, f.right.revision)
            .await
            .unwrap()
            .unwrap()
            .expose_secret(),
        "right-key"
    );
    assert!(!response.body.to_string().contains("left-rotated"));
}

#[tokio::test]
async fn doop_catalog_review_routes_scope_foreign_objects_and_competing_policy_versions() {
    let f = Box::pin(Fixture::new(&["get_profile"])).await;
    let empty = f
        .send(f.request("GET", f.company, f.left.id, "", Value::Null))
        .await;
    assert_eq!(empty.status, StatusCode::OK);
    assert_eq!(empty.body["operations"], json!([]));
    let body = policy(f.left.revision, None);
    let (first, second) = tokio::join!(
        f.send(f.request("PUT", f.company, f.left.id, "/get_profile", body.clone())),
        f.send(f.request("PUT", f.company, f.left.id, "/get_profile", body)),
    );
    let mut statuses = [first.status, second.status];
    statuses.sort();
    assert_eq!(
        statuses,
        [StatusCode::CREATED, StatusCode::CONFLICT],
        "{} / {}",
        first.body,
        second.body
    );
    let page = f
        .send(f.request("GET", f.company, f.left.id, "", Value::Null))
        .await;
    assert_eq!(page.body["operations"].as_array().unwrap().len(), 1);
    let operation = &page.body["operations"][0];
    assert_eq!(
        operation["operation"]["tool"]["policy"]["effect"], "write",
        "operator policy, not readOnlyHint"
    );
    assert_eq!(operation["granted"], true);
    assert_eq!(operation["schema_current"], true);
    let other = f
        .send(f.request(
            "PUT",
            f.other,
            f.right.id,
            "/get_profile",
            policy(f.right.revision, None),
        ))
        .await;
    assert_eq!(other.status, StatusCode::CREATED);
    for method in ["GET", "PUT", "DELETE"] {
        let suffix = if method == "GET" { "" } else { "/get_profile" };
        let mut body = json!({"expected_revision":f.right.revision,"expected_operation_version":other.body["version"]});
        if method == "PUT" {
            body["effect"] = json!("read");
            body["approval_required"] = json!(false);
        }
        let denied = f
            .send(f.request(method, f.company, f.right.id, suffix, body))
            .await;
        assert_eq!(denied.status, StatusCode::NOT_FOUND);
    }
    let request = f.request("PUT", f.company, f.left.id, "/get_profile", json!({
        "expected_revision":f.left.revision,"expected_operation_version":null,"effect":"read","approval_required":false,
        "output_schema":{"type":"object"}
    }));
    assert_eq!(
        f.send(request).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[tokio::test]
async fn doop_catalog_output_only_drift_is_visible_before_fresh_explicit_review() {
    let f = Box::pin(Fixture::new(&["get_profile"])).await;
    let review_request = |revision, version| {
        f.request(
            "PUT",
            f.company,
            f.left.id,
            "/get_profile",
            policy(revision, version),
        )
    };
    let approved = f.send(review_request(f.left.revision, None)).await;
    assert_eq!(approved.status, StatusCode::CREATED, "{}", approved.body);
    let old_version: Uuid = serde_json::from_value(approved.body["version"].clone()).unwrap();
    let before = super::super::projection(&f.left);
    let mut changed = f.left.discovered_tools[0].output_schema.clone().unwrap();
    changed["properties"]["contract_revision"] = json!({"type":"integer","minimum":1});
    changed["required"]
        .as_array_mut()
        .unwrap()
        .push(json!("contract_revision"));
    f.tools.lock().unwrap()[0].output_schema = Some(changed.clone());
    let refreshed = f
        .state
        .mcp
        .refresh(
            f.user,
            &f.left,
            crate::application::integrations::VerifiedOwner::from_verified_client(
                crate::adapters::integrations::owner::owner_for_user(f.user),
            ),
        )
        .await
        .unwrap();
    assert!(refreshed.tool_grants.is_empty());
    let visible = super::super::projection(&refreshed);
    assert_eq!(visible["discovered_tools"][0]["output_schema"], changed);
    assert_eq!(
        visible["discovered_tools"][0]["input_schema_fingerprint"],
        before["discovered_tools"][0]["input_schema_fingerprint"]
    );
    assert_ne!(
        visible["discovered_tools"][0]["output_schema_fingerprint"],
        before["discovered_tools"][0]["output_schema_fingerprint"]
    );
    assert_eq!(
        f.send(review_request(f.left.revision, Some(old_version)))
            .await
            .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        f.send(review_request(refreshed.revision, Some(old_version)))
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    let granted = f
        .state
        .mcp
        .update(
            f.user,
            &refreshed,
            crate::use_cases::mcp::McpConnectionWrite {
                slug: refreshed.slug.clone(),
                endpoint: refreshed.endpoint.clone(),
                enabled: refreshed.enabled,
                auth: refreshed.auth.clone(),
                discovered_tools: refreshed.discovered_tools.clone(),
                tool_grants: vec!["get_profile".to_owned().try_into().unwrap()],
            },
        )
        .await
        .unwrap();
    let current = f
        .send(f.request("GET", f.company, f.left.id, "", Value::Null))
        .await;
    assert_eq!(current.status, StatusCode::OK);
    let expected = current.body["operations"]
        .as_array()
        .unwrap()
        .first()
        .map(|review| serde_json::from_value(review["version"].clone()).unwrap());
    let fresh = f.send(review_request(granted.revision, expected)).await;
    assert_eq!(
        fresh.status,
        if expected.is_some() {
            StatusCode::OK
        } else {
            StatusCode::CREATED
        }
    );
    assert_eq!(
        fresh.body["operation"]["tool"]["contract"]["output_schema"],
        changed
    );
    assert_ne!(fresh.body["version"], json!(old_version));
}

#[tokio::test]
async fn doop_catalog_review_pages_are_bounded_and_cursor_visits_the_complete_selection() {
    let names = [
        "get_guide",
        "get_profile",
        "list_canvases",
        "create_canvas",
        "get_canvas",
        "get_frame",
        "create_frame",
        "edit_frame_html",
        "get_frame_screenshot",
    ];
    let f = Box::pin(Fixture::new(&names)).await;
    for name in names {
        assert_eq!(
            f.send(f.request(
                "PUT",
                f.company,
                f.left.id,
                &format!("/{name}"),
                policy(f.left.revision, None)
            ))
            .await
            .status,
            StatusCode::CREATED
        );
    }
    let first = f
        .send(f.request("GET", f.company, f.left.id, "", Value::Null))
        .await;
    assert_eq!(
        first.body["operations"].as_array().unwrap().len(),
        MCP_REVIEW_PAGE_SIZE
    );
    assert_eq!(
        MCP_REVIEW_PAGE_SIZE, 8,
        "keep the response below the existing discovery byte bound at maximum snapshot size"
    );
    let next = first.body["next"].as_str().unwrap();
    let last = f
        .send(f.request(
            "GET",
            f.company,
            f.left.id,
            &format!("?after={next}"),
            Value::Null,
        ))
        .await;
    assert_eq!(last.body["operations"].as_array().unwrap().len(), 1);
    assert!(last.body["next"].is_null());
    let actual = first.body["operations"]
        .as_array()
        .unwrap()
        .iter()
        .chain(last.body["operations"].as_array().unwrap())
        .map(|review| {
            review["operation"]["tool"]["contract"]["name"]
                .as_str()
                .unwrap()
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(actual, names.into_iter().collect());
}

#[tokio::test]
async fn doop_catalog_review_http_requires_verified_session_same_origin_and_bounded_body() {
    let f = Box::pin(Fixture::new(&["get_profile"])).await;
    let mut request = f.request(
        "PUT",
        f.company,
        f.left.id,
        "/get_profile",
        policy(f.left.revision, None),
    );
    request.extensions_mut().remove::<VerifiedSession>();
    assert_eq!(f.send(request).await.status, StatusCode::UNAUTHORIZED);
    let mut request = f.request(
        "PUT",
        f.company,
        f.left.id,
        "/get_profile",
        policy(f.left.revision, None),
    );
    request
        .headers_mut()
        .insert("origin", "https://foreign.test".parse().unwrap());
    assert_eq!(f.send(request).await.status, StatusCode::FORBIDDEN);
    let mut request = f.request(
        "PUT",
        f.company,
        f.left.id,
        "/get_profile",
        policy(f.left.revision, None),
    );
    *request.body_mut() = axum::body::Body::from(format!(
        "{}{}",
        " ".repeat(16 * 1024),
        policy(f.left.revision, None)
    ));
    assert_eq!(f.send(request).await.status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn doop_catalog_review_update_and_revoke_require_exact_live_versions() {
    let f = Box::pin(Fixture::new(&["get_profile"])).await;
    let put = |version| {
        f.request(
            "PUT",
            f.company,
            f.left.id,
            "/get_profile",
            policy(f.left.revision, version),
        )
    };
    let delete = |version| {
        f.request(
            "DELETE",
            f.company,
            f.left.id,
            "/get_profile",
            json!({"expected_revision":f.left.revision,"expected_operation_version":version}),
        )
    };
    let created = f.send(put(None)).await;
    assert_eq!(created.status, StatusCode::CREATED);
    let first: Uuid = serde_json::from_value(created.body["version"].clone()).unwrap();
    let changed = f.send(put(Some(first))).await;
    assert_eq!(changed.status, StatusCode::OK, "{}", changed.body);
    let current: Uuid = serde_json::from_value(changed.body["version"].clone()).unwrap();
    assert_ne!(first, current);
    assert_eq!(
        changed.body["operation"]["tool"]["policy"]["policy_revision"],
        2
    );
    let stale = f.send(delete(first)).await;
    assert_eq!(stale.status, StatusCode::CONFLICT);
    let still_active = f
        .send(f.request("GET", f.company, f.left.id, "", Value::Null))
        .await;
    assert_eq!(
        still_active.body["operations"][0]["version"],
        json!(current)
    );
    assert_eq!(f.send(delete(current)).await.status, StatusCode::NO_CONTENT);
    let revoked = f
        .send(f.request("GET", f.company, f.left.id, "", Value::Null))
        .await;
    assert_eq!(revoked.body["operations"], json!([]));
}
