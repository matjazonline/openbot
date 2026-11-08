use super::*;
use crate::application::workflow::{completion::WorkflowCompletion, lease::FencedWorkflowResult};
use crashes::{billed, exchange, spec};
use process::{ClaimMark, WorkerProcess};

async fn pointer(p: &PostgresPersistence, scope: ActivationRequest) -> Option<Uuid> {
    sqlx::query_scalar("SELECT conversation.active_exchange_id FROM agent_history_conversations AS conversation JOIN agent_history_exchanges AS exchange ON exchange.company_id=conversation.company_id AND exchange.conversation_id=conversation.id WHERE exchange.company_id=$1 AND exchange.id=$2")
        .bind(scope.company.as_uuid()).bind(scope.execution.as_uuid()).fetch_one(p.pool()).await.unwrap()
}
async fn cancel_queued(f: &ManualHistory, scope: ActivationRequest) {
    let p = f.f.s.p();
    let owner = pointer(p, scope).await;
    let result = p
        .cancel(CancelCommand {
            company_id: scope.company,
            run_id: scope.run,
            actor: f.actor,
            command_key: IdempotencyKey::parse("p07-cancel-queued").unwrap(),
            expected_revision: p
                .head(scope.company, scope.run)
                .await
                .unwrap()
                .unwrap()
                .revision,
        })
        .await
        .unwrap();
    assert!(matches!(result, CancelResult::Applied { .. }));
    assert_eq!(
        pointer(p, scope).await,
        owner,
        "queued cancellation must not release the other owner"
    );
    assert_eq!(exchange(p, scope).await.unwrap()["state"], "cancelled");
}
async fn parked(f: &ManualHistory, input: Value) -> ActivationRequest {
    let scope = pending(f, input).await;
    work(
        f.f.s.p(),
        PollCandidate {
            scope,
            work: PollWork::Job,
        },
    )
    .await;
    let row = exchange(f.f.s.p(), scope).await.unwrap();
    assert_eq!(row["state"], "queued");
    assert!(row["frozen_preparation"].is_null() && row["checkpoint_id"].is_null());
    let wait:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_waits WHERE company_id=$1 AND execution_id=$2 AND reason='agent_turn' AND state='waiting')")
        .bind(scope.company.as_uuid()).bind(scope.execution.as_uuid()).fetch_one(f.f.s.pool()).await.unwrap();
    assert!(wait);
    scope
}
async fn finish_held(f: &ManualHistory, claim: &ClaimedWorkflow) {
    let output = Box::pin(runtime(&f.f).execute(&WorkflowStepKind("agent.run".into()), claim))
        .await
        .completed_result()
        .unwrap();
    assert_eq!(output, json!("P07_HELD_OUTPUT"));
    f.f.s
        .p()
        .complete_io(FencedWorkflowResult {
            fence: claim.fence,
            output,
            policy: policy(),
        })
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn agent_history_e2e_process_crash_after_durable_enqueue_and_cancelled_middle_turn_recovers_in_order()
 {
    let mut f = Box::pin(ManualHistory::new()).await;
    let mut llm = scripted_llm(vec![
        LlmTurn::text("\"P07_PRIOR_OUTPUT\""),
        LlmTurn::text("\"P07_HELD_OUTPUT\""),
        LlmTurn::text("\"P07_TARGET_OUTPUT\""),
    ])
    .await;
    register_scripted_agent_base_url(f.f.agent, &llm.base_url);
    f.complete_ordinary(json!("P07_PRIOR_INPUT"), json!("P07_PRIOR_OUTPUT"))
        .await;
    let held = f.start(json!("P07_HELD_INPUT")).await;
    f.f.s
        .p()
        .prepare_agent(&held, policy())
        .await
        .unwrap()
        .expect_ready();
    let cancelled = parked(&f, json!("P07_CANCELLED_INPUT")).await;
    cancel_queued(&f, cancelled).await;
    let target = pending(&f, json!("P07_CURRENT_INPUT")).await;
    let config = spec(&f, target, &llm, CrashPoint::Enqueued);
    let mut child = WorkerProcess::spawn(&config);
    child.mark("AH_PROCESS_CLAIM").await;
    child.mark("AH_PROCESS_READY").await;
    let queued = exchange(f.f.s.p(), target).await.unwrap();
    assert_eq!(queued["position"], 4);
    assert_eq!(queued["state"], "queued");
    assert!(queued["frozen_preparation"].is_null() && queued["checkpoint_id"].is_null());
    assert_eq!(
        pointer(f.f.s.p(), target).await,
        Some(held.fence.scope.execution.as_uuid())
    );
    child.kill();
    finish_held(&f, &held).await;
    let reconstructed = PostgresPersistence::with_credential_cipher(
        f.f.s.pool().clone(),
        CredentialCipher::for_test(),
    );
    drive_reconstructed(&reconstructed, target).await;
    let done = exchange(&reconstructed, target).await.unwrap();
    assert_eq!(done["id"], queued["id"]);
    assert_eq!(done["position"], queued["position"]);
    assert_eq!(done["output"], "P07_TARGET_OUTPUT");
    assert_eq!(
        done["frozen_preparation"]["history"]["seed"],
        json!([
            {"input":"P07_PRIOR_INPUT","output":"P07_PRIOR_OUTPUT"},
            {"input":"P07_HELD_INPUT","output":"P07_HELD_OUTPUT"},
        ])
    );
    assert_eq!(
        billed(&reconstructed, target)
            .await
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let requests = llm.observed();
    assert_eq!(requests.len(), 3);
    assert!(!requests[2].to_string().contains("P07_CANCELLED_INPUT"));
    assert_eq!(llm.finish().await.unwrap(), 3);
    eprintln!(
        "AH-P07 genuine_process_crash point=Enqueued cancelled_middle=true same_identity=true provider_calls=3"
    );
}

#[tokio::test]
async fn agent_history_e2e_process_crash_before_successor_wakeup_recovers_by_poll_without_releasing_successor()
 {
    let mut f = Box::pin(ManualHistory::new()).await;
    let mut llm = scripted_llm(vec![
        LlmTurn::text("\"P07_PRIOR_OUTPUT\""),
        LlmTurn::text("\"P07_TARGET_OUTPUT\""),
        LlmTurn::text("\"P07_SUCCESSOR_OUTPUT\""),
    ])
    .await;
    register_scripted_agent_base_url(f.f.agent, &llm.base_url);
    f.complete_ordinary(json!("P07_PRIOR_INPUT"), json!("P07_PRIOR_OUTPUT"))
        .await;
    let owner = pending(&f, json!("P07_CURRENT_INPUT")).await;
    let config = spec(&f, owner, &llm, CrashPoint::BeforeWake);
    let mut child = WorkerProcess::spawn(&config);
    let mark: ClaimMark = serde_json::from_value(child.mark("AH_PROCESS_CLAIM").await).unwrap();
    child.mark("AH_PROCESS_PREPARED").await;
    let cancelled = parked(&f, json!("P07_CANCELLED_INPUT")).await;
    cancel_queued(&f, cancelled).await;
    let successor = parked(&f, json!("P07_SUCCESSOR_INPUT")).await;
    child.proceed();
    child.mark("AH_PROCESS_READY").await;
    let accepted = exchange(f.f.s.p(), owner).await.unwrap();
    assert_eq!(accepted["output"], "P07_TARGET_OUTPUT");
    let page = f.f.s.p().poll_work(None, 128).await.unwrap();
    let ready = page
        .candidates
        .into_iter()
        .find(|candidate| candidate.scope == successor && candidate.work == PollWork::Wait)
        .expect("durable ready wait before any wake consumer");
    child.kill();
    let reconstructed = PostgresPersistence::with_credential_cipher(
        f.f.s.pool().clone(),
        CredentialCipher::for_test(),
    );
    work(&reconstructed, ready).await;
    assert_eq!(
        pointer(&reconstructed, successor).await,
        Some(successor.execution.as_uuid())
    );
    drive_reconstructed(&reconstructed, owner).await;
    assert_eq!(exchange(&reconstructed, owner).await.unwrap(), accepted);
    assert!(
        !reconstructed
            .validate_io(mark.fence(owner), crash_policy())
            .await
            .unwrap()
    );
    assert_eq!(
        pointer(&reconstructed, successor).await,
        Some(successor.execution.as_uuid()),
        "completed replay cannot release the successor"
    );
    drive_reconstructed(&reconstructed, successor).await;
    let done = exchange(&reconstructed, successor).await.unwrap();
    assert_eq!(done["position"], 4);
    assert_eq!(done["output"], "P07_SUCCESSOR_OUTPUT");
    let requests = llm.observed();
    assert_eq!(requests.len(), 3);
    let actual = requests[2].to_string();
    for marker in [
        "P07_PRIOR_OUTPUT",
        "P07_TARGET_OUTPUT",
        "P07_SUCCESSOR_INPUT",
    ] {
        assert_eq!(actual.matches(marker).count(), 1);
    }
    assert!(!actual.contains("P07_CANCELLED_INPUT"));
    assert_eq!(
        billed(&reconstructed, successor)
            .await
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(llm.finish().await.unwrap(), 3);
    eprintln!(
        "AH-P07 genuine_process_crash point=BeforeWake polled=true successor_owned=true provider_calls=3"
    );
}
