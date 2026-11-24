//! agent-workflow-call p05 B2 (gate AWC-P05-D2 r2) against PostgreSQL: run inspection of a
//! `channel.call` (V2–V5), sample capture of call targets (V8) and the call gauges (V9). Pages
//! are rendered by the production run routes, so `call_access` decides every link.
use super::channel_call_support::*;
use super::workflow_tool_support::*;
use super::*;
use crate::application::workflow::agent::AgentToolObservation;
use crate::application::workflow::batch::WorkflowBatch;
use crate::application::workflow::channel_call_views::{CallPhase, CallWait};
use crate::application::workflow::channel_reply::{ReplyApprovalPolicy, ReplySettingsRevision};
use crate::application::workflow::decisions::WorkflowDecisions;
use crate::application::workflow::metrics::{WorkflowMetricPersistence, WorkflowMetrics};
use crate::application::workflow::run_views::{RunReadScope, RunView, RunViews};
use crate::application::workflow::sample::{
    SampleEffectKind, SampleRequest, SampleStatus, execute,
};
use crate::application::workflow::sample_capture::SampleDefinitionCapture;
use crate::services::test_support::{LlmTurn, register_scripted_agent_base_url, scripted_llm};
use serde_json::{Value, json};

const DECLARED_CALL: &str = "channel__call";
const SECRET: &str = "SECRET-ARGUMENT-7f3a";

async fn view(s: &CallSetup, viewer: Uuid, run: RunId) -> RunView {
    s.p()
        .run_view(RunReadScope {
            company: s.company(),
            run,
            viewer: WorkflowActor::authenticated(viewer).unwrap(),
        })
        .await
        .unwrap()
        .unwrap()
}

async fn phases(s: &CallSetup, run: RunId) -> Vec<CallPhase> {
    view(s, s.owner().user_id(), run)
        .await
        .channel_calls
        .items
        .into_iter()
        .map(|item| item.phase)
        .collect()
}

async fn phase(s: &CallSetup, run: RunId) -> CallPhase {
    let mut all = phases(s, run).await;
    assert_eq!(all.len(), 1, "{all:?}");
    all.remove(0)
}

/// The run pane (or one step's body) through the production route, as `user` sees it.
async fn page(s: &CallSetup, user: Uuid, run: RunId, execution: Option<Uuid>) -> AppResult<String> {
    crate::adapters::http::routes::workflows::run_page_for_tests(
        s.p(),
        user,
        s.company().as_uuid(),
        run.as_uuid(),
        execution,
    )
    .await
}

/// The call wait's audit events, in order.
async fn wait_events(s: &CallSetup, caller: RunId, call: Uuid) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT event.event_kind FROM workflow_run_events AS event \
           JOIN workflow_waits AS wait ON wait.company_id=event.company_id AND wait.id=event.wait_id \
          WHERE event.run_id=$1 AND wait.child_tool_call_id=$2 ORDER BY event.sequence",
    )
    .bind(caller.as_uuid())
    .bind(call)
    .fetch_all(s.pool())
    .await
    .unwrap()
}

/// Expire the lease of `job`'s claim, or restore it.
async fn lease_expired(s: &CallSetup, job: Uuid, expired: bool) {
    sqlx::query(
        "UPDATE background_tasks SET locked_at = locked_at + shift, lock_expires_at = lock_expires_at + shift \
           FROM (SELECT CASE WHEN $2 THEN interval '-1 hour' ELSE interval '1 hour' END AS shift) AS moved \
          WHERE id=$1",
    )
    .bind(job)
    .bind(expired)
    .execute(s.pool())
    .await
    .unwrap();
}

/// Make the pending jobs of `run`'s unfinished steps overdue past the stall bound.
async fn overdue(s: &CallSetup, run: RunId) {
    let moved = sqlx::query(
        "UPDATE background_tasks SET run_at = clock_timestamp() - interval '10 minutes' \
          WHERE status='pending' AND workflow_execution_id IN \
            (SELECT id FROM workflow_executions WHERE run_id=$1 AND completed_at IS NULL)",
    )
    .bind(run.as_uuid())
    .execute(s.pool())
    .await
    .unwrap();
    assert!(moved.rows_affected() > 0, "the child has a pending job");
}

async fn level(s: &CallSetup, user: Uuid, value: i16) {
    sqlx::query(
        "UPDATE department_members SET access_level=$3 WHERE company_id=$1 AND root_user_id=$2",
    )
    .bind(s.company().as_uuid())
    .bind(user)
    .bind(value)
    .execute(s.pool())
    .await
    .unwrap();
}

/// The call's own row in a page.
fn call_row(html: &str, call: Uuid) -> &str {
    let start = html.find(&format!(r#"data-call="{call}""#)).unwrap();
    let row = &html[start..];
    &row[..row.find("</li>").unwrap()]
}

/// A run chip's link to `run` (it opens the run in the workspace).
fn run_link(run: RunId) -> String {
    format!("run_id={}", run.as_uuid())
}

/// V2: the caller lists the call through each phase, the child names its caller, a step view
/// lists it for its execution, the wait has its audit pair, and no argument text is shown.
#[tokio::test]
async fn channel_call_views_caller_and_child() {
    let s = CallSetup::new().await;
    let owner = s.owner().user_id();
    let claim = s.claim("views-v2").await;
    let scope = claim.fence.scope;
    let arguments = s.arguments(SECRET);
    assert!(matches!(
        s.call(&claim, &arguments).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let record = s.record(scope.run).await;
    let child = RunId::new(record.child_run_id);
    let calls = view(&s, owner, scope.run).await.channel_calls;
    assert!(!calls.truncated);
    let item = &calls.items[0];
    assert_eq!(item.call, record.call_id);
    assert_eq!(item.execution, scope.execution.as_uuid());
    assert_eq!(item.agent, s.tool.agent);
    assert_eq!(item.desk, s.desk);
    assert_eq!(item.desk_name, "finance");
    assert_eq!(item.responder, s.responder.as_uuid());
    assert_eq!(item.child, child);
    assert!(item.child_readable);
    assert!(!item.past_deadline);
    assert_eq!(item.phase, CallPhase::ChildRunning { stalled: false });
    let origin = view(&s, owner, child).await.called_by.unwrap();
    assert_eq!(origin.call, record.call_id);
    assert_eq!(origin.caller, scope.run);
    assert_eq!(origin.caller_execution, scope.execution.as_uuid());
    assert_eq!(origin.desk, s.desk);
    assert_ne!(
        item.scope_manifest, origin.caller_manifest,
        "the child has its own dependency scope"
    );
    assert_eq!(
        wait_events(&s, scope.run, record.call_id).await,
        ["wait_parked"]
    );
    // The pages: the caller lists the call with its child, desk and responder, the step lists
    // it, the child names its caller; no argument text anywhere.
    let caller_page = page(&s, owner, scope.run, None).await.unwrap();
    assert!(caller_page.contains(&format!(r#"data-call="{}""#, record.call_id)));
    assert!(call_row(&caller_page, record.call_id).contains(&run_link(child)));
    assert!(caller_page.contains("finance"));
    assert!(caller_page.contains(&format!(
        "responder <span class=\"font-mono\">{}",
        &s.responder.as_uuid().to_string()[..8]
    )));
    assert!(caller_page.contains("Child running"));
    let step_page = page(&s, owner, scope.run, Some(scope.execution.as_uuid()))
        .await
        .unwrap();
    assert!(step_page.contains(&format!(r#"data-call="{}""#, record.call_id)));
    let child_page = page(&s, owner, child, None).await.unwrap();
    assert!(child_page.contains("data-called-by"));
    assert!(child_page.contains(&run_link(scope.run)));
    // The call section never shows the request; the child's own input panel is its input.
    for (name, html) in [("caller", &caller_page), ("step", &step_page)] {
        assert!(
            !html.contains(SECRET),
            "the {name} page shows argument text"
        );
    }
    let called_by = &child_page[child_page.find("data-called-by").unwrap()..];
    assert!(!called_by[..called_by.find("</p>").unwrap()].contains(SECRET));
    // The child succeeds: its answer is ready and not yet consumed.
    let finished = s.tool.finish_child(scope).await;
    assert_eq!(finished.run, child);
    assert_eq!(phase(&s, scope.run).await, CallPhase::ResponseReady);
    // The caller resumes; the wait closes with its audit pair.
    let resumed = continued(&s, scope).await;
    assert_eq!(
        phase(&s, scope.run).await,
        CallPhase::AwaitingCaller { stalled: false }
    );
    assert_eq!(
        wait_events(&s, scope.run, record.call_id).await,
        ["wait_parked", "wait_completed"]
    );
    // Its lease expires before it consumes the answer: stalled.
    lease_expired(&s, resumed.fence.scope.job.0, true).await;
    assert_eq!(
        phase(&s, scope.run).await,
        CallPhase::AwaitingCaller { stalled: true }
    );
    lease_expired(&s, resumed.fence.scope.job.0, false).await;
    // Consumed: settled from the stored result.
    assert_eq!(
        result(s.call(&resumed, &arguments).await.unwrap()),
        done(child)
    );
    assert_eq!(
        phase(&s, scope.run).await,
        CallPhase::Settled {
            closure: "done".into(),
            code: None
        }
    );
}

async fn metrics(s: &CallSetup) -> WorkflowMetrics {
    s.p().workflow_metrics().await.unwrap()
}

/// The seven call gauges, in table order.
fn gauges(m: &WorkflowMetrics) -> [f64; 6] {
    [
        m.channel_calls_open,
        m.channel_calls_review_waiting,
        m.channel_calls_response_ready,
        m.channel_calls_child_stalled,
        m.channel_calls_caller_stalled,
        m.channel_calls_past_deadline,
    ]
}

/// V3: a final review is told apart from a human decision step and from a stalled child, and a
/// call the caller already consumed as `revoked` is not shown waiting for it (F8).
#[tokio::test]
async fn channel_call_views_review_vs_stalled() {
    // A stalled child: its pending job is overdue.
    let s = CallSetup::new().await;
    let claim = s.claim("views-v3-stalled").await;
    let scope = claim.fence.scope;
    assert!(matches!(
        s.call(&claim, &s.arguments("Slow")).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    assert_eq!(
        phase(&s, scope.run).await,
        CallPhase::ChildRunning { stalled: false }
    );
    overdue(&s, RunId::new(s.record(scope.run).await.child_run_id)).await;
    assert_eq!(
        phase(&s, scope.run).await,
        CallPhase::ChildRunning { stalled: true }
    );
    // A final review: linked to its decision for a viewer who may open the child.
    let s = CallSetup::new().await;
    let reviewer = s.tool.fixture.principal(s.run_as).await;
    s.select_on(
        s.desk,
        s.responder,
        Some(ReplySettingsRevision::try_from(1).unwrap()),
        Some(ReplyApprovalPolicy::new(vec![reviewer], 600).unwrap()),
    )
    .await
    .unwrap();
    let claim = s.claim("views-v3-review").await;
    let scope = claim.fence.scope;
    assert!(matches!(
        s.call(&claim, &s.arguments("Please review")).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let child = s.tool.child_of(scope).await;
    s.p()
        .clone()
        .with_channel_reply_notices(super::super::approval_test_support::notices(s.p()))
        .advance_pure(child, budget())
        .await
        .unwrap();
    let decision: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_decisions WHERE run_id=$1 AND reply_proposal_id IS NOT NULL",
    )
    .bind(child.run.as_uuid())
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(
        phase(&s, scope.run).await,
        CallPhase::AwaitingFinalReview { decision }
    );
    let html = page(&s, s.owner().user_id(), scope.run, None)
        .await
        .unwrap();
    assert!(html.contains(&format!("/ui/workflow-decisions/{decision}")));
    assert!(html.contains("Awaiting final review"));
    // A human decision step inside the child is the child's own wait, not the final review.
    let s = Box::pin(CallSetup::with(role_destination())).await;
    let principal = s.tool.fixture.principal(s.run_as).await;
    crate::adapters::persistence::test_support::department_use_cases(s.p())
        .assign_role(
            s.owner().user_id(),
            s.company().as_uuid(),
            crate::entities::department::DepartmentId::new(s.desk_unit().await),
            crate::application::ports::department::DepartmentRoleAssignment {
                role: crate::entities::department::DepartmentRole::Reviewer,
                principal: crate::domain::entities::transport::PrincipalId::new(principal),
                decider: false,
            },
        )
        .await
        .unwrap();
    let claim = s.claim("views-v3-human").await;
    let scope = claim.fence.scope;
    assert!(matches!(
        s.call(&claim, &s.arguments("Decide")).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let child = s.tool.child_of(scope).await;
    s.p()
        .park_decision(child, &super::super::approval_test_support::notices(s.p()))
        .await
        .unwrap()
        .expect("the child parks on its human decision");
    assert_eq!(
        phase(&s, scope.run).await,
        CallPhase::ChildWaiting {
            reason: Some("decision".into())
        }
    );
    assert_eq!(metrics(&s).await.channel_calls_review_waiting, 0.0);
    // F8: the caller consumed a `revoked` answer (no stored result) and went on to a second
    // call in the same step. The first is closed, not awaiting the caller.
    let s = CallSetup::new().await;
    let (legal, _) = s.add_desk("legal", s.version, json!({})).await;
    sqlx::query("UPDATE channels SET access_mode='members' WHERE id=$1")
        .bind(s.desk)
        .execute(s.pool())
        .await
        .unwrap();
    s.participate(true).await;
    let claim = s.claim("views-v3-revoked").await;
    let scope = claim.fence.scope;
    let llm = scripted_llm(vec![
        LlmTurn::tool_call(DECLARED_CALL, s.arguments("First")),
        LlmTurn::tool_call(
            DECLARED_CALL,
            json!({"channel_id": legal.to_string(), "content": {"body": "Second"}}),
        ),
        LlmTurn::text("\"answered\""),
    ])
    .await;
    register_scripted_agent_base_url(s.tool.agent, &llm.base_url);
    let runtime = s.tool.runtime();
    let WorkflowHandlerOutcome::Suspended(_) = Box::pin(runtime.execute(&kind(), &claim)).await
    else {
        panic!("the first call parks the agent");
    };
    s.tool.finish_child(scope).await;
    s.participate(false).await;
    let resumed = continued(&s, scope).await;
    let WorkflowHandlerOutcome::Suspended(_) = Box::pin(runtime.execute(&kind(), &resumed)).await
    else {
        panic!("the second call parks the agent again");
    };
    let calls = view(&s, s.owner().user_id(), scope.run).await.channel_calls;
    assert_eq!(calls.items.len(), 2);
    assert_eq!(calls.items[0].desk, legal, "newest first");
    assert_eq!(
        calls.items[0].phase,
        CallPhase::ChildRunning { stalled: false }
    );
    assert!(
        matches!(
            calls.items[1].phase,
            CallPhase::ClosedWithoutResult {
                wait: CallWait::Completed,
                ..
            }
        ),
        "{:?}",
        calls.items[1].phase
    );
    let m = metrics(&s).await;
    assert_eq!(
        m.channel_calls_response_ready, 0.0,
        "the revoked call is consumed"
    );
    assert_eq!(m.channel_calls_open, 1.0);
    // A cut page says so.
    let cut = s
        .p()
        .channel_call_page(s.company(), scope.run, s.owner(), None, 1)
        .await
        .unwrap();
    assert!(cut.truncated);
    assert_eq!(cut.items.len(), 1);
    assert_eq!(cut.items[0].desk, legal);
}

/// V4: a viewer below the child's level gets no link to it; below the caller's, the call keeps
/// ids and phases but no desk name; another company's user reaches nothing.
#[tokio::test]
async fn channel_call_views_foreign_scope() {
    let s = CallSetup::new().await;
    let owner = s.owner().user_id();
    let admin = member(&s.tool.fixture, "admin").await;
    level(&s, admin, 0).await;
    let claim = s.claim("views-v4").await;
    let scope = claim.fence.scope;
    assert!(matches!(
        s.call(&claim, &s.arguments("Levels")).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let record = s.record(scope.run).await;
    let child = RunId::new(record.child_run_id);
    s.tool.finish_child(scope).await;
    taint(&s, child).await;
    assert!(!view(&s, admin, scope.run).await.channel_calls.items[0].child_readable);
    let admin_page = page(&s, admin, scope.run, None).await.unwrap();
    assert!(admin_page.contains(&format!(r#"data-call="{}""#, record.call_id)));
    assert!(
        !call_row(&admin_page, record.call_id).contains(&run_link(child)),
        "no link to an unreadable child"
    );
    let owner_page = page(&s, owner, scope.run, None).await.unwrap();
    assert!(
        call_row(&owner_page, record.call_id).contains(&run_link(child)),
        "{}",
        call_row(&owner_page, record.call_id)
    );
    // The caller above the viewer's level: a restricted pane keeps the call, not the desk name.
    taint(&s, scope.run).await;
    let restricted = page(&s, admin, scope.run, None).await.unwrap();
    assert!(restricted.contains(&format!(r#"data-call="{}""#, record.call_id)));
    let row = call_row(&restricted, record.call_id);
    assert!(row.contains("a desk"));
    assert!(!row.contains("finance"));
    let child_page = page(&s, admin, child, None).await.unwrap();
    assert!(child_page.contains("data-called-by"));
    let called_by = &child_page[child_page.find("data-called-by").unwrap()..];
    assert!(
        !called_by[..called_by.find("</p>").unwrap()].contains(&run_link(scope.run)),
        "no link to an unreadable caller"
    );
    // Another company's user.
    let stranger = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,username,email,password_hash) VALUES($1,$2,$3,'test')")
        .bind(stranger)
        .bind(stranger.simple().to_string())
        .bind(format!("{}@example.test", stranger.simple()))
        .execute(s.pool())
        .await
        .unwrap();
    for run in [scope.run, child] {
        assert!(matches!(
            page(&s, stranger, run, None).await,
            Err(AppError::NotFound(_))
        ));
    }
}

/// V5: a call child's provenance names its own scope and the call; the caller keeps the root
/// wording.
#[tokio::test]
async fn channel_call_views_dependency_scope() {
    let s = CallSetup::new().await;
    let owner = s.owner().user_id();
    let claim = s.claim("views-v5").await;
    let scope = claim.fence.scope;
    assert!(matches!(
        s.call(&claim, &s.arguments("Scope")).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let child = RunId::new(s.record(scope.run).await.child_run_id);
    let child_page = page(&s, owner, child, None).await.unwrap();
    assert!(child_page.contains(r#"data-scope="call""#), "{child_page}");
    assert!(child_page.contains("Selected when the call from"));
    assert!(child_page.contains(&run_link(scope.run)));
    let caller_page = page(&s, owner, scope.run, None).await.unwrap();
    assert!(caller_page.contains(r#"data-scope="root""#));
    assert!(!caller_page.contains(r#"data-scope="call""#));
}

async fn draft_revision(s: &CallSetup) -> DraftRevision {
    let revision: i64 = sqlx::query_scalar(
        "SELECT revision FROM workflow_definitions WHERE company_id=$1 AND id=$2",
    )
    .bind(s.company().as_uuid())
    .bind(s.tool.fixture.target.workflow.as_uuid())
    .fetch_one(s.pool())
    .await
    .unwrap();
    DraftRevision::new(revision as u64).unwrap()
}

fn sample(s: &CallSetup, targets: Vec<Uuid>, desk: Uuid, output: Option<Value>) -> SampleRequest {
    let mut call = json!({"name":"channel.call",
        "input":{"channel_id":desk,"content":{"body":"Is order 7 late?"}}});
    if let Some(output) = output {
        call["output"] = output;
    }
    SampleRequest {
        response_contract: None,
        response_approval_policy: None,
        call_targets: targets,
        agents: vec![],
        input: json!({}),
        params: json!({}),
        scripts: serde_json::from_value(json!([{
            "workflow_id": s.tool.fixture.target.workflow,
            "step": "start",
            "responses": [{"output": "Answered", "tools": [call]}]
        }]))
        .unwrap(),
    }
}

#[derive(Debug, PartialEq, sqlx::FromRow)]
struct Production {
    runs: i64,
    calls: i64,
    threads: i64,
    messages: i64,
    checkpoints: i64,
}

async fn production(s: &CallSetup) -> Production {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM workflow_runs WHERE company_id=$1) AS runs, \
           (SELECT count(*) FROM workflow_channel_calls WHERE company_id=$1) AS calls, \
           (SELECT count(*) FROM threads WHERE company_id=$1) AS threads, \
           (SELECT count(*) FROM thread_messages WHERE company_id=$1) AS messages, \
           (SELECT count(*) FROM task_harness_runs WHERE company_id=$1) AS checkpoints",
    )
    .bind(s.company().as_uuid())
    .fetch_one(s.pool())
    .await
    .unwrap()
}

/// The single simulated call of a captured and executed sample.
async fn simulate(s: &CallSetup, request: SampleRequest) -> AppResult<(Value, String)> {
    let capture = super::super::SampleCapture {
        persistence: s.p().clone(),
        http: vec![],
        mcp: vec![],
        refused_kinds: &[],
    };
    let prepared = capture
        .capture(s.tool.fixture.target, draft_revision(s).await, request)
        .await?;
    let report = execute(prepared)?;
    assert_eq!(report.status, SampleStatus::Completed, "{report:?}");
    let call = report
        .effects
        .iter()
        .find(|effect| effect.kind == SampleEffectKind::SimulatedCall)
        .unwrap();
    let printed = serde_json::to_string(&report).unwrap();
    assert!(!printed.contains(&s.responder.as_uuid().to_string()));
    Ok((
        call.output.clone(),
        report.configuration_hash.as_str().to_owned(),
    ))
}

/// V8: capture freezes each declared desk's launch facts and responder, refuses a desk the
/// author cannot reach, and writes nothing.
#[tokio::test]
async fn channel_call_sample_capture() {
    let s = CallSetup::new().await;
    let answer = json!({"closure":"done","child_run_id":Uuid::from_u128(5),
        "content":{"subject":"Status","body":"On time"}});
    let before = production(&s).await;
    let (output, identity) = simulate(&s, sample(&s, vec![s.desk], s.desk, Some(answer.clone())))
        .await
        .unwrap();
    assert_eq!(output, answer);
    assert_eq!(
        production(&s).await,
        before,
        "capture and execution write nothing"
    );
    // Undeclared: the sample stops (the pure suite asserts the diagnostic).
    let capture = super::super::SampleCapture {
        persistence: s.p().clone(),
        http: vec![],
        mcp: vec![],
        refused_kinds: &[],
    };
    let stopped = execute(
        capture
            .capture(
                s.tool.fixture.target,
                draft_revision(&s).await,
                sample(&s, vec![], s.desk, Some(answer.clone())),
            )
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(stopped.status, SampleStatus::Stopped);
    assert!(
        stopped
            .diagnostic
            .unwrap()
            .contains("channel_call.sample_target_undeclared")
    );
    // A desk in another company, and one that does not exist: not found.
    for desk in [foreign_channel(&s).await, Uuid::new_v4()] {
        assert!(matches!(
            simulate(&s, sample(&s, vec![desk], desk, None)).await,
            Err(AppError::NotFound(_))
        ));
    }
    // A new responder changes the configuration identity.
    let replacement = s.responder_as(s.run_as).await;
    s.select(
        replacement,
        Some(ReplySettingsRevision::try_from(1).unwrap()),
    )
    .await
    .unwrap();
    let (_, changed) = simulate(&s, sample(&s, vec![s.desk], s.desk, Some(answer.clone())))
        .await
        .unwrap();
    assert_ne!(changed, identity);
    // Frozen refusals answer at launch, as live.
    sqlx::query("UPDATE channels SET access_mode='members' WHERE id=$1")
        .bind(s.desk)
        .execute(s.pool())
        .await
        .unwrap();
    let (output, _) = simulate(&s, sample(&s, vec![s.desk], s.desk, None))
        .await
        .unwrap();
    assert_eq!(code(&output), "channel_call.target_unavailable");
    s.participate(true).await;
    sqlx::query("UPDATE channels SET enabled=false WHERE id=$1")
        .bind(s.desk)
        .execute(s.pool())
        .await
        .unwrap();
    let (output, _) = simulate(&s, sample(&s, vec![s.desk], s.desk, None))
        .await
        .unwrap();
    assert_eq!(code(&output), "channel_call.desk_disabled");
    sqlx::query("UPDATE channels SET enabled=true WHERE id=$1")
        .bind(s.desk)
        .execute(s.pool())
        .await
        .unwrap();
    s.clear(ReplySettingsRevision::try_from(2).unwrap()).await;
    let (output, _) = simulate(&s, sample(&s, vec![s.desk], s.desk, None))
        .await
        .unwrap();
    assert_eq!(code(&output), "channel_call.no_responder");
    assert_eq!(production(&s).await, before);
}

/// V9: each call gauge counts its state exactly.
#[tokio::test]
async fn channel_call_views_metrics() {
    let s = CallSetup::new().await;
    assert_eq!(gauges(&metrics(&s).await), [0.0; 6]);
    let claim = s.claim("views-v9").await;
    let scope = claim.fence.scope;
    let arguments = s.arguments("Count me");
    assert!(matches!(
        s.call(&claim, &arguments).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let child = RunId::new(s.record(scope.run).await.child_run_id);
    let m = metrics(&s).await;
    assert_eq!(gauges(&m), [1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    assert!(m.channel_call_oldest_seconds >= 0.0);
    overdue(&s, child).await;
    assert_eq!(gauges(&metrics(&s).await), [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    sqlx::query(
        "UPDATE workflow_waits SET created_at=clock_timestamp() - interval '2 hours', \
           deadline=clock_timestamp() - interval '1 second' WHERE run_id=$1 AND state='waiting'",
    )
    .bind(scope.run.as_uuid())
    .execute(s.pool())
    .await
    .unwrap();
    assert_eq!(gauges(&metrics(&s).await), [1.0, 0.0, 0.0, 1.0, 0.0, 1.0]);
    sqlx::query(
        "UPDATE workflow_waits SET deadline=clock_timestamp() + interval '1 hour' \
          WHERE run_id=$1 AND state='waiting'",
    )
    .bind(scope.run.as_uuid())
    .execute(s.pool())
    .await
    .unwrap();
    s.tool.finish_child(scope).await;
    // Succeeded with the wait open, then with it completed: response ready either way.
    assert_eq!(gauges(&metrics(&s).await), [1.0, 0.0, 1.0, 0.0, 0.0, 0.0]);
    let resumed = continued(&s, scope).await;
    assert_eq!(gauges(&metrics(&s).await), [0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);
    lease_expired(&s, resumed.fence.scope.job.0, true).await;
    assert_eq!(gauges(&metrics(&s).await), [0.0, 0.0, 1.0, 0.0, 1.0, 0.0]);
    lease_expired(&s, resumed.fence.scope.job.0, false).await;
    result(s.call(&resumed, &arguments).await.unwrap());
    assert_eq!(gauges(&metrics(&s).await), [0.0; 6]);
    // A final-reply review counts; the reviewed child is the only open call.
    let reviewer = s.tool.fixture.principal(s.run_as).await;
    s.select_on(
        s.desk,
        s.responder,
        Some(ReplySettingsRevision::try_from(1).unwrap()),
        Some(ReplyApprovalPolicy::new(vec![reviewer], 600).unwrap()),
    )
    .await
    .unwrap();
    s.tool.cancel(scope, "views-v9").await;
    let claim = s.claim("views-v9-review").await;
    let scope = claim.fence.scope;
    assert!(matches!(
        s.call(&claim, &s.arguments("Review me")).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let child = s.tool.child_of(scope).await;
    s.p()
        .clone()
        .with_channel_reply_notices(super::super::approval_test_support::notices(s.p()))
        .advance_pure(child, budget())
        .await
        .unwrap();
    assert_eq!(gauges(&metrics(&s).await), [1.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
}
