//! agent-workflow-call p03 batch B2 (gate AWC-P03-D1 r2 D4–D6) against PostgreSQL: a parked
//! channel call is continued by its child's terminal fact, and the resumed attempt replays the
//! saved call from its immutable record: live authority first, then the child's outcome through
//! the captured reply contract, absorbed into the caller only when disclosed.
use super::channel_call_support::*;
use super::workflow_tool_support::*;
use super::*;
use crate::adapters::persistence::workflow::knowledge;
use crate::application::workflow::activation::ActivationRequest;
use crate::application::workflow::agent::AgentToolObservation;
use crate::application::workflow::batch::WorkflowBatch;
use crate::application::workflow::channel_call::{
    ChannelCallDispatch, ChannelCallToolCall, INVALID_INPUT, RESPONSE_INVALID, REVOKED,
};
use crate::application::workflow::channel_reply::ReplySettingsRevision;
use crate::application::workflow::polling::{PollWork, WorkflowPolling};
use crate::application::workflow::tools::{CHILD_CANCELLED, CHILD_FAILED, TOOL_RESULT_TOO_LARGE};
use crate::entities::access_level::AccessLevel;
use crate::services::test_support::{LlmTurn, register_scripted_agent_base_url, scripted_llm};
use chrono::Utc;
use serde_json::json;
use std::time::Duration;

const DECLARED_CALL: &str = "channel__call";

async fn waiting_waits(s: &CallSetup, run: RunId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM workflow_waits WHERE run_id=$1 AND state='waiting'")
        .bind(run.as_uuid())
        .fetch_one(s.pool())
        .await
        .unwrap()
}

async fn continuation(s: &CallSetup, scope: ActivationRequest) -> i16 {
    sqlx::query_scalar("SELECT continuation_count FROM background_tasks WHERE id=$1")
        .bind(scope.job.0)
        .fetch_one(s.pool())
        .await
        .unwrap()
}

/// Cancel `run` through the run control, re-reading its revision when a racing transition
/// moved it. Each attempt takes its own command key: the control stores a conflicting attempt
/// under its key and expected revision, and refuses that key with another revision as a
/// mismatched replay (AWC-P04-B1-F1).
async fn cancel(s: &CallSetup, run: RunId, key: &str) {
    for attempt in 0..10 {
        let revision = s
            .p()
            .head(s.company(), run)
            .await
            .unwrap()
            .unwrap()
            .revision;
        match s
            .p()
            .cancel(CancelCommand {
                company_id: s.company(),
                actor: s.owner(),
                run_id: run,
                command_key: IdempotencyKey::parse(format!("{key}-{attempt}")).unwrap(),
                expected_revision: revision,
            })
            .await
            .unwrap()
        {
            CancelResult::Applied { .. } | CancelResult::AlreadyTerminalOrApplied { .. } => return,
            CancelResult::RevisionConflict { .. } => continue,
            CancelResult::NotFound => panic!("no run {run}"),
        }
    }
    panic!("cancel kept conflicting");
}

#[tokio::test]
async fn channel_call_resume_scripted_agent_continues() {
    let s = CallSetup::new().await;
    let claim = s.claim("resume-r1").await;
    let scope = claim.fence.scope;
    let arguments = s.arguments("Is customer 42 over its limit?");
    let llm = scripted_llm(vec![
        LlmTurn::tool_call(DECLARED_CALL, arguments.clone()),
        LlmTurn::text("\"answered\""),
    ])
    .await;
    register_scripted_agent_base_url(s.tool.agent, &llm.base_url);
    let runtime = s.tool.runtime();
    let WorkflowHandlerOutcome::Suspended(_) = Box::pin(runtime.execute(&kind(), &claim)).await
    else {
        panic!("the call parks the agent");
    };
    let child = s.tool.finish_child(scope).await;
    assert_eq!(run_state(s.p(), child.run).await, "succeeded");
    let resumed = continued(&s, scope).await;
    assert_eq!(resumed.fence.scope.execution, scope.execution);
    let WorkflowHandlerOutcome::Completed(Ok(output)) =
        Box::pin(runtime.execute(&kind(), &resumed)).await
    else {
        panic!("the resumed agent consumes the answer and finishes");
    };
    assert_eq!(output, json!("answered"));
    assert_eq!(llm.finish().await.unwrap(), 2);
    assert_eq!(saved_result(s.p(), scope).await, done(child.run));
    assert_eq!(continuation(&s, scope).await, 1);
    assert_eq!(children(s.p(), scope.run).await, 1);
}

/// C2 (gate D6): a scripted agent answering a thread continues after the call; the caller
/// note lands on its thread mid-turn without disturbing the frozen history.
#[tokio::test]
async fn channel_call_trail_agent_caller_continues() {
    let s = CallSetup::new().await;
    let (claim, thread) = s.claim_in_thread("trail-agent").await;
    let scope = claim.fence.scope;
    let arguments = s.arguments("Is customer 42 over its limit?");
    let llm = scripted_llm(vec![
        LlmTurn::tool_call(DECLARED_CALL, arguments.clone()),
        LlmTurn::text("\"answered\""),
    ])
    .await;
    register_scripted_agent_base_url(s.tool.agent, &llm.base_url);
    let runtime = s.tool.runtime();
    let WorkflowHandlerOutcome::Suspended(_) = Box::pin(runtime.execute(&kind(), &claim)).await
    else {
        panic!("the call parks the agent");
    };
    let child = s.tool.finish_child(scope).await;
    let resumed = continued(&s, scope).await;
    let WorkflowHandlerOutcome::Completed(Ok(output)) =
        Box::pin(runtime.execute(&kind(), &resumed)).await
    else {
        panic!("the resumed agent consumes the answer and finishes");
    };
    assert_eq!(output, json!("answered"));
    assert_eq!(llm.finish().await.unwrap(), 2);
    assert_eq!(saved_result(s.p(), scope).await, done(child.run));
    let notes: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM thread_messages AS filed JOIN messages AS message \
           ON message.company_id=filed.company_id AND message.id=filed.message_id \
         WHERE filed.thread_id=$1 AND message.subject LIKE 'Call to %'",
    )
    .bind(thread)
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(notes, 1);
}

#[tokio::test]
async fn channel_call_resume_replay_never_reselects() {
    let s = CallSetup::new().await;
    // The call asks for a deadline of its own, which passes before the replay.
    let deadline = Utc::now() + chrono::Duration::seconds(3);
    let arguments = json!({"channel_id": s.desk.to_string(), "content": {"body": "Soon"},
        "deadline": deadline.to_rfc3339()});
    let (scope, child) = finished(&s, "resume-r2", &arguments).await;
    // Another responder is selected while the caller waits.
    let other = s.responder_as(s.run_as).await;
    s.select(other, Some(ReplySettingsRevision::try_from(1).unwrap()))
        .await
        .unwrap();
    let resumed = continued(&s, scope).await;
    tokio::time::sleep((deadline - Utc::now()).to_std().unwrap_or_default()).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let first = result(s.call(&resumed, &arguments).await.unwrap());
    let again = result(s.call(&resumed, &arguments).await.unwrap());
    assert_eq!(first, done(child));
    assert_eq!(again, first);
    assert_eq!(s.records().await, 1);
    assert_eq!(children(s.p(), scope.run).await, 1);
    // The saved call's ID with other arguments.
    let saved = s.tool.channel_call(&arguments, &resumed).await;
    let changed = ChannelCallToolCall {
        arguments: s.arguments("Changed"),
        ..saved
    };
    let refused = result(s.p().call_channel(resumed.fence, changed).await.unwrap());
    assert_eq!(code(&refused), INVALID_INPUT);
}

#[tokio::test]
async fn channel_call_resume_outcomes() {
    // Succeeded: the answer is disclosed and its facts absorbed.
    let s = CallSetup::new().await;
    let arguments = s.arguments("Fine");
    let (scope, child) = finished(&s, "resume-r3-done", &arguments).await;
    taint(&s, child).await;
    let resumed = continued(&s, scope).await;
    assert_eq!(
        result(s.call(&resumed, &arguments).await.unwrap()),
        done(child)
    );
    assert_eq!(levels(&s, scope.run).await, (2, false));
    // Failed: the child's output breaks its own schema.
    let mut failing = destination(None);
    failing["output_schema"] = json!({"type": "object", "required": ["answer"]});
    let s = Box::pin(CallSetup::with(failing)).await;
    let arguments = s.arguments("Fails");
    let (scope, child) = finished(&s, "resume-r3-failed", &arguments).await;
    assert_eq!(run_state(s.p(), child).await, "failed");
    taint(&s, child).await;
    let before = levels(&s, scope.run).await;
    let resumed = continued(&s, scope).await;
    assert_eq!(
        code(&result(s.call(&resumed, &arguments).await.unwrap())),
        CHILD_FAILED
    );
    assert_eq!(levels(&s, scope.run).await, before);
    // Cancelled while the caller waits (the child is still queued: nothing advanced it).
    let s = CallSetup::new().await;
    let arguments = s.arguments("Cancel me");
    let claim = s.claim("resume-r3-cancelled").await;
    let scope = claim.fence.scope;
    assert!(matches!(
        s.call(&claim, &arguments).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let child = s.tool.child_of(scope).await;
    cancel(&s, child.run, "cancel-call-child").await;
    let before = levels(&s, scope.run).await;
    let resumed = continued(&s, scope).await;
    assert_eq!(
        code(&result(s.call(&resumed, &arguments).await.unwrap())),
        CHILD_CANCELLED
    );
    assert_eq!(levels(&s, scope.run).await, before);
    // A stored reply that does not decode is a defect, guarded by `response_invalid`; an
    // unprojectable output fails inside the child instead (AWC-C004 r2). The proposal is
    // corrupted consistently with its integrity hash, triggers bypassed for this raw update.
    let s = CallSetup::new().await;
    let arguments = s.arguments("Undecodable");
    let (scope, child) = finished(&s, "resume-r3-invalid", &arguments).await;
    taint(&s, child).await;
    let mut tx = s.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE channel_reply_proposals SET content=broken.content, integrity_document=broken.document, \
           integrity=encode(sha256(convert_to(broken.document::text,'UTF8')),'hex') \
         FROM (SELECT id, '{\"subject\":\"Status\"}'::jsonb AS content, \
                 jsonb_set(integrity_document,'{content}','{\"subject\":\"Status\"}'::jsonb) AS document \
               FROM channel_reply_proposals WHERE run_id=$1) AS broken \
         WHERE channel_reply_proposals.id=broken.id",
    )
    .bind(child.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let before = levels(&s, scope.run).await;
    let resumed = continued(&s, scope).await;
    assert_eq!(
        code(&result(s.call(&resumed, &arguments).await.unwrap())),
        RESPONSE_INVALID
    );
    assert_eq!(levels(&s, scope.run).await, before);
    // An answer larger than a tool result may be.
    let mut large = destination(None);
    large["steps"]["answer"]["with"]["value"] =
        json!({"literal": {"done": true, "fields": {"note": "x".repeat(131_000)}}});
    let s = Box::pin(CallSetup::with(large)).await;
    let arguments = s.arguments("Large");
    let (scope, child) = finished(&s, "resume-r3-large", &arguments).await;
    taint(&s, child).await;
    let before = levels(&s, scope.run).await;
    let resumed = continued(&s, scope).await;
    assert_eq!(
        code(&result(s.call(&resumed, &arguments).await.unwrap())),
        TOOL_RESULT_TOO_LARGE
    );
    assert_eq!(levels(&s, scope.run).await, before);
}

/// What a waiting call loses before its replay.
#[derive(Debug, Clone, Copy)]
enum Revocation {
    Grant,
    Desk,
    Binding,
    Capability,
}

async fn revoke(s: &CallSetup, revocation: Revocation) {
    match revocation {
        Revocation::Grant => s.participate(false).await,
        Revocation::Desk => {
            sqlx::query("UPDATE channels SET enabled=false WHERE id=$1")
                .bind(s.desk)
                .execute(s.pool())
                .await
                .unwrap();
        }
        Revocation::Binding => {
            let p = s.p();
            let state = p.binding(s.company(), s.responder).await.unwrap().unwrap();
            BindingService::new(Preflight, p.clone(), p.clone(), p.clone())
                .deactivate(SetBindingActivity {
                    target: BindingTarget {
                        company: s.company(),
                        actor: s.owner(),
                        binding: s.responder,
                    },
                    expected: state.revision,
                })
                .await
                .unwrap();
        }
        Revocation::Capability => {
            // The agent is republished with its workflow tool only.
            let workflow_tool =
                crate::adapters::persistence::agent_binding_selection::resolve_bindings_on(
                    &mut s.pool().acquire().await.unwrap(),
                    s.company().as_uuid(),
                    None,
                    &[TypeName::parse(TOOL).unwrap()],
                )
                .await
                .unwrap();
            crate::application::use_cases::agent::AgentPersistence::update(
                s.p(),
                s.tool.agent,
                crate::application::use_cases::agent::AgentWrite {
                    name: "Assistant".into(),
                    slug: "assistant".into(),
                    provider: Some(crate::services::test_support::SCRIPTED_PROVIDER.into()),
                    model: Some(crate::services::test_support::SCRIPTED_MODEL.into()),
                    tool_bindings: Some(workflow_tool),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            s.tool.fixture.publish_agent(s.tool.agent).await;
        }
    }
}

#[tokio::test]
async fn channel_call_resume_revoked() {
    for revocation in [
        Revocation::Grant,
        Revocation::Desk,
        Revocation::Binding,
        Revocation::Capability,
    ] {
        let s = CallSetup::new().await;
        if let Revocation::Grant = revocation {
            sqlx::query("UPDATE channels SET access_mode='members' WHERE id=$1")
                .bind(s.desk)
                .execute(s.pool())
                .await
                .unwrap();
            s.participate(true).await;
        }
        let arguments = s.arguments("Still allowed?");
        // Revoked while the call waits: nothing is disclosed or absorbed.
        let (scope, child) = finished(&s, "resume-r4", &arguments).await;
        taint(&s, child).await;
        let before = levels(&s, scope.run).await;
        revoke(&s, revocation).await;
        let resumed = continued(&s, scope).await;
        let answer = result(s.call(&resumed, &arguments).await.unwrap());
        assert_eq!(code(&answer), REVOKED, "{revocation:?}: {answer}");
        assert!(answer.get("content").is_none(), "{revocation:?}");
        assert_eq!(levels(&s, scope.run).await, before, "{revocation:?}");
        assert_eq!(
            run_state(s.p(), scope.run).await,
            "running",
            "{revocation:?}"
        );
    }
}

#[tokio::test]
async fn channel_call_resume_revocation_races_consumption() {
    for revocation in [
        Revocation::Grant,
        Revocation::Desk,
        Revocation::Binding,
        Revocation::Capability,
    ] {
        let s = CallSetup::new().await;
        if let Revocation::Grant = revocation {
            sqlx::query("UPDATE channels SET access_mode='members' WHERE id=$1")
                .bind(s.desk)
                .execute(s.pool())
                .await
                .unwrap();
            s.participate(true).await;
        }
        let arguments = s.arguments("Race");
        let (scope, child) = finished(&s, "resume-r4-race", &arguments).await;
        let resumed = continued(&s, scope).await;
        // The writer and the consumption serialize on the rows the consumption share-locks:
        // either order, and neither deadlocks.
        let (_, consumed) = tokio::time::timeout(Duration::from_secs(20), async {
            tokio::join!(revoke(&s, revocation), s.call(&resumed, &arguments))
        })
        .await
        .unwrap_or_else(|_| panic!("{revocation:?}: deadlocked"));
        let answer = result(consumed.unwrap());
        assert!(
            answer == done(child) || code(&answer) == REVOKED,
            "{revocation:?}: {answer}"
        );
    }
}

#[tokio::test]
async fn channel_call_resume_competing_resumers() {
    let s = CallSetup::new().await;
    let arguments = s.arguments("Resume once");
    let (scope, child) = finished(&s, "resume-r5", &arguments).await;
    let (one, two) = tokio::join!(s.p().resume_wait(scope), s.p().resume_wait(scope));
    let progress = [one, two];
    assert_eq!(
        progress
            .iter()
            .filter(|progress| matches!(progress, Ok(WaitProgress::Continued)))
            .count(),
        1,
        "{progress:?}"
    );
    assert_eq!(waiting_waits(&s, scope.run).await, 0);
    assert_eq!(continuation(&s, scope).await, 1);
    let resumed = s
        .p()
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    let llm = scripted_llm(vec![LlmTurn::text("\"once\"")]).await;
    register_scripted_agent_base_url(s.tool.agent, &llm.base_url);
    let WorkflowHandlerOutcome::Completed(Ok(output)) =
        Box::pin(s.tool.runtime().execute(&kind(), &resumed)).await
    else {
        panic!("the one continuation finishes");
    };
    assert_eq!(output, json!("once"));
    assert_eq!(saved_result(s.p(), scope).await, done(child));
    let invocations: i32 = sqlx::query_scalar(
        "SELECT jsonb_array_length(checkpoint->'invocations') FROM task_harness_runs WHERE workflow_execution_id=$1",
    )
    .bind(scope.execution.as_uuid())
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(invocations, 1);
}

#[tokio::test]
async fn channel_call_resume_cancel_races() {
    // Cancel racing the launch: whichever wins, no live child stays under the cancelled caller.
    let s = CallSetup::new().await;
    let claim = s.claim("resume-r6-launch").await;
    let scope = claim.fence.scope;
    let saved = s.tool.channel_call(&s.arguments("Racing"), &claim).await;
    let (dispatched, _) = tokio::join!(
        s.p().call_channel(claim.fence, saved),
        cancel(&s, scope.run, "cancel-launch")
    );
    assert_eq!(
        run_state(s.p(), scope.run).await,
        "cancelled",
        "{dispatched:?}"
    );
    let live: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_runs WHERE parent_run_id=$1 AND state IN ('queued','running','waiting')",
    )
    .bind(scope.run.as_uuid())
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(live, 0, "{dispatched:?}");
    // Cancel racing the child's completion: the late terminal fact never resurrects the caller.
    let claim = s.claim("resume-r6-complete").await;
    let scope = claim.fence.scope;
    assert!(matches!(
        s.call(&claim, &s.arguments("Completing")).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let child = s.tool.child_of(scope).await;
    let (finished, _) = tokio::join!(
        s.p().advance_pure(child, budget()),
        cancel(&s, scope.run, "cancel-complete")
    );
    let _ = finished;
    let _ = s.p().resume_wait(scope).await;
    assert_eq!(run_state(s.p(), scope.run).await, "cancelled");
    assert!(
        s.p()
            .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(waiting_waits(&s, scope.run).await, 0);
}

#[tokio::test]
async fn channel_call_resume_archive_cancels_call_child() {
    // The caller runs on the company, outside the archived unit; its child, still queued,
    // runs on the desk.
    let s = CallSetup::new().await;
    let arguments = s.arguments("Archive me");
    let claim = s.claim("resume-r7").await;
    let scope = claim.fence.scope;
    assert!(matches!(
        s.call(&claim, &arguments).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let child = s.tool.child_of(scope).await;
    crate::adapters::persistence::test_support::department_use_cases(s.p())
        .archive_department(
            s.owner().user_id(),
            s.company().as_uuid(),
            crate::entities::department::DepartmentId::new(s.desk_unit().await),
            crate::application::ports::department::OpenWork::Cancel,
        )
        .await
        .unwrap();
    assert_eq!(run_state(s.p(), child.run).await, "cancelled");
    let resumed = continued(&s, scope).await;
    assert_eq!(
        code(&result(s.call(&resumed, &arguments).await.unwrap())),
        CHILD_CANCELLED
    );
}

#[tokio::test]
async fn channel_call_resume_destination_audience() {
    let s = CallSetup::new().await;
    let claim = s.claim("resume-r8").await;
    assert!(matches!(
        s.call(&claim, &s.arguments("Audience")).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let record = s.record(claim.fence.scope.run).await;
    let child = RunId::new(record.child_run_id);
    let destination = || async {
        let mut db = s.pool().acquire().await.unwrap();
        knowledge::destination_level_on(&mut db, s.company(), child)
            .await
            .unwrap()
    };
    assert_eq!(destination().await, AccessLevel::PUBLIC);
    // A raised allowance, as request rules will give (decision U4: none yet). On an agent-only
    // thread the destination is the ceiling, not the empty audience.
    taint(&s, child).await;
    assert_eq!(destination().await, AccessLevel::try_from(2).unwrap());
    // A human reader at a lower level lowers it.
    let reader = member(&s.tool.fixture, "member").await;
    sqlx::query(
        "UPDATE department_members SET access_level=1 WHERE company_id=$1 AND root_user_id=$2",
    )
    .bind(s.company().as_uuid())
    .bind(reader)
    .execute(s.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO thread_principals (company_id, channel_id, thread_id, principal_id, role) \
         VALUES ($1, $2, $3, $4, 'participant')",
    )
    .bind(s.company().as_uuid())
    .bind(s.desk)
    .bind(record.thread_id)
    .bind(s.tool.fixture.principal(reader).await)
    .execute(s.pool())
    .await
    .unwrap();
    assert_eq!(destination().await, AccessLevel::try_from(1).unwrap());
}

#[tokio::test]
async fn channel_call_resume_restart_at_each_boundary() {
    let s = CallSetup::new().await;
    let claim = s.claim("resume-r9").await;
    let scope = claim.fence.scope;
    let arguments = s.arguments("Survive restarts");
    let llm = scripted_llm(vec![
        LlmTurn::tool_call(DECLARED_CALL, arguments.clone()),
        LlmTurn::text("\"after restarts\""),
    ])
    .await;
    register_scripted_agent_base_url(s.tool.agent, &llm.base_url);
    // After the dispatch commit: nothing in memory carries the call.
    let WorkflowHandlerOutcome::Suspended(_) =
        Box::pin(s.tool.runtime().execute(&kind(), &claim)).await
    else {
        panic!("parks");
    };
    let restarted = PostgresPersistence::new(s.pool().clone());
    let work = |p: PostgresPersistence| async move {
        p.poll_work(None, 100)
            .await
            .unwrap()
            .candidates
            .into_iter()
            .filter(|candidate| candidate.scope.run == scope.run)
            .map(|candidate| candidate.work)
            .collect::<Vec<_>>()
    };
    assert_eq!(work(restarted.clone()).await, Vec::<PollWork>::new());
    // After the child's terminal fact: a restarted poller finds the wakeup.
    let child = s.tool.child_of(scope).await;
    restarted.advance_pure(child, budget()).await.unwrap();
    let restarted = PostgresPersistence::new(s.pool().clone());
    assert_eq!(work(restarted.clone()).await, vec![PollWork::Wait]);
    // After the wakeup: the job is pending for any worker.
    assert_eq!(
        restarted.resume_wait(scope).await.unwrap(),
        WaitProgress::Continued
    );
    let restarted = PostgresPersistence::new(s.pool().clone());
    assert_eq!(work(restarted.clone()).await, vec![PollWork::Job]);
    // After the replay, before the harness saved it: the worker dies with the result in hand.
    let first = restarted
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    let saved = s.tool.channel_call(&arguments, &first).await;
    let replayed = result(restarted.call_channel(first.fence, saved).await.unwrap());
    assert_eq!(replayed, done(child.run));
    let mut tx = s.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE background_tasks SET locked_at=clock_timestamp()-interval '10 seconds',lock_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1")
        .bind(scope.job.0)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let restarted = PostgresPersistence::new(s.pool().clone());
    assert!(restarted.retire_expired_io(scope, lease()).await.unwrap());
    let mut tx = s.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE background_tasks SET run_at=clock_timestamp() WHERE id=$1")
        .bind(scope.job.0)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let last = restarted
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .expect("the retried attempt");
    let WorkflowHandlerOutcome::Completed(Ok(output)) =
        Box::pin(s.tool.runtime().execute(&kind(), &last)).await
    else {
        panic!("the retried attempt replays the call and finishes");
    };
    assert_eq!(output, json!("after restarts"));
    // One child, one result, one execution, a monotone continuation and no stranded wait.
    assert_eq!(llm.finish().await.unwrap(), 2);
    assert_eq!(saved_result(s.p(), scope).await, done(child.run));
    assert_eq!(children(s.p(), scope.run).await, 1);
    assert_eq!(s.records().await, 1);
    assert_eq!(first.fence.scope.execution, scope.execution);
    assert_eq!(last.fence.scope.execution, scope.execution);
    assert_eq!(continuation(&s, scope).await, 1);
    assert_eq!(waiting_waits(&s, scope.run).await, 0);
    // The one lost attempt is the job's one retry (doc 06 A scenario 5, retry accounting).
    let retries: i32 = sqlx::query_scalar("SELECT retry_count FROM background_tasks WHERE id=$1")
        .bind(scope.job.0)
        .fetch_one(s.pool())
        .await
        .unwrap();
    assert_eq!(retries, 1);
}
