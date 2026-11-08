use super::*;
use crate::adapters::persistence::workflow::{
    agent_history_race_fixture::{OwnerGate, Target},
    agent_history_retention_fixture as clock,
};
use crate::application::workflow::{
    completion::WorkflowCompletion, storage::WorkflowStorageMaintenance,
};
use crate::domain::workflow::{FailureClass, FailureCode, RetrySafety, StepFailure};
use crate::services::test_support::ScriptedLlm;

async fn accepted(f: &ManualHistory, claim: &ClaimedWorkflow) -> Value {
    sqlx::query_scalar("SELECT to_jsonb(exchange) FROM agent_history_exchanges AS exchange WHERE company_id=$1 AND id=$2")
        .bind(claim.fence.scope.company.as_uuid()).bind(claim.fence.scope.execution.as_uuid())
        .fetch_one(f.f.s.pool()).await.unwrap()
}

async fn execute(f: &ManualHistory, claim: &ClaimedWorkflow, expected: Value) {
    let result =
        Box::pin(runtime(&f.f).execute(&WorkflowStepKind("agent.run".into()), claim)).await;
    assert_eq!(result.completed_result().unwrap(), expected);
}

async fn complete_job(f: &ManualHistory, claim: &ClaimedWorkflow, output: Value) {
    f.f.s
        .p()
        .complete_io(FencedWorkflowResult {
            fence: claim.fence,
            output,
            policy: policy(),
        })
        .await
        .unwrap()
        .expect("genuine completed source job");
}

async fn fail_job(f: &ManualHistory, claim: &ClaimedWorkflow) {
    let cause = LeaseReleaseCause::Classified(WorkflowFailure {
        failure: StepFailure::new(
            FailureClass::Terminal,
            FailureCode::parse("provider.rejected").unwrap(),
            None,
        )
        .unwrap(),
        safety: RetrySafety::SafeToRetry,
    });
    assert!(
        f.f.s
            .p()
            .release_io(claim.fence, policy(), cause)
            .await
            .unwrap()
    );
}

async fn newer_completed_jobs(f: &mut ManualHistory) -> ScriptedLlm {
    let recent: i64 = sqlx::query_scalar("SELECT recent_payloads FROM agent_history_limits()")
        .fetch_one(f.f.s.pool())
        .await
        .unwrap();
    assert_eq!(
        recent, 64,
        "keep the actual recent-window pressure boundary"
    );
    for batch in 0..4 {
        let llm = scripted_llm(
            (0..16)
                .map(|_| LlmTurn::text("\"newer accepted\""))
                .collect(),
        )
        .await;
        register_scripted_agent_base_url(f.f.agent, &llm.base_url);
        for item in 0..16 {
            let claim = f.start(json!({"batch":batch,"item":item})).await;
            execute(f, &claim, json!("newer accepted")).await;
            complete_job(f, &claim, json!("newer accepted")).await;
        }
        if batch == 3 {
            return llm;
        }
        assert_eq!(llm.finish().await.unwrap(), 16);
    }
    unreachable!("four fixed batches fill the native64 recent bound")
}

async fn retry_command(f: &ManualHistory, claim: &ClaimedWorkflow) -> RetryCommand {
    let scope = claim.fence.scope;
    RetryCommand {
        company_id: scope.company,
        run_id: scope.run,
        actor: f.actor,
        command_key: IdempotencyKey::parse("accepted-history-pressure-retry").unwrap(),
        expected_revision: f
            .f
            .s
            .p()
            .head(scope.company, scope.run)
            .await
            .unwrap()
            .unwrap()
            .revision,
    }
}

async fn assert_native_pin(f: &ManualHistory, first: &ClaimedWorkflow) {
    let p = f.f.s.p();
    let error = sqlx::query("UPDATE agent_history_exchanges SET payload_retained=false,payload_expired_at=clock_timestamp(),output=NULL,frozen_preparation=NULL WHERE company_id=$1 AND id=$2")
        .bind(first.fence.scope.company.as_uuid()).bind(first.fence.scope.execution.as_uuid())
        .execute(p.pool()).await.unwrap_err();
    let database = error.as_database_error().unwrap();
    assert_eq!(database.code().as_deref(), Some("23514"));
    assert_eq!(
        database.message(),
        "agent payload is pinned or expiry changed metadata"
    );
}

async fn billed_calls(f: &ManualHistory) -> Value {
    sqlx::query_scalar("SELECT COALESCE(jsonb_agg(to_jsonb(call) ORDER BY request_id),'[]'::jsonb) FROM model_usage_calls AS call WHERE company_id=$1")
        .bind(f.f.s.fixture.target.company.as_uuid()).fetch_one(f.f.s.pool()).await.unwrap()
}

#[tokio::test]
async fn agent_history_retention_completed_output_failed_job_pressure_competes_with_native_retry_without_model_reentry()
 {
    let mut f = Box::pin(ManualHistory::new()).await;
    let initial = scripted_llm(vec![LlmTurn::text("\"original accepted\"")]).await;
    register_scripted_agent_base_url(f.f.agent, &initial.base_url);
    let first = f.start(json!("original input")).await;
    execute(&f, &first, json!("original accepted")).await;
    let saved = accepted(&f, &first).await;
    fail_job(&f, &first).await;
    assert_eq!(initial.finish().await.unwrap(), 1);
    let last_provider = newer_completed_jobs(&mut f).await;
    let p = f.f.s.p();
    let candidate = clock::candidate(p, first.fence.scope).await;
    assert!(
        !p.expire_history(candidate).await.unwrap(),
        "eligibility itself must preserve completed replay before deadline"
    );
    assert_native_pin(&f, &first).await;
    assert_eq!(accepted(&f, &first).await, saved);
    let usage_before = billed_calls(&f).await;
    let command = retry_command(&f, &first).await;
    let gate = OwnerGate::install(p.pool(), Target::Retry(first.fence.scope.job.0)).await;
    let owner = p.retry(command);
    let cleanup = async {
        gate.arrived(first.fence.scope.company.as_uuid()).await;
        assert!(!p.expire_history(candidate).await.unwrap());
        gate.release().await;
    };
    let (retried, ()) = tokio::join!(owner, cleanup);
    assert!(matches!(retried.unwrap(), RetryResult::Applied { .. }));
    let replacement = p
        .claim_io(
            first.fence.scope,
            WorkflowWorkerId(Uuid::new_v4()),
            policy(),
        )
        .await
        .unwrap()
        .unwrap();
    execute(&f, &replacement, json!("original accepted")).await;
    assert_eq!(
        accepted(&f, &first).await,
        saved,
        "accepted exchange/order/preparation/output stay immutable"
    );
    let calls: i64 =
        sqlx::query_scalar("SELECT count(*) FROM model_usage_calls WHERE company_id=$1")
            .bind(first.fence.scope.company.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(calls, 65);
    assert_eq!(
        billed_calls(&f).await,
        usage_before,
        "native replay changes no charged-call identity or accounting"
    );
    assert_eq!(
        last_provider.finish().await.unwrap(),
        16,
        "replay must not enter the strict live provider"
    );
    complete_job(&f, &replacement, json!("original accepted")).await;
    assert!(
        p.expire_history(candidate).await.unwrap(),
        "real job completion removes replay pin under unchanged64-newer pressure"
    );
    let expired = accepted(&f, &first).await;
    assert_eq!(expired["state"], "completed");
    assert_eq!(expired["position"], saved["position"]);
    assert_eq!(expired["output_digest"], saved["output_digest"]);
    assert_eq!(expired["payload_retained"], false);
}
