use super::*;
use crate::adapters::persistence::workflow::tests::action_outcome_support::CompletedOutcome;
use crate::application::workflow::{polling::WorkflowStepKind, worker::WorkflowHandler};
use crate::services::test_support::{LlmTurn, register_scripted_agent_base_url, scripted_llm};

async fn frozen(f: &HistoryThread, claim: &ClaimedWorkflow) -> Value {
    sqlx::query_scalar("SELECT frozen_preparation FROM agent_history_exchanges WHERE company_id=$1 AND execution_id=$2")
        .bind(claim.fence.scope.company.as_uuid()).bind(claim.fence.scope.execution.as_uuid())
        .fetch_one(f.s.pool()).await.unwrap()
}

#[tokio::test]
async fn agent_history_selection_second_real_call_sees_complete_pair_and_current_input_last() {
    let f = Box::pin(HistoryThread::new()).await;
    let mut llm = scripted_llm(vec![
        LlmTurn::text("\"first\""),
        LlmTurn::text("\"second\""),
    ])
    .await;
    register_scripted_agent_base_url(f.agent, &llm.base_url);
    let first = f.next().await;
    let kind = WorkflowStepKind("agent.run".into());
    assert_eq!(
        Box::pin(runtime(&f).execute(&kind, &first))
            .await
            .completed_result()
            .unwrap(),
        json!("first")
    );
    let initial = frozen(&f, &first).await;
    assert_eq!(initial["history"]["seed"], json!([]));
    let second = f.next().await;
    let prepared =
        f.s.p()
            .prepare_agent(&second, policy())
            .await
            .unwrap()
            .expect_ready();
    let spec = prepared.prepared.spec(second.fence).unwrap();
    assert_eq!(spec.seed.pairs().len(), 1);
    assert_eq!(
        spec.seed.pairs()[0].input,
        first.activation.inputs["context"]
    );
    assert_eq!(spec.seed.pairs()[0].output, json!("first"));
    assert_eq!(
        prepared.prepared.logical_input().value(),
        &second.activation.inputs["context"]
    );
    let before = frozen(&f, &second).await;
    assert_eq!(before["history"]["omitted_eligible"], 0);
    assert_eq!(
        Box::pin(runtime(&f).execute(&kind, &second))
            .await
            .completed_result()
            .unwrap(),
        json!("second")
    );
    assert_eq!(frozen(&f, &second).await, before);
    let requests = llm.observed();
    assert_eq!(requests.len(), 2);
    let messages = requests[1]["messages"].as_array().unwrap();
    assert_eq!(
        messages
            .iter()
            .map(|m| m["role"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["system", "user", "assistant", "user"]
    );
    assert_eq!(
        messages[2]["content"],
        json!([{"type":"text","text":"\"first\""}])
    );
    assert_eq!(messages.last().unwrap()["content"], spec.initial_prompt);
    assert!(messages.iter().all(|m| m.get("tool_calls").is_none()));
    assert_eq!(llm.finish().await.unwrap(), 2);
}

#[path = "agent_history_context_level_tests.rs"]
mod level;

#[path = "agent_history_freeze_guard_tests.rs"]
mod native;

#[path = "agent_history_private_context_tests.rs"]
mod private;
