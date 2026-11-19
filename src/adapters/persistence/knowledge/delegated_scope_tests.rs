//! Plan06 01B.6: a unit manager administers the manual knowledge of its whole subtree.
use super::scope_support::*;
use super::*;
use crate::{
    adapters::persistence::test_support::{WriteBarrier, competing_writes, department_use_cases},
    entities::department::{DepartmentId, DepartmentRole},
};
use std::collections::BTreeSet;

struct Subtree {
    a: DepartmentId,
    c: DepartmentId,
    d: DepartmentId,
    sibling: DepartmentId,
}

/// The member leads A; D sits three levels below it and Sibling beside it.
async fn led_subtree(f: &Fixture) -> Subtree {
    without_placement(f).await;
    let a = unit(f, "A", None).await;
    let b = unit(f, "B", Some(a)).await;
    let c = unit(f, "C", Some(b)).await;
    let d = unit(f, "D", Some(c)).await;
    let sibling = unit(f, "Sibling", None).await;
    grant(f, a, DepartmentRole::Lead).await;
    Subtree { a, c, d, sibling }
}

async fn revoke_lead(f: &Fixture, unit: DepartmentId) -> AppResult<()> {
    department_use_cases(&f.store)
        .clear_role(
            f.owner.actor_id,
            f.owner.company.as_uuid(),
            unit,
            DepartmentRole::Lead,
            person(f).await,
        )
        .await
}

async fn names_in(f: &Fixture, unit: DepartmentId) -> BTreeSet<String> {
    sqlx::query_scalar("SELECT name FROM knowledge_documents WHERE company_id=$1 AND unit_id=$2")
        .bind(f.owner.company.as_uuid())
        .bind(unit.as_uuid())
        .fetch_all(&f.pool)
        .await
        .unwrap()
        .into_iter()
        .collect()
}

#[tokio::test]
async fn delegated_admin_knowledge_lead_manages_the_whole_subtree_within_its_level() {
    let db = own_database().await.unwrap();
    let f = Fixture::new(db.pool.clone()).await;
    let other = Fixture::new(db.pool.clone()).await;
    let s = led_subtree(&f).await;
    let deep = upload(&f, s.d, "0_deep.md", "owner upload").await;
    let above = upload(&f, s.d, "6_deep.md", "above the member level").await;
    let beside = upload(&f, s.sibling, "0_beside.md", "sibling").await;

    let page = f.store.list(f.member, Some(s.d)).await.unwrap();
    assert_eq!(page.default_unit, Some(s.d));
    let listed: BTreeSet<_> = page.layers.iter().map(|layer| layer.id).collect();
    assert!(listed.contains(&deep.id));
    assert!(!listed.contains(&above.id) && !listed.contains(&beside.id));
    assert!(
        manual(&f, f.member)
            .await
            .iter()
            .any(|chunk| chunk.unit_id == s.d)
    );

    f.store
        .replace(target(f.member, s.d), &[prepared("0_deep.md", "manager")])
        .await
        .unwrap();
    assert_eq!(f.content(deep.id).await, "manager");
    let moved = upload(&f, s.c, "0_moved.md", "moves within").await;
    f.store
        .move_document(move_command(f.member, &moved, s.d))
        .await
        .unwrap();
    let moved = f
        .store
        .list(f.member, Some(s.d))
        .await
        .unwrap()
        .layers
        .into_iter()
        .find(|layer| layer.id == moved.id)
        .expect("the moved layer is listed in its managed destination");
    f.store
        .delete(DeleteKnowledgeLayer {
            scope: f.member,
            document_id: moved.id,
            expected_revision: moved.revision,
        })
        .await
        .unwrap();
    assert!(matches!(
        f.store
            .replace(target(f.member, s.d), &[prepared("6_deep.md", "above")])
            .await,
        Err(AppError::Forbidden(_))
    ));
    assert_eq!(f.content(above.id).await, "above the member level");

    // Neither sideways nor upward, nor into another company.
    let mine = upload(&f, s.a, "0_mine.md", "stays in the subtree").await;
    for refused in [s.sibling, f.root, other.root] {
        assert!(matches!(
            f.store
                .replace(target(f.member, refused), &[prepared("0_no.md", "no")])
                .await,
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            f.store
                .move_document(move_command(f.member, &mine, refused))
                .await,
            Err(AppError::NotFound(_))
        ));
    }
    assert!(matches!(
        f.store.list(f.member, Some(s.sibling)).await,
        Err(AppError::NotFound(_))
    ));
    assert!(matches!(
        f.store
            .delete(DeleteKnowledgeLayer {
                scope: f.member,
                document_id: beside.id,
                expected_revision: beside.revision,
            })
            .await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(
        names_in(&f, s.sibling).await,
        BTreeSet::from(["beside.md".into()])
    );

    // Management ends with the role; the deep unit is not a placement of its own.
    revoke_lead(&f, s.a).await.unwrap();
    assert!(matches!(
        f.store
            .replace(target(f.member, s.d), &[prepared("0_late.md", "late")])
            .await,
        Err(AppError::NotFound(_))
    ));
    assert!(matches!(
        f.store.list(f.member, Some(s.d)).await,
        Err(AppError::NotFound(_))
    ));
}

#[tokio::test]
async fn delegated_admin_competing_knowledge_upload_precedes_role_removal() {
    let db = own_database().await.unwrap();
    let f = Fixture::new(db.pool.clone()).await;
    let s = led_subtree(&f).await;
    let documents = [prepared("0_first.md", "under the old role")];
    let result = competing_writes(
        &f.pool,
        WriteBarrier::KnowledgeDocuments,
        f.store.replace(target(f.member, s.d), &documents),
        revoke_lead(&f, s.a),
    )
    .await;
    result.first.unwrap();
    result.second.unwrap();
    assert_eq!(names_in(&f, s.d).await, BTreeSet::from(["first.md".into()]));
    assert!(matches!(
        f.store
            .replace(target(f.member, s.d), &[prepared("0_after.md", "after")])
            .await,
        Err(AppError::NotFound(_))
    ));
}

#[tokio::test]
async fn delegated_admin_competing_role_removal_precedes_knowledge_upload() {
    let db = own_database().await.unwrap();
    let f = Fixture::new(db.pool.clone()).await;
    let s = led_subtree(&f).await;
    let documents = [prepared("0_late.md", "after the role is gone")];
    f.store
        .preflight_upload(target(f.member, s.d), &[documents[0].file.size()])
        .await
        .unwrap();
    let result = competing_writes(
        &f.pool,
        WriteBarrier::DeskGrants,
        revoke_lead(&f, s.a),
        f.store.replace(target(f.member, s.d), &documents),
    )
    .await;
    result.first.unwrap();
    assert!(matches!(result.second, Err(AppError::NotFound(_))));
    assert!(names_in(&f, s.d).await.is_empty());
}
