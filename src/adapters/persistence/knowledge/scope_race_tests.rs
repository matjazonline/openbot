use super::scope_support::*;
use super::*;
use crate::adapters::persistence::test_support::{
    wait_until_a_backend_is_blocked, wait_until_backends_are_blocked,
};

#[derive(Clone, Copy)]
enum Order {
    MoveFirst,
    ReplaceFirst,
}

enum Write {
    Move(MoveKnowledgeDocument),
    Replace(KnowledgeUploadTarget),
}
struct OrderedWrites {
    first: Write,
    second: Write,
}

async fn perform(store: Arc<PostgresPersistence>, write: Write) -> AppResult<()> {
    match write {
        Write::Move(command) => store.move_document(command).await,
        Write::Replace(target) => store
            .replace(target, &[prepared("0_notes.md", "replacement")])
            .await
            .map(|_| ()),
    }
}

#[tokio::test]
async fn knowledge_scope_move_competes_with_replace_in_both_commit_orders() {
    for order in [Order::MoveFirst, Order::ReplaceFirst] {
        ordered_race(order).await;
    }
}

async fn ordered_race(order: Order) {
    let db = own_database().await.unwrap();
    let f = Fixture::new(db.pool.clone()).await;
    let g = group(&f).await;
    sqlx::raw_sql(
        "CREATE FUNCTION pause_scope_write() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN PERFORM pg_advisory_xact_lock(771034); RETURN NEW; END $$;
        CREATE TRIGGER pause_scope_write BEFORE INSERT OR UPDATE ON knowledge_documents
        FOR EACH ROW EXECUTE FUNCTION pause_scope_write()",
    )
    .execute(&f.pool)
    .await
    .unwrap();
    let mut gate = f.pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(771034)")
        .execute(&mut *gate)
        .await
        .unwrap();
    // Replacing the public layer must leave this private anchor revision valid in either order.
    let moving = Write::Move(move_command(f.owner, &g.private, g.target));
    let replacing = Write::Replace(target(f.owner, g.source));
    let writes = match order {
        Order::MoveFirst => OrderedWrites {
            first: moving,
            second: replacing,
        },
        Order::ReplaceFirst => OrderedWrites {
            first: replacing,
            second: moving,
        },
    };
    let first = tokio::spawn(perform(f.store.clone(), writes.first));
    wait_until_a_backend_is_blocked(&f.pool).await;
    let second = tokio::spawn(perform(f.store.clone(), writes.second));
    wait_until_backends_are_blocked(&f.pool, 2).await;
    gate.commit().await.unwrap();
    first.await.unwrap().unwrap();
    second.await.unwrap().unwrap();
    assert_order(&f, &g, order).await;
}

#[tokio::test]
async fn knowledge_scope_move_and_upload_recheck_source_target_hidden_layers_and_revoked_grants() {
    use crate::entities::department::DepartmentRole;
    let db = own_database().await.unwrap();
    let f = Fixture::new(db.pool.clone()).await;
    let g = group(&f).await;
    without_placement(&f).await;
    grant(&f, g.source, DepartmentRole::Reviewer).await;
    assert!(matches!(
        f.store
            .move_document(move_command(f.member, &g.public, g.target))
            .await,
        Err(AppError::NotFound(_))
    ));
    grant(&f, g.target, DepartmentRole::Reviewer).await;
    upload(&f, g.source, "7_notes.md", "hidden layer").await;
    assert!(matches!(
        f.store
            .move_document(move_command(f.member, &g.public, g.target))
            .await,
        Err(AppError::Forbidden(_))
    ));
    assert_eq!(
        f.store
            .list(f.owner, Some(g.source))
            .await
            .unwrap()
            .layers
            .len(),
        3
    );
    assert!(
        f.store
            .list(f.owner, Some(g.target))
            .await
            .unwrap()
            .layers
            .is_empty()
    );
    assert_eq!(f.content(g.public.id).await, "public");
    let ancestor = upload(&f, f.root, "0_ancestor.md", "read only ancestor").await;
    assert!(
        f.store
            .list(f.member, None)
            .await
            .unwrap()
            .layers
            .iter()
            .any(|layer| layer.id == ancestor.id)
    );
    assert!(matches!(
        f.store
            .move_document(move_command(f.member, &ancestor, g.source))
            .await,
        Err(AppError::NotFound(_))
    ));
    let target = target(f.member, g.target);
    let documents = [prepared("0_new.md", "revoked after preflight")];
    f.store
        .preflight_upload(target, &[documents[0].file.size()])
        .await
        .unwrap();
    crate::adapters::persistence::test_support::department_use_cases(&f.store)
        .clear_role(
            f.owner.actor_id,
            f.owner.company.as_uuid(),
            g.target,
            crate::application::ports::department::DepartmentRoleRemoval {
                role: DepartmentRole::Reviewer,
                principal: person(&f).await,
                successor: None,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        f.store.replace(target, &documents).await,
        Err(AppError::NotFound(_))
    ));
    assert!(
        f.store
            .list(f.owner, Some(g.target))
            .await
            .unwrap()
            .layers
            .is_empty()
    );
}

async fn assert_order(f: &Fixture, group: &Group, order: Order) {
    let moved = f
        .store
        .list(f.owner, Some(group.target))
        .await
        .unwrap()
        .layers;
    assert_eq!(moved.len(), 2);
    assert!(moved.iter().any(|layer| layer.id == group.public.id));
    assert!(moved.iter().any(|layer| layer.id == group.private.id));
    assert_eq!(f.content(group.private.id).await, "private");
    let source = f
        .store
        .list(f.owner, Some(group.source))
        .await
        .unwrap()
        .layers;
    match order {
        Order::ReplaceFirst => {
            assert!(source.is_empty());
            assert_eq!(f.content(group.public.id).await, "replacement");
            assert_eq!(
                moved
                    .iter()
                    .find(|layer| layer.id == group.public.id)
                    .unwrap()
                    .revision,
                3
            );
        }
        Order::MoveFirst => {
            assert_eq!(source.len(), 1);
            assert_ne!(source[0].id, group.public.id);
            assert_eq!(source[0].revision, 1);
            assert_eq!(f.content(source[0].id).await, "replacement");
            assert_eq!(f.content(group.public.id).await, "public");
            assert!(moved.iter().all(|layer| layer.revision == 2));
        }
    }
    let split: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_chunks AS chunk JOIN knowledge_documents AS document ON document.id=chunk.document_id WHERE document.company_id=$1 AND chunk.unit_id<>document.unit_id)")
        .bind(f.owner.company.as_uuid()).fetch_one(&f.pool).await.unwrap();
    assert!(!split);
}
