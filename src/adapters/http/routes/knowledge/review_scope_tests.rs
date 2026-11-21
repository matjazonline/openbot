//! Regression cases for the four independent source-review findings.
use super::scopes::{cleanup, unit, upload_in};
use super::*;
use crate::entities::department::{DepartmentId, DepartmentRole};
use serde_json::{Value, json};

async fn grant(s: &Setup, unit: DepartmentId) {
    let principal = crate::adapters::persistence::memory::person_principal_on(
        &mut s.f.persistence.pool().acquire().await.unwrap(),
        s.f.company.id,
        s.f.member.id,
    )
    .await
    .unwrap();
    s.f.departments()
        .assign_role(
            s.f.owner.id,
            s.f.company.id,
            unit,
            crate::application::ports::department::DepartmentRoleAssignment {
                role: DepartmentRole::Reviewer,
                principal: principal,
                decider: false,
            },
        )
        .await
        .unwrap();
}

fn move_requests(
    s: &Setup,
    source: &KnowledgeDocumentLayer,
    target: DepartmentId,
) -> [HttpRequest<Body>; 2] {
    [
        request(
            s.f.member.id,
            Method::POST,
            &format!(
                "/api/companies/{}/knowledge/documents/{}/move",
                s.f.company.id, source.id
            ),
            serde_json::to_vec(
                &json!({"expected_revision":source.revision,"target_unit_id":target}),
            )
            .unwrap(),
            "application/json",
        ),
        request(
            s.f.member.id,
            Method::POST,
            &format!(
                "/ui/knowledge/{}/move?company_id={}&unit_id={}",
                source.id, s.f.company.id, source.unit_id
            ),
            format!(
                "expected_revision={}&target_unit_id={target}",
                source.revision
            )
            .into_bytes(),
            "application/x-www-form-urlencoded",
        ),
    ]
}

async fn refused_move(
    s: &Setup,
    source: &KnowledgeDocumentLayer,
    target: DepartmentId,
    status: StatusCode,
    message: &str,
) {
    let before = serde_json::to_value(s.layers(s.f.owner.id).await).unwrap();
    for input in move_requests(s, source, target) {
        let response = s.router().oneshot(input).await.unwrap();
        assert_eq!(response.status(), status);
        let body = text(response).await;
        assert!(body.contains(message), "{body}");
        if status == StatusCode::FORBIDDEN {
            assert!(!body.contains("already exists"));
        }
        assert_eq!(
            serde_json::to_value(s.layers(s.f.owner.id).await).unwrap(),
            before
        );
    }
}

#[tokio::test]
async fn knowledge_scope_review_hidden_destination_collision_is_generic_but_visible_collision_is_explained()
 {
    let s = Setup::new().await.unwrap();
    let source_unit = unit(&s, "Source", "source").await;
    let target = unit(&s, "Target", "target").await;
    grant(&s, source_unit).await;
    grant(&s, target).await;
    let id = upload_in(&s, source_unit, "0_shared.md").await;
    upload_in(&s, target, "5_shared.md").await;
    let visible = s
        .services
        .library
        .documents()
        .list(
            KnowledgeScope {
                company: CompanyId::new(s.f.company.id),
                actor_id: s.f.member.id,
            },
            Some(target),
        )
        .await
        .unwrap();
    assert!(
        visible.layers.is_empty(),
        "the target filename is hidden at the mover's level"
    );
    let source = s
        .layers(s.f.owner.id)
        .await
        .into_iter()
        .find(|layer| layer.id == id)
        .unwrap();
    refused_move(
        &s,
        &source,
        target,
        StatusCode::FORBIDDEN,
        "The whole document cannot be moved with your current access",
    )
    .await;
    upload_in(&s, target, "0_shared.md").await;
    refused_move(
        &s,
        &source,
        target,
        StatusCode::CONFLICT,
        "A document with this name already exists",
    )
    .await;
    assert_eq!(s.model.queries.load(Ordering::SeqCst), 0);
    cleanup(&s).await;
}

fn search_requests(s: &Setup, unit: DepartmentId) -> [HttpRequest<Body>; 2] {
    [
        request(
            s.f.member.id,
            Method::POST,
            &format!("/api/companies/{}/knowledge/search", s.f.company.id),
            serde_json::to_vec(&json!({"query":"notes","unit_id":unit})).unwrap(),
            "application/json",
        ),
        request(
            s.f.member.id,
            Method::POST,
            &format!(
                "/ui/knowledge/search?company_id={}&unit_id={unit}",
                s.f.company.id
            ),
            b"query=notes".to_vec(),
            "application/x-www-form-urlencoded",
        ),
    ]
}

#[tokio::test]
async fn knowledge_scope_review_filtered_manual_search_refuses_foreign_and_unreadable_units_before_model_admission()
 {
    let s = Setup::new().await.unwrap();
    let unreadable = unit(&s, "Unreadable", "unreadable").await;
    let foreign = s
        .services
        .library
        .documents()
        .list(
            KnowledgeScope {
                company: CompanyId::new(s.f.foreign.id),
                actor_id: s.f.stranger.id,
            },
            None,
        )
        .await
        .unwrap()
        .upload_units
        .into_iter()
        .find(|unit| unit.is_root)
        .unwrap()
        .id;
    for unit in [unreadable, foreign] {
        for input in search_requests(&s, unit) {
            assert_eq!(
                s.router().oneshot(input).await.unwrap().status(),
                StatusCode::NOT_FOUND
            );
            assert_eq!(s.model.queries.load(Ordering::SeqCst), 0);
        }
    }
    let held = s
        .services
        .library
        .embed_query("hold the only query slot")
        .await
        .unwrap();
    for input in search_requests(&s, foreign) {
        assert_eq!(
            s.router().oneshot(input).await.unwrap().status(),
            StatusCode::NOT_FOUND,
            "scope refusal precedes model-busy refusal"
        );
    }
    assert_eq!(s.model.queries.load(Ordering::SeqCst), 1);
    drop(held);
    cleanup(&s).await;
}

fn link(html: &str, marker: &str) -> String {
    html.split_once(&format!("{marker} href=\""))
        .unwrap()
        .1
        .split('"')
        .next()
        .unwrap()
        .replace("&amp;", "&")
}

async fn get(s: &Setup, url: &str) -> Response {
    s.router()
        .oneshot(request(
            s.f.owner.id,
            Method::GET,
            url,
            Vec::new(),
            "text/html",
        ))
        .await
        .unwrap()
}

#[tokio::test]
async fn knowledge_scope_review_native_move_reaches_257th_destination_without_filtering_out_source_or_raising_picker_bound()
 {
    let s = Setup::new().await.unwrap();
    let source = upload_in(&s, s.root, "0_move.md").await;
    let mut last = s.root;
    for index in 0..256 {
        last = unit(
            &s,
            &format!("Destination {index:03}"),
            &format!("destination-{index:03}"),
        )
        .await;
    }
    let source_url = format!(
        "/ui/knowledge/{source}/move?company_id={}&unit_id={}",
        s.f.company.id, s.root
    );
    let response = get(&s, &source_url).await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = text(response).await;
    assert!(
        !html.contains(&format!("<option value=\"{last}\"")),
        "last target is beyond the first picker page"
    );
    let response = get(&s, &link(&html, "data-knowledge-destinations")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = text(response).await;
    assert!(html.matches("data-knowledge-destination=").count() <= 256);
    assert!(html.contains("Choose Destination 255"));
    let selected = link(&html, &format!("data-knowledge-destination=\"{last}\""));
    let response = get(&s, &selected).await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = text(response).await;
    assert!(html.contains("Move move.md"));
    assert!(html.contains(&format!("<option value=\"{last}\" selected")));
    assert!(
        html.matches("<option ").count() <= 258,
        "bound includes the placeholder and selected destination"
    );
    let response = s
        .router()
        .oneshot(request(
            s.f.owner.id,
            Method::POST,
            &format!(
                "/ui/knowledge/{source}/move?company_id={}&unit_id={}",
                s.f.company.id, s.root
            ),
            format!("expected_revision=1&target_unit_id={last}").into_bytes(),
            "application/x-www-form-urlencoded",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let layer = s
        .layers(s.f.owner.id)
        .await
        .into_iter()
        .find(|layer| layer.id == source)
        .unwrap();
    assert_eq!((layer.unit_id, layer.revision), (last, 2));
    // Keep the HTTP-heavy foreign-ID checks out of the boundary fixture's parent frame.
    Box::pin(foreign_move_ids(&s, source, &source_url)).await;
    cleanup(&s).await;
}

async fn foreign_move_ids(s: &Setup, source: Uuid, source_url: &str) {
    let foreign = s
        .services
        .library
        .documents()
        .list(
            KnowledgeScope {
                company: CompanyId::new(s.f.foreign.id),
                actor_id: s.f.stranger.id,
            },
            None,
        )
        .await
        .unwrap()
        .upload_units[0]
        .id;
    let uploaded = s
        .upload(
            s.f.stranger.id,
            s.f.foreign.id,
            &[File {
                name: "0_foreign.md",
                bytes: b"foreign",
            }],
        )
        .await;
    assert_eq!(uploaded.status(), StatusCode::CREATED);
    let foreign_doc: Value = serde_json::from_str(&text(uploaded).await).unwrap();
    let foreign_doc = Uuid::parse_str(foreign_doc[0]["id"].as_str().unwrap()).unwrap();
    for url in [
        format!("{source_url}&target_unit_id={foreign}"),
        format!(
            "/ui/knowledge/{source}/move/destinations?company_id={}&after={foreign}",
            s.f.company.id
        ),
        format!(
            "/ui/knowledge/{foreign_doc}/move/destinations?company_id={}",
            s.f.company.id
        ),
    ] {
        assert_eq!(get(s, &url).await.status(), StatusCode::NOT_FOUND);
    }
}
