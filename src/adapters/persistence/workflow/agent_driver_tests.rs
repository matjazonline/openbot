//! D4 driver behaviour around saved calls: protected-tool approval cycles, the token pause
//! resuming the exact saved request, invented tool names, output repair and target rebinds.
use super::*;

/// D4: a protected saved tool call parks the agent for approval; approval continues the same
/// job, and the saved call executes exactly once before the model answers.
#[tokio::test]
async fn workflow_agent_run_protected_tool_waits_for_approval_and_dispatches_once() {
    use crate::application::workflow::decisions::WorkflowDecisions;
    let (origin, server) = server(HTTP_OK).await;
    let setup = Box::pin(agent_setup_full(&origin, None, true)).await;
    let llm = scripted_llm(vec![
        LlmTurn::tool_call(
            "registered__request",
            json!({"method":"POST","path":"/messages","body":{"message":"hello"}}),
        ),
        LlmTurn::text("\"approved\""),
    ])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let runtime = agent_runtime(&setup);
    let kind = WorkflowStepKind("agent.run".into());
    let p = &setup.fixture.persistence;
    let scope = setup.claim.fence.scope;
    let first = Box::pin(runtime.execute(&kind, &setup.claim)).await;
    assert!(
        matches!(first, WorkflowHandlerOutcome::Suspended(_)),
        "{first:?}"
    );
    let dispatched: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_action_dispatches")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(dispatched, 0, "nothing is sent before approval");
    let command = crate::adapters::persistence::workflow::tests::approval_test_support::command(
        p,
        scope,
        setup.fixture.target.actor,
        "approve",
    )
    .await;
    p.submit_decision(command).await.unwrap();
    let continued = p
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .expect("approval continues the parked agent job");
    let outcome = Box::pin(runtime.execute(&kind, &continued)).await;
    assert_eq!(outcome.completed_result().unwrap(), json!("approved"));
    assert!(server.await.unwrap().starts_with("POST /messages"));
    assert_eq!(llm.finish().await.unwrap(), 2);
    let (dispatches, retries): (i64, i32) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM workflow_action_dispatches),(SELECT retry_count FROM background_tasks WHERE id=$1)",
    )
    .bind(scope.job.0)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!((dispatches, retries), (1, 0));
}

/// D4 F007: a request over the token allowance parks before I/O with no usage or debit;
/// approval only reopens the job; the fresh claim admits the exact saved request once.
#[tokio::test]
async fn workflow_agent_run_token_pause_resumes_with_the_saved_request() {
    let setup = Box::pin(agent_setup_source("http://127.0.0.1:1", None, false, |source, actor| {
        source["limits"]["root_budget"] = json!({"activations":10,"model_calls":10,"repetitions":10,
            "tokens":{"max_tokens":100,"reviewers":[{"kind":"user","id":actor}]}});
    }))
    .await;
    let llm = scripted_llm(vec![LlmTurn::text("\"after approval\"")]).await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let runtime = agent_runtime(&setup);
    let kind = WorkflowStepKind("agent.run".into());
    let p = &setup.fixture.persistence;
    let run = setup.claim.fence.scope.run;
    let first = Box::pin(runtime.execute(&kind, &setup.claim)).await;
    assert!(
        matches!(first, WorkflowHandlerOutcome::Suspended(_)),
        "{first:?}"
    );
    assert_eq!(
        usage_rows(p, run).await,
        0,
        "no provider call or usage before approval"
    );
    approve_open(&setup, "token-approve").await;
    let continued = reclaim(&setup).await;
    let outcome = Box::pin(runtime.execute(&kind, &continued)).await;
    assert_eq!(outcome.completed_result().unwrap(), json!("after approval"));
    assert_eq!(llm.finish().await.unwrap(), 1);
    let same: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_decisions AS decision JOIN model_usage_calls AS usage ON usage.request_id=decision.token_request_id WHERE decision.run_id=$1 AND decision.subject_kind='token_allowance')")
        .bind(run.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert!(
        same,
        "the admitted request is exactly the approved saved request"
    );
    assert_eq!(usage_rows(p, run).await, 1);
}

/// D4: two protected saved calls in one run, each with its own approval cycle; each call is
/// sent exactly once and the model is never asked to recreate a saved call.
#[tokio::test]
async fn workflow_agent_run_two_approval_cycles_dispatch_each_call_once() {
    let (origin, server) = servers(2).await;
    let setup = Box::pin(agent_setup_full(&origin, None, true)).await;
    let llm = scripted_llm(vec![
        LlmTurn::tool_call(
            "registered__request",
            json!({"method":"POST","path":"/first"}),
        ),
        LlmTurn::tool_call(
            "registered__request",
            json!({"method":"POST","path":"/second"}),
        ),
        LlmTurn::text("\"both\""),
    ])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let runtime = agent_runtime(&setup);
    let kind = WorkflowStepKind("agent.run".into());
    assert!(matches!(
        Box::pin(runtime.execute(&kind, &setup.claim)).await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    approve_open(&setup, "approve-first").await;
    let second = reclaim(&setup).await;
    let parked = Box::pin(runtime.execute(&kind, &second)).await;
    assert!(
        matches!(parked, WorkflowHandlerOutcome::Suspended(_)),
        "{parked:?}"
    );
    approve_open(&setup, "approve-second").await;
    let third = reclaim(&setup).await;
    let outcome = Box::pin(runtime.execute(&kind, &third)).await;
    assert_eq!(outcome.completed_result().unwrap(), json!("both"));
    let seen = server.await.unwrap();
    assert!(seen[0].starts_with("POST /first") && seen[1].starts_with("POST /second"));
    assert_eq!(llm.finish().await.unwrap(), 3);
    let (dispatches, retries, continuations): (i64, i32, i16) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM workflow_action_dispatches),retry_count,continuation_count FROM background_tasks WHERE id=$1",
    )
    .bind(setup.claim.fence.scope.job.0)
    .fetch_one(setup.fixture.persistence.pool())
    .await
    .unwrap();
    assert_eq!((dispatches, retries, continuations), (2, 0, 2));
}

/// Only prepared declarations dispatch: an invented tool name gets a refusal result and the
/// model continues; nothing reaches ActionService.
#[tokio::test]
async fn workflow_agent_run_refuses_model_invented_tool_without_dispatch() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let llm = scripted_llm(vec![
        LlmTurn::tool_call(
            "registered__request_admin",
            json!({"method":"DELETE","path":"/all"}),
        ),
        LlmTurn::text("\"refused\""),
    ])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let outcome = Box::pin(
        agent_runtime(&setup).execute(&WorkflowStepKind("agent.run".into()), &setup.claim),
    )
    .await;
    assert_eq!(outcome.completed_result().unwrap(), json!("refused"));
    let mut llm = llm;
    let requests = llm.observed();
    assert!(requests[1]["messages"].to_string().contains("unknown_tool"));
    assert_eq!(llm.finish().await.unwrap(), 2);
    let dispatches: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_action_dispatches")
        .fetch_one(setup.fixture.persistence.pool())
        .await
        .unwrap();
    assert_eq!(dispatches, 0);
}

/// Invalid final output is repaired through the workflow binding: tool-free, charged, and
/// only the validated JSON value completes the step.
#[tokio::test]
async fn workflow_agent_run_repairs_invalid_output_once() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let llm = scripted_llm(vec![
        LlmTurn::text("not json"),
        LlmTurn::text("\"repaired\""),
    ])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let outcome = Box::pin(
        agent_runtime(&setup).execute(&WorkflowStepKind("agent.run".into()), &setup.claim),
    )
    .await;
    assert_eq!(outcome.completed_result().unwrap(), json!("repaired"));
    let mut llm = llm;
    let requests = llm.observed();
    assert!(
        requests[1]
            .get("tools")
            .is_none_or(|tools| tools.as_array().is_none_or(Vec::is_empty))
    );
    assert_eq!(llm.finish().await.unwrap(), 2);
    assert_eq!(
        usage_rows(&setup.fixture.persistence, setup.claim.fence.scope.run).await,
        2
    );
}

/// A saved call never follows a registry rebind: after the operation moves to another
/// connection, re-preparation differs from the immutable checkpoint and fails terminally.
#[tokio::test]
async fn workflow_agent_run_refuses_rebound_tool_target() {
    let (origin, _server) = server(HTTP_OK).await;
    let setup = Box::pin(agent_setup_full(&origin, None, true)).await;
    let llm = scripted_llm(vec![LlmTurn::tool_call(
        "registered__request",
        json!({"method":"POST","path":"/messages"}),
    )])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let runtime = agent_runtime(&setup);
    let kind = WorkflowStepKind("agent.run".into());
    assert!(matches!(
        Box::pin(runtime.execute(&kind, &setup.claim)).await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    let p = &setup.fixture.persistence;
    let (company, actor) = (setup.fixture.target.company, setup.fixture.target.actor);
    p.revoke_http_operation(
        company,
        actor,
        setup.connection.id,
        setup.connection.revision,
        TypeName::parse("registered.request").unwrap(),
    )
    .await
    .unwrap();
    let other = Box::pin(protected_connection(&setup.fixture, "http://127.0.0.1:2")).await;
    assert_ne!(other.id, setup.connection.id);
    approve_open(&setup, "approve-rebind").await;
    let continued = reclaim(&setup).await;
    let outcome = Box::pin(runtime.execute(&kind, &continued)).await;
    assert_eq!(
        outcome
            .completed_result()
            .unwrap_err()
            .failure
            .code()
            .as_str(),
        "agent.rejected"
    );
    let dispatches: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_action_dispatches")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(dispatches, 0);
    assert_eq!(llm.finish().await.unwrap(), 1);
}
