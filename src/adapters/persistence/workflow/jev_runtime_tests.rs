use super::jev_tests::{admit, bound, bound_source, claim, create, fixture};
use super::*;
use crate::adapters::workflow_jev::{
    JevHttp,
    tests::{output, server},
};
use crate::application::workflow::completion::WorkflowCompletion;
use crate::application::workflow::{jev::*, lease::*, polling::*, worker::*};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
fn policy() -> LeasePolicy {
    LeasePolicy::new(Duration::from_secs(30)).unwrap()
}
async fn counts(p: &PostgresPersistence, claim: &ClaimedWorkflow) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('calls',usage.model_calls,'reserved',(SELECT count(*) FROM model_usage_calls WHERE company_id=$1 AND run_id=$2),'settled',(SELECT count(*) FROM model_usage_calls WHERE company_id=$1 AND run_id=$2 AND settled_at IS NOT NULL),'outputs',(SELECT count(*) FROM workflow_jev_invocations WHERE company_id=$1 AND run_id=$2 AND output IS NOT NULL)) FROM workflow_root_budget_usage AS usage JOIN workflow_run_budgets AS link USING(company_id,root_run_id) WHERE link.company_id=$1 AND link.run_id=$2")
        .bind(claim.fence.scope.company.as_uuid()).bind(claim.fence.scope.run.as_uuid()).fetch_one(p.pool()).await.unwrap()
}
pub(super) async fn expire_reclaim(
    p: &PostgresPersistence,
    claim: &ClaimedWorkflow,
) -> ClaimedWorkflow {
    sqlx::query("UPDATE background_tasks SET locked_at=clock_timestamp()-interval '40 seconds',lock_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1").bind(claim.fence.scope.job.0).execute(p.pool()).await.unwrap();
    assert!(
        p.retire_expired_io(claim.fence.scope, policy())
            .await
            .unwrap()
    );
    assert!(
        p.claim_io(
            claim.fence.scope,
            WorkflowWorkerId(Uuid::new_v4()),
            policy()
        )
        .await
        .unwrap()
        .is_none()
    );
    sqlx::query(
        "UPDATE background_tasks SET run_at=clock_timestamp()-interval '1 second' WHERE id=$1",
    )
    .bind(claim.fence.scope.job.0)
    .execute(p.pool())
    .await
    .unwrap();
    p.claim_io(
        claim.fence.scope,
        WorkflowWorkerId(Uuid::new_v4()),
        policy(),
    )
    .await
    .unwrap()
    .unwrap()
}
async fn limited_source(f: &Fixture, c: &JevConnection, model_calls: u32) -> BindingTarget {
    let mut source: Value =
        serde_json::from_str(&registry::example("jev.evaluate").unwrap().source).unwrap();
    source["limits"]["root_budget"] =
        json!({"activations":10,"model_calls":model_calls,"repetitions":10});
    bound_source(f, c, source).await
}
#[tokio::test]
async fn jev_runtime_competing_claimants_admissions_and_stale_settlement() {
    let f = fixture().await;
    let c = create(&f).await;
    let target = bound(&f, &c).await;
    let AdmissionResult::Created(run) = admit(&f, target, "compete").await.unwrap() else {
        panic!()
    };
    let (execution,job):(Uuid,Uuid)=sqlx::query_as("SELECT execution.id,job.id FROM workflow_executions AS execution JOIN background_tasks AS job ON job.workflow_execution_id=execution.id WHERE execution.run_id=$1").bind(run.as_uuid()).fetch_one(f.persistence.pool()).await.unwrap();
    let scope = crate::application::workflow::activation::ActivationRequest {
        company: target.company,
        run,
        execution: ExecutionId::new(execution),
        job: crate::application::workflow::activation::WorkflowJobId(job),
    };
    let (a, b) = tokio::join!(
        f.persistence
            .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), policy()),
        f.persistence
            .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), policy())
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(usize::from(a.is_some()) + usize::from(b.is_some()), 1);
    let first = a.or(b).unwrap();
    let request = JevRequest::prepare(&first.activation.inputs).unwrap();
    let (a, b) = tokio::join!(
        f.persistence.admit_jev(&first, &request, policy()),
        f.persistence.admit_jev(&first, &request, policy())
    );
    let grant = match (a.unwrap(), b.unwrap()) {
        (JevAdmissionResult::New(grant), JevAdmissionResult::AlreadyAdmitted)
        | (JevAdmissionResult::AlreadyAdmitted, JevAdmissionResult::New(grant)) => grant,
        _ => panic!("one fresh grant"),
    };
    assert_eq!(
        counts(&f.persistence, &first).await,
        json!({"calls":1,"reserved":1,"settled":0,"outputs":0})
    );
    let second = expire_reclaim(&f.persistence, &first).await;
    let response = request.validate_response(output()).unwrap();
    assert!(
        f.persistence
            .settle_jev(
                &first,
                &grant,
                &request,
                &response,
                Duration::ZERO,
                policy()
            )
            .await
            .is_err()
    );
    let JevAdmissionResult::New(next) = f
        .persistence
        .admit_jev(&second, &request, policy())
        .await
        .unwrap()
    else {
        panic!()
    };
    f.persistence
        .settle_jev(
            &second,
            &next,
            &request,
            &response,
            Duration::ZERO,
            policy(),
        )
        .await
        .unwrap();
    assert_eq!(
        counts(&f.persistence, &second).await,
        json!({"calls":2,"reserved":2,"settled":1,"outputs":1})
    );
}
#[tokio::test]
async fn jev_runtime_crash_after_response_reconstruction_and_budget_bound() {
    let f = fixture().await;
    let c = create(&f).await;
    let target = limited_source(&f, &c, 2).await;
    let first = claim(&f, target).await;
    let request = JevRequest::prepare(&first.activation.inputs).unwrap();
    let JevAdmissionResult::New(_grant) = f
        .persistence
        .admit_jev(&first, &request, policy())
        .await
        .unwrap()
    else {
        panic!()
    };
    let (http, server) = server(
        200,
        "",
        serde_json::to_vec(&output()).unwrap(),
        Duration::ZERO,
    )
    .await;
    let key = f.persistence.authorize_jev(&first).await.unwrap();
    let _lost = http
        .evaluate(&request, &key, first.window.run_deadline)
        .await
        .unwrap();
    server.await.unwrap();
    let second = expire_reclaim(&f.persistence, &first).await;
    let JevAdmissionResult::New(_second) = f
        .persistence
        .admit_jev(&second, &request, policy())
        .await
        .unwrap()
    else {
        panic!()
    };
    let third = expire_reclaim(&f.persistence, &second).await;
    assert!(matches!(
        f.persistence
            .admit_jev(&third, &request, policy())
            .await
            .unwrap(),
        JevAdmissionResult::BudgetExhausted
    ));
    assert_eq!(
        counts(&f.persistence, &third).await,
        json!({"calls":2,"reserved":2,"settled":0,"outputs":0})
    );
}
#[tokio::test]
async fn jev_runtime_committed_output_replays_after_reconstruction_without_provider() {
    let f = fixture().await;
    let c = create(&f).await;
    let target = bound(&f, &c).await;
    let first = claim(&f, target).await;
    let (http, server) = server(
        200,
        "",
        serde_json::to_vec(&output()).unwrap(),
        Duration::ZERO,
    )
    .await;
    let runtime = JevRuntime::new(f.persistence.clone(), Arc::new(http), policy());
    let WorkflowHandlerOutcome::Completed(Ok(value)) = runtime
        .execute(&WorkflowStepKind("jev.evaluate".into()), &first)
        .await
    else {
        panic!()
    };
    assert_eq!(value, output());
    server.await.unwrap();
    let second = expire_reclaim(&f.persistence, &first).await;
    let offline = JevHttp::loopback("http://127.0.0.1:1/v1/systemone");
    let runtime = JevRuntime::new(f.persistence.clone(), Arc::new(offline), policy());
    let WorkflowHandlerOutcome::Completed(Ok(replayed)) = runtime
        .execute(&WorkflowStepKind("jev.evaluate".into()), &second)
        .await
    else {
        panic!()
    };
    assert_eq!(replayed, output());
    assert_eq!(
        counts(&f.persistence, &second).await,
        json!({"calls":1,"reserved":1,"settled":1,"outputs":1})
    );
    assert!(
        f.persistence
            .complete_io(crate::application::workflow::lease::FencedWorkflowResult {
                fence: second.fence,
                output: replayed,
                policy: policy()
            })
            .await
            .unwrap()
            .is_some()
    );
}
#[tokio::test]
async fn jev_runtime_current_rotated_key_used_at_provider() {
    let f = fixture().await;
    let c = create(&f).await;
    let target = bound(&f, &c).await;
    let first = claim(&f, target).await;
    let _rotated = f
        .persistence
        .rotate_jev(
            c.company,
            target.actor,
            c.id,
            c.revision,
            secrecy::SecretString::from("current-key"),
        )
        .await
        .unwrap();
    let (http, server) = server(
        200,
        "",
        serde_json::to_vec(&output()).unwrap(),
        Duration::ZERO,
    )
    .await;
    let runtime = JevRuntime::new(f.persistence.clone(), Arc::new(http), policy());
    assert!(matches!(
        runtime
            .execute(&WorkflowStepKind("jev.evaluate".into()), &first)
            .await,
        WorkflowHandlerOutcome::Completed(Ok(_))
    ));
    assert!(server.await.unwrap().contains("Bearer current-key"));
}
#[tokio::test]
async fn jev_runtime_production_worker_mixed_fixture_once() {
    let f = fixture().await;
    let c = create(&f).await;
    let target = bound(&f, &c).await;
    let AdmissionResult::Created(run) = admit(&f, target, "worker").await.unwrap() else {
        panic!()
    };
    let (http, server) = server(
        200,
        "",
        serde_json::to_vec(&output()).unwrap(),
        Duration::ZERO,
    )
    .await;
    let engine = crate::infra::workflow::WorkflowEngine::for_tests(f.persistence.clone())
        .with_jev_test_provider(Arc::new(http));
    let worker = WorkflowWorker {
        port: &f.persistence,
        handler: engine.handler(),
        notices: None,
        worker: WorkflowWorkerId(Uuid::new_v4()),
        lease: policy(),
        poll: PollPolicy::new(Duration::from_secs(1), 16).unwrap(),
        active_jobs: None,
    };
    let candidates = f.persistence.poll_work(None, 16).await.unwrap();
    let candidate = candidates
        .candidates
        .into_iter()
        .find(|c| c.scope.run == run)
        .unwrap();
    worker
        .process(candidate, &CancellationToken::new())
        .await
        .unwrap();
    server.await.unwrap();
    let saved: Value =
        sqlx::query_scalar("SELECT output FROM workflow_jev_invocations WHERE run_id=$1")
            .bind(run.as_uuid())
            .fetch_one(f.persistence.pool())
            .await
            .unwrap();
    assert_eq!(saved, output());
    let state: String = sqlx::query_scalar("SELECT state FROM workflow_runs WHERE id=$1")
        .bind(run.as_uuid())
        .fetch_one(f.persistence.pool())
        .await
        .unwrap();
    assert_eq!(state, "succeeded");
    let calls:i64=sqlx::query_scalar("SELECT count(*) FROM model_usage_calls WHERE run_id=$1 AND agent_id IS NULL AND provider='typesafe' AND input_tokens=120 AND output_tokens=12").bind(run.as_uuid()).fetch_one(f.persistence.pool()).await.unwrap();
    assert_eq!(calls, 1);
}

#[tokio::test]
async fn jev_runtime_revoked_or_model_removed_terminal_without_reservation() {
    for enabled in [false, true] {
        let f = fixture().await;
        let c = create(&f).await;
        let target = bound(&f, &c).await;
        let first = claim(&f, target).await;
        f.persistence
            .update_jev(
                c.company,
                target.actor,
                c.id,
                JevConnectionUpdate {
                    expected_revision: c.revision,
                    enabled,
                    models: [JevModel::parse("jev-1.13.0").unwrap()].into(),
                },
            )
            .await
            .unwrap();
        let runtime = JevRuntime::new(
            f.persistence.clone(),
            Arc::new(JevHttp::loopback("http://127.0.0.1:1/v1/systemone")),
            policy(),
        );
        let WorkflowHandlerOutcome::Completed(Err(error)) = runtime
            .execute(&WorkflowStepKind("jev.evaluate".into()), &first)
            .await
        else {
            panic!()
        };
        assert_eq!(
            error.failure.class(),
            crate::domain::workflow::FailureClass::Terminal
        );
        assert_eq!(
            counts(&f.persistence, &first).await,
            json!({"calls":0,"reserved":0,"settled":0,"outputs":0})
        );
    }
}
#[tokio::test]
async fn jev_runtime_token_allowance_blocks_entry_and_preserves_accounting() {
    let f = fixture().await;
    let c = create(&f).await;
    let mut source: Value =
        serde_json::from_str(&registry::example("jev.evaluate").unwrap().source).unwrap();
    source["limits"]["root_budget"] = json!({"activations":10,"model_calls":10,"repetitions":10,"tokens":{"max_tokens":100,"reviewers":[{"kind":"user","id":f.target.actor.user_id()}]}});
    let target = bound_source(&f, &c, source).await;
    let first = claim(&f, target).await;
    let request = JevRequest::prepare(&first.activation.inputs).unwrap();
    assert!(matches!(
        f.persistence
            .admit_jev(&first, &request, policy())
            .await
            .unwrap(),
        JevAdmissionResult::TokenLimit
    ));
    assert_eq!(
        counts(&f.persistence, &first).await,
        json!({"calls":0,"reserved":0,"settled":0,"outputs":0})
    );
}
#[tokio::test]
async fn jev_runtime_failures_remain_charged_and_secret_free() {
    let f = fixture().await;
    let c = create(&f).await;
    let target = bound(&f, &c).await;
    let first = claim(&f, target).await;
    let (http, server) = server(
        429,
        "Retry-After: 0\r\n",
        b"fixture-jev-secret".to_vec(),
        Duration::ZERO,
    )
    .await;
    let runtime = JevRuntime::new(f.persistence.clone(), Arc::new(http), policy());
    let WorkflowHandlerOutcome::Completed(Err(error)) = runtime
        .execute(&WorkflowStepKind("jev.evaluate".into()), &first)
        .await
    else {
        panic!()
    };
    assert_eq!(
        error.failure.class(),
        crate::domain::workflow::FailureClass::Retryable
    );
    server.await.unwrap();
    let row:Value=sqlx::query_scalar("SELECT to_jsonb(invocation)-'prepared' FROM workflow_jev_invocations AS invocation WHERE run_id=$1").bind(first.fence.scope.run.as_uuid()).fetch_one(f.persistence.pool()).await.unwrap();
    assert_eq!(row["failure_code"], "jev.rate_limited");
    assert_eq!(row["retryable"], true);
    assert!(!row.to_string().contains("secret"));
    assert_eq!(
        counts(&f.persistence, &first).await,
        json!({"calls":1,"reserved":1,"settled":0,"outputs":0})
    );
}

#[path = "jev_runtime_recovery_tests.rs"]
mod recovery;
