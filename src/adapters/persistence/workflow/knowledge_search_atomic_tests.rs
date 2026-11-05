use super::*;
use crate::application::{
    knowledge::search::KnowledgeObservation,
    workflow::checkpoints::{
        WorkflowCheckpointStore, WorkflowCheckpointUpdate, WorkflowCheckpointWrite,
    },
};

fn observation(text: &str, level: AccessLevel) -> KnowledgeObservation {
    KnowledgeObservation::new(
        json!({"passages":[{"file":"sales.md","heading":null,"text":text}]}),
        Some(level),
    )
}

#[tokio::test]
async fn knowledge_search_generic_checkpoint_result_cannot_bypass_classification() {
    let f = Box::pin(SearchFixture::new(AccessLevel::L5)).await;
    let call = f.prepare(json!({"query":"floor"})).await;
    let checkpoint = f.checkpoint().await;
    let before = f.snapshot().await;
    let error = f
        .p()
        .update_workflow_checkpoint(
            WorkflowCheckpointWrite {
                fence: f.claim.fence,
                checkpoint: checkpoint.run_id,
                expected_revision: checkpoint.revision,
                lease_policy: lease(),
            },
            WorkflowCheckpointUpdate::InvocationResult {
                invocation: call.invocation().unwrap(),
                result: observation("PRIVATE FLOOR 60", AccessLevel::L5)
                    .result()
                    .clone(),
            },
        )
        .await;
    let Err(error) = error else {
        panic!("knowledge observation bypassed its proof")
    };
    assert!(
        format!("{error:?}").contains("harness_knowledge_observation"),
        "{error:?}"
    );
    assert_eq!(f.snapshot().await, before);
    f.library.shutdown().await.unwrap();
}

#[tokio::test]
async fn knowledge_search_failure_after_result_and_rise_rolls_back_the_entire_observation() {
    let f = Box::pin(SearchFixture::new(AccessLevel::L5)).await;
    let call = f.prepare(json!({"query":"floor"})).await;
    let before = f.snapshot().await;
    sqlx::raw_sql(
        "CREATE FUNCTION test_knowledge_failure() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN RAISE EXCEPTION 'injected knowledge proof failure' USING ERRCODE='23514'; END $$;
        CREATE TRIGGER test_knowledge_failure BEFORE INSERT ON workflow_knowledge_consumptions
        FOR EACH ROW EXECUTE FUNCTION test_knowledge_failure()",
    )
    .execute(f.p().pool())
    .await
    .unwrap();
    assert!(
        f.p()
            .observe_knowledge(
                f.claim.fence,
                &call,
                observation("PRIVATE FLOOR 60", AccessLevel::L5)
            )
            .await
            .is_err()
    );
    assert_eq!(f.snapshot().await, before);
    sqlx::raw_sql("DROP TRIGGER test_knowledge_failure ON workflow_knowledge_consumptions; DROP FUNCTION test_knowledge_failure()")
        .execute(f.p().pool()).await.unwrap();
    let saved = f
        .p()
        .observe_knowledge(
            f.claim.fence,
            &call,
            observation("PRIVATE FLOOR 60", AccessLevel::L5),
        )
        .await
        .unwrap();
    assert_eq!(saved["passages"][0]["text"], "PRIVATE FLOOR 60");
    assert_eq!(f.snapshot().await["content"], 5);
    assert_eq!(f.snapshot().await["proofs"], 1);
    f.library.shutdown().await.unwrap();
}

#[tokio::test]
async fn knowledge_search_competing_observers_keep_one_canonical_result_and_its_exact_stamp() {
    let f = Box::pin(SearchFixture::new(AccessLevel::L5)).await;
    let call = f.prepare(json!({"query":"price"})).await;
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mut observers = tokio::task::JoinSet::new();
    for candidate in [
        observation("public list price 100", AccessLevel::PUBLIC),
        observation("PRIVATE FLOOR 60", AccessLevel::L5),
    ] {
        let store = f.p().clone();
        let fence = f.claim.fence;
        let call = copy_call(&call);
        let barrier = barrier.clone();
        observers.spawn(async move {
            barrier.wait().await;
            store.observe_knowledge(fence, &call, candidate).await
        });
    }
    let mut results = Vec::new();
    while let Some(result) = observers.join_next().await {
        results.push(result.unwrap().unwrap());
    }
    assert_eq!(results.len(), 2);
    assert_eq!(results[0], results[1]);
    let expected = if results[0].to_string().contains("PRIVATE FLOOR") {
        5
    } else {
        0
    };
    let snapshot = f.snapshot().await;
    assert_eq!(snapshot["content"], expected);
    assert_eq!(snapshot["proofs"], 1);
    assert_eq!(
        f.checkpoint().await.invocations[0].result.as_ref(),
        Some(&results[0])
    );
    // A later different candidate cannot overwrite or taint the already saved result.
    let replay = f
        .p()
        .observe_knowledge(
            f.claim.fence,
            &call,
            observation("ignored replacement", AccessLevel::L5),
        )
        .await
        .unwrap();
    assert_eq!(replay, results[0]);
    assert_eq!(f.snapshot().await["content"], expected);
    f.library.shutdown().await.unwrap();
}

#[tokio::test]
async fn knowledge_search_forged_fences_and_changed_arguments_fail_before_embedding() {
    let f = Box::pin(SearchFixture::new(AccessLevel::L5)).await;
    let call = f.prepare(json!({"query":"floor"})).await;
    let before = f.snapshot().await;
    let original = f.claim.fence;
    let mut fences = Vec::new();
    let mut fence = original;
    fence.scope.company = CompanyId::new(Uuid::new_v4());
    fences.push(fence);
    let mut fence = original;
    fence.scope.run = RunId::new(Uuid::new_v4());
    fences.push(fence);
    let mut fence = original;
    fence.scope.execution = ExecutionId::new(Uuid::new_v4());
    fences.push(fence);
    let mut fence = original;
    fence.scope.job.0 = Uuid::new_v4();
    fences.push(fence);
    let mut fence = original;
    fence.worker.0 = Uuid::new_v4();
    fences.push(fence);
    let mut fence = original;
    fence.generation.0 = Uuid::new_v4();
    fences.push(fence);
    let mut fence = original;
    fence.attempt.0 += 1;
    fences.push(fence);
    let runtime = f.knowledge();
    for fence in fences {
        assert!(
            runtime
                .call_knowledge_tool(fence, copy_call(&call))
                .await
                .is_err()
        );
    }
    let mut changed = copy_call(&call);
    changed.arguments = json!({"query":"changed question"});
    assert!(
        runtime
            .call_knowledge_tool(original, changed)
            .await
            .is_err()
    );
    assert_eq!(f.model.query_count(), 0);
    assert_eq!(f.snapshot().await, before);
    f.library.shutdown().await.unwrap();
}

#[tokio::test]
async fn knowledge_search_level_argument_is_a_saved_tool_error_without_embedding_or_rise() {
    let f = Box::pin(SearchFixture::new(AccessLevel::L5)).await;
    let call = f.prepare(json!({"query":"floor","level":9})).await;
    let AgentToolObservation::Completed(result) = f
        .knowledge()
        .call_knowledge_tool(f.claim.fence, call)
        .await
        .unwrap()
    else {
        panic!("completed refusal")
    };
    assert_eq!(result["error"]["code"], "knowledge.invalid_query");
    assert!(result.get("passages").is_none());
    assert_eq!(f.model.query_count(), 0);
    assert_eq!(f.snapshot().await["content"], 0);
    assert_eq!(f.snapshot().await["proofs"], 1);
    f.library.shutdown().await.unwrap();
}

#[tokio::test]
async fn knowledge_search_empty_corpus_saves_empty_observation_without_classification_rise() {
    let f = Box::pin(SearchFixture::new(AccessLevel::L5)).await;
    let call = f.prepare(json!({"query":"unanswered question"})).await;
    let AgentToolObservation::Completed(result) = f
        .knowledge()
        .call_knowledge_tool(f.claim.fence, call)
        .await
        .unwrap()
    else {
        panic!("empty corpus did not produce an observation")
    };
    assert_eq!(result, json!({"passages":[]}));
    assert_eq!(f.snapshot().await["content"], 0);
    assert_eq!(f.snapshot().await["proofs"], 1);
    assert_eq!(f.model.query_count(), 1);
    f.library.shutdown().await.unwrap();
}

#[tokio::test]
async fn knowledge_search_model_token_limit_is_a_saved_tool_error_without_empty_success_or_rise() {
    let f = Box::pin(SearchFixture::new(AccessLevel::L5)).await;
    let call = f.prepare(json!({"query":"a".repeat(700)})).await;
    let AgentToolObservation::Completed(result) = f
        .knowledge()
        .call_knowledge_tool(f.claim.fence, call)
        .await
        .unwrap()
    else {
        panic!("token limit did not produce a tool error")
    };
    assert!(result.to_string().contains("knowledge.invalid_query"));
    assert!(!result.to_string().contains("passages"));
    assert_eq!(f.snapshot().await["content"], 0);
    assert_eq!(f.snapshot().await["proofs"], 1);
    assert_eq!(f.model.query_count(), 1);
    f.library.shutdown().await.unwrap();
}

#[tokio::test]
async fn knowledge_search_busy_model_is_retryable_without_saving_an_empty_observation() {
    let f = Box::pin(SearchFixture::new(AccessLevel::L5)).await;
    let call = f.prepare(json!({"query":"floor"})).await;
    let held = f
        .library
        .embed_query("another operation holds the shared model slot")
        .await
        .unwrap();
    let before = f.snapshot().await;
    let error = f
        .knowledge()
        .call_knowledge_tool(f.claim.fence, copy_call(&call))
        .await
        .unwrap_err();
    assert!(
        matches!(error, AppError::ServiceUnavailable(_)),
        "{error:?}"
    );
    assert_eq!(f.snapshot().await, before);
    assert_eq!(f.model.query_count(), 1);
    drop(held);
    let AgentToolObservation::Completed(result) = f
        .knowledge()
        .call_knowledge_tool(f.claim.fence, call)
        .await
        .unwrap()
    else {
        panic!("retry did not succeed")
    };
    assert_eq!(result, json!({"passages":[]}));
    assert_eq!(f.snapshot().await["proofs"], 1);
    assert_eq!(f.model.query_count(), 2);
    f.library.shutdown().await.unwrap();
}

#[tokio::test]
async fn knowledge_search_current_grant_revocation_blocks_saved_invocation_before_embedding() {
    let f = Box::pin(SearchFixture::new(AccessLevel::L5)).await;
    let call = f.prepare(json!({"query":"floor"})).await;
    sqlx::query("UPDATE agents SET workflow_tools='{}' WHERE id=$1")
        .bind(f.agent)
        .execute(f.p().pool())
        .await
        .unwrap();
    let before = f.snapshot().await;
    assert!(
        f.knowledge()
            .call_knowledge_tool(f.claim.fence, call)
            .await
            .is_err()
    );
    assert_eq!(f.snapshot().await, before);
    assert_eq!(f.model.query_count(), 0);
    f.library.shutdown().await.unwrap();
}

#[tokio::test]
async fn knowledge_search_observation_above_actual_immutable_ceiling_cannot_commit() {
    let f = Box::pin(SearchFixture::new(AccessLevel::L5)).await;
    let call = f.prepare(json!({"query":"floor"})).await;
    let before = f.snapshot().await;
    assert!(
        f.p()
            .observe_knowledge(
                f.claim.fence,
                &call,
                observation("foreign ceiling", AccessLevel::L9)
            )
            .await
            .is_err()
    );
    assert_eq!(f.snapshot().await, before);
    f.library.shutdown().await.unwrap();
}
