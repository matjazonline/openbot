use super::scope_support::*;
use super::*;
use crate::{
    application::knowledge::search::KnowledgeChunks,
    entities::{
        access_level::AccessLevel,
        department::{DepartmentId, DepartmentRole},
    },
};
use std::collections::BTreeSet;

#[tokio::test]
async fn knowledge_scope_schema_owner_chunk_tuple_and_per_unit_layer_identity() {
    let db = own_database().await.unwrap();
    let f = Fixture::new(db.pool.clone()).await;
    let other = Fixture::new(db.pool.clone()).await;
    let a = unit(&f, "A", None).await;
    let b = unit(&f, "B", None).await;
    let first = upload(&f, a, "0_notes.md", "first").await;
    let second = upload(&f, b, "0_notes.md", "second").await;
    assert_ne!(first.id, second.id);
    let replaced = upload(&f, a, "0-NOTES.md", "replacement").await;
    assert_eq!(replaced.id, first.id);
    assert_eq!(replaced.revision, 2);
    assert_eq!(replaced.unit_id, a);
    assert_eq!(f.content(second.id).await, "second");
    let error = sqlx::query("UPDATE knowledge_chunks SET unit_id=$1 WHERE document_id=$2")
        .bind(b.as_uuid())
        .bind(first.id)
        .execute(&f.pool)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23503")
    );
    let error = sqlx::query("UPDATE knowledge_documents SET unit_id=$1 WHERE id=$2")
        .bind(other.root.as_uuid())
        .bind(first.id)
        .execute(&f.pool)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("knowledge_documents_unit_fk")
    );
    let mut tx = f.pool.begin().await.unwrap();
    sqlx::query("DELETE FROM channels WHERE company_id=$1 AND department_id=$2")
        .bind(f.owner.company.as_uuid())
        .bind(a.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = sqlx::query("DELETE FROM departments WHERE company_id=$1 AND id=$2")
        .bind(f.owner.company.as_uuid())
        .bind(a.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("knowledge_documents_unit_fk")
    );
    tx.rollback().await.unwrap();
    #[derive(sqlx::FromRow)]
    struct Owners {
        document_unit: Uuid,
        chunk_unit: Uuid,
    }
    let row: Owners = sqlx::query_as("SELECT document.unit_id AS document_unit,chunk.unit_id AS chunk_unit FROM knowledge_documents AS document JOIN knowledge_chunks AS chunk ON chunk.document_id=document.id WHERE document.id=$1")
        .bind(first.id).fetch_one(&f.pool).await.unwrap();
    assert_eq!(row.document_unit, a.as_uuid());
    assert_eq!(row.chunk_unit, a.as_uuid());
}

#[tokio::test]
async fn knowledge_scope_search_manual_roles_ancestors_managed_subtree_and_level() {
    let db = own_database().await.unwrap();
    let f = Fixture::new(db.pool.clone()).await;
    without_placement(&f).await;
    let a = unit(&f, "A", None).await;
    let b = unit(&f, "B", Some(a)).await;
    let c = unit(&f, "C", Some(b)).await;
    let sibling = unit(&f, "Sibling", None).await;
    for (unit, name) in [
        (f.root, "root"),
        (a, "a"),
        (b, "b"),
        (c, "c"),
        (sibling, "sibling"),
    ] {
        upload(&f, unit, &format!("0_{name}.md"), "same question").await;
    }
    upload(&f, a, "6_hidden.md", "private").await;
    assert_eq!(
        units(&f.store.list(f.member, None).await.unwrap().layers),
        BTreeSet::from([f.root])
    );
    grant(&f, a, DepartmentRole::Reviewer).await;
    assert_eq!(
        units(&f.store.list(f.member, None).await.unwrap().layers),
        BTreeSet::from([f.root, a])
    );
    grant(&f, a, DepartmentRole::Lead).await;
    // Leading A manages A's whole subtree, past the one-step child B; never the sibling.
    let visible = f.store.list(f.member, None).await.unwrap();
    assert_eq!(units(&visible.layers), BTreeSet::from([f.root, a, b, c]));
    assert!(
        !visible
            .layers
            .iter()
            .any(|layer| layer.name.as_str() == "hidden.md")
    );
    assert_eq!(
        manual(&f, f.member)
            .await
            .iter()
            .map(|chunk| chunk.unit_id)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([f.root, a, b, c])
    );
    assert!(
        manual(&f, f.owner)
            .await
            .iter()
            .any(|chunk| chunk.unit_id == c)
    );
    let query = super::search::request(
        target(f.owner, b),
        AccessLevel::PUBLIC,
        serde_json::json!({"query":"same"}),
    );
    let chunks = f.store.visible(query).await.unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].unit_id, b);
    let mut absent = super::search::request(
        target(f.owner, f.root),
        AccessLevel::PUBLIC,
        serde_json::json!({"query":"same"}),
    );
    absent.units.clear();
    assert!(
        matches!(f.store.visible(absent).await, Err(AppError::BadRequest(code)) if code=="knowledge.scope_unavailable")
    );
}

#[tokio::test]
async fn knowledge_scope_upload_requires_current_placement_and_parent_lead_without_sideways_grants()
{
    let db = own_database().await.unwrap();
    let f = Fixture::new(db.pool.clone()).await;
    let other = Fixture::new(db.pool.clone()).await;
    without_placement(&f).await;
    let empty = f.store.list(f.member, None).await.unwrap();
    assert!(empty.upload_units.is_empty());
    assert_eq!(empty.default_unit, None);
    assert_eq!(
        f.store.list(f.owner, None).await.unwrap().default_unit,
        Some(f.root)
    );
    assert!(
        f.store
            .replace(
                target(f.member, f.root),
                &[prepared("0_denied.md", "no placement")]
            )
            .await
            .is_err()
    );
    let a = unit(&f, "A", None).await;
    let b = unit(&f, "B", Some(a)).await;
    let c = unit(&f, "C", Some(b)).await;
    crate::adapters::persistence::test_support::department_use_cases(&f.store)
        .add_member(
            f.owner.actor_id,
            f.owner.company.as_uuid(),
            a,
            person(&f).await,
        )
        .await
        .unwrap();
    assert_eq!(
        f.store.list(f.member, None).await.unwrap().default_unit,
        Some(a)
    );
    f.store
        .replace(target(f.member, a), &[prepared("0_a.md", "member")])
        .await
        .unwrap();
    assert!(
        f.store
            .replace(target(f.member, b), &[prepared("0_b.md", "not yet")])
            .await
            .is_err()
    );
    grant(&f, a, DepartmentRole::Lead).await;
    f.store
        .replace(target(f.member, b), &[prepared("5_b.md", "report")])
        .await
        .unwrap();
    // Leading A manages its whole subtree, including the grandchild C.
    f.store
        .replace(
            target(f.member, c),
            &[prepared("0_c.md", "managed subtree")],
        )
        .await
        .unwrap();
    for denied in [f.root, other.root] {
        assert!(matches!(
            f.store
                .replace(target(f.member, denied), &[prepared("0_no.md", "no")])
                .await,
            Err(AppError::NotFound(_))
        ));
    }
    assert!(matches!(
        f.store
            .replace(target(f.member, a), &[prepared("6_no.md", "high")])
            .await,
        Err(AppError::Forbidden(_))
    ));
    grant(&f, c, DepartmentRole::Reviewer).await;
    assert_eq!(
        f.store.list(f.member, None).await.unwrap().default_unit,
        None
    );
    assert_eq!(
        f.store.list(f.member, Some(c)).await.unwrap().default_unit,
        Some(c)
    );
    assert!(matches!(
        f.store.list(f.member, Some(other.root)).await,
        Err(AppError::NotFound(_))
    ));
    let outsider = KnowledgeScope {
        company: f.owner.company,
        actor_id: other.owner.actor_id,
    };
    assert!(matches!(
        f.store
            .replace(
                target(outsider, a),
                &[prepared("0_no.md", "foreign person")]
            )
            .await,
        Err(AppError::NotFound(_))
    ));
    assert_admin(&f, c).await;
}

async fn assert_admin(f: &Fixture, unit: DepartmentId) {
    let admin = account(&f.store).await;
    crate::adapters::persistence::test_support::insert_root_member(
        &f.pool,
        f.owner.company,
        admin.id,
        crate::entities::company_member::CompanyAccessRole::Admin,
        AccessLevel::L5,
    )
    .await;
    let scope = KnowledgeScope {
        company: f.owner.company,
        actor_id: admin.id,
    };
    assert_eq!(
        f.store.list(scope, None).await.unwrap().default_unit,
        Some(f.root)
    );
    f.store
        .replace(
            target(scope, unit),
            &[prepared("5_admin.md", "admin target")],
        )
        .await
        .unwrap();
    assert!(matches!(
        f.store
            .replace(
                target(scope, unit),
                &[prepared("6_admin.md", "above admin level")]
            )
            .await,
        Err(AppError::Forbidden(_))
    ));
}

#[tokio::test]
async fn knowledge_scope_move_all_layers_keeps_ids_chunks_and_refuses_stale_or_colliding_targets() {
    let db = own_database().await.unwrap();
    let f = Fixture::new(db.pool.clone()).await;
    let g = group(&f).await;
    f.store
        .move_document(move_command(f.owner, &g.public, g.target))
        .await
        .unwrap();
    let layers = f.store.list(f.owner, Some(g.target)).await.unwrap().layers;
    assert_eq!(
        layers.iter().map(|layer| layer.id).collect::<BTreeSet<_>>(),
        BTreeSet::from([g.public.id, g.private.id])
    );
    assert!(layers.iter().all(|layer| layer.revision == 2));
    assert!(
        f.store
            .list(f.owner, Some(g.source))
            .await
            .unwrap()
            .layers
            .is_empty()
    );
    let chunk_units: Vec<Uuid> =
        sqlx::query_scalar("SELECT unit_id FROM knowledge_chunks WHERE company_id=$1 ORDER BY id")
            .bind(f.owner.company.as_uuid())
            .fetch_all(&f.pool)
            .await
            .unwrap();
    assert_eq!(chunk_units, vec![g.target.as_uuid(); 2]);
    assert!(matches!(
        f.store
            .move_document(move_command(f.owner, &g.public, g.source))
            .await,
        Err(AppError::Conflict(_))
    ));
    upload(&f, g.source, "3_notes.md", "collision").await;
    assert!(matches!(
        f.store
            .move_document(move_command(f.owner, &layers[0], g.source))
            .await,
        Err(AppError::Conflict(_))
    ));
    assert_eq!(
        f.store
            .list(f.owner, Some(g.target))
            .await
            .unwrap()
            .layers
            .len(),
        2
    );
}

#[tokio::test]
async fn knowledge_scope_move_competing_claimants_cannot_split_layers_or_use_the_same_revision_twice()
 {
    let db = own_database().await.unwrap();
    let f = Fixture::new(db.pool.clone()).await;
    let g = group(&f).await;
    let third = unit(&f, "Third", None).await;
    let (a, b) = tokio::join!(
        f.store
            .move_document(move_command(f.owner, &g.public, g.target)),
        f.store
            .move_document(move_command(f.owner, &g.public, third))
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert!(a.is_ok() || matches!(a, Err(AppError::Conflict(_))));
    assert!(b.is_ok() || matches!(b, Err(AppError::Conflict(_))));
    let rows = f.store.list(f.owner, None).await.unwrap().layers;
    assert_eq!(rows.len(), 2);
    assert_eq!(units(&rows).len(), 1);
    assert!(rows.iter().all(|layer| layer.revision == 2));
}
