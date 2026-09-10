//! Production inbound dispatch, shared human policy, real PostgreSQL, scripted model only.
use super::*;
use crate::{
    entities::harness::HarnessKind,
    services::{
        harness::runs::{HarnessRunStore, RunCheckpoint, RunState},
        test_support::{ScriptedExchange, ScriptedResponse, scripted_scenario},
    },
    use_cases::approval::ApprovalPersistence,
};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};

fn batch() -> Value {
    let calls = [
        (
            "todo-before",
            "todo",
            json!({"operation":"set","items":[{"id":"plan","content":"Follow the approved plan","status":"pending"}]}),
        ),
        (
            "checkpoint-original",
            "request_approval",
            json!({"title":"Approve plan","proposal":"Use the saved plan and return the prepared answer."}),
        ),
        ("todo-after", "todo", json!({"operation":"list"})),
        (
            "echo-after",
            "echo",
            json!({"message":"saved argument proof"}),
        ),
    ];
    json!({"id":"batch-one","object":"chat.completion","created":0,"model":SCRIPTED_MODEL,
        "choices":[{"index":0,"finish_reason":"tool_calls","message":{"role":"assistant","content":null,
            "tool_calls":calls.into_iter().map(|(id,name,args)| json!({"id":id,"type":"function","function":{"name":name,"arguments":args.to_string()}})).collect::<Vec<_>>()}}],
        "usage":{"prompt_tokens":20,"completion_tokens":30,"total_tokens":50}})
}

async fn checkpoint(fx: &Fixture, task_id: Uuid) -> RunCheckpoint {
    let value: Value =
        sqlx::query_scalar("SELECT checkpoint FROM task_harness_runs WHERE task_id = $1")
            .bind(task_id)
            .fetch_one(&fx.pool)
            .await
            .unwrap();
    serde_json::from_value(value).unwrap()
}

#[tokio::test]
async fn rig_resumes_a_saved_batch_after_approval_and_atomically_dispatches_once() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let _guard = crate::adapters::persistence::test_support::UNSCOPED_CLAIM
        .lock()
        .await;
    let requests = Arc::new(AtomicUsize::new(0));
    let first = requests.clone();
    let second = requests.clone();
    let llm = scripted_scenario(vec![
        ScriptedExchange::new(
            move |request| {
                first.fetch_add(1, Ordering::SeqCst);
                if !request.header_matches("authorization", "Bearer scripted-key") {
                    return Err("tenant credential");
                }
                Ok(())
            },
            ScriptedResponse::json(batch()),
        ),
        ScriptedExchange::new(
            move |request| {
                second.fetch_add(1, Ordering::SeqCst);
                let results: Vec<_> = request.body["messages"]
                    .as_array()
                    .ok_or("messages")?
                    .iter()
                    .filter(|item| item["role"] == "tool")
                    .collect();
                let ids: Vec<_> = results
                    .iter()
                    .map(|item| item["tool_call_id"].as_str().unwrap_or(""))
                    .collect();
                if ids
                    != [
                        "todo-before",
                        "checkpoint-original",
                        "todo-after",
                        "echo-after",
                    ]
                {
                    return Err("saved batch order and correlation");
                }
                if !results[1]["content"].to_string().contains("approved") {
                    return Err("decision receipt");
                }
                if !results[2]["content"]
                    .to_string()
                    .contains("Follow the approved plan")
                {
                    return Err("todo state lost on restart");
                }
                if !results[3]["content"]
                    .to_string()
                    .contains("saved argument proof")
                {
                    return Err("original arguments changed");
                }
                Ok(())
            },
            ScriptedResponse::turn(LlmTurn::text("The approved answer is ready."), 1),
        ),
    ])
    .await;
    let fx = fixture_for_harness(pool.clone(), Some(&llm.base_url), HarnessKind::Rig).await;
    let ingest = fx
        .threads
        .ingest_test_email(inbound(
            &fx.owner_email,
            &fx.address(&fx.channel_a),
            "<rig-approval@example.test>",
            "Please prepare an answer",
        ))
        .await
        .unwrap();
    assert!(ingest.accepted);
    let task_id = ingest.task_id.unwrap();
    let first_lease = fx.claim(task_id).await;
    let outcome = fx
        .threads
        .execute_claimed_agent_task_and_dispatch(
            &ingest,
            ReplyDelivery::Send,
            first_lease,
            ingest.correlation_id().unwrap(),
        )
        .await
        .unwrap();
    assert!(matches!(outcome, DispatchOutcome::Suspended));
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    let parked = checkpoint(&fx, task_id).await;
    assert_eq!(parked.state, RunState::Waiting);
    assert_eq!(parked.turns[0].calls.len(), 4);
    assert_eq!(
        parked.invocations.len(),
        2,
        "later tools cannot run before approval"
    );
    let (token,): (Uuid,) = sqlx::query_as(
        "SELECT token FROM human_approvals WHERE task_id = $1 AND status = 'pending'",
    )
    .bind(task_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let restarted = PostgresPersistence::new(pool.clone());
    restarted
        .decide_approval_and_transition(
            &token.to_string(),
            crate::entities::approval::ApprovalStatus::Approved,
            Utc::now(),
        )
        .await
        .unwrap()
        .unwrap();
    let lease = fx.claim(task_id).await;
    assert_ne!(lease.execution_generation, first_lease.execution_generation);
    // Every execution creates a new Rig runtime. The only conversation crossing this seam is JSON.
    let outcome = fx
        .threads
        .execute_claimed_agent_task_and_dispatch(
            &ingest,
            ReplyDelivery::Send,
            lease,
            ingest.correlation_id().unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(llm.finish().await, Ok(2));
    assert!(matches!(outcome, DispatchOutcome::Replied(_)));
    let complete = checkpoint(&fx, task_id).await;
    assert_eq!(complete.run_id, parked.run_id);
    assert_eq!(complete.reservations.len(), 2);
    assert_eq!(complete.executions.len(), 2);
    assert_eq!(complete.invocations.len(), 4);
    assert_eq!(
        complete.final_output.unwrap().content,
        "The approved answer is ready."
    );
    let consumed: bool = sqlx::query_scalar(
        "SELECT final_output_consumed_at IS NOT NULL FROM task_harness_runs WHERE task_id = $1",
    )
    .bind(task_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(consumed);
    assert!(matches!(
        restarted
            .reserve_model(
                &crate::services::harness::runs::RunWrite {
                    company_id: fx.company.id,
                    run_id: parked.run_id,
                    lease: first_lease,
                    expected_revision: parked.revision
                },
                crate::services::harness::runs::ModelReservation {
                    repair: None,
                    request_id: crate::entities::harness_run::ModelRequestId(Uuid::new_v4()),
                    input_tokens: 1,
                    output_tokens: 1
                }
            )
            .await
            .unwrap(),
        crate::services::harness::runs::WriteOutcome::OwnershipLost
    ));
    CompanyPersistence::delete(fx.persistence.as_ref(), fx.company.id)
        .await
        .unwrap();
}

async fn approve_pending(fx: &Fixture, task_id: Uuid) {
    let token: Uuid = sqlx::query_scalar(
        "SELECT token FROM human_approvals WHERE task_id = $1 AND status = 'pending'",
    )
    .bind(task_id)
    .fetch_one(&fx.pool)
    .await
    .unwrap();
    ApprovalUseCases::new(
        fx.persistence.clone(),
        fx.deliveries.clone(),
        loop_test_config(),
    )
    .process_link_action(&token.to_string(), "approve")
    .await
    .unwrap();
}

async fn accept_partial(
    fx: &Fixture,
    lease: crate::entities::task::TaskLeaseRef,
    outreach_id: Uuid,
) {
    use crate::entities::delegation::*;
    let version: i64 = sqlx::query_scalar("SELECT version FROM task_outreaches WHERE id = $1")
        .bind(outreach_id)
        .fetch_one(&fx.pool)
        .await
        .unwrap();
    let principal: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND user_id = $2")
            .bind(fx.company.id)
            .bind(fx.company.user_id)
            .fetch_one(&fx.pool)
            .await
            .unwrap();
    fx.persistence
        .execute_delegation_command(crate::task_queue::DelegationCommandRequest {
            command: DelegationCommand {
                company_id: fx.company.id,
                task_id: lease.task_id,
                command_id: Uuid::new_v4(),
                expected_version: version as u64,
                actor: DelegationActor {
                    principal_id: crate::entities::transport::PrincipalId::from(principal),
                    authority: DelegationAuthority::CompanyManager,
                },
                reason: DelegationReason::PartialResultsAccepted,
                reason_detail: None,
                operation: DelegationOperation::ProceedWithPartial { outreach_id },
            },
            replacement: None,
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn explicit_approval_does_not_approve_later_outreach_and_partial_results_resume_without_resend()
 {
    let Some(pool) = test_pool().await else {
        return;
    };
    let _guard = crate::adapters::persistence::test_support::UNSCOPED_CLAIM
        .lock()
        .await;
    let mut response = batch();
    response["choices"][0]["message"]["tool_calls"] = json!([
        {"id":"plan","type":"function","function":{"name":"request_approval","arguments":json!({"title":"Review","proposal":"Ask the supplier, then answer."}).to_string()}},
        {"id":"outreach","type":"function","function":{"name":"outreach_and_await_quorum","arguments":json!({"target_channels":["supplier"],"subject":"Supplier question","body":"Please provide a delivery date."}).to_string()}},
        {"id":"later","type":"function","function":{"name":"echo","arguments":json!({"message":"after outreach"}).to_string()}}
    ]);
    let llm = scripted_scenario(vec![
        ScriptedExchange::new(|_| Ok(()), ScriptedResponse::json(response)),
        ScriptedExchange::new(
            |request| {
                let results: Vec<_> = request.body["messages"]
                    .as_array()
                    .ok_or("messages")?
                    .iter()
                    .filter(|item| item["role"] == "tool")
                    .collect();
                if results
                    .iter()
                    .map(|r| r["tool_call_id"].as_str().unwrap_or(""))
                    .collect::<Vec<_>>()
                    != ["plan", "outreach", "later"]
                {
                    return Err("saved call order");
                }
                if !results[1]["content"]
                    .to_string()
                    .contains("proceed_partial")
                {
                    return Err("durable outreach result");
                }
                Ok(())
            },
            ScriptedResponse::turn(LlmTurn::text("Partial results accepted."), 1),
        ),
    ])
    .await;
    let fx = fixture_for_harness(pool, Some(&llm.base_url), HarnessKind::Rig).await;
    let ingest = fx
        .threads
        .ingest_test_email(inbound(
            &fx.owner_email,
            &fx.address(&fx.channel_a),
            "<rig-outreach@example.test>",
            "Ask the supplier",
        ))
        .await
        .unwrap();
    let task_id = ingest.task_id.unwrap();
    for expected_receipts in [1, 2] {
        let lease = fx.claim(task_id).await;
        assert!(matches!(
            fx.threads
                .execute_claimed_agent_task_and_dispatch(
                    &ingest,
                    ReplyDelivery::Send,
                    lease,
                    ingest.correlation_id().unwrap()
                )
                .await
                .unwrap(),
            DispatchOutcome::Suspended
        ));
        assert_eq!(fx.status_of(task_id).await, TaskStatus::PendingApproval);
        assert_eq!(
            checkpoint(&fx, task_id).await.invocations.len(),
            expected_receipts
        );
        let effects: i64 =
            sqlx::query_scalar("SELECT count(*) FROM task_outreaches WHERE task_id = $1")
                .bind(task_id)
                .fetch_one(&fx.pool)
                .await
                .unwrap();
        assert_eq!(effects, 0, "each approval precedes its own effect");
        approve_pending(&fx, task_id).await;
    }
    let lease = fx.claim(task_id).await;
    assert!(matches!(
        fx.threads
            .execute_claimed_agent_task_and_dispatch(
                &ingest,
                ReplyDelivery::Send,
                lease,
                ingest.correlation_id().unwrap()
            )
            .await
            .unwrap(),
        DispatchOutcome::Suspended
    ));
    let waiting = checkpoint(&fx, task_id).await;
    assert_eq!(waiting.state, RunState::Waiting);
    assert_eq!(waiting.reservations.len(), 1);
    let outreach = waiting.outreach_wait.unwrap().outreach_id;
    accept_partial(&fx, lease, outreach).await;
    assert_eq!(fx.status_of(task_id).await, TaskStatus::Pending);
    let resumed = checkpoint(&fx, task_id).await;
    assert_eq!(
        resumed.invocations[1].result.as_ref().unwrap()["status"],
        "proceed_partial"
    );
    let lease = fx.claim(task_id).await;
    assert!(matches!(
        fx.threads
            .execute_claimed_agent_task_and_dispatch(
                &ingest,
                ReplyDelivery::Send,
                lease,
                ingest.correlation_id().unwrap()
            )
            .await
            .unwrap(),
        DispatchOutcome::Replied(_)
    ));
    assert_eq!(llm.finish().await, Ok(2));
    let effects: i64 =
        sqlx::query_scalar("SELECT count(*) FROM task_outreaches WHERE task_id = $1")
            .bind(task_id)
            .fetch_one(&fx.pool)
            .await
            .unwrap();
    assert_eq!(effects, 1);
    CompanyPersistence::delete(fx.persistence.as_ref(), fx.company.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn provider_deadline_cancels_the_claim_and_recovery_keeps_the_unknown_reservation() {
    use crate::entities::task::{TaskFailure, TaskFailureOutcome, TaskStopReason};
    let Some(pool) = test_pool().await else {
        return;
    };
    let _guard = crate::adapters::persistence::test_support::UNSCOPED_CLAIM
        .lock()
        .await;
    let (arrived_tx, arrived_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let mut blocked = ScriptedResponse::turn(LlmTurn::text("This response must not commit."), 0);
    blocked.barrier = Some((arrived_tx, release_rx));
    let llm = scripted_scenario(vec![
        ScriptedExchange::new(|_| Ok(()), blocked),
        ScriptedExchange::new(
            |_| Ok(()),
            ScriptedResponse::turn(LlmTurn::text("Recovered within the original budget."), 1),
        ),
    ])
    .await;
    let fx = fixture_for_harness(pool, Some(&llm.base_url), HarnessKind::Rig).await;
    sqlx::query("UPDATE agents SET run_timeout_secs = 1 WHERE company_id = $1")
        .bind(fx.company.id)
        .execute(&fx.pool)
        .await
        .unwrap();
    let ingest = fx
        .threads
        .ingest_test_email(inbound(
            &fx.owner_email,
            &fx.address(&fx.channel_a),
            "<rig-timeout@example.test>",
            "Answer once",
        ))
        .await
        .unwrap();
    let task_id = ingest.task_id.unwrap();
    let lease = fx.claim(task_id).await;
    let execute = fx.threads.execute_claimed_agent_task_and_dispatch(
        &ingest,
        ReplyDelivery::Send,
        lease,
        ingest.correlation_id().unwrap(),
    );
    let (arrived, outcome) = tokio::join!(
        tokio::time::timeout(std::time::Duration::from_secs(5), arrived_rx),
        execute
    );
    arrived.unwrap().unwrap();
    assert!(matches!(
        outcome,
        Err(crate::app_error::AppError::Timeout(_))
    ));
    let interrupted = checkpoint(&fx, task_id).await;
    assert_eq!(interrupted.reservations.len(), 1);
    assert!(interrupted.turns.is_empty());
    assert!(interrupted.invocations.is_empty());
    release_tx.send(()).unwrap();
    fx.persistence
        .mark_task_failed(TaskFailure {
            lease,
            error: "fixture timeout",
            next_run_at: Utc::now(),
            outcome: TaskFailureOutcome::Retry,
            reason: TaskStopReason::TimedOut,
        })
        .await
        .unwrap();
    let lease = fx.claim(task_id).await;
    let result = fx
        .threads
        .execute_claimed_agent_task_and_dispatch(
            &ingest,
            ReplyDelivery::Send,
            lease,
            ingest.correlation_id().unwrap(),
        )
        .await
        .unwrap();
    assert!(matches!(result, DispatchOutcome::Replied(_)));
    assert_eq!(llm.finish().await, Ok(2));
    let recovered = checkpoint(&fx, task_id).await;
    assert_eq!(recovered.run_id, interrupted.run_id);
    assert_eq!(recovered.reservations.len(), 2);
    assert_eq!(recovered.turns.len(), 1);
    CompanyPersistence::delete(fx.persistence.as_ref(), fx.company.id)
        .await
        .unwrap();
}
