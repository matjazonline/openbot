//! Two application replicas reach the actual company writer lock before replacing one layer.
use super::*;
use crate::application::knowledge::{KnowledgeLibrary, test_support::DeterministicTextEmbedder};

#[tokio::test]
async fn knowledge_e2e_competing_application_replicas_replace_one_name_and_level_without_duplicates()
 {
    let j = Box::pin(SalesJourney::new(Vec::new(), Audience::Team)).await;
    // A second process owns its own library/slot. Both use native deterministic fixtures;
    // production still has exactly one loaded model per Foundation/process.
    let replica = Arc::new(KnowledgeLibrary::new(
        Arc::new(j.d.p().clone()),
        Arc::new(DeterministicTextEmbedder::default()),
    ));
    let mut gate = j.d.p().pool().begin().await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(j.d.fixture.target.company.as_uuid())
        .execute(&mut *gate)
        .await
        .unwrap();
    let actor = scope(&j.d, j.d.fixture.target.actor.user_id());
    let mut writes = Vec::new();
    for (library, body) in [
        (j.d.knowledge.clone(), "first writer"),
        (replica.clone(), "second writer"),
    ] {
        writes.push(tokio::spawn(async move {
            library
                .upload(actor, vec![file("3_race.md", body)])
                .await
                .map(|_| ())
        }));
    }
    let (mut observer, _) = crate::adapters::persistence::workflow::tests::admission_tests::lease_tests::resolution_gate::connect(&j).await;
    crate::adapters::persistence::workflow::tests::admission_tests::lease_tests::resolution_gate::waiters(
        &mut observer, blocker, 2, "competing knowledge replicas",
        &[&||writes.iter().all(|write| !write.is_finished())],
    ).await;
    gate.rollback().await.unwrap();
    for write in writes {
        write.await.unwrap().unwrap();
    }
    let documents:Vec<(i64,String)>=sqlx::query_as("SELECT revision,content FROM knowledge_documents WHERE company_id=$1 AND name='race.md' AND access_level=3")
        .bind(j.d.fixture.target.company.as_uuid()).fetch_all(j.d.p().pool()).await.unwrap();
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].0, 2);
    assert!(["first writer", "second writer"].contains(&documents[0].1.as_str()));
    replica.shutdown().await.unwrap();
    j.d.knowledge.shutdown().await.unwrap();
    assert_eq!(j.d.llm.finish().await.unwrap(), 0);
}
