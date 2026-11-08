use super::*;
use crate::application::workflow::{
    lifecycle::BindingLifecycle,
    tools::{WorkflowToolWrite, WorkflowTools},
};
use crate::entities::agent_capability::{
    AgentBindingId, AgentBindingSource, AgentToolAlias, AgentToolBinding,
};

async fn with_question_child(d: &Deployment) -> Uuid {
    let mut child = example(3).source;
    child["steps"]["start"]["type"] = json!("channel.no_reply");
    let bound = d.bind(child, None).await;
    let state = d
        .p()
        .binding(bound.company, bound.binding)
        .await
        .unwrap()
        .unwrap();
    let child = state.configuration.bundle().compiled().graph().definition();
    let name = crate::domain::workflow::TypeName::parse("history.ask").unwrap();
    d.p()
        .register_workflow_tool(
            bound.company,
            bound.actor,
            WorkflowToolWrite {
                name: name.clone(),
                workflow: child.workflow_id,
                version: child.version_id,
                params: json!({}),
                expected: None,
            },
        )
        .await
        .unwrap();
    let tool = AgentToolBinding {
        id: AgentBindingId::new(Uuid::new_v4()),
        alias: AgentToolAlias::parse("ask").unwrap(),
        source: AgentBindingSource::Workflow {
            registration: name,
            workflow: child.workflow_id,
            version: child.version_id,
            params: json!({}),
        },
    };
    let agent: Uuid =
        sqlx::query_scalar("SELECT id FROM agents WHERE company_id=$1 AND slug='assistant'")
            .bind(d.fixture.target.company.as_uuid())
            .fetch_one(d.p().pool())
            .await
            .unwrap();
    AgentPersistence::update(
        d.p(),
        agent,
        AgentWrite {
            name: "Assistant".into(),
            slug: "assistant".into(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            system_prompt: Some("Ask the registered child, then answer.".into()),
            tool_bindings: Some(vec![tool]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    d.fixture.publish_agent(agent).await;
    let mut parent = example(0);
    model(&mut parent);
    Box::pin(bind(d, parent)).await;
    agent
}

#[tokio::test]
async fn agent_history_continuation_real_thread_reply_reaches_child_before_parent_owned_turn_queue()
{
    let d = Box::pin(Deployment::new(vec![])).await;
    let agent = with_question_child(&d).await;
    let run =
        super::release::materialize(&d, customer_email(&d, "history-active-parent", None)).await;
    let input: Value =
        sqlx::query_scalar("SELECT input FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(d.fixture.target.company.as_uuid())
            .bind(run.as_uuid())
            .fetch_one(d.p().pool())
            .await
            .unwrap();
    let mut llm = scripted_llm(vec![
        LlmTurn::tool_call("ask", input),
        LlmTurn::text(r#"{"body":"Parent accepted answer"}"#),
    ])
    .await;
    crate::services::test_support::register_scripted_agent_base_url(agent, &llm.base_url);
    let delivery = Delivery::start(&d, ProviderCase::Accept);
    Box::pin(
        d.run_until("child question while parent owns active turn", async || {
            waiting(&d, 1).await && delivered(&d, 1).await
        }),
    )
    .await;
    let before = active_parent_turn(&d, run).await;
    let (_, rfc) = prompt(&d, 0).await;
    let accepted = answer(&d, "history-active-answer", &rfc, "Authorized human result").await;
    let reply_intents = accepted.intent_ids;
    Box::pin(d.run_until(
        "parent continues once after real child answer",
        async || responses_published(&d, 1).await && delivered(&d, 2).await,
    ))
    .await;
    let provider = delivery.stop().await;
    let after = exchanges(&d).await;
    assert_eq!(after.len(), 1);
    assert_eq!(after[0]["id"], before["id"]);
    assert_eq!(after[0]["state"], "completed");
    assert_eq!(after[0]["preparation"], before["preparation"]);
    assert_eq!(after[0]["output"], json!({"body":"Parent accepted answer"}));
    assert_eq!(
        d.count("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
            .await,
        2
    );
    let new_work: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_runs WHERE company_id=$1 AND trigger_id=ANY($2)",
    )
    .bind(d.fixture.target.company.as_uuid())
    .bind(reply_intents)
    .fetch_one(d.p().pool())
    .await
    .unwrap();
    assert_eq!(
        new_work, 0,
        "a correlated human reply cannot admit another ordinary root"
    );
    assert_eq!(provider.messages().len(), 2);
    let requests = llm.observed();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].to_string().contains("Authorized human result"));
    assert_eq!(llm.finish().await.unwrap(), 2);
    assert_eq!(d.llm.finish().await.unwrap(), 0);
}

async fn active_parent_turn(d: &Deployment, run: RunId) -> Value {
    let before = exchanges(d).await;
    assert_eq!(before.len(), 1);
    assert_eq!(before[0]["state"], "active");
    assert_eq!(before[0]["run"], json!(run));
    let active: Uuid = sqlx::query_scalar(
        "SELECT active_exchange_id FROM agent_history_conversations WHERE company_id=$1 AND id=$2",
    )
    .bind(d.fixture.target.company.as_uuid())
    .bind(
        before[0]["conversation"]
            .as_str()
            .unwrap()
            .parse::<Uuid>()
            .unwrap(),
    )
    .fetch_one(d.p().pool())
    .await
    .unwrap();
    assert_eq!(json!(active), before[0]["id"]);
    assert_eq!(
        d.count("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
            .await,
        2
    );
    before[0].clone()
}
