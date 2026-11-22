//! `unit_archive_scopes_` for knowledge (Plan08 02C.1): an archived unit's documents stay where
//! they are and readable under the existing rules, and nothing new lands in it.
use super::scope_support::*;
use super::*;
use crate::{
    application::ports::department::OpenWork,
    entities::department::{DepartmentId, DepartmentRole},
};

async fn archive(f: &Fixture, unit: DepartmentId) {
    crate::adapters::persistence::test_support::department_use_cases(&f.store)
        .archive_department(
            f.owner.actor_id,
            f.owner.company.as_uuid(),
            unit,
            OpenWork::Refuse,
        )
        .await
        .unwrap();
}

fn archived<T: std::fmt::Debug>(result: AppResult<T>) -> bool {
    matches!(result, Err(AppError::UnitArchived))
}

async fn unit_of(f: &Fixture, document: Uuid) -> DepartmentId {
    DepartmentId::new(
        sqlx::query_scalar("SELECT unit_id FROM knowledge_documents WHERE id=$1")
            .bind(document)
            .fetch_one(&f.pool)
            .await
            .unwrap(),
    )
}

#[tokio::test]
async fn unit_archive_scopes_knowledge_takes_nothing_in_and_lets_documents_out() {
    let db = own_database().await.unwrap();
    let f = Fixture::new(db.pool.clone()).await;
    let parent = unit(&f, "Parent", None).await;
    let child = unit(&f, "Child", Some(parent)).await;
    let active = unit(&f, "Active", None).await;
    let kept = upload(&f, child, "0_kept.md", "kept").await;
    let leaving = upload(&f, child, "0_leaving.md", "leaving").await;
    let removed = upload(&f, child, "0_removed.md", "removed").await;
    let outside = upload(&f, active, "0_outside.md", "outside").await;
    archive(&f, child).await;

    // Uploads, their preflight and moves into the archived unit refuse; nothing changed.
    assert!(archived(
        f.store
            .replace(target(f.owner, child), &[prepared("0_new.md", "new")])
            .await
    ));
    assert!(archived(
        f.store.preflight_upload(target(f.owner, child), &[]).await
    ));
    assert!(archived(
        f.store
            .move_document(move_command(f.owner, &outside, child))
            .await
    ));
    assert_eq!(unit_of(&f, outside.id).await, active);
    assert_eq!(f.content(kept.id).await, "kept");
    // The database refuses what the application skips.
    let error = sqlx::query("UPDATE knowledge_documents SET unit_id=$1 WHERE id=$2")
        .bind(child.as_uuid())
        .bind(outside.id)
        .execute(&f.pool)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("unit_archived")
    );

    // Moving out and deleting stay allowed, so the unit can be emptied.
    f.store
        .move_document(move_command(f.owner, &leaving, active))
        .await
        .expect("a document moves out of an archived unit");
    assert_eq!(unit_of(&f, leaving.id).await, active);
    f.store
        .delete(DeleteKnowledgeLayer {
            scope: f.owner,
            document_id: removed.id,
            expected_revision: removed.revision,
        })
        .await
        .expect("a document of an archived unit can be deleted");

    // The picker leaves the archived unit out unless a document already sits in it.
    let page = f
        .store
        .unit_choices(
            f.owner,
            KnowledgeUnitQuery {
                selected: None,
                after: None,
            },
        )
        .await
        .unwrap();
    assert!(page.units.iter().all(|choice| choice.id != child));
    assert!(page.units.iter().any(|choice| choice.id == active));
    let page = f
        .store
        .unit_choices(
            f.owner,
            KnowledgeUnitQuery {
                selected: Some(child),
                after: None,
            },
        )
        .await
        .unwrap();
    assert!(page.units.iter().any(|choice| choice.id == child));
}

#[tokio::test]
async fn unit_archive_scopes_parent_lead_reads_the_archived_child_and_a_sibling_does_not() {
    let db = own_database().await.unwrap();
    let f = Fixture::new(db.pool.clone()).await;
    without_placement(&f).await;
    let parent = unit(&f, "Parent", None).await;
    let child = unit(&f, "Child", Some(parent)).await;
    let sibling = unit(&f, "Sibling", Some(parent)).await;
    let kept = upload(&f, child, "0_kept.md", "kept").await;
    archive(&f, child).await;

    // A lead of the archived child's sibling reads nothing of it.
    grant(&f, sibling, DepartmentRole::Lead).await;
    assert!(f.store.list(f.member, Some(child)).await.is_err());
    let sideways = f.store.list(f.member, None).await.unwrap();
    assert!(!units(&sideways.layers).contains(&child));

    // The parent's lead still reads it through the one-step rule, and cannot write there.
    grant(&f, parent, DepartmentRole::Lead).await;
    let read = f.store.list(f.member, Some(child)).await.unwrap();
    assert_eq!(
        read.layers.iter().map(|layer| layer.id).collect::<Vec<_>>(),
        vec![kept.id]
    );
    assert!(archived(
        f.store
            .replace(target(f.member, child), &[prepared("0_new.md", "new")])
            .await
    ));

    // Archiving retires no namespace: retirement stays tied to deletion.
    let removing: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM memory_namespaces WHERE company_id=$1 AND unit_id=$2 \
         AND state='removing'",
    )
    .bind(f.owner.company.as_uuid())
    .bind(child.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(removing, 0);
    let lifecycles: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM memory_lifecycles WHERE company_id=$1 AND owner_id=$2",
    )
    .bind(f.owner.company.as_uuid())
    .bind(child.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(lifecycles, 0);
}

/// Plan08 02C.4: membership and roles in an archived unit grant only desk history, so its lead
/// reads neither its knowledge nor its children's; the parent's lead still does.
#[tokio::test]
async fn unit_archive_scopes_roles_in_an_archived_unit_read_no_knowledge() {
    let db = own_database().await.unwrap();
    let f = Fixture::new(db.pool.clone()).await;
    without_placement(&f).await;
    let top = unit(&f, "Top", None).await;
    let parent = unit(&f, "Parent", Some(top)).await;
    let child = unit(&f, "Child", Some(parent)).await;
    upload(&f, parent, "0_own.md", "own").await;
    upload(&f, child, "0_below.md", "below").await;
    grant(&f, parent, DepartmentRole::Lead).await;
    assert!(f.store.list(f.member, Some(parent)).await.is_ok());
    assert!(f.store.list(f.member, Some(child)).await.is_ok());

    archive(&f, parent).await;
    assert!(f.store.list(f.member, Some(parent)).await.is_err());
    assert!(f.store.list(f.member, Some(child)).await.is_err());
    let all = f.store.list(f.member, None).await.unwrap();
    let read = units(&all.layers);
    assert!(
        !read.contains(&parent) && !read.contains(&child),
        "{read:?}"
    );

    // The parent's lead still reads the archived unit through the one-step rule.
    grant(&f, top, DepartmentRole::Lead).await;
    assert!(f.store.list(f.member, Some(parent)).await.is_ok());
}
