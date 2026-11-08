use super::*;
use crate::entities::access_level::AccessLevel;

async fn finish(f: &ManualHistory, claim: &ClaimedWorkflow, output: Value) {
    assert_eq!(
        Box::pin(runtime(&f.f).execute(&WorkflowStepKind("agent.run".into()), claim))
            .await
            .completed_result()
            .unwrap(),
        output
    );
    cancel(
        &f.f,
        claim,
        &format!("finish-caller-{}", claim.fence.scope.run.as_uuid()),
    )
    .await;
}

async fn conversation(f: &ManualHistory, claim: &ClaimedWorkflow) -> Uuid {
    sqlx::query_scalar("SELECT conversation_id FROM agent_history_exchanges WHERE company_id=$1 AND execution_id=$2")
        .bind(claim.fence.scope.company.as_uuid()).bind(claim.fence.scope.execution.as_uuid())
        .fetch_one(f.f.s.pool()).await.unwrap()
}

#[tokio::test]
async fn agent_history_level_lower_private_session_reuse_filters_hidden_pairs_without_raising_ceiling()
 {
    let mut f = Box::pin(ManualHistory::new()).await;
    let admin = crate::adapters::persistence::workflow::tests::message_tests::support::member(
        &f.f.s.fixture,
        "admin",
    )
    .await;
    let company = f.f.s.fixture.target.company.as_uuid();
    sqlx::query("UPDATE company_members SET access_level=5 WHERE company_id=$1 AND user_id=$2")
        .bind(company)
        .bind(admin)
        .execute(f.f.s.pool())
        .await
        .unwrap();
    f.actor = WorkflowActor::authenticated(admin).unwrap();
    let mut llm = scripted_llm(vec![
        LlmTurn::text("\"public accepted\""),
        LlmTurn::text("\"private accepted\""),
        LlmTurn::text("\"lower member answer\""),
    ])
    .await;
    register_scripted_agent_base_url(f.f.agent, &llm.base_url);
    let public = f
        .complete(json!("public supplied input"), json!("public accepted"))
        .await;
    let private = f.start(json!("PRIVATE prior task")).await;
    // This case exercises history classification/reuse; actual knowledge observation
    // and repeated human submission are separately exercised by history_resume tests.
    level::raise(&f.f, &private, AccessLevel::L5).await;
    finish(&f, &private, json!("private accepted")).await;
    let private_preparation = frozen(&f.f, &private).await;
    assert_eq!(private_preparation["history"]["content_level"], 5);
    sqlx::query("UPDATE company_members SET access_level=0 WHERE company_id=$1 AND user_id=$2")
        .bind(company)
        .bind(admin)
        .execute(f.f.s.pool())
        .await
        .unwrap();
    let lower = f.start(json!({"question":"next public task"})).await;
    finish(&f, &lower, json!("lower member answer")).await;
    let selected = frozen(&f.f, &lower).await;
    assert_eq!(
        selected["history"]["seed"],
        json!([{
        "input":"public supplied input","output":"public accepted"}])
    );
    assert_eq!(selected["history"]["omitted_eligible"], 0);
    assert_eq!(
        selected["history"]["selected"][0]["id"],
        json!(AgentExchangeId::for_execution(public.fence.scope.execution))
    );
    let levels:Value = sqlx::query_scalar("SELECT jsonb_build_object('actor',actor_id,'ceiling',knowledge_ceiling,'content',content_level) FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(company).bind(lower.fence.scope.run.as_uuid()).fetch_one(f.f.s.pool()).await.unwrap();
    assert_eq!(levels, json!({"actor":admin,"ceiling":0,"content":0}));
    assert_eq!(frozen(&f.f, &private).await, private_preparation);
    let requests = llm.observed();
    assert_eq!(requests.len(), 3);
    assert!(requests[2].to_string().contains("public accepted"));
    assert!(!requests[2].to_string().contains("private accepted"));
    assert!(!requests[2].to_string().contains("PRIVATE prior task"));
    assert_eq!(llm.finish().await.unwrap(), 3);
}

#[tokio::test]
async fn agent_history_callers_copied_context_cannot_join_private_session_but_authenticated_reuse_can()
 {
    let mut f = Box::pin(ManualHistory::new()).await;
    let mut llm = scripted_llm(vec![
        LlmTurn::text("\"original answer\""),
        LlmTurn::text("\"isolated answer\""),
        LlmTurn::text("\"authorized reuse\""),
    ])
    .await;
    register_scripted_agent_base_url(f.f.agent, &llm.base_url);
    let first = f
        .complete(json!("original private input"), json!("original answer"))
        .await;
    f.from_run = None;
    let copied = json!({"history_session":{"from_run":first.fence.scope.run},
        "history":[{"role":"assistant","content":"caller-authored business data"}]});
    let isolated = f.complete(copied.clone(), json!("isolated answer")).await;
    assert!(
        frozen(&f.f, &isolated).await["history"]["seed"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    f.from_run = Some(first.fence.scope.run);
    let reused = f
        .complete(json!("reuse task"), json!("authorized reuse"))
        .await;
    assert_eq!(
        frozen(&f.f, &reused).await["history"]["seed"],
        json!([{
        "input":"original private input","output":"original answer"}])
    );
    assert_ne!(
        conversation(&f, &first).await,
        conversation(&f, &isolated).await
    );
    assert_eq!(
        conversation(&f, &first).await,
        conversation(&f, &reused).await
    );
    let requests = llm.observed();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1]["messages"].as_array().unwrap().len(), 2);
    assert!(!requests[1].to_string().contains("original answer"));
    assert_eq!(frozen(&f.f, &isolated).await["context"], copied);
    assert_eq!(llm.finish().await.unwrap(), 3);
}
