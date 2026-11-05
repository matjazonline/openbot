use super::*;
use crate::adapters::persistence::test_support::wait_until_a_backend_is_blocked;
#[tokio::test]
async fn knowledge_upload_competing_replacements_keep_only_the_last_committers_matching_chunks() {
    let Some(db) = own_database().await else {
        return;
    };
    let f = Fixture::new(db.pool.clone()).await;
    let original = f.replace(f.owner, "3_notes.md", "original").await;
    let first = [prepared("3_notes.md", "first")];
    let second = [prepared("3-NOTES.md", "second")];
    let (a, b) = tokio::join!(
        f.store.replace(f.owner, &first),
        f.store.replace(f.owner, &second)
    );
    a.unwrap();
    b.unwrap();
    let rows = f.store.list(f.owner).await.unwrap().layers;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].revision, 3);
    assert_eq!(rows[0].id, original.id);
    #[derive(sqlx::FromRow)]
    struct Content {
        content: String,
        body: String,
    }
    let row=sqlx::query_as::<_,Content>("SELECT document.content,chunk.body FROM knowledge_documents AS document JOIN knowledge_chunks AS chunk ON chunk.document_id=document.id WHERE document.id=$1")
        .bind(original.id).fetch_one(&f.pool).await.unwrap();
    assert_eq!(row.content, row.body);
    assert!(["first", "second"].contains(&row.content.as_str()));
}
#[tokio::test]
async fn knowledge_upload_demotion_after_preflight_or_before_a_waiting_write_refuses_every_layer() {
    let Some(db) = own_database().await else {
        return;
    };
    let f = Fixture::new(db.pool.clone()).await;
    let documents = vec![prepared("5_notes.md", "secret")];
    let sizes = documents
        .iter()
        .map(|document| document.file.size())
        .collect::<Vec<_>>();
    f.store.preflight_upload(f.member, &sizes).await.unwrap();
    let mut demotion = f.pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(f.owner.company.as_uuid())
        .execute(&mut *demotion)
        .await
        .unwrap();
    sqlx::query("UPDATE company_members SET access_level=0 WHERE company_id=$1 AND user_id=$2")
        .bind(f.member.company.as_uuid())
        .bind(f.member.actor_id)
        .execute(&mut *demotion)
        .await
        .unwrap();
    let store = f.store.clone();
    let scope = f.member;
    let writer = tokio::spawn(async move { store.replace(scope, &documents).await });
    wait_until_a_backend_is_blocked(&f.pool).await;
    demotion.commit().await.unwrap();
    assert!(matches!(writer.await.unwrap(), Err(AppError::Forbidden(_))));
    assert!(f.store.list(f.owner).await.unwrap().layers.is_empty());
}
#[tokio::test]
async fn knowledge_upload_competing_quota_writers_and_duplicate_projection_cannot_exceed_the_company_limit()
 {
    let Some(db) = own_database().await else {
        return;
    };
    let f = Fixture::new(db.pool.clone()).await;
    let text = "x".repeat(MAX_KNOWLEDGE_FILE_BYTES);
    let seed: Vec<_> = (0..9)
        .map(|index| prepared(&format!("0_seed{index}.md"), &text))
        .collect();
    f.store.replace(f.owner, &seed).await.unwrap();
    let first = [prepared("0_a.md", &text)];
    let second = [prepared("0_b.md", &text)];
    let (a, b) = tokio::join!(
        f.store.replace(f.owner, &first),
        f.store.replace(f.owner, &second)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let usage: i64 = sqlx::query_scalar(
        "SELECT sum(octet_length(content))::bigint FROM knowledge_documents WHERE company_id=$1",
    )
    .bind(f.owner.company.as_uuid())
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(usage, MAX_COMPANY_KNOWLEDGE_BYTES as i64);
    // The intermediate new layer exceeds the byte quota; the final occurrence is empty.
    let temporary = [
        prepared("0_temporary.md", &text),
        prepared("0-TEMPORARY.md", ""),
    ];
    let final_rows = f.store.replace(f.owner, &temporary).await.unwrap();
    assert_eq!(final_rows.len(), 1);
    assert_eq!(final_rows[0].revision, 2);
    assert_eq!(final_rows[0].text_bytes, 0);
    let duplicates = [prepared("0_seed0.md", &text), prepared("0-SEED0.md", "")];
    let rows = f.store.replace(f.owner, &duplicates).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].text_bytes, 0);
    let too_large = vec![
        prepared("0_extra.md", &text),
        prepared("0_extra2.md", &text),
    ];
    assert!(matches!(
        f.store.replace(f.owner, &too_large).await,
        Err(AppError::BadRequest(_))
    ));
}

#[tokio::test]
async fn knowledge_upload_waiting_stale_delete_cannot_remove_a_competing_replacement() {
    use crate::adapters::persistence::test_support::wait_until_backends_are_blocked;
    let Some(db) = own_database().await else {
        return;
    };
    let f = Fixture::new(db.pool.clone()).await;
    let layer = f.replace(f.owner, "0_notes.md", "old").await;
    sqlx::query("CREATE FUNCTION pause_knowledge_replacement() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_xact_lock(771019); RETURN NEW; END $$").execute(&f.pool).await.unwrap();
    sqlx::query("CREATE TRIGGER pause_knowledge_replacement BEFORE UPDATE ON knowledge_documents FOR EACH ROW EXECUTE FUNCTION pause_knowledge_replacement()").execute(&f.pool).await.unwrap();
    let mut gate = f.pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(771019)")
        .execute(&mut *gate)
        .await
        .unwrap();
    let writer_store = f.store.clone();
    let scope = f.owner;
    let writer = tokio::spawn(async move {
        writer_store
            .replace(scope, &[prepared("0_notes.md", "new")])
            .await
    });
    wait_until_a_backend_is_blocked(&f.pool).await;
    let delete_store = f.store.clone();
    let delete = tokio::spawn(async move {
        delete_store
            .delete(DeleteKnowledgeLayer {
                scope,
                document_id: layer.id,
                expected_revision: 1,
            })
            .await
    });
    wait_until_backends_are_blocked(&f.pool, 2).await;
    gate.commit().await.unwrap();
    let updated = writer.await.unwrap().unwrap().remove(0);
    assert_eq!(updated.revision, 2);
    assert!(matches!(delete.await.unwrap(), Err(AppError::Conflict(_))));
    assert_eq!(f.content(updated.id).await, "new");
}

#[tokio::test]
async fn knowledge_upload_layer_cap_and_hidden_company_quota_return_generic_errors_without_hints() {
    let Some(db) = own_database().await else {
        return;
    };
    let f = Fixture::new(db.pool.clone()).await;
    sqlx::query("INSERT INTO knowledge_documents (id,company_id,name,access_level,content,content_sha256,uploaded_by) SELECT gen_random_uuid(),$1,'hidden-'||sequence::text||'.md',9,'',sha256(''::bytea),$2 FROM generate_series(1,4096) AS seed(sequence)")
        .bind(f.owner.company.as_uuid()).bind(f.owner.actor_id).execute(&f.pool).await.unwrap();
    assert!(f.store.list(f.member).await.unwrap().layers.is_empty());
    let error = f
        .store
        .preflight_upload(f.member, &[prepared("0_visible.md", "").file.size()])
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::BadRequest(_)));
    let message = error.to_string();
    assert!(!message.contains("4096"));
    assert!(!message.contains("hidden-"));
    let replaced = f.replace(f.owner, "9_hidden-1.md", "").await;
    assert_eq!(replaced.revision, 2);
}
