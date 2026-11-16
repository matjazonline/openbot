//! Received tool/native outcomes continue their saved owner, before any final reply.
use super::*;
use crate::adapters::persistence::workflow::tests::{
    local_agent_tests::Setup,
    message_tests::{
        agent_tests::{agent_runtime, published_message_agent},
        support as messages,
    },
};
use crate::application::workflow::{
    completion::WorkflowCompletion,
    lease::{FencedWorkflowResult, WorkflowHandlerOutcome},
    polling::WorkflowStepKind,
    worker::WorkflowHandler,
};
use crate::services::test_support::{
    LlmTurn, ScriptedLlm, register_scripted_agent_base_url, scripted_llm,
};

#[derive(Clone, Copy)]
enum ToolReceipt {
    Pending,
    Accepted,
    Failed,
}

async fn snapshot(f: &UnusedFinalPolicy) -> Value {
    let mut state = child_snapshot(f).await;
    state["effects"] = sqlx::query_scalar(
        "SELECT jsonb_build_object( \
         'receipts',(SELECT jsonb_agg(to_jsonb(receipt) ORDER BY invocation_id) FROM workflow_action_receipts AS receipt WHERE company_id=$1), \
         'messages',(SELECT jsonb_agg(to_jsonb(action) ORDER BY invocation_id) FROM workflow_message_actions AS action WHERE company_id=$1), \
         'local',(SELECT jsonb_agg(to_jsonb(operation) ORDER BY id) FROM local_agent_operations AS operation WHERE company_id=$1))",
    ).bind(f.fixture.scope.company.as_uuid()).fetch_one(f.fixture.p.pool()).await.unwrap();
    state
}

async fn tool_receipt(receipt: ToolReceipt) {
    let s = Box::pin(Intents::new()).await;
    let receiver = member(&s.fixture, "member").await;
    let agent = Box::pin(published_message_agent(&s.fixture)).await;
    let mut source = kind_source("agent.run");
    source["steps"]["start"]["with"]["agent"] = json!({"literal":"assistant"});
    let f = Box::pin(unused_final_policy_for(
        s,
        source,
        FinalPublication::AgentMessaging,
    ))
    .await;
    let p = &f.fixture.p;
    let scope = f.fixture.scope;
    let claim = p
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), messages::lease())
        .await
        .unwrap()
        .unwrap();
    let mut turns = vec![LlmTurn::tool_call(
        "message__send",
        json!({
        "content":{"subject":"Scope","body":"Received tool outcome"},
        "destinations":[{"kind":"user","id":receiver}]}),
    )];
    if !matches!(receipt, ToolReceipt::Pending) {
        turns.push(LlmTurn::text("\"sent\""));
    }
    let llm = scripted_llm(turns).await;
    register_scripted_agent_base_url(agent, &llm.base_url);
    let runtime = agent_runtime(p);
    let kind = WorkflowStepKind("agent.run".into());
    let outcome = Box::pin(runtime.execute(&kind, &claim)).await;
    let WorkflowHandlerOutcome::Suspended(parked) = outcome else {
        panic!("actual tool call must park: {outcome:?}");
    };
    assert!(
        p.confirm_suspension(claim.fence, parked.wait)
            .await
            .unwrap()
    );
    // Box the received-result/provider phase to retain the stock test stack.
    Box::pin(consume_tool(receipt, &f, &runtime, llm, &kind)).await;
}

async fn consume_tool(
    receipt: ToolReceipt,
    f: &UnusedFinalPolicy,
    runtime: &impl WorkflowHandler,
    llm: ScriptedLlm,
    kind: &WorkflowStepKind,
) {
    let p = &f.fixture.p;
    let scope = f.fixture.scope;
    if matches!(receipt, ToolReceipt::Pending) {
        let before = snapshot(f).await;
        for _ in 0..2 {
            assert_eq!(p.resume_wait(scope).await.unwrap(), WaitProgress::Pending);
            assert_eq!(snapshot(f).await, before);
        }
        assert_eq!(llm.finish().await.unwrap(), 1);
        return;
    }
    let status = if matches!(receipt, ToolReceipt::Accepted) {
        "delivered"
    } else {
        "dead"
    };
    messages::deliver(
        p.pool(),
        messages::delivery(p.pool(), scope.run, 0).await,
        status,
    )
    .await;
    assert_eq!(
        p.consume_message_receipts(scope, messages::lease())
            .await
            .unwrap(),
        1
    );
    assert_eq!(p.resume_wait(scope).await.unwrap(), WaitProgress::Continued);
    let stored = snapshot(f).await;
    assert_eq!(stored["proposals"], Value::Null);
    assert_eq!(stored["effects"]["receipts"].as_array().unwrap().len(), 1);
    assert_eq!(p.resume_wait(scope).await.unwrap(), WaitProgress::Refused);
    assert_eq!(snapshot(f).await, stored);
    let continued = p
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), messages::lease())
        .await
        .unwrap()
        .unwrap();
    let WorkflowHandlerOutcome::Completed(Ok(output)) =
        Box::pin(runtime.execute(kind, &continued)).await
    else {
        panic!("the resumed actual agent must consume its saved receipt");
    };
    assert_eq!(output, json!("sent"));
    assert_eq!(
        llm.finish().await.unwrap(),
        2,
        "the saved tool call must not republish"
    );
    let before_final = snapshot(f).await;
    assert!(matches!(
        p.complete_io(FencedWorkflowResult {
            fence: continued.fence,
            output,
            policy: messages::lease()
        })
        .await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(
        snapshot(f).await,
        before_final,
        "the later actual final still requires current reviewers"
    );
}

#[tokio::test]
async fn workflow_reviewer_follow_pending_model_tool_effect_ignores_unused_retired_final_reviewers()
{
    Box::pin(tool_receipt(ToolReceipt::Pending)).await;
}

#[tokio::test]
async fn workflow_reviewer_follow_accepted_model_tool_receipt_ignores_unused_retired_final_reviewers()
 {
    Box::pin(tool_receipt(ToolReceipt::Accepted)).await;
}

#[tokio::test]
async fn workflow_reviewer_follow_failed_model_tool_receipt_ignores_unused_retired_final_reviewers()
{
    Box::pin(tool_receipt(ToolReceipt::Failed)).await;
}

struct NativeWait {
    configured: UnusedFinalPolicy,
    local: Setup,
}

async fn native_wait() -> NativeWait {
    let mut local = Box::pin(Setup::new()).await;
    let fixture = Fixture {
        _db: local.fixture._db.clone(),
        persistence: local.fixture.persistence.clone(),
        target: DefinitionTarget {
            company: local.fixture.target.company,
            actor: local.fixture.target.actor,
            workflow: WorkflowId::new(Uuid::new_v4()),
        },
    };
    let mut draft = fixture.save(None);
    let mut initial: Value = serde_json::from_str(&draft.content.source).unwrap();
    initial["workflow_id"] = json!(fixture.target.workflow.to_string());
    draft.content.source = initial.to_string();
    let s = Box::pin(Intents::from_draft(fixture, draft)).await;
    let mut source = kind_source("local_agent.run");
    source["steps"]["start"]["with"] = json!({
        "profile_id":{"literal":local.profile.id},"prompt":{"literal":"received native outcome"}});
    let configured = Box::pin(unused_final_policy_for(
        s,
        source,
        FinalPublication::Messaging,
    ))
    .await;
    local.scope = configured.fixture.scope;
    let p = &configured.fixture.p;
    let claim = p
        .claim_io(
            local.scope,
            WorkflowWorkerId(Uuid::new_v4()),
            messages::lease(),
        )
        .await
        .unwrap()
        .unwrap();
    let outcome = Box::pin(
        local
            .handler()
            .execute(&WorkflowStepKind("local_agent.run".into()), &claim),
    )
    .await;
    let WorkflowHandlerOutcome::Suspended(parked) = outcome else {
        panic!("actual native call must park: {outcome:?}");
    };
    assert!(
        p.confirm_suspension(claim.fence, parked.wait)
            .await
            .unwrap()
    );
    NativeWait { configured, local }
}

#[tokio::test]
async fn workflow_reviewer_follow_pending_local_agent_effect_ignores_unused_retired_final_reviewers()
 {
    let f = Box::pin(native_wait()).await;
    let before = snapshot(&f.configured).await;
    for _ in 0..2 {
        assert_eq!(
            f.configured
                .fixture
                .p
                .resume_wait(f.local.scope)
                .await
                .unwrap(),
            WaitProgress::Pending
        );
        assert_eq!(snapshot(&f.configured).await, before);
    }
}

#[tokio::test]
async fn workflow_reviewer_follow_received_local_agent_effect_ignores_unused_retired_final_reviewers()
 {
    let f = Box::pin(native_wait()).await;
    // Existing native-protocol fixture commits the real received observations;
    // this does not claim an external native provider executed a task.
    Box::pin(f.local.native_success()).await;
    let p = &f.configured.fixture.p;
    let scope = f.local.scope;
    assert_eq!(p.resume_wait(scope).await.unwrap(), WaitProgress::Continued);
    let stored = snapshot(&f.configured).await;
    assert_eq!(stored["proposals"], Value::Null);
    assert_eq!(p.resume_wait(scope).await.unwrap(), WaitProgress::Refused);
    assert_eq!(snapshot(&f.configured).await, stored);
    let claim = p
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), messages::lease())
        .await
        .unwrap()
        .unwrap();
    let outcome = Box::pin(
        f.local
            .handler()
            .execute(&WorkflowStepKind("local_agent.run".into()), &claim),
    )
    .await;
    let WorkflowHandlerOutcome::Completed(Ok(output)) = outcome else {
        panic!("received native result: {outcome:?}");
    };
    let before_final = snapshot(&f.configured).await;
    assert!(matches!(
        p.complete_io(FencedWorkflowResult {
            fence: claim.fence,
            output,
            policy: messages::lease()
        })
        .await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(snapshot(&f.configured).await, before_final);
}
