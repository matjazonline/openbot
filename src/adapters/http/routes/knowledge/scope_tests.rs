use super::*;
use crate::entities::department::DepartmentId;
use futures::StreamExt;

async fn send(s: &Setup, method: Method, url: &str, body: Vec<u8>, kind: &str) -> Response {
    s.router()
        .oneshot(request(s.f.owner.id, method, url, body, kind))
        .await
        .unwrap()
}

pub(super) async fn cleanup(s: &Setup) {
    s.services.library.shutdown().await.unwrap();
    sqlx::query("DELETE FROM companies WHERE id=ANY($1)")
        .bind(vec![s.f.company.id, s.f.foreign.id])
        .execute(s.f.persistence.pool())
        .await
        .unwrap();
}

pub(super) async fn unit(s: &Setup, name: &str, slug: &str) -> DepartmentId {
    s.f.departments()
        .create_department(
            s.f.owner.id,
            s.f.company.id,
            crate::application::ports::department::DepartmentWrite {
                name: name.into(),
                slug: slug.into(),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap()
        .id
}

pub(super) async fn upload_in(s: &Setup, unit: DepartmentId, name: &str) -> Uuid {
    let response = send(
        s,
        Method::POST,
        &format!("/api/companies/{}/knowledge/documents", s.f.company.id),
        multipart_in(
            unit,
            &[File {
                name,
                bytes: b"department notes",
            }],
        ),
        "multipart/form-data; boundary=knowledge-test",
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let body: serde_json::Value = serde_json::from_str(&text(response).await).unwrap();
    Uuid::parse_str(body[0]["id"].as_str().unwrap()).unwrap()
}

fn unit_field(value: &str) -> Vec<u8> {
    format!(
        "--knowledge-test\r\nContent-Disposition: form-data; name=\"unit_id\"\r\n\r\n{value}\r\n"
    )
    .into_bytes()
}

fn member_search_request(s: &Setup) -> HttpRequest<Body> {
    request(
        s.f.member.id,
        Method::POST,
        &format!("/api/companies/{}/knowledge/search", s.f.company.id),
        serde_json::to_vec(&serde_json::json!({"query":"department notes"})).unwrap(),
        "application/json",
    )
}

#[tokio::test]
async fn knowledge_scope_search_manual_rechecks_revoked_grant_after_embedding() {
    let s = Setup::new().await.unwrap();
    let department = unit(&s, "Query grant", "query-grant").await;
    upload_in(&s, department, "0_notes.md").await;
    let principal = crate::adapters::persistence::memory::person_principal_on(
        &mut s.f.persistence.pool().acquire().await.unwrap(),
        s.f.company.id,
        s.f.member.id,
    )
    .await
    .unwrap();
    let departments = s.f.departments();
    departments
        .assign_role(
            s.f.owner.id,
            s.f.company.id,
            department,
            crate::entities::department::DepartmentRole::Reviewer,
            principal,
        )
        .await
        .unwrap();
    let before = s.router().oneshot(member_search_request(&s)).await.unwrap();
    assert_eq!(before.status(), StatusCode::OK);
    let before: serde_json::Value = serde_json::from_str(&text(before).await).unwrap();
    assert_eq!(before["passages"][0]["text"], "department notes");
    let (entered, waiting) = tokio::sync::oneshot::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    *s.model.query_gate.lock().unwrap() = Some(QueryGate {
        entered,
        release: blocked,
    });
    // Keep the HTTP future out of the competing grant mutation's parent poll frame.
    let search = Box::pin(s.router().oneshot(member_search_request(&s)));
    let revoke = async {
        tokio::time::timeout(std::time::Duration::from_secs(5), waiting)
            .await
            .unwrap()
            .unwrap();
        departments
            .clear_role(
                s.f.owner.id,
                s.f.company.id,
                department,
                crate::entities::department::DepartmentRole::Reviewer,
                principal,
            )
            .await
            .unwrap();
        release.send(()).unwrap();
    };
    let (response, ()) = tokio::join!(search, revoke);
    let response = response.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text(response).await).unwrap(),
        serde_json::json!({"passages":[]})
    );
    assert_eq!(s.model.queries.load(Ordering::SeqCst), 2);
    assert_eq!(
        s.layers(s.f.owner.id).await.len(),
        1,
        "revocation hides rather than deletes the document"
    );
    cleanup(&s).await;
}

#[tokio::test]
async fn knowledge_scope_upload_http_requires_first_unit_and_rejects_before_collecting_file_body() {
    let s = Setup::new().await.unwrap();
    let files = [File {
        name: "0_notes.md",
        bytes: b"notes",
    }];
    let url = format!("/api/companies/{}/knowledge/documents", s.f.company.id);
    let mut malformed = unit_field("invalid");
    malformed.extend(multipart(&files));
    let mut duplicate = unit_field(&s.root.to_string());
    duplicate.extend(unit_field(&s.root.to_string()));
    duplicate.extend(multipart(&files));
    for body in [multipart(&files), malformed, duplicate] {
        let response = send(
            &s,
            Method::POST,
            &url,
            body,
            "multipart/form-data; boundary=knowledge-test",
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    let foreign: Uuid =
        sqlx::query_scalar("SELECT id FROM departments WHERE company_id=$1 AND is_root")
            .bind(s.f.foreign.id)
            .fetch_one(s.f.persistence.pool())
            .await
            .unwrap();
    let mut prefix = unit_field(&foreign.to_string());
    prefix.extend_from_slice(b"--knowledge-test\r\nContent-Disposition: form-data; name=\"files\"; filename=\"0_notes.md\"\r\nContent-Type: text/plain\r\n\r\n");
    // Multipart may poll the transport ahead, but file data never becomes available.
    // Returning 404 before the deadline proves authorization precedes collecting that body.
    let chunks =
        futures::stream::once(
            async move { Ok::<_, std::io::Error>(axum::body::Bytes::from(prefix)) },
        )
        .chain(futures::stream::pending::<
            Result<axum::body::Bytes, std::io::Error>,
        >());
    let request = request(
        s.f.owner.id,
        Method::POST,
        &url,
        Vec::new(),
        "multipart/form-data; boundary=knowledge-test",
    )
    .map(|_| Body::from_stream(chunks));
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        s.router().oneshot(request),
    )
    .await
    .expect("reject foreign unit before waiting for its file body")
    .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(s.model.calls.load(Ordering::SeqCst), 0);
    assert!(s.layers(s.f.owner.id).await.is_empty());
    cleanup(&s).await;
}

#[tokio::test]
async fn knowledge_scope_http_search_move_and_move_form_scope_valid_foreign_ids() {
    let s = Setup::new().await.unwrap();
    let own = upload_in(&s, s.root, "0_own.md").await;
    let foreign = s
        .upload(
            s.f.stranger.id,
            s.f.foreign.id,
            &[File {
                name: "0_foreign.md",
                bytes: b"foreign secret",
            }],
        )
        .await;
    assert_eq!(foreign.status(), StatusCode::CREATED);
    let foreign: serde_json::Value = serde_json::from_str(&text(foreign).await).unwrap();
    let foreign_doc = Uuid::parse_str(foreign[0]["id"].as_str().unwrap()).unwrap();
    let foreign_unit = Uuid::parse_str(foreign[0]["unit_id"].as_str().unwrap()).unwrap();
    let queries_before = s.model.queries.load(Ordering::SeqCst);
    let response = send(
        &s,
        Method::POST,
        &format!("/api/companies/{}/knowledge/search", s.f.foreign.id),
        serde_json::to_vec(&serde_json::json!({"query":"secret"})).unwrap(),
        "application/json",
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(s.model.queries.load(Ordering::SeqCst), queries_before);
    let body =
        serde_json::to_vec(&serde_json::json!({"query":"secret","unit_id":foreign_unit})).unwrap();
    assert_eq!(
        send(
            &s,
            Method::POST,
            &format!("/api/companies/{}/knowledge/search", s.f.company.id),
            body,
            "application/json"
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    for url in [
        format!(
            "/ui/knowledge/{foreign_doc}/move?company_id={}",
            s.f.company.id
        ),
        format!(
            "/api/companies/{}/knowledge/documents?unit_id={foreign_unit}",
            s.f.company.id
        ),
    ] {
        assert_eq!(
            send(&s, Method::GET, &url, Vec::new(), "text/html")
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
    }
    let body = serde_json::to_vec(
        &serde_json::json!({"expected_revision":1,"target_unit_id":foreign_unit}),
    )
    .unwrap();
    assert_eq!(
        send(
            &s,
            Method::POST,
            &format!(
                "/api/companies/{}/knowledge/documents/{own}/move",
                s.f.company.id
            ),
            body,
            "application/json"
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    let body =
        serde_json::to_vec(&serde_json::json!({"expected_revision":1,"target_unit_id":s.root}))
            .unwrap();
    assert_eq!(
        send(
            &s,
            Method::POST,
            &format!(
                "/api/companies/{}/knowledge/documents/{foreign_doc}/move",
                s.f.company.id
            ),
            body,
            "application/json"
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    let layers = s.layers(s.f.owner.id).await;
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].id, own);
    assert_eq!(layers[0].revision, 1);
    assert_eq!(layers[0].unit_id, s.root);
    cleanup(&s).await;
}

#[tokio::test]
async fn knowledge_scope_upload_library_groups_filters_and_native_move_restores_form_on_failure() {
    let s = Setup::new().await.unwrap();
    upload_in(&s, s.root, "0_notes.md").await;
    let child = unit(&s, "Child <img onerror=alert(1)>", "child").await;
    let third = unit(&s, "Third", "third").await;
    let anchor = upload_in(&s, child, "0_notes.md").await;
    upload_in(&s, child, "5_notes.md").await;
    let page = text(
        send(
            &s,
            Method::GET,
            &format!("/ui/knowledge?company_id={}", s.f.company.id),
            Vec::new(),
            "text/html",
        )
        .await,
    )
    .await;
    assert!(
        page.find(&format!("data-knowledge-unit=\"{}\"", s.root))
            .unwrap()
            < page
                .find(&format!("data-knowledge-unit=\"{child}\""))
                .unwrap()
    );
    assert!(page.contains("Child &lt;img onerror=alert(1)&gt;"));
    assert!(!page.contains("Child <img"));
    let page = text(
        send(
            &s,
            Method::GET,
            &format!(
                "/ui/knowledge?company_id={}&unit_id={child}",
                s.f.company.id
            ),
            Vec::new(),
            "text/html",
        )
        .await,
    )
    .await;
    assert!(!page.contains(&format!("data-knowledge-unit=\"{}\"", s.root)));
    assert!(page.contains(&format!("value=\"{child}\" selected")));
    let moving = format!(
        "/ui/knowledge/{anchor}/move?company_id={}&unit_id={child}",
        s.f.company.id
    );
    let response = send(
        &s,
        Method::POST,
        &moving,
        format!("expected_revision=1&target_unit_id={}", s.root).into_bytes(),
        "application/x-www-form-urlencoded",
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let page = text(response).await;
    assert!(page.contains("already exists"));
    assert!(page.contains("name=\"target_unit_id\""));
    assert!(!page.contains("aria-busy=\"true\""));
    let response = send(
        &s,
        Method::POST,
        &moving,
        format!("expected_revision=1&target_unit_id={third}").into_bytes(),
        "application/x-www-form-urlencoded",
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert!(
        response
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap()
            .contains(&format!("unit_id={third}"))
    );
    let moved = s
        .services
        .library
        .documents()
        .list(
            KnowledgeScope {
                company: CompanyId::new(s.f.company.id),
                actor_id: s.f.owner.id,
            },
            Some(third),
        )
        .await
        .unwrap();
    assert_eq!(moved.layers.len(), 2);
    assert!(moved.layers.iter().all(|layer| layer.revision == 2));
    cleanup(&s).await;
}

#[tokio::test]
async fn knowledge_scope_search_native_form_accepts_maximum_encoded_unicode_and_rejects_over_limit()
{
    let s = Setup::new().await.unwrap();
    upload_in(&s, s.root, "0_notes.md").await;
    let url = format!("/ui/knowledge/search?company_id={}", s.f.company.id);
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("query", &"😀".repeat(1000))
        .finish()
        .into_bytes();
    assert!(body.len() > 4096 && body.len() < 16384);
    let response = send(
        &s,
        Method::POST,
        &url,
        body,
        "application/x-www-form-urlencoded",
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(text(response).await.contains("Knowledge search results"));
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("query", &"😀".repeat(1001))
        .finish()
        .into_bytes();
    assert_eq!(
        send(
            &s,
            Method::POST,
            &url,
            body,
            "application/x-www-form-urlencoded"
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        send(
            &s,
            Method::POST,
            &url,
            format!("query={}", "a".repeat(17000)).into_bytes(),
            "application/x-www-form-urlencoded"
        )
        .await
        .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(s.model.queries.load(Ordering::SeqCst), 1);
    cleanup(&s).await;
}
