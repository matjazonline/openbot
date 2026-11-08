use super::*;
use process::{ClaimMark, StatementGate, WorkerProcess};

pub(super) async fn exchange(p: &PostgresPersistence, scope: ActivationRequest) -> Option<Value> {
    sqlx::query_scalar("SELECT to_jsonb(exchange) FROM agent_history_exchanges AS exchange WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.execution.as_uuid()).fetch_optional(p.pool()).await.unwrap()
}
pub(super) async fn billed(p: &PostgresPersistence, scope: ActivationRequest) -> Value {
    sqlx::query_scalar("SELECT COALESCE(jsonb_agg(to_jsonb(call) ORDER BY request_id),'[]'::jsonb) FROM model_usage_calls AS call WHERE company_id=$1")
        .bind(scope.company.as_uuid()).fetch_one(p.pool()).await.unwrap()
}
pub(super) fn spec(
    f: &ManualHistory,
    scope: ActivationRequest,
    llm: &crate::services::test_support::ScriptedLlm,
    point: CrashPoint,
) -> ProcessSpec {
    ProcessSpec {
        database: OwnedDatabaseName(
            f.f.s
                .pool()
                .connect_options()
                .get_database()
                .unwrap()
                .into(),
        ),
        endpoint: FixtureEndpoint(llm.base_url.clone()),
        agent: f.f.agent,
        company: scope.company.as_uuid(),
        run: scope.run.as_uuid(),
        execution: scope.execution.as_uuid(),
        job: scope.job.0,
        point,
        listener: llm.inherited_listener(),
    }
}
async fn check_frontier(
    p: &PostgresPersistence,
    scope: ActivationRequest,
    point: CrashPoint,
) -> Option<Value> {
    let saved = exchange(p, scope).await;
    match point {
        CrashPoint::Acquisition => assert!(
            saved.is_none(),
            "in-flight turn/queue transaction is not a durable freeze"
        ),
        CrashPoint::Frozen => {
            let row = saved.as_ref().unwrap();
            assert_eq!(row["state"], "queued");
            assert!(row["frozen_preparation"].is_object() && row["checkpoint_id"].is_null());
            let owns:bool=sqlx::query_scalar("SELECT conversation.active_exchange_id=exchange.id FROM agent_history_exchanges AS exchange JOIN agent_history_conversations AS conversation ON conversation.company_id=exchange.company_id AND conversation.id=exchange.conversation_id WHERE exchange.company_id=$1 AND exchange.id=$2")
                .bind(scope.company.as_uuid()).bind(scope.execution.as_uuid()).fetch_one(p.pool()).await.unwrap();
            assert!(
                owns,
                "durable frozen preparation owns the turn before checkpoint activation"
            );
        }
        CrashPoint::ModelSettled => {
            let row = saved.as_ref().unwrap();
            assert_eq!(row["state"], "active");
            assert!(row["output"].is_null() && row["checkpoint_id"].is_string());
            assert_eq!(
                billed(p, scope).await.as_array().unwrap().len(),
                2,
                "provider settlement committed before final-output transaction"
            );
        }
        CrashPoint::FinalCheckpoint => {
            assert_eq!(saved.as_ref().unwrap()["output"], "P07_TARGET_OUTPUT")
        }
        _ => panic!("separate durable queue/wakeup journey"),
    }
    saved
}
async fn assert_reconstructed(
    f: &ManualHistory,
    scope: ActivationRequest,
    saved: Option<Value>,
    point: CrashPoint,
) {
    let p = f.f.s.p();
    let row = exchange(p, scope).await.unwrap();
    assert_eq!(row["id"], json!(scope.execution.as_uuid()));
    assert_eq!(row["position"], 2);
    assert_eq!(row["state"], "completed");
    assert_eq!(row["output"], "P07_TARGET_OUTPUT");
    assert_eq!(
        row["frozen_preparation"]["history"]["seed"],
        json!([{"input":"P07_PRIOR_INPUT","output":"P07_PRIOR_OUTPUT"}])
    );
    if let Some(before) = saved {
        assert_eq!(row["frozen_preparation"], before["frozen_preparation"]);
        if point == CrashPoint::FinalCheckpoint {
            assert_eq!(row, before);
        }
    }
    let input: Value = sqlx::query_scalar(
        "SELECT frozen_inputs FROM workflow_executions WHERE company_id=$1 AND id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.execution.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert!(
        !input.to_string().contains("P07_PRIOR_OUTPUT"),
        "original logical inputs never acquire recursive seed copies"
    );
    assert_eq!(billed(p, scope).await.as_array().unwrap().len(), 2);
}
async fn crash_case(point: CrashPoint) {
    let mut f = Box::pin(ManualHistory::new()).await;
    let mut llm = scripted_llm(vec![
        LlmTurn::text("\"P07_PRIOR_OUTPUT\""),
        LlmTurn::text("\"P07_TARGET_OUTPUT\""),
    ])
    .await;
    register_scripted_agent_base_url(f.f.agent, &llm.base_url);
    f.complete_ordinary(json!("P07_PRIOR_INPUT"), json!("P07_PRIOR_OUTPUT"))
        .await;
    let scope = pending(&f, json!("P07_CURRENT_INPUT")).await;
    let config = spec(&f, scope, &llm, point);
    let gate = StatementGate::install(f.f.s.pool(), &config).await;
    let mut child = WorkerProcess::spawn(&config);
    let mark: ClaimMark = serde_json::from_value(child.mark("AH_PROCESS_CLAIM").await).unwrap();
    if let Some(gate) = &gate {
        gate.arrived(scope.company).await;
    } else {
        child.mark("AH_PROCESS_READY").await;
    }
    let saved = check_frontier(f.f.s.p(), scope, point).await;
    let charged = billed(f.f.s.p(), scope).await;
    child.kill();
    if let Some(gate) = gate {
        gate.release().await;
    }
    let reconstructed = PostgresPersistence::with_credential_cipher(
        f.f.s.pool().clone(),
        CredentialCipher::for_test(),
    );
    drive_reconstructed(&reconstructed, scope).await;
    assert!(
        !reconstructed
            .validate_io(mark.fence(scope), crash_policy())
            .await
            .unwrap()
    );
    assert_reconstructed(&f, scope, saved, point).await;
    if matches!(
        point,
        CrashPoint::ModelSettled | CrashPoint::FinalCheckpoint
    ) {
        assert_eq!(billed(&reconstructed, scope).await, charged);
    }
    let requests = llm.observed();
    assert_eq!(requests.len(), 2);
    let actual = requests[1].to_string();
    for marker in ["P07_PRIOR_INPUT", "P07_PRIOR_OUTPUT", "P07_CURRENT_INPUT"] {
        assert_eq!(actual.matches(marker).count(), 1);
    }
    assert_eq!(llm.finish().await.unwrap(), 2);
    eprintln!(
        "AH-P07 genuine_process_crash point={point:?} durable_identity=true exact_context=true provider_calls=2"
    );
}

#[tokio::test]
async fn agent_history_e2e_process_crashes_reconstruct_durable_frontiers_without_duplicate_calls() {
    for point in [
        CrashPoint::Acquisition,
        CrashPoint::Frozen,
        CrashPoint::ModelSettled,
        CrashPoint::FinalCheckpoint,
    ] {
        Box::pin(crash_case(point)).await;
    }
}
