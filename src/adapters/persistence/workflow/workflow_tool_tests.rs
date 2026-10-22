//! Agent workflow tools at runtime (07-D2 §D2.3–§D2.4): a saved model call starts its pinned
//! child and parks the agent; the child's terminal fact continues the agent job, which resumes
//! the exact pending call with the child's result or a typed error.
use super::*;
use crate::application::workflow::{
    agent::AgentToolObservation,
    batch::WorkflowBatch,
    completion::CommitDisposition,
    polling::{PollWork, WorkflowPolling},
    tools::{
        CHILD_CANCELLED, CHILD_FAILED, CHILD_INVALID_INPUT, TOOL_LIMIT, TOOL_RESULT_TOO_LARGE,
    },
    waits::WaitFailureReason,
};
use crate::domain::workflow::{ChildCause, TriggerSource};
use crate::services::test_support::{LlmTurn, register_scripted_agent_base_url, scripted_llm};

use super::workflow_tool_support::*;

#[tokio::test]
async fn workflow_tool_resumes_the_pending_call_after_a_child_human_decision() {
    let setup = Box::pin(ToolSetup::new(decision_child())).await;
    let p = setup.persistence();
    let claim = setup.claim("tool-run").await;
    let scope = claim.fence.scope;
    let llm = scripted_llm(vec![
        LlmTurn::tool_call(DECLARED, json!({"question": "Refund order 42?"})),
        LlmTurn::text("\"done\""),
    ])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let runtime = setup.runtime();
    let suspended = match Box::pin(runtime.execute(&kind(), &claim)).await {
        WorkflowHandlerOutcome::Suspended(suspended) => suspended,
        WorkflowHandlerOutcome::Completed(result) => panic!(
            "expected a park: {:?}",
            result.err().map(|f| format!("{:?}", f.failure))
        ),
    };
    // One child, started with exactly the arguments, the frozen params and the parent's
    // deadline; the agent's attempt and job are closed and the run waits on the child.
    assert_eq!(
        call_parked(p, scope).await,
        CallParked {
            run: "waiting".into(),
            reason: Some("child_run".into()),
            job: "completed".into(),
            stop: Some("child_parked".into()),
            children: 1,
        }
    );
    let child = setup.child_of(scope).await;
    let (input, params, version, inherited): (Value, Value, Uuid, bool) = sqlx::query_as(
        "SELECT child.input, child.params, child.version_id, child.deadline = parent.deadline \
         FROM workflow_runs AS child JOIN workflow_runs AS parent ON parent.id = child.parent_run_id \
         WHERE child.id = $1",
    )
    .bind(child.run.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(input, json!({"question": "Refund order 42?"}));
    assert_eq!(params, json!({"reviewer": setup.reviewer.to_string()}));
    assert_eq!(version, setup.child.as_uuid());
    assert!(inherited);
    let wait: (Uuid, Option<Uuid>, Option<i32>) = sqlx::query_as(
        "SELECT id, child_tool_call_id, parked_attempt_number FROM workflow_waits WHERE child_run_id=$1",
    )
    .bind(child.run.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(wait.0, suspended.wait.as_uuid());
    assert_eq!(wait.2, Some(claim.fence.attempt.0));
    // Inspection reads the child's cause back as this saved call of this execution.
    let head = p.head(child.company, child.run).await.unwrap().unwrap();
    let TriggerSource::Child {
        parent: ChildCause::ToolCall(cause),
    } = head.causality.trigger().source()
    else {
        panic!("{:?}", head.causality);
    };
    assert_eq!(Some(cause.call().as_uuid()), wait.1);
    assert_eq!(cause.execution().execution_id(), scope.execution);
    // The child's reviewer, named by the registration's frozen params, decides.
    let next = setup.decide(child, "approve", "Approved").await;
    p.advance_pure(next, budget()).await.unwrap();
    assert!(matches!(
        p.resume_wait(scope).await.unwrap(),
        WaitProgress::Continued
    ));
    // The reopened agent resumes the exact pending call with the child's output.
    let continued = p
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .expect("the child continued the parked agent job");
    let WorkflowHandlerOutcome::Completed(Ok(output)) =
        Box::pin(runtime.execute(&kind(), &continued)).await
    else {
        panic!("the resumed agent consumes the child's result and answers");
    };
    assert_eq!(output, json!("done"));
    assert_eq!(llm.finish().await.unwrap(), 2);
    assert_eq!(
        saved_result(p, scope).await,
        json!({"choice": "approve", "answer": "Approved"})
    );
    let after: (i64, i16) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM workflow_runs WHERE parent_run_id=$1), \
                (SELECT continuation_count FROM background_tasks WHERE id=$2)",
    )
    .bind(scope.run.as_uuid())
    .bind(scope.job.0)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(after, (1, 1));
}

#[tokio::test]
async fn workflow_tool_typed_errors_reach_the_model_without_fabricated_success() {
    // Arguments the child refuses: a typed error, no child, and the agent answers.
    let setup = Box::pin(ToolSetup::new(echo_child())).await;
    let p = setup.persistence();
    let claim = setup.claim("invalid-arguments").await;
    let llm = scripted_llm(vec![
        LlmTurn::tool_call(DECLARED, json!({"question": 42})),
        LlmTurn::text("\"gave up\""),
    ])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let runtime = setup.runtime();
    let WorkflowHandlerOutcome::Completed(Ok(output)) =
        Box::pin(runtime.execute(&kind(), &claim)).await
    else {
        panic!("an invalid call is answered at once");
    };
    assert_eq!(output, json!("gave up"));
    assert_eq!(call_parked(p, claim.fence.scope).await.children, 0);
    assert_eq!(
        saved_result(p, claim.fence.scope).await["error"]["code"],
        CHILD_INVALID_INPUT
    );
    llm.finish().await.unwrap();

    // A cancelled child: a typed error, never the child's success.
    let claim = setup.claim("cancelled-child").await;
    let scope = claim.fence.scope;
    let llm = scripted_llm(vec![
        LlmTurn::tool_call(DECLARED, json!({"question": "Refund?"})),
        LlmTurn::text("\"escalated\""),
    ])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let WorkflowHandlerOutcome::Suspended(_) = Box::pin(runtime.execute(&kind(), &claim)).await
    else {
        panic!("parks");
    };
    let child = setup.child_of(scope).await;
    // An operator cancels the child through the run control.
    setup.cancel(child, "cancel-child").await;
    assert!(matches!(
        p.resume_wait(scope).await.unwrap(),
        WaitProgress::Continued
    ));
    let continued = p
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    let WorkflowHandlerOutcome::Completed(Ok(output)) =
        Box::pin(runtime.execute(&kind(), &continued)).await
    else {
        panic!("the agent answers after the typed error");
    };
    assert_eq!(output, json!("escalated"));
    assert_eq!(
        saved_result(p, scope).await["error"]["code"],
        CHILD_CANCELLED
    );
    llm.finish().await.unwrap();
}

#[tokio::test]
async fn workflow_tool_dispatch_starts_one_child_per_call_and_one_resumer_continues() {
    let setup = Box::pin(ToolSetup::new(echo_child())).await;
    let p = setup.persistence();
    let claim = setup.claim("duplicate-dispatch").await;
    let scope = claim.fence.scope;
    let call = setup.call(
        setup.frozen_pin(setup.parent).await,
        json!({"question": "Twice?"}),
    );
    // Two dispatches of one saved call under one fence: one parks, the other finds the fence
    // closed by the park; a later one too.
    let (one, two) = tokio::join!(setup.dispatch(&claim, &call), setup.dispatch(&claim, &call));
    let outcomes = [one, two];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, Ok(AgentToolObservation::Parked(_))))
            .count(),
        1,
        "{outcomes:?}"
    );
    assert_eq!(
        outcomes.iter().filter(|outcome| outcome.is_err()).count(),
        1
    );
    assert!(setup.dispatch(&claim, &call).await.is_err());
    assert_eq!(children(p, scope.run).await, 1);
    setup.finish_child(scope).await;
    // Two resumers of the finished child: exactly one continues the parked job.
    let (one, two) = tokio::join!(p.resume_wait(scope), p.resume_wait(scope));
    let progress = [one, two];
    assert_eq!(
        progress
            .iter()
            .filter(|progress| matches!(progress, Ok(WaitProgress::Continued)))
            .count(),
        1,
        "{progress:?}"
    );
    let job: (i16, String) =
        sqlx::query_as("SELECT continuation_count, status FROM background_tasks WHERE id=$1")
            .bind(scope.job.0)
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(job, (1, "pending".into()));
    // The reopened attempt replays the call from immutable facts, as often as it asks.
    let continued = p
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    for _ in 0..2 {
        let AgentToolObservation::Completed(result) =
            setup.dispatch(&continued, &call).await.unwrap()
        else {
            panic!("a finished call answers");
        };
        assert_eq!(result, json!({"choice": "approve", "answer": "Twice?"}));
    }
    assert_eq!(children(p, scope.run).await, 1);
}

/// A child whose choice and answer come from its input, to fail or grow on demand.
fn choice_child() -> Value {
    let mut child = echo_child();
    child["input_schema"] = json!({"type":"object","properties":{
        "question":{"type":"string","maxLength":200000},"choice":{"type":"string","maxLength":16}},
        "required":["question","choice"],"additionalProperties":false});
    child["steps"]["answer"]["with"]["value"]["object"]["choice"] = json!({"ref":"/input/choice"});
    child
}

/// Dispatch `arguments` in a new run, finish its child and replay the call's result.
async fn replayed(setup: &ToolSetup, key: &str, arguments: Value) -> Value {
    let p = setup.persistence();
    let claim = setup.claim(key).await;
    let scope = claim.fence.scope;
    let call = setup.call(setup.frozen_pin(setup.parent).await, arguments);
    assert!(matches!(
        setup.dispatch(&claim, &call).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    setup.finish_child(scope).await;
    assert_eq!(p.resume_wait(scope).await.unwrap(), WaitProgress::Continued);
    let continued = p
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    let AgentToolObservation::Completed(result) = setup.dispatch(&continued, &call).await.unwrap()
    else {
        panic!("a finished call answers");
    };
    // Free the company's dispatch capacity for the next run.
    setup.cancel(scope, key).await;
    result
}

#[tokio::test]
async fn workflow_tool_child_failure_limit_and_result_bound_are_typed_errors() {
    let setup = Box::pin(ToolSetup::new(choice_child())).await;
    let p = setup.persistence();
    assert_eq!(
        replayed(
            &setup,
            "succeeds",
            json!({"question": "q", "choice": "reject"})
        )
        .await,
        json!({"choice": "reject", "answer": "q"})
    );
    // A child that fails: its output breaks its own schema.
    let failed = replayed(&setup, "fails", json!({"question": "q", "choice": "maybe"})).await;
    assert_eq!(failed["error"]["code"], CHILD_FAILED, "{failed}");
    // A result larger than a tool result may be.
    let large = json!({"question": "x".repeat(140_000), "choice": "approve"});
    let oversized = replayed(&setup, "oversized", large).await;
    assert_eq!(
        oversized["error"]["code"], TOOL_RESULT_TOO_LARGE,
        "{oversized}"
    );
    // A job that could not be continued once more starts no child. Reaching 63 continuations
    // takes 63 real calls; the counter's guard is bypassed for this one raw setup update.
    let scope = admitted_as(
        &setup.fixture,
        setup.target,
        RelatedAssociation::Company,
        "limit",
    )
    .await;
    let mut tx = p.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE background_tasks SET continuation_count=63 WHERE id=$1")
        .bind(scope.job.0)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let claim = p
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    let call = setup.call(
        setup.frozen_pin(setup.parent).await,
        json!({"question": "q", "choice": "approve"}),
    );
    let AgentToolObservation::Completed(limited) = setup.dispatch(&claim, &call).await.unwrap()
    else {
        panic!("the limit answers at once");
    };
    assert_eq!(limited["error"]["code"], TOOL_LIMIT);
    assert_eq!(children(p, claim.fence.scope.run).await, 0);
}

#[tokio::test]
async fn workflow_tool_child_output_outside_the_pinned_schema_fails_the_parent_wait() {
    let setup = Box::pin(ToolSetup::new(echo_child())).await;
    let p = setup.persistence();
    let claim = setup.claim("invalid-output").await;
    let scope = claim.fence.scope;
    let call = setup.call(
        setup.frozen_pin(setup.parent).await,
        json!({"question": "q"}),
    );
    setup.dispatch(&claim, &call).await.unwrap();
    let child = setup.finish_child(scope).await;
    // Corrupted outside the runtime (triggers bypassed for this one raw update), as only a
    // defect or a manual edit could; the parent re-checks it against its pinned copy.
    let mut tx = p.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *tx)
        .await
        .unwrap();
    let corrupted = sqlx::query("UPDATE workflow_executions SET committed_output='{\"choice\":\"maybe\",\"answer\":\"q\"}'::jsonb WHERE run_id=$1 AND completed_at IS NOT NULL")
        .bind(child.run.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    assert_eq!(corrupted.rows_affected(), 1);
    tx.commit().await.unwrap();
    for disposition in [CommitDisposition::Committed, CommitDisposition::Replayed] {
        assert_eq!(
            p.resume_wait(scope).await.unwrap(),
            WaitProgress::Failed {
                disposition,
                reason: WaitFailureReason::InvalidOutput,
            }
        );
    }
    let settled: (String, String, Option<String>) = sqlx::query_as(
        "SELECT run.state, wait.state, wait.failure_code FROM workflow_runs AS run \
         JOIN workflow_waits AS wait ON wait.company_id=run.company_id AND wait.run_id=run.id \
         WHERE run.id=$1 AND wait.reason='child_run'",
    )
    .bind(scope.run.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(
        settled,
        (
            "failed".into(),
            "failed".into(),
            Some("invalid_output".into())
        )
    );
}

#[tokio::test]
async fn workflow_tool_parent_cancel_cascades_and_a_restarted_worker_resumes_the_call() {
    let setup = Box::pin(ToolSetup::new(decision_child())).await;
    let p = setup.persistence();
    // Cancelling the parked parent cancels the call's child.
    let claim = setup.claim("cancel-parent").await;
    let scope = claim.fence.scope;
    let call = setup.call(
        setup.frozen_pin(setup.parent).await,
        json!({"question": "q"}),
    );
    setup.dispatch(&claim, &call).await.unwrap();
    let child = setup.child_of(scope).await;
    setup.cancel(scope, "cancel-parent").await;
    assert_eq!(run_state(p, scope.run).await, "cancelled");
    assert_eq!(run_state(p, child.run).await, "cancelled");

    // A worker that restarts while the call waits: nothing in memory carries the call. A fresh
    // persistence and runtime find the continuation by polling and finish the same call.
    let claim = setup.claim("restart").await;
    let scope = claim.fence.scope;
    let llm = scripted_llm(vec![
        LlmTurn::tool_call(DECLARED, json!({"question": "Refund?"})),
        LlmTurn::text("\"after restart\""),
    ])
    .await;
    register_scripted_agent_base_url(setup.agent, &llm.base_url);
    let WorkflowHandlerOutcome::Suspended(_) =
        Box::pin(setup.runtime().execute(&kind(), &claim)).await
    else {
        panic!("parks");
    };
    let child = setup.child_of(scope).await;
    let next = setup.decide(child, "reject", "No").await;
    p.advance_pure(next, budget()).await.unwrap();
    let restarted = PostgresPersistence::new(p.pool().clone());
    let work: Vec<PollWork> = restarted
        .poll_work(None, 100)
        .await
        .unwrap()
        .candidates
        .into_iter()
        .filter(|candidate| candidate.scope == scope)
        .map(|candidate| candidate.work)
        .collect();
    assert_eq!(work, vec![PollWork::Wait]);
    assert_eq!(
        restarted.resume_wait(scope).await.unwrap(),
        WaitProgress::Continued
    );
    let continued = restarted
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    let WorkflowHandlerOutcome::Completed(Ok(output)) =
        Box::pin(setup.runtime().execute(&kind(), &continued)).await
    else {
        panic!("the restarted worker finishes the call");
    };
    assert_eq!(output, json!("after restart"));
    assert_eq!(
        saved_result(p, scope).await,
        json!({"choice": "reject", "answer": "No"})
    );
    assert_eq!(llm.finish().await.unwrap(), 2);
    assert_eq!(children(p, scope.run).await, 1);
}
