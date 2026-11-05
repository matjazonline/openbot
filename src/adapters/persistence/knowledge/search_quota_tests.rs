use super::*;
use crate::{application::knowledge::search::KnowledgeChunks, entities::access_level::AccessLevel};

fn document(name: &str, count: usize) -> PreparedKnowledgeDocument {
    let source = "# H\npiece\n\n".repeat(count);
    let mut result = prepared(name, &source);
    result.chunks = (0..count)
        .map(|_| {
            let mut vector = [0.0; 384];
            vector[0] = 1.0;
            EmbeddedKnowledgeChunk {
                chunk: KnowledgeChunk {
                    heading: Some("H".into()),
                    body: "piece".into(),
                },
                embedding: vector,
            }
        })
        .collect();
    result
}

#[tokio::test]
async fn knowledge_search_company_chunk_cap_competing_writes_override_delete_and_corruption_guard()
{
    let database = own_database().await.unwrap();
    let f = Fixture::new(database.pool.clone()).await;
    f.store
        .replace(
            f.owner,
            &[document("0_first.md", 4096), document("0_second.md", 4094)],
        )
        .await
        .unwrap();
    let mut writers = tokio::task::JoinSet::new();
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    for name in ["0_left.md", "0_right.md"] {
        let store = f.store.clone();
        let scope = f.owner;
        let barrier = barrier.clone();
        let documents = [document(name, 2)];
        writers.spawn(async move {
            barrier.wait().await;
            store.replace(scope, &documents).await
        });
    }
    let mut winners = Vec::new();
    let mut refused = 0;
    while let Some(result) = writers.join_next().await {
        match result.unwrap() {
            Ok(mut layers) => winners.push(layers.remove(0)),
            Err(AppError::BadRequest(_)) => refused += 1,
            Err(error) => panic!("unexpected replacement failure: {error}"),
        }
    }
    assert_eq!(winners.len(), 1);
    assert_eq!(refused, 1);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM knowledge_chunks WHERE company_id=$1")
            .bind(f.owner.company.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(count, 8192);
    let winner = winners.remove(0);
    f.store
        .delete(DeleteKnowledgeLayer {
            scope: f.owner,
            document_id: winner.id,
            expected_revision: winner.revision,
        })
        .await
        .unwrap();
    let after = f
        .store
        .replace(f.owner, &[document("0_after.md", 2)])
        .await
        .unwrap()
        .remove(0);

    assert_corrupted_scan_is_refused(&f, after.id).await;

    // Replacing an existing layer releases its old chunks before the final total is checked.
    f.store
        .replace(f.owner, &[document("0_first.md", 1)])
        .await
        .unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM knowledge_chunks WHERE company_id=$1")
            .bind(f.owner.company.as_uuid())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(count, 4097);
}

async fn assert_corrupted_scan_is_refused(f: &Fixture, document_id: Uuid) {
    // A direct SQL corruption cannot turn the scan bound into a partial successful answer.
    let extra = Uuid::new_v4();
    sqlx::query("INSERT INTO knowledge_chunks (id,company_id,document_id,access_level,ordinal,heading,body,embedding_model,embedding)
        SELECT $1,company_id,document_id,access_level,2,heading,body,embedding_model,embedding
        FROM knowledge_chunks WHERE document_id=$2 LIMIT 1")
        .bind(extra).bind(document_id).execute(&f.pool).await.unwrap();
    let query = super::search::request(
        f.owner.company,
        AccessLevel::PUBLIC,
        serde_json::json!({"query":"price"}),
    );
    assert!(matches!(
        f.store.visible(query).await,
        Err(AppError::ServiceUnavailable(_))
    ));
    sqlx::query("DELETE FROM knowledge_chunks WHERE id=$1")
        .bind(extra)
        .execute(&f.pool)
        .await
        .unwrap();
}
