use super::*;
use crate::{
    application::{
        knowledge::{KnowledgeScope, KnowledgeUploadFile, search::KnowledgeSearchLimit},
        workflow::agent::AgentToolObservation,
        workflow::knowledge::{
            KNOWLEDGE_TOOL_DESCRIPTION, KnowledgeSearchPersistence, KnowledgeToolDispatch,
            SEARCH_KNOWLEDGE,
        },
    },
    entities::access_level::AccessLevel,
};

#[path = "knowledge_search_support.rs"]
mod support;
use support::*;

#[path = "knowledge_search_atomic_tests.rs"]
mod atomic;

#[path = "knowledge_search_lease_tests.rs"]
mod ownership;

#[tokio::test]
async fn knowledge_search_driver_persists_observation_and_classification_before_continuing() {
    let f = Box::pin(SearchFixture::new(AccessLevel::L5)).await;
    f.upload_sales().await;
    let mut llm = scripted_llm(vec![
        LlmTurn::tool_call(
            SEARCH_KNOWLEDGE,
            json!({"query":"What is the AX-4410 discount floor?"}),
        ),
        LlmTurn::text("\"From sales.md: the discount floor is 60.\""),
    ])
    .await;
    register_scripted_agent_base_url(f.agent, &llm.base_url);
    let runtime = f.runtime();
    let result = Box::pin(runtime.execute(&WorkflowStepKind("agent.run".into()), &f.claim)).await;
    assert_eq!(
        result.completed_result().unwrap(),
        json!("From sales.md: the discount floor is 60.")
    );
    let requests = llm.observed();
    let tool = &requests[0]["tools"][0]["function"];
    assert_eq!(tool["name"], SEARCH_KNOWLEDGE);
    assert_eq!(tool["description"], KNOWLEDGE_TOOL_DESCRIPTION);
    assert_eq!(tool["parameters"]["additionalProperties"], false);
    assert!(
        !tool["parameters"]["properties"]
            .as_object()
            .unwrap()
            .contains_key("level")
    );
    assert!(
        requests[1]["messages"]
            .to_string()
            .contains("PRIVATE FLOOR 60")
    );
    let snapshot = f.snapshot().await;
    assert_eq!(snapshot["content"], 5);
    assert_eq!(snapshot["proofs"], 1);
    assert_eq!(f.model.query_count(), 1);
    assert_eq!(llm.finish().await.unwrap(), 2);
    f.library.shutdown().await.unwrap();
}

#[tokio::test]
async fn knowledge_search_low_ceiling_never_observes_private_exact_content_or_raises() {
    let f = Box::pin(SearchFixture::new(AccessLevel::PUBLIC)).await;
    f.upload_sales().await;
    let call = f.prepare(json!({"query":"AX-4410 private floor?"})).await;
    let result = f
        .knowledge()
        .call_knowledge_tool(f.claim.fence, call)
        .await
        .unwrap();
    let AgentToolObservation::Completed(result) = result else {
        panic!("read completed locally")
    };
    assert!(!result.to_string().contains("PRIVATE FLOOR"));
    assert!(result.to_string().contains("public list price"));
    assert_eq!(f.snapshot().await["content"], 0);
    assert_eq!(f.snapshot().await["proofs"], 1);
    f.library.shutdown().await.unwrap();
}

#[tokio::test]
async fn knowledge_search_replay_uses_the_canonical_result_without_embedding_again() {
    let f = Box::pin(SearchFixture::new(AccessLevel::L5)).await;
    f.upload_sales().await;
    let call = f.prepare(json!({"query":"AX-4410 floor?"})).await;
    let runtime = f.knowledge();
    let AgentToolObservation::Completed(first) = runtime
        .call_knowledge_tool(f.claim.fence, copy_call(&call))
        .await
        .unwrap()
    else {
        panic!("completed")
    };
    f.library
        .upload(
            KnowledgeScope {
                company: f.claim.fence.scope.company,
                actor_id: f.fixture.target.actor.user_id(),
            },
            vec![
                KnowledgeUploadFile::from_upload(
                    "5_sales.md",
                    b"AX-4410 CHANGED PRIVATE FLOOR".to_vec(),
                )
                .unwrap(),
            ],
        )
        .await
        .unwrap();
    let AgentToolObservation::Completed(replayed) = runtime
        .call_knowledge_tool(f.claim.fence, copy_call(&call))
        .await
        .unwrap()
    else {
        panic!("completed")
    };
    assert_eq!(replayed, first);
    assert!(!replayed.to_string().contains("CHANGED"));
    assert_eq!(f.model.query_count(), 1);
    assert_eq!(f.snapshot().await["content"], 5);
    assert_eq!(f.snapshot().await["proofs"], 1);
    f.library.shutdown().await.unwrap();
}
