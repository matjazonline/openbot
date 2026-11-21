use super::*;
use crate::application::knowledge::{KnowledgeScope, KnowledgeUploadFile, KnowledgeUploadTarget};

#[tokio::test]
async fn knowledge_scope_review_department_limit_refuses_only_knowledge_and_preserves_authorized_roster_and_desks()
 {
    // Exercise both the shared resolver's combined-chain bound and the child-row bound.
    for children in [255, 257] {
        Box::pin(limited_card(children)).await;
    }
}

async fn limited_card(children: usize) {
    let f = Fixture::new().await.unwrap();
    let unit = create(&f, "Readable department", "readable-department").await;
    let principal = crate::entities::transport::PrincipalId::new(person(&f, &f.member).await);
    f.departments()
        .add_member(f.owner.id, f.company.id, unit, principal)
        .await
        .unwrap();
    f.departments()
        .assign_role(
            f.owner.id,
            f.company.id,
            unit,
            crate::application::ports::department::DepartmentRoleAssignment {
                role: crate::entities::department::DepartmentRole::Lead,
                principal: principal,
                decider: false,
            },
        )
        .await
        .unwrap();
    let owner = workspace(&f, &f.owner);
    Box::pin(owner.knowledge.upload(
        KnowledgeUploadTarget {
            scope: KnowledgeScope {
                company: crate::domain::workflow::CompanyId::new(f.company.id),
                actor_id: f.owner.id,
            },
            unit,
        },
        vec![
                KnowledgeUploadFile::from_upload(
                    "0_scope-card-canary.md",
                    b"visible only when knowledge can be authorized".to_vec(),
                )
                .unwrap(),
            ],
    ))
    .await
    .unwrap();
    for index in 0..children {
        f.departments()
            .create_department(
                f.owner.id,
                f.company.id,
                crate::application::ports::department::DepartmentWrite {
                    name: format!("Report {index:03}"),
                    slug: format!("report-{index:03}"),
                    parent_id: Some(unit),
                    ..Default::default()
                },
                None,
            )
            .await
            .unwrap();
    }
    let response = show_department(
        workspace(&f, &f.member),
        Path((f.company.id, unit.as_uuid())),
    )
    .await
    .unwrap()
    .into_response();
    let (status, _, html) = body(response).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Readable department"));
    assert!(html.contains("id=\"department-members\"") && html.contains("id=\"department-desks\""));
    assert!(html.contains("Department desk"));
    assert!(html.contains("data-knowledge-unavailable=\"scope-limit\""));
    assert!(!html.contains("0_scope-card-canary.md"));
    let layers = owner
        .knowledge
        .documents()
        .list(
            KnowledgeScope {
                company: crate::domain::workflow::CompanyId::new(f.company.id),
                actor_id: f.owner.id,
            },
            Some(unit),
        )
        .await
        .unwrap()
        .layers;
    assert_eq!(
        layers.len(),
        1,
        "a refused card does not discard the document"
    );
    owner.knowledge.shutdown().await.unwrap();
    sqlx::query("DELETE FROM companies WHERE id=ANY($1)")
        .bind(vec![f.company.id, f.foreign.id])
        .execute(f.persistence.pool())
        .await
        .unwrap();
}

#[tokio::test]
async fn knowledge_scope_upload_department_pane_lists_owned_visible_documents_and_move_guidance() {
    let f = Fixture::new().await.unwrap();
    let unit = create(&f, "Knowledge department", "knowledge-department").await;
    let owner = workspace(&f, &f.owner);
    let scope = KnowledgeScope {
        company: crate::domain::workflow::CompanyId::new(f.company.id),
        actor_id: f.owner.id,
    };
    owner
        .knowledge
        .upload(
            KnowledgeUploadTarget { scope, unit },
            vec![
                KnowledgeUploadFile::from_upload("0_notes.md", b"public".to_vec()).unwrap(),
                KnowledgeUploadFile::from_upload("5_notes.md", b"private".to_vec()).unwrap(),
            ],
        )
        .await
        .unwrap();
    let response = show_department(owner, Path((f.company.id, unit.as_uuid())))
        .await
        .unwrap()
        .into_response();
    let (_, _, html) = body(response).await;
    assert!(html.contains("id=\"department-knowledge\""));
    assert!(html.contains("0_notes.md") && html.contains("5_notes.md"));
    assert!(html.contains(&format!(
        "/ui/knowledge?company_id={}&amp;unit_id={unit}",
        f.company.id
    )));
    assert!(html.contains("Move or delete this department’s documents before deleting"));
    let member = crate::entities::transport::PrincipalId::new(person(&f, &f.member).await);
    f.departments()
        .add_member(f.owner.id, f.company.id, unit, member)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE department_members SET access_level=0 WHERE company_id=$1 AND root_user_id=$2",
    )
    .bind(f.company.id)
    .bind(f.member.id)
    .execute(f.persistence.pool())
    .await
    .unwrap();
    let response = show_department(
        workspace(&f, &f.member),
        Path((f.company.id, unit.as_uuid())),
    )
    .await
    .unwrap()
    .into_response();
    let (_, _, html) = body(response).await;
    assert!(html.contains("0_notes.md"));
    assert!(!html.contains("5_notes.md"));
    sqlx::query("DELETE FROM companies WHERE id=ANY($1)")
        .bind(vec![f.company.id, f.foreign.id])
        .execute(f.persistence.pool())
        .await
        .unwrap();
}
