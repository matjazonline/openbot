//! D4 driver proofs (06-F019): token pauses at both stages through `supervise_io`, denial,
//! restart after each model/effect boundary with provider and effect counts, an uncertain
//! effect, and a dynamically selected agent. Request IDs come from the usage ledger.
use super::*;
use crate::application::workflow::{
    activation::ActivationRequest,
    decisions::WorkflowDecisions,
    lease::{WorkflowLeaseRecovery, WorkflowLeases},
    supervise::{StopReason, SupervisedResult, supervise_io},
};
use crate::domain::workflow::WaitId;
use crate::services::test_support::{ScriptedExchange, ScriptedResponse, scripted_scenario};
use tokio::sync::oneshot;

pub(super) fn kind() -> WorkflowStepKind {
    WorkflowStepKind("agent.run".into())
}

/// Drives one attempt the way the worker does, including the exact suspension confirmation.
pub(super) async fn supervised(
    runtime: &WorkflowAgentRuntime<PostgresPersistence, AgentActionTools>,
    p: &PostgresPersistence,
    claim: &ClaimedWorkflow,
) -> SupervisedResult {
    let kind = kind();
    supervise_io(
        p,
        claim.clone(),
        lease(),
        tokio::time::Instant::now() + std::time::Duration::from_secs(60),
        Box::pin(runtime.execute(&kind, claim)),
        std::future::pending::<()>(),
    )
    .await
}

pub(super) fn tokens(max_tokens: u64) -> impl FnOnce(&mut Value, Uuid) {
    move |source, actor| {
        source["limits"]["root_budget"] = json!({"activations":10,"model_calls":10,"repetitions":10,
            "tokens":{"max_tokens":max_tokens,"reviewers":[{"kind":"user","id":actor}]}});
    }
}

/// A Chat Completions turn reporting `prompt_tokens` of input usage.
pub(super) fn costly(turn: LlmTurn, index: usize, prompt_tokens: u64) -> ScriptedResponse {
    let mut body: Value = serde_json::from_str(&ScriptedResponse::turn(turn, index).body).unwrap();
    body["usage"] = json!({"prompt_tokens":prompt_tokens,"completion_tokens":1,
        "total_tokens":prompt_tokens + 1});
    ScriptedResponse::json(body)
}

pub(super) fn exchange(response: ScriptedResponse) -> ScriptedExchange {
    ScriptedExchange::new(|_| Ok(()), response)
}

/// A turn whose request arrives (provider entry) and is then held until released; the
/// connection closes without a response.
pub(super) fn held(index: usize) -> (ScriptedExchange, oneshot::Receiver<()>, oneshot::Sender<()>) {
    let (arrived, arrival) = oneshot::channel();
    let (release, released) = oneshot::channel();
    let mut response = ScriptedResponse::turn(LlmTurn::text("\"lost\""), index);
    response.disconnect = true;
    response.barrier = Some((arrived, released));
    (exchange(response), arrival, release)
}

/// The process dies while the attempt waits on `arrival`: the driver future is dropped.
pub(super) async fn crash_at(
    runtime: &WorkflowAgentRuntime<PostgresPersistence, AgentActionTools>,
    claim: &ClaimedWorkflow,
    arrival: oneshot::Receiver<()>,
) {
    let kind = kind();
    let driver = Box::pin(runtime.execute(&kind, claim));
    tokio::select! {
        outcome = driver => panic!("the attempt must still be in flight: {outcome:?}"),
        arrived = arrival => arrived.unwrap(),
    }
}

/// Lease expiry, exact-job retirement and the retry becoming due, as the sweeper does it.
pub(super) async fn restart(setup: &AgentSetup) -> ClaimedWorkflow {
    let p = &setup.fixture.persistence;
    let scope = setup.claim.fence.scope;
    sqlx::query("UPDATE background_tasks SET locked_at=clock_timestamp()-interval '10 seconds',lock_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1")
        .bind(scope.job.0)
        .execute(p.pool())
        .await
        .unwrap();
    assert!(p.retire_expired_io(scope, lease()).await.unwrap());
    due(setup).await;
    reclaim(setup).await
}

pub(super) async fn due(setup: &AgentSetup) {
    sqlx::query(
        "UPDATE background_tasks SET run_at=clock_timestamp()-interval '1 second' WHERE id=$1",
    )
    .bind(setup.claim.fence.scope.job.0)
    .execute(setup.fixture.persistence.pool())
    .await
    .unwrap();
}

/// (request id, settled) in request order, plus the job's failure debit.
pub(super) async fn ledger(setup: &AgentSetup) -> (Vec<(Uuid, bool)>, i32) {
    let p = &setup.fixture.persistence;
    let scope = setup.claim.fence.scope;
    let rows = sqlx::query_as("SELECT request_id,settled_at IS NOT NULL FROM model_usage_calls WHERE run_id=$1 ORDER BY requested_at,request_id")
        .bind(scope.run.as_uuid())
        .fetch_all(p.pool())
        .await
        .unwrap();
    let retries = sqlx::query_scalar("SELECT retry_count FROM background_tasks WHERE id=$1")
        .bind(scope.job.0)
        .fetch_one(p.pool())
        .await
        .unwrap();
    (rows, retries)
}

async fn dispatches(p: &PostgresPersistence, run: RunId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM workflow_action_dispatches WHERE run_id=$1")
        .bind(run.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap()
}

/// F018/F019(a): a response over the allowance parks after settlement; approval reopens the
/// job; the fresh claim runs the saved tool call without a model call and parks again before
/// the next request; the second approval admits exactly that request. Both parks are
/// confirmed by the supervisor for their exact fence and wait.
#[tokio::test]
async fn workflow_agent_run_settlement_pause_and_second_pause_resume_exact_requests() {
    let (origin, server) = server(HTTP_OK).await;
    let setup = Box::pin(agent_setup_source(&origin, None, false, tokens(20_000))).await;
    let llm = scripted_scenario(vec![
        exchange(costly(
            LlmTurn::tool_call(
                "registered__request",
                json!({"method":"POST","path":"/costly"}),
            ),
            0,
            25_000,
        )),
        exchange(ScriptedResponse::turn(LlmTurn::text("\"done\""), 1)),
    ])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let runtime = agent_runtime(&setup);
    let p = &setup.fixture.persistence;
    let SupervisedResult::Suspended(settlement) = supervised(&runtime, p, &setup.claim).await
    else {
        panic!("usage over the allowance parks after settlement");
    };
    assert!(
        !p.confirm_suspension(setup.claim.fence, WaitId::new(Uuid::new_v4()))
            .await
            .unwrap(),
        "only the exact parked wait is confirmed"
    );
    approve_open(&setup, "settlement-approve").await;
    let second = reclaim(&setup).await;
    let SupervisedResult::Suspended(reservation) = supervised(&runtime, p, &second).await else {
        panic!("the next request no longer fits the extended allowance");
    };
    assert_ne!(settlement, reservation);
    assert!(
        !p.confirm_suspension(second.fence, settlement)
            .await
            .unwrap(),
        "an earlier wait never confirms a later attempt"
    );
    approve_open(&setup, "reservation-approve").await;
    let third = reclaim(&setup).await;
    let outcome = Box::pin(runtime.execute(&kind(), &third)).await;
    assert_eq!(outcome.completed_result().unwrap(), json!("done"));
    assert!(server.await.unwrap().starts_with("POST /costly"));
    assert_eq!(llm.finish().await.unwrap(), 2);
    let (rows, retries) = ledger(&setup).await;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|(_, settled)| *settled));
    assert_eq!(retries, 0);
    let stages: Vec<(String, Uuid)> = sqlx::query_as("SELECT token_stage,token_request_id FROM workflow_decisions WHERE run_id=$1 AND subject_kind='token_allowance' ORDER BY created_at,id")
        .bind(setup.claim.fence.scope.run.as_uuid())
        .fetch_all(p.pool())
        .await
        .unwrap();
    assert_eq!(
        stages,
        vec![
            ("settlement".into(), rows[0].0),
            ("reservation".into(), rows[1].0)
        ],
        "each decision names the exact request it reviewed"
    );
    assert_eq!(dispatches(p, setup.claim.fence.scope.run).await, 1);
}

/// F019(a) denial: a rejected allowance never reopens the job, so no provider call or usage.
#[tokio::test]
async fn workflow_agent_run_token_denial_never_enters_the_provider() {
    let setup = Box::pin(agent_setup_source(
        "http://127.0.0.1:1",
        None,
        false,
        tokens(100),
    ))
    .await;
    let llm = scripted_llm(vec![]).await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let runtime = agent_runtime(&setup);
    let p = &setup.fixture.persistence;
    let scope = setup.claim.fence.scope;
    assert!(matches!(
        supervised(&runtime, p, &setup.claim).await,
        SupervisedResult::Suspended(_)
    ));
    let reject = crate::adapters::persistence::workflow::tests::approval_test_support::command(
        p,
        scope,
        setup.fixture.target.actor,
        "reject",
    )
    .await;
    p.submit_decision(reject).await.unwrap();
    due(&setup).await;
    assert!(
        p.claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
            .await
            .unwrap()
            .is_none()
    );
    let state: String = sqlx::query_scalar("SELECT state FROM workflow_runs WHERE id=$1")
        .bind(scope.run.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(state, "failed");
    assert_eq!(ledger(&setup).await, (vec![], 0));
    assert_eq!(llm.finish().await.unwrap(), 0);
}

/// F019(b) provider entry before settlement: the lost request stays charged and unresolved;
/// the restarted attempt never reuses its ID and pays one new request.
#[tokio::test]
async fn workflow_agent_run_restart_after_provider_entry_charges_a_new_request() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let (lost, arrival, release) = held(0);
    let llm = scripted_scenario(vec![
        lost,
        exchange(ScriptedResponse::turn(LlmTurn::text("\"recovered\""), 1)),
    ])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let runtime = agent_runtime(&setup);
    crash_at(&runtime, &setup.claim, arrival).await;
    release.send(()).unwrap();
    let fresh = restart(&setup).await;
    let outcome = Box::pin(runtime.execute(&kind(), &fresh)).await;
    assert_eq!(outcome.completed_result().unwrap(), json!("recovered"));
    assert_eq!(llm.finish().await.unwrap(), 2);
    let (rows, retries) = ledger(&setup).await;
    assert_eq!(rows.len(), 2);
    assert_ne!(rows[0].0, rows[1].0);
    assert_eq!(
        (rows[0].1, rows[1].1),
        (false, true),
        "the lost call stays charged"
    );
    assert_eq!(retries, 1);
}

/// F019(b) reservation before I/O: the provider is never reached, the reservation stays
/// charged, and the retried attempt admits one new separately charged request.
#[tokio::test]
async fn workflow_agent_run_reservation_without_provider_io_retries_with_a_new_request() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let unreachable = scripted_llm(vec![]).await;
    register_scripted_agent_base_url(setup.agent, &unreachable.base_url);
    assert_eq!(unreachable.finish().await.unwrap(), 0);
    let runtime = agent_runtime(&setup);
    let p = &setup.fixture.persistence;
    assert!(matches!(
        supervised(&runtime, p, &setup.claim).await,
        SupervisedResult::Stopped(StopReason::HandlerFailed)
    ));
    let llm = scripted_llm(vec![LlmTurn::text("\"second\"")]).await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    due(&setup).await;
    let retry = reclaim(&setup).await;
    let outcome = Box::pin(runtime.execute(&kind(), &retry)).await;
    assert_eq!(outcome.completed_result().unwrap(), json!("second"));
    assert_eq!(llm.finish().await.unwrap(), 1);
    let (rows, retries) = ledger(&setup).await;
    assert_eq!(rows.len(), 2);
    assert_ne!(rows[0].0, rows[1].0);
    assert_eq!((rows[0].1, rows[1].1), (false, true));
    assert_eq!(retries, 1);
}

/// F019(b) after the receipt: the restarted attempt reuses the saved tool result (one
/// effect), and only the model request that was in flight is repeated under a new ID.
#[tokio::test]
async fn workflow_agent_run_restart_after_receipt_reuses_the_saved_effect() {
    let (origin, server) = server(HTTP_OK).await;
    let setup = Box::pin(agent_setup(&origin)).await;
    let (lost, arrival, release) = held(1);
    let llm = scripted_scenario(vec![
        exchange(ScriptedResponse::turn(
            LlmTurn::tool_call(
                "registered__request",
                json!({"method":"POST","path":"/once"}),
            ),
            0,
        )),
        lost,
        exchange(ScriptedResponse::turn(
            LlmTurn::text("\"after restart\""),
            2,
        )),
    ])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let runtime = agent_runtime(&setup);
    crash_at(&runtime, &setup.claim, arrival).await;
    release.send(()).unwrap();
    let fresh = restart(&setup).await;
    let outcome = Box::pin(runtime.execute(&kind(), &fresh)).await;
    assert_eq!(outcome.completed_result().unwrap(), json!("after restart"));
    assert!(server.await.unwrap().starts_with("POST /once"));
    assert_eq!(llm.finish().await.unwrap(), 3);
    let (rows, retries) = ledger(&setup).await;
    assert_eq!(
        rows.iter().map(|(_, settled)| *settled).collect::<Vec<_>>(),
        vec![true, false, true]
    );
    assert_eq!(retries, 1);
    assert_eq!(
        dispatches(&setup.fixture.persistence, setup.claim.fence.scope.run).await,
        1
    );
}

/// Accepts one request, records it, and closes without answering.
async fn silent() -> (String, tokio::task::JoinHandle<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = vec![0; 8192];
        let read = socket.read(&mut bytes).await.unwrap();
        String::from_utf8(bytes[..read].to_vec()).unwrap()
    });
    (origin, task)
}

async fn reconciliations(p: &PostgresPersistence, scope: ActivationRequest) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM workflow_action_reconciliations WHERE run_id=$1 AND execution_id=$2",
    )
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap()
}

/// F019(g): a sent call that gets no answer fails the agent terminally; because its marker
/// has no receipt, the shared retry predicate hands it to reconciliation instead of a retry.
#[tokio::test]
async fn workflow_agent_run_unanswered_effect_is_terminal_and_reconciled() {
    let (origin, server) = silent().await;
    let setup = Box::pin(agent_setup(&origin)).await;
    let llm = scripted_llm(vec![LlmTurn::tool_call(
        "registered__request",
        json!({"method":"POST","path":"/maybe"}),
    )])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let runtime = agent_runtime(&setup);
    let p = &setup.fixture.persistence;
    let scope = setup.claim.fence.scope;
    let outcome = Box::pin(runtime.execute(&kind(), &setup.claim)).await;
    let failure = outcome.completed_result().unwrap_err().failure;
    assert_eq!(failure.code().as_str(), "agent.rejected");
    assert_eq!(failure.class(), FailureClass::Terminal);
    assert!(server.await.unwrap().starts_with("POST /maybe"));
    assert!(matches!(
        supervised(&runtime, p, &setup.claim).await,
        SupervisedResult::Stopped(StopReason::HandlerFailed)
    ));
    due(&setup).await;
    assert!(
        p.claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
            .await
            .unwrap()
            .is_none(),
        "an unknown effect is never retried blindly"
    );
    assert_eq!(reconciliations(p, scope).await, 1);
    assert_eq!(dispatches(p, scope.run).await, 1);
    assert_eq!(llm.finish().await.unwrap(), 1);
}

/// F019(g) crash while the effect is in flight: a duplicate driver on the same fence finds
/// the marker without a receipt (`PossibleDispatchExists`) and stops with an indeterminate
/// effect; lease expiry then reconciles instead of granting a retry that could repeat it.
#[tokio::test]
async fn workflow_agent_run_crash_during_effect_is_indeterminate_and_reconciled() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let setup = Box::pin(agent_setup(&origin)).await;
    let (arrived, arrival) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = vec![0; 8192];
        let read = socket.read(&mut bytes).await.unwrap();
        arrived.send(()).unwrap();
        // Hold the connection open until the test ends: the effect outcome stays unknown.
        let _ = socket.read(&mut bytes).await;
        String::from_utf8(bytes[..read].to_vec()).unwrap()
    });
    let llm = scripted_llm(vec![LlmTurn::tool_call(
        "registered__request",
        json!({"method":"POST","path":"/inflight"}),
    )])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let runtime = agent_runtime(&setup);
    crash_at(&runtime, &setup.claim, arrival).await;
    let p = &setup.fixture.persistence;
    let scope = setup.claim.fence.scope;
    let duplicate = Box::pin(runtime.execute(&kind(), &setup.claim)).await;
    let failure = duplicate.completed_result().unwrap_err().failure;
    assert_eq!(failure.code().as_str(), "agent.effect_unknown");
    assert_eq!(failure.class(), FailureClass::Terminal);
    sqlx::query("UPDATE background_tasks SET locked_at=clock_timestamp()-interval '10 seconds',lock_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1")
        .bind(scope.job.0)
        .execute(p.pool())
        .await
        .unwrap();
    assert!(p.retire_expired_io(scope, lease()).await.unwrap());
    due(&setup).await;
    assert!(
        p.claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(reconciliations(p, scope).await, 1);
    assert_eq!(dispatches(p, scope.run).await, 1);
    assert_eq!(llm.finish().await.unwrap(), 1);
    server.abort();
}

/// F019(i): a parameter-enum agent selector captures every listed agent at publication and
/// runs exactly the agent the binding selects.
#[tokio::test]
async fn workflow_agent_run_dynamic_selection_runs_the_bound_agent() {
    let mut fixture = Fixture::new().await;
    fixture.persistence = PostgresPersistence::with_credential_cipher(
        fixture.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    let p = fixture.persistence.clone();
    let company = fixture.target.company;
    let mut agents = Vec::new();
    for (slug, prompt) in [
        ("sales", "You sell."),
        ("support", "You support customers."),
    ] {
        let agent = AgentPersistence::create(
            &p,
            company.as_uuid(),
            AgentWrite {
                name: slug.into(),
                slug: slug.into(),
                provider: Some(SCRIPTED_PROVIDER.into()),
                model: Some(SCRIPTED_MODEL.into()),
                system_prompt: Some(prompt.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        agents.push(agent.id);
    }
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
            inner: http::HttpPublicationDirectory {
                persistence: p.clone(),
                selections: vec![],
            },
        },
    );
    let mut source: Value =
        serde_json::from_str(&registry::example("agent.run").unwrap().source).unwrap();
    source["parameter_schema"] = json!({"type":"object","properties":{"agent":{"type":"string",
        "enum":["sales","support"]}},"required":["agent"],"additionalProperties":false});
    source["steps"]["start"]["with"]["agent"] = json!({"ref":"/params/agent"});
    let mut draft = fixture.save(None);
    draft.content.source = source.to_string();
    definitions.save(draft).await.unwrap();
    let publication = fixture.publish();
    definitions.publish(publication.clone()).await.unwrap();
    let frozen: Vec<String> = sqlx::query_scalar("SELECT DISTINCT agent->>'key' FROM workflow_versions AS version, jsonb_array_elements((convert_from(version.bundle,'UTF8')::jsonb)->'versions') AS stored, jsonb_array_elements(stored->'snapshots'->'agents') AS agent WHERE version.id=$1 ORDER BY 1")
        .bind(publication.version.as_uuid())
        .fetch_all(p.pool())
        .await
        .unwrap();
    assert_eq!(
        frozen,
        vec!["sales", "support"],
        "every selectable agent is captured"
    );
    let target = BindingTarget {
        company,
        actor: fixture.target.actor,
        binding: WorkflowBindingId::new(Uuid::new_v4()),
    };
    let bindings = BindingService::new(Preflight, p.clone(), p.clone(), p.clone());
    let state = bindings
        .configure(ConfigureBinding {
            target,
            expected: None,
            association: RelatedAssociation::Company,
            version: publication.version,
            params: json!({"agent":"support"}),
            resources: BTreeMap::new(),
            run_as: None,
            message_trigger: None,
        })
        .await
        .unwrap();
    bindings
        .activate(SetBindingActivity {
            target,
            expected: state.revision,
        })
        .await
        .unwrap();
    let claim = Box::pin(claimed(&fixture, target, RelatedAssociation::Company)).await;
    let mut llm = scripted_llm(vec![LlmTurn::text("\"supported\"")]).await;
    // Only the selected agent has a provider endpoint; selecting `sales` would fail.
    register_scripted_agent_base_url(agents[1], &llm.base_url);
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
            Arc::new(MessageRuntime::new(
                p.clone(),
                composer(&p),
                "agents.test".into(),
                lease(),
            )),
        ),
        Some(notices(&p)),
        lease(),
    );
    let outcome = Box::pin(runtime.execute(&kind(), &claim)).await;
    assert_eq!(outcome.completed_result().unwrap(), json!("supported"));
    assert!(
        llm.observed()[0]["messages"]
            .to_string()
            .contains("You support customers.")
    );
    assert_eq!(llm.finish().await.unwrap(), 1);
    let charged: Uuid =
        sqlx::query_scalar("SELECT agent_id FROM model_usage_calls WHERE run_id=$1")
            .bind(claim.fence.scope.run.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(charged, agents[1]);
}

/// F019(e): output that stays invalid after the bounded repairs ends the step terminally
/// through the workflow binding; every repair call is charged and none carries tools.
#[tokio::test]
async fn workflow_agent_run_invalid_output_after_repairs_is_terminal() {
    let setup = Box::pin(agent_setup("http://127.0.0.1:1")).await;
    let calls = 1 + crate::services::response_contract::MAX_REPAIR_CALLS;
    let mut llm = scripted_llm((0..calls).map(|_| LlmTurn::text("not json")).collect()).await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let outcome = Box::pin(agent_runtime(&setup).execute(&kind(), &setup.claim)).await;
    let failure = outcome.completed_result().unwrap_err().failure;
    assert_eq!(failure.code().as_str(), "agent.invalid_output");
    assert_eq!(failure.class(), FailureClass::Terminal);
    let requests = llm.observed();
    assert!(requests[1..].iter().all(|request| {
        request
            .get("tools")
            .is_none_or(|tools| tools.as_array().is_none_or(Vec::is_empty))
    }));
    assert_eq!(llm.finish().await.unwrap(), calls);
    let (rows, _) = ledger(&setup).await;
    assert_eq!(rows.len(), calls);
    assert!(rows.iter().all(|(_, settled)| *settled));
}
