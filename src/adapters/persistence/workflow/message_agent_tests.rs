//! D4 sequential saved effects end to end: a native `agent.run` whose model calls
//! `message.send` parks on the delivery wait, the receipt continues the same agent job
//! (continuation only; SQL refuses anything else), and the resumed driver consumes the
//! receipt without republishing.
use super::*;
use crate::adapters::{
    harness::rig::{WorkflowAgentRuntime, providers::ProviderRegistry},
    mcp::{HttpMcpClient, policy::EndpointPolicy},
    persistence::workflow::{
        agent_publication::AgentPublicationDirectory, tests::approval_test_support::notices,
    },
    workflow_agent::AgentActionTools,
    workflow_http::{HttpDestinationPolicy, HttpRuntime},
    workflow_mcp::McpRuntime,
};
use crate::application::{
    use_cases::{
        agent::{AgentPersistence, AgentWrite},
        company::{CompanyModelConnectionWrite, CompanyPersistence},
    },
    workflow::{
        lease::{WorkflowHandlerOutcome, WorkflowLeases, WorkflowWorkerId},
        polling::WorkflowStepKind,
        waits::{WaitProgress, WorkflowWaits},
        worker::WorkflowHandler,
    },
};
use crate::services::test_support::{
    LlmTurn, SCRIPTED_MODEL, SCRIPTED_PROVIDER, register_scripted_agent_base_url, scripted_llm,
};

#[tokio::test]
async fn workflow_agent_message_tool_parks_on_delivery_and_continues_with_its_receipt() {
    let fixture = Fixture::new().await;
    let channel = new_channel(&fixture, "runs").await;
    let user = member(&fixture, "member").await;
    let p = fixture.persistence.clone();
    let company = fixture.target.company;
    let agent = AgentPersistence::create(
        &p,
        company.as_uuid(),
        AgentWrite {
            name: "Assistant".into(),
            slug: "assistant".into(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            system_prompt: Some("Notify the member, then answer.".into()),
            workflow_tools: Some(vec![TypeName::parse("message.send").unwrap()]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    CompanyPersistence::replace_model_connections_for_user(
        &p,
        fixture.target.actor.user_id(),
        company.as_uuid(),
        vec![CompanyModelConnectionWrite {
            provider: SCRIPTED_PROVIDER.into(),
            api_key: Some("scripted-key".into()),
            models: vec![SCRIPTED_MODEL.into()],
            is_default: true,
        }],
    )
    .await
    .unwrap();
    let definitions = DefinitionService::new(
        Preflight,
        p.clone(),
        WorkflowSourceDecoder,
        AgentPublicationDirectory {
            persistence: p.clone(),
            inner: message::MessagePublicationDirectory {
                persistence: p.clone(),
            },
        },
    );
    let mut draft = fixture.save(None);
    draft.content.source = registry::example("agent.run").unwrap().source;
    definitions.save(draft).await.unwrap();
    let publication = fixture.publish();
    definitions.publish(publication.clone()).await.unwrap();
    let target = Box::pin(bound(&fixture, publication.version)).await;
    let claim = Box::pin(claimed(&fixture, target, channel)).await;
    let scope = claim.fence.scope;
    let llm = scripted_llm(vec![
        LlmTurn::tool_call(
            "message__send",
            json!({"content":{"subject":"Status","body":"Your request is in progress."},
                   "destinations":[{"kind":"user","id":user.to_string()}]}),
        ),
        LlmTurn::text("\"sent\""),
    ])
    .await;
    register_scripted_agent_base_url(agent.id, &llm.base_url);
    let runtime = WorkflowAgentRuntime::new(
        ProviderRegistry::standard().unwrap(),
        p.clone(),
        AgentActionTools::new(
            Arc::new(HttpRuntime::new(
                p.clone(),
                HttpDestinationPolicy::default(),
                lease(),
            )),
            Arc::new(McpRuntime::new(
                p.clone(),
                Arc::new(HttpMcpClient::new(EndpointPolicy::default())),
                lease(),
            )),
            Arc::new(runtime(&p)),
        ),
        Some(notices(&p)),
        lease(),
    );
    let kind = WorkflowStepKind("agent.run".into());
    let WorkflowHandlerOutcome::Suspended(parked) = Box::pin(runtime.execute(&kind, &claim)).await
    else {
        panic!("the saved message tool call parks the agent on its delivery wait");
    };
    let waiting: (String, Option<String>) =
        sqlx::query_as("SELECT state, waiting_reason FROM workflow_runs WHERE id=$1")
            .bind(scope.run.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(waiting, ("waiting".into(), Some("effect".into())));
    deliver(
        p.pool(),
        delivery(p.pool(), scope.run, 0).await,
        "delivered",
    )
    .await;
    assert_eq!(p.consume_message_receipts(scope, lease()).await.unwrap(), 1);
    // 06-F020: the receipt only continues the agent job; completing its wait while the job
    // stays parked (as a step completion would) is refused when the transaction commits.
    let mut tx = p.pool().begin().await.unwrap();
    sqlx::query("UPDATE workflow_waits SET state='completed',settled_at=clock_timestamp() WHERE id=$1 AND state='waiting'")
        .bind(parked.wait.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    let refused = tx.commit().await.unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("model-tool delivery only continues its agent job"),
        "{refused}"
    );
    assert!(matches!(
        p.resume_wait(scope).await.unwrap(),
        WaitProgress::Continued
    ));
    let continued = p
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .expect("the delivery continued the parked agent job");
    let outcome = Box::pin(runtime.execute(&kind, &continued)).await;
    let WorkflowHandlerOutcome::Completed(Ok(output)) = outcome else {
        panic!("the resumed agent consumes the receipt and answers");
    };
    assert_eq!(output, json!("sent"));
    assert_eq!(llm.finish().await.unwrap(), 2);
    let counts: (i64, i64, i64, i16, i32) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM workflow_message_actions WHERE run_id=$1),
                (SELECT count(*) FROM workflow_message_destinations WHERE run_id=$1),
                (SELECT count(*) FROM workflow_waits WHERE run_id=$1 AND id=$2 AND state='completed'),
                (SELECT continuation_count FROM background_tasks WHERE id=$3),
                (SELECT retry_count FROM background_tasks WHERE id=$3)",
    )
    .bind(scope.run.as_uuid())
    .bind(parked.wait.as_uuid())
    .bind(scope.job.0)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(counts, (1, 1, 1, 1, 0));
}
