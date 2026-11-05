use super::*;
use crate::{
    adapters::http::routes::authorization_test_support::Fixture,
    application::session::{SessionId, VerifiedSession},
    entities::value_objects::EmbeddingModelId,
};
use axum::{
    body::{Body, to_bytes},
    http::{Method, Request as HttpRequest},
    middleware,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::ServiceExt;
struct Model {
    calls: AtomicUsize,
}
impl TextEmbedder for Model {
    fn model_id(&self) -> EmbeddingModelId {
        "test-model".into()
    }
    fn passage_token_count(&self, text: &str) -> AppResult<usize> {
        Ok(text.chars().count() / 4 + 4)
    }
    fn embed_passages(&self, passages: &[&str]) -> AppResult<Vec<[f32; 384]>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut vector = [0.0; 384];
        vector[0] = 1.0;
        Ok(vec![vector; passages.len()])
    }
    fn embed_query(&self, _: &str) -> AppResult<[f32; 384]> {
        panic!("a library upload must not query the model")
    }
}
struct Setup {
    f: Fixture,
    services: Services,
    model: Arc<Model>,
}
impl Setup {
    async fn new() -> Option<Self> {
        let f = Fixture::new().await?;
        let model = Arc::new(Model {
            calls: AtomicUsize::new(0),
        });
        let services = Services {
            library: Arc::new(KnowledgeLibrary::new(f.persistence.clone(), model.clone())),
            companies: f.companies.clone(),
            users: f.users.clone(),
            config: f.config.clone(),
        };
        Some(Self { f, services, model })
    }
    fn router(&self) -> Router {
        routes::<Services>()
            .with_state(self.services.clone())
            .layer(middleware::from_fn_with_state(
                self.services.config.clone(),
                crate::adapters::http::security::browser_security,
            ))
    }
    async fn upload(&self, user: Uuid, company: Uuid, files: &[File<'_>]) -> Response {
        self.router()
            .oneshot(request(
                user,
                Method::POST,
                &format!("/api/companies/{company}/knowledge/documents"),
                multipart(files),
                "multipart/form-data; boundary=knowledge-test",
            ))
            .await
            .unwrap()
    }
    async fn layers(&self, user: Uuid) -> Vec<KnowledgeDocumentLayer> {
        self.services
            .library
            .documents()
            .list(KnowledgeScope {
                company: CompanyId::new(self.f.company.id),
                actor_id: user,
            })
            .await
            .unwrap()
            .layers
    }
}
struct File<'a> {
    name: &'a str,
    bytes: &'a [u8],
}
fn multipart(files: &[File<'_>]) -> Vec<u8> {
    let mut body = Vec::new();
    for file in files {
        body.extend_from_slice(format!("--knowledge-test\r\nContent-Disposition: form-data; name=\"files\"; filename=\"{}\"\r\nContent-Type: text/plain\r\n\r\n",file.name).as_bytes());
        body.extend_from_slice(file.bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(b"--knowledge-test--\r\n");
    body
}
fn request(user: Uuid, method: Method, url: &str, body: Vec<u8>, kind: &str) -> HttpRequest<Body> {
    let mut request = HttpRequest::builder()
        .method(method)
        .uri(url)
        .header("host", "mailagents.com")
        .header("origin", "http://mailagents.com")
        .header("cookie", "session=test")
        .header("content-type", kind)
        .body(Body::from(body))
        .unwrap();
    request.extensions_mut().insert(VerifiedSession {
        user_id: user,
        session_id: SessionId(Uuid::new_v4()),
    });
    request
}
async fn text(response: Response) -> String {
    String::from_utf8(
        to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}
#[tokio::test]
async fn knowledge_upload_http_whole_batch_validation_member_ceiling_layers_and_escaped_library() {
    let Some(s) = Setup::new().await else { return };
    let invalid = s
        .upload(
            s.f.owner.id,
            s.f.company.id,
            &[
                File {
                    name: "0_good.md",
                    bytes: b"valid",
                },
                File {
                    name: "bad.md",
                    bytes: b"bad",
                },
            ],
        )
        .await;
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_eq!(s.model.calls.load(Ordering::SeqCst), 0);
    assert!(s.layers(s.f.owner.id).await.is_empty());
    let denied = s
        .upload(
            s.f.member.id,
            s.f.company.id,
            &[File {
                name: "7_secret.md",
                bytes: b"private",
            }],
        )
        .await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    assert_eq!(s.model.calls.load(Ordering::SeqCst), 0);
    let response = s
        .upload(
            s.f.owner.id,
            s.f.company.id,
            &[
                File {
                    name: "0_<svg onload=alert(1)>.md",
                    bytes: b"public",
                },
                File {
                    name: "6_classified.md",
                    bytes: b"private",
                },
            ],
        )
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let api = s
        .router()
        .oneshot(request(
            s.f.member.id,
            Method::GET,
            &format!("/api/companies/{}/knowledge/documents", s.f.company.id),
            vec![],
            "application/json",
        ))
        .await
        .unwrap();
    let payload = text(api).await;
    assert!(payload.contains("svg onload"));
    assert!(!payload.contains("classified"));
    assert!(!payload.contains("private"));
    let page = s
        .router()
        .oneshot(request(
            s.f.member.id,
            Method::GET,
            &format!("/ui/knowledge?company_id={}", s.f.company.id),
            vec![],
            "text/html",
        ))
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    let html = text(page).await;
    assert_library_page(&html);
}
#[tokio::test]
async fn knowledge_upload_http_all_six_routes_hide_valid_foreign_company_and_document_ids_before_body_work()
 {
    let Some(s) = Setup::new().await else { return };
    assert_eq!(
        s.upload(
            s.f.stranger.id,
            s.f.foreign.id,
            &[File {
                name: "0_foreign.md",
                bytes: b"foreign"
            }]
        )
        .await
        .status(),
        StatusCode::CREATED
    );
    let foreign = s
        .services
        .library
        .documents()
        .list(KnowledgeScope {
            company: CompanyId::new(s.f.foreign.id),
            actor_id: s.f.stranger.id,
        })
        .await
        .unwrap()
        .layers
        .remove(0);
    let attempts = foreign_attempts(&s, foreign.id);
    for attempt in attempts {
        let response = s
            .router()
            .oneshot(request(
                s.f.owner.id,
                attempt.method.clone(),
                &attempt.url,
                attempt.body,
                attempt.kind,
            ))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "{} {}",
            attempt.method,
            attempt.url
        );
    }
    assert_eq!(s.model.calls.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn knowledge_upload_http_delete_revision_unknown_fields_and_cookie_origin_are_enforced() {
    let Some(s) = Setup::new().await else { return };
    assert_eq!(
        s.upload(
            s.f.member.id,
            s.f.company.id,
            &[File {
                name: "0_notes.md",
                bytes: b"public"
            }]
        )
        .await
        .status(),
        StatusCode::CREATED
    );
    let layer = s.layers(s.f.member.id).await.remove(0);
    let url = format!(
        "/api/companies/{}/knowledge/documents/{}",
        s.f.company.id, layer.id
    );
    for payload in [
        "{\"expected_revision\":0}",
        "{\"expected_revision\":1,\"access_level\":9}",
    ] {
        let response = s
            .router()
            .oneshot(request(
                s.f.member.id,
                Method::DELETE,
                &url,
                payload.as_bytes().to_vec(),
                "application/json",
            ))
            .await
            .unwrap();
        assert!(response.status().is_client_error());
    }
    let mut forged = request(
        s.f.member.id,
        Method::DELETE,
        &url,
        b"{\"expected_revision\":1}".to_vec(),
        "application/json",
    );
    forged
        .headers_mut()
        .insert("origin", "https://attacker.test".parse().unwrap());
    assert_eq!(
        s.router().oneshot(forged).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let response = s
        .router()
        .oneshot(request(
            s.f.member.id,
            Method::DELETE,
            &url,
            b"{\"expected_revision\":1}".to_vec(),
            "application/json",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(s.layers(s.f.owner.id).await.is_empty());
    let unauthenticated = HttpRequest::builder()
        .uri(format!(
            "/api/companies/{}/knowledge/documents",
            s.f.company.id
        ))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        s.router().oneshot(unauthenticated).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn knowledge_upload_http_unavailable_or_canceled_writes_require_refresh_instead_of_claiming_no_change()
 {
    for error in [
        AppError::ServiceUnavailable("private diagnostic".into()),
        AppError::Timeout("canceled".into()),
    ] {
        let response = mutation_error(error);
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let message = text(response).await;
        assert!(message.contains("could not be confirmed"));
        assert!(message.contains("Refresh the library"));
        assert!(!message.contains("private diagnostic"));
    }
}

fn assert_library_page(html: &str) {
    assert!(!html.contains("<svg onload="));
    assert!(html.contains("&lt;svg onload=alert(1)&gt;"));
    assert!(!html.contains("classified"));
    assert!(html.contains("data-submit=\"busy-once\""));
    assert!(html.contains("data-pending-label=\"Indexing…\""));
    assert!(html.contains("aria-current=\"page\""));
}

struct Attempt {
    method: Method,
    url: String,
    body: Vec<u8>,
    kind: &'static str,
}
fn foreign_attempts(s: &Setup, document: Uuid) -> [Attempt; 6] {
    let api = format!("/api/companies/{}/knowledge/documents", s.f.foreign.id);
    let own_api = format!("/api/companies/{}/knowledge/documents", s.f.company.id);
    [
        Attempt {
            method: Method::GET,
            url: api.clone(),
            body: vec![],
            kind: "application/json",
        },
        Attempt {
            method: Method::POST,
            url: api,
            body: multipart(&[File {
                name: "bad.md",
                bytes: b"invalid",
            }]),
            kind: "multipart/form-data; boundary=knowledge-test",
        },
        Attempt {
            method: Method::DELETE,
            url: format!("{own_api}/{}", document),
            body: b"{\"expected_revision\":1}".to_vec(),
            kind: "application/json",
        },
        Attempt {
            method: Method::GET,
            url: format!("/ui/knowledge?company_id={}", s.f.foreign.id),
            body: vec![],
            kind: "text/html",
        },
        Attempt {
            method: Method::POST,
            url: format!("/ui/knowledge/upload?company_id={}", s.f.foreign.id),
            body: multipart(&[File {
                name: "bad.md",
                bytes: b"invalid",
            }]),
            kind: "multipart/form-data; boundary=knowledge-test",
        },
        Attempt {
            method: Method::POST,
            url: format!(
                "/ui/knowledge/{}/delete?company_id={}",
                document, s.f.company.id
            ),
            body: b"expected_revision=1".to_vec(),
            kind: "application/x-www-form-urlencoded",
        },
    ]
}

#[path = "multipart_tests.rs"]
mod multipart_tests;
