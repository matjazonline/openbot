//! 06-D6 C/D/E: `ai.classify` and `decision.agent` on the shared durable driver — tool-free
//! requests, declared labels and choices only, current authority over exactly the prepared
//! capabilities, token pauses and restarts like `agent.run`.
use super::recovery::{crash_at, exchange, held, ledger, restart, supervised, tokens};
use super::*;
use crate::application::workflow::supervise::SupervisedResult;
use crate::services::test_support::{ScriptedResponse, scripted_scenario};

fn classify(source: &mut Value) {
    source["steps"]["start"] = json!({"type":"ai.classify","with":{
        "agent":{"literal":"assistant"},
        "context":{"literal":{"message":"I was charged twice"}},
        "output_schema":{"literal":{"type":"string","enum":["billing","sales"]}}},
        "routes":{"success":"$end"}});
}

fn decide(source: &mut Value) {
    source["steps"]["start"] = json!({"type":"decision.agent","with":{
        "agent":{"literal":"assistant"},
        "context":{"literal":{"draft":"Thanks, refunded."}},
        "data_schema":{"literal":{"type":"string","maxLength":128}}},
        "routes":{"choices":{"approve":"$end","revise":"$end"}}});
}

async fn step_setup(step: fn(&mut Value)) -> AgentSetup {
    Box::pin(agent_setup_source(
        "http://127.0.0.1:1",
        None,
        false,
        |source, _| step(source),
    ))
    .await
}

/// The run's frozen bundle accepts `output` as the step's committed result.
async fn accepted(setup: &AgentSetup, output: &Value) -> bool {
    let stored: Vec<u8> = sqlx::query_scalar("SELECT bundle FROM workflow_runs WHERE id=$1")
        .bind(setup.claim.fence.scope.run.as_uuid())
        .fetch_one(setup.fixture.persistence.pool())
        .await
        .unwrap();
    publication::restore_bundle(
        &stored,
        &crate::adapters::workflow_source::WorkflowSourceDecoder,
    )
    .unwrap()
    .compiled()
    .validate_step_output(&setup.claim.activation.step, output)
    .is_ok()
}

fn tool_free(request: &Value) -> bool {
    request
        .get("tools")
        .is_none_or(|tools| tools.as_array().is_none_or(Vec::is_empty))
}

/// The model kind is read from the claimed step; the handler's kind argument only routes.
async fn run(
    runtime: &WorkflowAgentRuntime<PostgresPersistence, AgentActionTools>,
    claim: &ClaimedWorkflow,
) -> WorkflowHandlerOutcome {
    Box::pin(runtime.execute(&WorkflowStepKind("ai.classify".into()), claim)).await
}

#[tokio::test]
async fn workflow_classify_answers_a_declared_label_with_a_tool_free_request() {
    let setup = step_setup(classify).await;
    let mut llm = scripted_llm(vec![LlmTurn::text("\"billing\"")]).await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let runtime = agent_runtime(&setup);
    let outcome = run(&runtime, &setup.claim).await;
    let output = outcome.completed_result().unwrap();
    assert_eq!(output, json!("billing"));
    assert!(accepted(&setup, &output).await);
    let requests = llm.observed();
    assert!(tool_free(&requests[0]), "a classifier never receives tools");
    assert!(
        requests[0]["messages"]
            .to_string()
            .contains("untrusted input")
    );
    assert_eq!(llm.finish().await.unwrap(), 1);
    let charged: Uuid =
        sqlx::query_scalar("SELECT agent_id FROM model_usage_calls WHERE run_id=$1")
            .bind(setup.claim.fence.scope.run.as_uuid())
            .fetch_one(setup.fixture.persistence.pool())
            .await
            .unwrap();
    assert_eq!(charged, setup.agent);
}

#[tokio::test]
async fn workflow_classify_repairs_an_undeclared_label_then_fails_terminally() {
    let setup = step_setup(classify).await;
    let calls = 1 + crate::services::response_contract::MAX_REPAIR_CALLS;
    let llm = scripted_llm((0..calls).map(|_| LlmTurn::text("\"refund\"")).collect()).await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let runtime = agent_runtime(&setup);
    let failure = run(&runtime, &setup.claim)
        .await
        .completed_result()
        .unwrap_err()
        .failure;
    assert_eq!(failure.code().as_str(), "agent.invalid_output");
    assert_eq!(failure.class(), FailureClass::Terminal);
    assert_eq!(llm.finish().await.unwrap(), calls);
}

#[tokio::test]
async fn workflow_decision_agent_chooses_only_a_declared_route_with_valid_data() {
    let setup = step_setup(decide).await;
    let mut llm = scripted_llm(vec![
        // An invented route is never accepted: the contract repairs it.
        LlmTurn::text(r#"{"choice":"publish","data":"now"}"#),
        LlmTurn::text(r#"{"choice":"approve","data":"Looks right"}"#),
    ])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let runtime = agent_runtime(&setup);
    let outcome = run(&runtime, &setup.claim).await;
    let output = outcome.completed_result().unwrap();
    assert_eq!(output, json!({"choice":"approve","data":"Looks right"}));
    assert!(accepted(&setup, &output).await);
    assert!(!accepted(&setup, &json!({"choice":"publish","data":"now"})).await);
    let requests = llm.observed();
    assert!(requests.iter().all(tool_free));
    assert!(
        requests[0].to_string().contains("revise"),
        "choices are offered"
    );
    assert_eq!(llm.finish().await.unwrap(), 2);
}

/// 06-F026: a data schema with local `$defs` references keeps resolving inside the
/// decision contract, exactly as the compiled step contract the engine validates.
#[tokio::test]
async fn workflow_decision_agent_data_schema_local_references_resolve() {
    let setup = step_setup(|source| {
        decide(source);
        source["steps"]["start"]["with"]["data_schema"] = json!({"literal":{
            "$defs":{"note":{"type":"string","maxLength":64}},
            "type":"object","required":["note"],"additionalProperties":false,
            "properties":{"note":{"$ref":"#/$defs/note"}}}});
    })
    .await;
    let llm = scripted_llm(vec![LlmTurn::text(
        r#"{"choice":"revise","data":{"note":"Add the order number"}}"#,
    )])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let runtime = agent_runtime(&setup);
    let output = run(&runtime, &setup.claim)
        .await
        .completed_result()
        .unwrap();
    assert_eq!(
        output,
        json!({"choice":"revise","data":{"note":"Add the order number"}})
    );
    assert!(accepted(&setup, &output).await);
    assert!(
        !accepted(
            &setup,
            &json!({"choice":"revise","data":{"note":"x".repeat(65)}})
        )
        .await,
        "the referenced definition is enforced"
    );
    assert_eq!(llm.finish().await.unwrap(), 1);
}

/// 06-D6 D: classify and decide check only agent and model authority; a removed tool or
/// unlinked skill does not block them, a removed model does.
#[tokio::test]
async fn workflow_model_steps_check_only_the_capabilities_they_use() {
    let prompt = json!([{"kind":"prompt","text":"Know the refund policy."}]);
    let setup = Box::pin(agent_setup_source(
        "http://127.0.0.1:1",
        Some(prompt),
        false,
        |source, _| classify(source),
    ))
    .await;
    let p = &setup.fixture.persistence;
    sqlx::query("UPDATE agents SET workflow_tools='{}' WHERE id=$1")
        .bind(setup.agent)
        .execute(p.pool())
        .await
        .unwrap();
    sqlx::query("DELETE FROM agent_skills WHERE agent_id=$1")
        .bind(setup.agent)
        .execute(p.pool())
        .await
        .unwrap();
    let mut llm = scripted_llm(vec![LlmTurn::text("\"sales\"")]).await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let runtime = agent_runtime(&setup);
    let outcome = run(&runtime, &setup.claim).await;
    assert_eq!(outcome.completed_result().unwrap(), json!("sales"));
    assert!(
        !llm.observed()[0].to_string().contains("refund policy"),
        "skill text never reaches a classifier"
    );
    assert_eq!(llm.finish().await.unwrap(), 1);

    let revoked = step_setup(classify).await;
    let llm = scripted_llm(vec![]).await;
    register_scripted_agent_base_url(revoked.agent, &llm.base_url);
    sqlx::query(
        "UPDATE company_model_connections SET models='{another-model}' WHERE company_id=$1",
    )
    .bind(revoked.fixture.target.company.as_uuid())
    .execute(revoked.fixture.persistence.pool())
    .await
    .unwrap();
    let runtime = agent_runtime(&revoked);
    let failure = run(&runtime, &revoked.claim)
        .await
        .completed_result()
        .unwrap_err()
        .failure;
    assert_eq!(failure.code().as_str(), "agent.rejected");
    assert_eq!(llm.finish().await.unwrap(), 0);
}

/// Token pauses and restarts behave exactly as for `agent.run` (one driver).
#[tokio::test]
async fn workflow_classify_token_pause_and_restart_use_the_shared_driver() {
    let setup = Box::pin(agent_setup_source(
        "http://127.0.0.1:1",
        None,
        false,
        |source, actor| {
            classify(source);
            tokens(100)(source, actor);
        },
    ))
    .await;
    let llm = scripted_llm(vec![LlmTurn::text("\"billing\"")]).await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let runtime = agent_runtime(&setup);
    let p = &setup.fixture.persistence;
    assert!(matches!(
        supervised(&runtime, p, &setup.claim).await,
        SupervisedResult::Suspended(_)
    ));
    approve_open(&setup, "classify-approve").await;
    let continued = reclaim(&setup).await;
    let outcome = run(&runtime, &continued).await;
    assert_eq!(outcome.completed_result().unwrap(), json!("billing"));
    assert_eq!(llm.finish().await.unwrap(), 1);

    let crashed = step_setup(classify).await;
    let (lost, arrival, release) = held(0);
    let llm = scripted_scenario(vec![
        lost,
        exchange(ScriptedResponse::turn(LlmTurn::text("\"sales\""), 1)),
    ])
    .await;
    register_scripted_agent_base_url(crashed.agent, &llm.base_url);
    let runtime = agent_runtime(&crashed);
    crash_at(&runtime, &crashed.claim, arrival).await;
    release.send(()).unwrap();
    let fresh = restart(&crashed).await;
    let outcome = run(&runtime, &fresh).await;
    assert_eq!(outcome.completed_result().unwrap(), json!("sales"));
    assert_eq!(llm.finish().await.unwrap(), 2);
    let (rows, retries) = ledger(&crashed).await;
    assert_eq!(
        rows.iter().map(|(_, settled)| *settled).collect::<Vec<_>>(),
        vec![false, true]
    );
    assert_eq!(retries, 1);
}
