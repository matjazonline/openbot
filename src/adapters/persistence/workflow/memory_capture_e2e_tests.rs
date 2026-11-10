//! Capture through the production agent runtime and checkpoint acceptance (phase 05 E8, U3):
//! a completed private-session exchange of the creator's own manual run becomes one generation
//! intent in that person's memory, carrying only the logical input and accepted output; a
//! switched-on capture target is provisioned at preparation when it is not ready yet.
use super::*;
use crate::application::memory::{
    CaptureLimits, MemoryCaptureChange, MemoryCaptureKind, MemoryDeliveryWorker,
    MemorySettingsStore,
};

/// An agent without memory tools or recall, in a company with memory enabled, generation allowed
/// by the operator, and the agent's private-session capture switched on by an owner.
async fn private_capture() -> Tools {
    let t = Box::pin(Tools::with_tools(AccessLevel::L5, &[])).await;
    let owner = t.s.fixture.target.actor;
    t.p()
        .enable_memory(t.company(), owner, 0, PROVISIONER)
        .await
        .unwrap();
    sqlx::query("UPDATE memory_tenants SET generation_cap = 100000 WHERE company_id = $1")
        .bind(t.company().as_uuid())
        .execute(t.p().pool())
        .await
        .unwrap();
    t.fake.state.lock().unwrap().generation_cap = 100_000;
    t.p()
        .set_memory_capture(
            t.company(),
            owner,
            &MemoryCaptureChange {
                agent: t.s.agent,
                kind: MemoryCaptureKind::PrivateSessions,
                enabled: true,
                limits: CaptureLimits::DEFAULT,
                reason: "Remember my own sessions".into(),
                expected_revision: 0,
            },
        )
        .await
        .unwrap();
    t
}

/// The production runtime with the configured recall (which also requests capture targets).
fn capturing_runtime(t: &Tools) -> WorkflowAgentRuntime<PostgresPersistence, AgentActionTools> {
    let mut runtime = t.s.runtime();
    runtime.set_recall(recall(t));
    runtime
}

async fn generation_intents(t: &Tools) -> Vec<(String, Uuid, Value)> {
    sqlx::query_as(
        "SELECT idempotency_key, namespace_id, request FROM memory_deliveries \
         WHERE company_id = $1 AND kind = 'generation' ORDER BY created_at",
    )
    .bind(t.company().as_uuid())
    .fetch_all(t.p().pool())
    .await
    .unwrap()
}

/// E8: the creator's own manual run in an unattached private session, accepted, becomes one
/// generation intent in their user memory: the frozen logical input and the accepted output at
/// the run's levels, nothing from the tool round in between, sent once by the delivery worker.
#[tokio::test]
async fn memory_capture_an_accepted_private_session_exchange_is_captured_for_its_creator() {
    let t = private_capture().await;
    let user = t.actor().await;
    let namespace = t.ready(NamespaceOwner::User(user)).await;
    let llm = scripted_llm(vec![
        LlmTurn::tool_call(SEARCH_KNOWLEDGE, json!({"query":"TOOL-ARGUMENT-MARKER"})),
        LlmTurn::text("\"CAPTURED-ANSWER\""),
    ])
    .await;
    register_scripted_agent_base_url(t.s.agent, &llm.base_url);
    let outcome =
        Box::pin(capturing_runtime(&t).execute(&WorkflowStepKind("agent.run".into()), &t.s.claim))
            .await;
    assert_eq!(
        outcome.completed_result().unwrap(),
        json!("CAPTURED-ANSWER")
    );
    assert_eq!(llm.finish().await.unwrap(), 2);
    let scope = t.s.claim.fence.scope;
    let (exchange, context): (Uuid, Value) = sqlx::query_as(
        "SELECT id, frozen_preparation->'context' FROM agent_history_exchanges \
         WHERE company_id=$1 AND run_id=$2 AND execution_id=$3",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .fetch_one(t.p().pool())
    .await
    .unwrap();
    let (ceiling, content, exact): (i16, i16, bool) = sqlx::query_as(
        "SELECT knowledge_ceiling::smallint, content_level::smallint, content_exact \
         FROM workflow_runs WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_one(t.p().pool())
    .await
    .unwrap();
    let intents = generation_intents(&t).await;
    assert_eq!(intents.len(), 1);
    let (key, target, request) = &intents[0];
    assert_eq!(key, &format!("ob.g.{}", exchange.simple()));
    assert_eq!(*target, namespace, "the creator's own memory");
    let armed: Value = sqlx::query_scalar(
        "SELECT frozen_preparation->'capture' FROM agent_history_exchanges WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(exchange)
    .fetch_one(t.p().pool())
    .await
    .unwrap();
    assert_eq!(armed, json!(true), "armed by the configured runtime (U7)");
    let body = &request["body"];
    assert_eq!(body["producing_ceiling"], json!(ceiling));
    let consumed = if exact {
        json!({"kind":"exact","level":content})
    } else {
        json!({"kind":"unknown"})
    };
    assert_eq!(body["consumed"], consumed);
    let events = body["events"].as_array().unwrap();
    assert_eq!(events.len(), 2);
    let input: Value = serde_json::from_str(events[0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(input, context, "the exchange's logical input");
    assert_eq!(events[1]["text"], json!("\"CAPTURED-ANSWER\""));
    for event in events {
        assert_eq!(event["access_level"], json!(content));
    }
    let sent = body.to_string();
    assert!(
        !sent.contains("TOOL-ARGUMENT-MARKER"),
        "no tool arguments or results"
    );
    let worker = MemoryDeliveryWorker::new(Arc::new(t.p().clone()), t.client.clone());
    let stop = tokio_util::sync::CancellationToken::new();
    assert!(worker.pass(&stop).await.unwrap());
    assert!(!worker.pass(&stop).await.unwrap());
    let state = t.fake.state.lock().unwrap();
    assert_eq!(state.generations.len(), 1);
    assert_eq!(&state.generations[key].1, body);
}

/// E8: with the switch on but the creator's memory not created yet, preparation requests it (own
/// transaction) and acceptance captures nothing, rather than an intent that could never deliver.
#[tokio::test]
async fn memory_capture_a_switched_on_target_is_provisioned_at_preparation() {
    let t = private_capture().await;
    let user = t.actor().await;
    assert!(t.namespace(NamespaceOwner::User(user)).await.is_none());
    let llm = scripted_llm(vec![LlmTurn::text("\"done\"")]).await;
    register_scripted_agent_base_url(t.s.agent, &llm.base_url);
    let outcome =
        Box::pin(capturing_runtime(&t).execute(&WorkflowStepKind("agent.run".into()), &t.s.claim))
            .await;
    assert_eq!(outcome.completed_result().unwrap(), json!("done"));
    assert_eq!(llm.finish().await.unwrap(), 1);
    let (_, state) = t
        .namespace(NamespaceOwner::User(user))
        .await
        .expect("requested at preparation");
    assert_eq!(state, "provisioning");
    assert!(generation_intents(&t).await.is_empty());
    assert!(
        t.namespace(NamespaceOwner::Agent(t.s.agent))
            .await
            .is_none(),
        "only the switched-on kind's target"
    );
}

/// E8 (U3): without a switch for private sessions nothing is captured or provisioned.
#[tokio::test]
async fn memory_capture_an_agent_without_the_switch_captures_nothing() {
    let t = Box::pin(Tools::with_tools(AccessLevel::L5, &[])).await;
    let owner = t.s.fixture.target.actor;
    t.p()
        .enable_memory(t.company(), owner, 0, PROVISIONER)
        .await
        .unwrap();
    sqlx::query("UPDATE memory_tenants SET generation_cap = 100000 WHERE company_id = $1")
        .bind(t.company().as_uuid())
        .execute(t.p().pool())
        .await
        .unwrap();
    let user = t.actor().await;
    t.ready(NamespaceOwner::User(user)).await;
    let llm = scripted_llm(vec![LlmTurn::text("\"done\"")]).await;
    register_scripted_agent_base_url(t.s.agent, &llm.base_url);
    let outcome =
        Box::pin(capturing_runtime(&t).execute(&WorkflowStepKind("agent.run".into()), &t.s.claim))
            .await;
    assert_eq!(outcome.completed_result().unwrap(), json!("done"));
    assert_eq!(llm.finish().await.unwrap(), 1);
    assert!(generation_intents(&t).await.is_empty());
}

/// U7 (review MEM-P05-B5-F2): on a runtime without the memory service configured nothing is
/// captured, even with memory enabled, the switch on, the operator's cap above 0 and the target
/// namespace ready: the preparation is frozen without the capture flag, and acceptance checks it.
#[tokio::test]
async fn memory_capture_nothing_is_captured_without_a_configured_service() {
    let t = private_capture().await;
    let user = t.actor().await;
    t.ready(NamespaceOwner::User(user)).await;
    let llm = scripted_llm(vec![LlmTurn::text("\"done\"")]).await;
    register_scripted_agent_base_url(t.s.agent, &llm.base_url);
    // The runtime's own default guard: no memory service composed.
    let outcome = Box::pin(
        t.s.runtime()
            .execute(&WorkflowStepKind("agent.run".into()), &t.s.claim),
    )
    .await;
    assert_eq!(outcome.completed_result().unwrap(), json!("done"));
    assert_eq!(llm.finish().await.unwrap(), 1);
    let scope = t.s.claim.fence.scope;
    let frozen: Value = sqlx::query_scalar(
        "SELECT frozen_preparation FROM agent_history_exchanges \
         WHERE company_id=$1 AND run_id=$2 AND execution_id=$3",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .fetch_one(t.p().pool())
    .await
    .unwrap();
    assert!(frozen.get("capture").is_none(), "not armed");
    assert!(generation_intents(&t).await.is_empty());
}

/// The tracing output of one test (scoped to its thread; the runtime is current-thread, and
/// the interest cache is rebuilt so callsites another test registered first still reach it).
#[derive(Clone, Default)]
struct Captured(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Captured {
    type Writer = Captured;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// p06 A2-2..A2-4: a capture and its delivery carry ids, never content. The positive control
/// proves the acceptance and delivery lines were captured, with the intent key as correlation
/// id; the service saw the same id; the delivery outcome was counted; and the exchange's
/// marker text appears in no log line.
#[tokio::test(flavor = "current_thread")]
async fn memory_operations_capture_and_delivery_logs_carry_ids_not_content() {
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(captured.clone())
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    tracing::callsite::rebuild_interest_cache();
    let t = private_capture().await;
    let user = t.actor().await;
    t.ready(NamespaceOwner::User(user)).await;
    let llm = scripted_llm(vec![LlmTurn::text("\"HYGIENE-OB-MARKER answer\"")]).await;
    register_scripted_agent_base_url(t.s.agent, &llm.base_url);
    let outcome =
        Box::pin(capturing_runtime(&t).execute(&WorkflowStepKind("agent.run".into()), &t.s.claim))
            .await;
    assert!(outcome.completed_result().is_ok());
    llm.finish().await.unwrap();
    let intents = generation_intents(&t).await;
    let (key, _, _) = &intents[0];
    let monitor = Arc::new(crate::adapters::monitoring::in_memory_monitor::InMemoryMonitor::new());
    let worker = MemoryDeliveryWorker::new(Arc::new(t.p().clone()), t.client.clone())
        .with_monitoring(monitor.clone());
    assert!(
        worker
            .pass(&tokio_util::sync::CancellationToken::new())
            .await
            .unwrap()
    );
    assert!(
        t.fake
            .correlations()
            .contains(&("generations/request".to_owned(), Some(key.clone()))),
        "{:?}",
        t.fake.correlations()
    );
    use crate::domain::monitoring::MonitoringService;
    assert_eq!(
        monitor.get_stats_json()["custom_counters"]["memory_deliveries_settled_total{kind=generation,state=delivered}"],
        json!(1)
    );
    let logs = String::from_utf8_lossy(&captured.0.lock().unwrap()).into_owned();
    for needle in [
        "Memory capture decided at acceptance",
        "Memory delivery pass",
    ] {
        assert!(logs.contains(needle), "captured {needle}:\n{logs}");
    }
    assert!(
        logs.lines()
            .any(|line| line.contains("Memory delivery pass") && line.contains(key.as_str())),
        "the delivery line carries its correlation id:\n{logs}"
    );
    assert!(
        !logs.contains("HYGIENE-OB-MARKER"),
        "content leaked:\n{logs}"
    );
}
