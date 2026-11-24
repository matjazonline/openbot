//! agent-workflow-call p03 batch B1 (gate AWC-P03-D1 r2) against PostgreSQL: a saved
//! `channel.call` dispatched through the production port (`ChannelCallDispatch`) writes its
//! record, fresh desk thread, source message, child and wait in one transaction and parks the
//! caller; every refusal before or inside the savepoint leaves only its tool result.
use super::channel_call_support::*;
use super::workflow_tool_support::*;
use super::*;
use crate::adapters::persistence::workflow::{channel_call_source, channel_call_target::CallStart};
use crate::app_error::ExecutionFailure;
use crate::application::workflow::actions::ActionScope;
use crate::application::workflow::agent::AgentToolObservation;
use crate::application::workflow::channel_call::{
    ChannelCallDispatch, ChannelCallRequest, ChannelCallToolCall, DESTINATION_UNAVAILABLE,
    INVALID_INPUT, TARGET_UNAVAILABLE,
};
use crate::application::workflow::channel_reply::{ReplyApprovalPolicy, ReplySettingsRevision};
use crate::application::workflow::tools::{CHILD_FAILED, TOOL_LIMIT};
use crate::entities::agent_capability::{AgentBindingId, AgentId};
use chrono::Utc;
use serde_json::{Value, json};
use std::time::Duration;

fn parked(observation: AgentToolObservation) -> Uuid {
    match observation {
        AgentToolObservation::Parked(wait) => wait.as_uuid(),
        other => panic!("expected a park, got {other:?}"),
    }
}

fn waiting_parent() -> CallParked {
    CallParked {
        run: "waiting".into(),
        reason: Some("child_run".into()),
        job: "completed".into(),
        stop: Some("child_parked".into()),
        children: 1,
    }
}

fn running_parent() -> CallParked {
    CallParked {
        run: "running".into(),
        reason: None,
        job: "processing".into(),
        stop: None,
        children: 0,
    }
}

/// The caller's stored source message, as L1 asserts it.
#[derive(Debug, PartialEq, sqlx::FromRow)]
struct SourceRow {
    body: String,
    level: i16,
    exact: Option<bool>,
    author_kind: String,
    author_agent: Option<Uuid>,
    role: String,
    direction: String,
    entry_kind: String,
    producer: Option<Uuid>,
}

#[tokio::test]
async fn channel_call_launch_parks_and_commits_atomically() {
    let s = CallSetup::new().await;
    let claim = s.claim("launch-l1").await;
    let scope = claim.fence.scope;
    let before = s.desk_rows(s.desk).await;
    let saved = s
        .tool
        .channel_call(&s.arguments("Is customer 42 over its limit?"), &claim)
        .await;
    let wait = parked(
        s.p()
            .call_channel(claim.fence, saved.clone())
            .await
            .unwrap(),
    );
    // The caller waits on the child; its attempt and job are closed.
    assert_eq!(call_parked(s.p(), scope).await, waiting_parent());
    let record = s.record(scope.run).await;
    assert_eq!(record.call_id, saved.call.as_uuid());
    let named: (Uuid, Option<Uuid>, Option<Uuid>) = sqlx::query_as(
        "SELECT id, child_tool_call_id, child_run_id FROM workflow_waits WHERE run_id=$1 AND reason='child_run'",
    )
    .bind(scope.run.as_uuid())
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(
        named,
        (wait, Some(record.call_id), Some(record.child_run_id))
    );
    // One fresh desk thread holding one source message: authored by the calling agent, stamped
    // with the caller's level and exactness, sealed to the caller through its record.
    let (caller_level, caller_exact): (i16, bool) = sqlx::query_as(
        "SELECT content_level::smallint, content_exact FROM workflow_runs WHERE id=$1",
    )
    .bind(scope.run.as_uuid())
    .fetch_one(s.pool())
    .await
    .unwrap();
    let source: SourceRow = sqlx::query_as(
        "SELECT message.clean_text_body AS body, message.access_level::smallint AS level, \
           message.content_exact AS exact, author.kind AS author_kind, author.agent_id AS author_agent, \
           message.role, message.direction, member.entry_kind, seal.producer_run_id AS producer \
         FROM thread_messages AS member \
         JOIN messages AS message ON message.company_id=member.company_id AND message.id=member.message_id \
         JOIN principals AS author ON author.id=message.author_principal_id \
         LEFT JOIN workflow_message_producer_seals AS seal \
           ON seal.company_id=message.company_id AND seal.message_id=message.id \
         WHERE member.company_id=$1 AND member.channel_id=$2 AND member.thread_id=$3 AND member.message_id=$4",
    )
    .bind(s.company().as_uuid())
    .bind(s.desk)
    .bind(record.thread_id)
    .bind(record.source_message_id)
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(
        source,
        SourceRow {
            body: "Is customer 42 over its limit?".into(),
            level: caller_level,
            exact: Some(caller_exact),
            author_kind: "agent".into(),
            author_agent: Some(s.tool.agent),
            role: "agent".into(),
            direction: "inbound".into(),
            entry_kind: "delegation".into(),
            producer: Some(scope.run.as_uuid()),
        }
    );
    // The child runs on that thread with the caller's deadline (no shorter one was asked).
    let (thread, desk, same_deadline): (Option<Uuid>, Option<Uuid>, bool) = sqlx::query_as(
        "SELECT child.thread_id, child.channel_id, child.deadline=parent.deadline \
         FROM workflow_runs AS child JOIN workflow_runs AS parent ON parent.id=child.parent_run_id \
         WHERE child.id=$1",
    )
    .bind(record.child_run_id)
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(
        (thread, desk, same_deadline),
        (Some(record.thread_id), Some(s.desk), true)
    );
    // Nothing else: the source is never ingress.
    assert_eq!(
        s.desk_rows(s.desk).await,
        DeskRows {
            threads: before.threads + 1,
            messages: before.messages + 1,
            ..before
        }
    );
    assert_eq!(before, DeskRows::NONE);
}

#[tokio::test]
async fn channel_call_launch_refusal_leaves_no_thread_or_message() {
    // A role the desk's unit chain does not hold: refused inside the savepoint.
    let s = Box::pin(CallSetup::with(role_destination())).await;
    let claim = s.claim("launch-l2-role").await;
    let answer = result(s.call(&claim, &s.arguments("Roles")).await.unwrap());
    assert_eq!(code(&answer), DESTINATION_UNAVAILABLE);
    assert_eq!(s.desk_rows(s.desk).await, DeskRows::NONE);
    assert_no_call_rows(&s).await;
    assert_eq!(
        call_parked(s.p(), claim.fence.scope).await,
        running_parent()
    );
    // The desk unit's cap: refused inside the savepoint too.
    let s = CallSetup::new().await;
    s.exhaust(s.desk_unit().await).await;
    let claim = s.claim("launch-l2-cap").await;
    let answer = result(s.call(&claim, &s.arguments("Over cap")).await.unwrap());
    assert_eq!(code(&answer), crate::app_error::BudgetExhausted::CODE);
    assert_eq!(s.desk_rows(s.desk).await, DeskRows::NONE);
    assert_no_call_rows(&s).await;
    assert_eq!(
        call_parked(s.p(), claim.fence.scope).await,
        running_parent()
    );
}

#[tokio::test]
async fn channel_call_launch_origin_is_proven() {
    let s = CallSetup::new().await;
    let claim = s.claim("launch-l3").await;
    let saved = s.tool.channel_call(&s.arguments("Genuine"), &claim).await;
    let other_binding: Uuid = sqlx::query_scalar(
        "SELECT binding_id FROM agent_draft_bindings WHERE agent_id=$1 AND binding_id<>$2",
    )
    .bind(s.tool.agent)
    .bind(saved.origin.binding.as_uuid())
    .fetch_one(s.pool())
    .await
    .unwrap();
    let forge = |origin, call, arguments| ChannelCallToolCall {
        origin,
        call,
        arguments,
    };
    let cases = [
        (
            "another agent",
            forge(
                crate::application::agent::AgentToolOrigin {
                    agent: AgentId::new(Uuid::new_v4()),
                    ..saved.origin
                },
                saved.call,
                saved.arguments.clone(),
            ),
        ),
        (
            "no saved call",
            forge(
                saved.origin,
                ToolCallId::new(Uuid::new_v4()),
                saved.arguments.clone(),
            ),
        ),
        (
            "other arguments than the saved call",
            forge(saved.origin, saved.call, s.arguments("Forged")),
        ),
        (
            "the agent's workflow-tool binding",
            forge(
                crate::application::agent::AgentToolOrigin {
                    binding: AgentBindingId::new(other_binding),
                    ..saved.origin
                },
                saved.call,
                saved.arguments.clone(),
            ),
        ),
    ];
    for (case, forged) in cases {
        let refused = s.p().call_channel(claim.fence, forged).await;
        assert!(refused.is_err(), "{case}: {refused:?}");
        assert_no_call_rows(&s).await;
        assert_eq!(s.desk_rows(s.desk).await, DeskRows::NONE, "{case}");
    }
    // The genuine saved call still starts.
    parked(s.p().call_channel(claim.fence, saved).await.unwrap());
    assert_eq!(s.records().await, 1);
}

#[tokio::test]
async fn channel_call_launch_continuation_limit() {
    let s = CallSetup::new().await;
    // A job that could not be continued once more. Reaching 63 continuations takes 63 real
    // calls; the counter's guard is bypassed for this one raw setup update.
    let scope = admitted_as(
        &s.tool.fixture,
        s.tool.target,
        RelatedAssociation::Company,
        "launch-l4",
    )
    .await;
    let mut tx = s.pool().begin().await.unwrap();
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
    let claim = s
        .p()
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    // The write right is judged first: an unwritable desk is unavailable, not a limit.
    sqlx::query("UPDATE channels SET access_mode='members' WHERE id=$1")
        .bind(s.desk)
        .execute(s.pool())
        .await
        .unwrap();
    let arguments = s.arguments("Limited");
    let answer = result(s.call(&claim, &arguments).await.unwrap());
    assert_eq!(code(&answer), TARGET_UNAVAILABLE);
    sqlx::query("UPDATE channels SET access_mode='public' WHERE id=$1")
        .bind(s.desk)
        .execute(s.pool())
        .await
        .unwrap();
    // Then destination admission: the tree limit, before any row.
    let answer = result(s.call(&claim, &arguments).await.unwrap());
    assert_eq!(code(&answer), TOOL_LIMIT);
    assert_no_call_rows(&s).await;
    assert_eq!(s.desk_rows(s.desk).await, DeskRows::NONE);
}

#[tokio::test]
async fn channel_call_launch_guard_names_the_wait() {
    let s = CallSetup::new().await;
    let claim = s.claim("launch-l5").await;
    let caller = claim.fence.scope;
    let arguments = s.arguments("Guarded");
    let mut tx = s.pool().begin().await.unwrap();
    let CallStart::Started { child, .. } = s.start_in(&mut tx, &claim, &arguments).await.unwrap()
    else {
        panic!("started");
    };
    // The record is launched only by its caller's tool-call wait on its child.
    let refused = forged(
        &mut tx,
        "UPDATE workflow_waits SET child_tool_call_id=gen_random_uuid() WHERE company_id=$1 AND child_run_id=$2",
        (s.company().as_uuid(), child.as_uuid()),
        "workflow_channel_call_launch",
    )
    .await;
    assert_eq!(refused, Err("workflow_channel_call_launch".to_owned()));
    assert_eq!(
        immediate(&mut tx, "workflow_channel_call_launch").await,
        Ok(())
    );
    tx.rollback().await.unwrap();
    // A Workflow-stamped desk message for the caller without its record: the producer seal
    // refuses it.
    let deadline: chrono::DateTime<Utc> =
        sqlx::query_scalar("SELECT deadline FROM workflow_runs WHERE id=$1")
            .bind(caller.run.as_uuid())
            .fetch_one(s.pool())
            .await
            .unwrap();
    let request = ChannelCallRequest::parse(&arguments, Utc::now(), deadline).unwrap();
    let mut tx = s.pool().begin().await.unwrap();
    let author = crate::adapters::persistence::memory::agent_principal_on(
        &mut tx,
        s.company().as_uuid(),
        s.tool.agent,
    )
    .await
    .unwrap();
    channel_call_source::store_on(
        &mut tx,
        ActionScope {
            company: caller.company,
            run: caller.run,
            execution: caller.execution,
        },
        s.desk,
        &request,
        author,
    )
    .await
    .unwrap();
    let refused = sqlx::query("SET CONSTRAINTS workflow_message_producer_complete IMMEDIATE")
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("canonical workflow producer must prove its exact normalized output"),
        "{refused}"
    );
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn channel_call_launch_two_targets_differ() {
    let s = CallSetup::new().await;
    let other = s
        .tool
        .fixture
        .publish_workflow(
            WorkflowId::new(Uuid::from_u128(DESTINATION + 1)),
            destination(None),
        )
        .await;
    let (second, second_binding) = s.add_desk("payments", other, json!({"tier": "gold"})).await;
    let one = s.claim("launch-l6-a").await;
    let two = s.claim("launch-l6-b").await;
    parked(s.call(&one, &s.arguments("First")).await.unwrap());
    let to_second = json!({"channel_id": second.to_string(), "content": {"body": "Second"}});
    parked(s.call(&two, &to_second).await.unwrap());
    for (claim, desk, binding, version, params) in [
        (&one, s.desk, s.responder, s.version, json!({})),
        (&two, second, second_binding, other, json!({"tier": "gold"})),
    ] {
        let child: (Option<Uuid>, Uuid, Uuid, Value) = sqlx::query_as(
            "SELECT channel_id, binding_id, version_id, params FROM workflow_runs WHERE parent_run_id=$1",
        )
        .bind(claim.fence.scope.run.as_uuid())
        .fetch_one(s.pool())
        .await
        .unwrap();
        assert_eq!(
            child,
            (Some(desk), binding.as_uuid(), version.as_uuid(), params)
        );
        // The caller itself is on no channel.
        let caller: Option<Uuid> =
            sqlx::query_scalar("SELECT channel_id FROM workflow_runs WHERE id=$1")
                .bind(claim.fence.scope.run.as_uuid())
                .fetch_one(s.pool())
                .await
                .unwrap();
        assert_eq!(caller, None);
    }
}

#[tokio::test]
async fn channel_call_launch_history_is_the_fresh_thread() {
    let mut history: Value =
        serde_json::from_str(&registry::example("context.history").unwrap().source).unwrap();
    history["input_schema"] = json!({"type": "object"});
    let s = Box::pin(CallSetup::with(history)).await;
    // An earlier call leaves another thread on the same desk.
    let earlier = s.claim("launch-l7-a").await;
    parked(s.call(&earlier, &s.arguments("Earlier")).await.unwrap());
    let claim = s.claim("launch-l7-b").await;
    parked(s.call(&claim, &s.arguments("Now")).await.unwrap());
    let record = s.record(claim.fence.scope.run).await;
    let members: Vec<Uuid> = sqlx::query_scalar(
        "SELECT message_id FROM workflow_history_members WHERE run_id=$1 ORDER BY ordinal",
    )
    .bind(record.child_run_id)
    .fetch_all(s.pool())
    .await
    .unwrap();
    assert_eq!(members, vec![record.source_message_id]);
    let eligible: i64 =
        sqlx::query_scalar("SELECT eligible_count FROM workflow_history_snapshots WHERE run_id=$1")
            .bind(record.child_run_id)
            .fetch_one(s.pool())
            .await
            .unwrap();
    assert_eq!(eligible, 1);
}

#[tokio::test]
async fn channel_call_launch_competing_claimants_converge() {
    let s = CallSetup::new().await;
    let claim = s.claim("launch-l8").await;
    let saved = s.tool.channel_call(&s.arguments("Twice"), &claim).await;
    let (one, two) = tokio::join!(
        s.p().call_channel(claim.fence, saved.clone()),
        s.p().call_channel(claim.fence, saved.clone())
    );
    let outcomes = [one, two];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, Ok(AgentToolObservation::Parked(_))))
            .count(),
        1,
        "{outcomes:?}"
    );
    assert!(
        outcomes.iter().any(|outcome| matches!(
            outcome,
            Err(AppError::Execution(ExecutionFailure::OwnershipLost))
        )),
        "{outcomes:?}"
    );
    assert_eq!(s.records().await, 1);
    assert_eq!(children(s.p(), claim.fence.scope.run).await, 1);
    assert_eq!(s.desk_rows(s.desk).await.threads, 1);
}

#[tokio::test]
async fn channel_call_launch_lease_expired_at_commit() {
    let s = CallSetup::new().await;
    let claim = s.claim("launch-l9").await;
    let scope = claim.fence.scope;
    let saved = s.tool.channel_call(&s.arguments("Late"), &claim).await;
    // The destination version's row is held, so the child insert waits past the lease's
    // expiry, after the fence was locked; the park is then refused and everything rolls back.
    let mut blocker = s.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_versions WHERE id=$1 FOR UPDATE")
        .bind(s.version.as_uuid())
        .execute(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("UPDATE background_tasks SET lock_expires_at=clock_timestamp()+interval '300 milliseconds' WHERE id=$1")
        .bind(scope.job.0)
        .execute(s.pool())
        .await
        .unwrap();
    let release = async {
        tokio::time::sleep(Duration::from_millis(600)).await;
        blocker.commit().await.unwrap();
    };
    let (_, dispatched) = tokio::join!(release, s.p().call_channel(claim.fence, saved));
    let Err(AppError::Conflict(refusal)) = &dispatched else {
        panic!("{dispatched:?}");
    };
    assert!(refusal.contains("park refused"), "{refusal}");
    assert_no_call_rows(&s).await;
    assert_eq!(s.desk_rows(s.desk).await, DeskRows::NONE);
    assert_eq!(call_parked(s.p(), scope).await, running_parent());
    // The expired attempt is retired and the job waits for its retry.
    assert!(s.p().retire_expired_io(scope, lease()).await.unwrap());
    let job: (String, bool) = sqlx::query_as(
        "SELECT status, retry_count < max_retries FROM background_tasks WHERE id=$1",
    )
    .bind(scope.job.0)
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(job, ("pending".into(), true));
}

/// A reviewed desk takes calls (AWC-C004, p04 D4): the call starts and its record captures
/// the desk's policy, which governs the child's final review.
#[tokio::test]
async fn channel_call_launch_review_desk_captures_policy() {
    let s = CallSetup::new().await;
    let reviewer = s.tool.fixture.principal(s.run_as).await;
    let policy = ReplyApprovalPolicy::new(vec![reviewer], 60).unwrap();
    s.select_on(
        s.desk,
        s.responder,
        Some(ReplySettingsRevision::try_from(1).unwrap()),
        Some(policy.clone()),
    )
    .await
    .unwrap();
    let claim = s.claim("launch-l10").await;
    parked(s.call(&claim, &s.arguments("Reviewed")).await.unwrap());
    let captured: Option<Value> =
        sqlx::query_scalar("SELECT approval_policy FROM workflow_channel_calls WHERE run_id=$1")
            .bind(claim.fence.scope.run.as_uuid())
            .fetch_one(s.pool())
            .await
            .unwrap();
    assert_eq!(captured, Some(serde_json::to_value(&policy).unwrap()));
}

#[tokio::test]
async fn channel_call_launch_malformed_desk_answers_first() {
    let s = CallSetup::new().await;
    let claim = s.claim("launch-l11").await;
    let saved = s
        .tool
        .channel_call(
            &json!({"channel_id": "not-a-desk", "content": {"body": "x"}}),
            &claim,
        )
        .await;
    // The company row is held: any lock the dispatch took would wait on it.
    let mut blocker = s.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR UPDATE")
        .bind(s.company().as_uuid())
        .execute(&mut *blocker)
        .await
        .unwrap();
    let answered = tokio::time::timeout(
        Duration::from_secs(5),
        s.p().call_channel(claim.fence, saved),
    )
    .await
    .expect("answers without waiting on a lock")
    .unwrap();
    blocker.rollback().await.unwrap();
    assert_eq!(code(&result(answered)), INVALID_INPUT);
    assert_no_call_rows(&s).await;
}

#[tokio::test]
async fn channel_call_launch_shorter_deadline_fails_child() {
    let s = CallSetup::new().await;
    let claim = s.claim("launch-l12").await;
    let scope = claim.fence.scope;
    let deadline = Utc::now() + chrono::Duration::seconds(3);
    let arguments = json!({
        "channel_id": s.desk.to_string(),
        "content": {"body": "Quickly"},
        "deadline": deadline.to_rfc3339(),
    });
    parked(s.call(&claim, &arguments).await.unwrap());
    let child = s.tool.child_of(scope).await;
    // The child's deadline is the call's, earlier than the caller's.
    let (child_deadline, shorter): (chrono::DateTime<Utc>, bool) = sqlx::query_as(
        "SELECT child.deadline, child.deadline<parent.deadline FROM workflow_runs AS child \
         JOIN workflow_runs AS parent ON parent.id=child.parent_run_id WHERE child.id=$1",
    )
    .bind(child.run.as_uuid())
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(
        child_deadline.timestamp_micros(),
        deadline.timestamp_micros()
    );
    assert!(shorter);
    // Its expiry fails the child, and the caller is told `workflow.child_failed`.
    let wait = (deadline - Utc::now()).to_std().unwrap_or_default() + Duration::from_millis(200);
    tokio::time::sleep(wait).await;
    assert!(s.p().expire_run(child).await.unwrap());
    assert_eq!(run_state(s.p(), child.run).await, "failed");
    assert_eq!(
        s.p().resume_wait(scope).await.unwrap(),
        WaitProgress::Continued
    );
    let continued = s
        .p()
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    let answer = result(s.call(&continued, &arguments).await.unwrap());
    assert_eq!(code(&answer), CHILD_FAILED);
    assert_eq!(continued.fence.scope.execution, scope.execution);
}
