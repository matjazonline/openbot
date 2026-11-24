//! agent-workflow-call p04 batch B1 (gate AWC-P04-D1 r2) against PostgreSQL: a call child owns
//! its response through the existing reply pipeline. Its terminal step fixes the closure, the
//! destination's captured review applies, and the caller reads the approved proposal once,
//! storing what it disclosed.
use super::channel_call_support::*;
use super::workflow_tool_support::*;
use super::*;
use crate::application::workflow::activation::ActivationRequest;
use crate::application::workflow::agent::AgentToolObservation;
use crate::application::workflow::batch::WorkflowBatch;
use crate::application::workflow::channel_call::{
    DENIED, HAND_OFF_INVALID, INTERACTIVE_UNSUPPORTED, RESPONSE_STEP_UNOWNED, REVIEW_EXPIRED,
    REVIEW_REJECTED, REVOKED,
};
use crate::application::workflow::channel_reply::{
    ChannelReplyContract, FinalizationProgress, ReplyApprovalPolicy, ReplyReference,
    ReplySettingsRevision, ReplyText, WorkflowFinalizations,
};
use crate::application::workflow::decisions::{
    DecisionCommandKey, DecisionId, SubmitDecision, WorkflowDecisions,
};
use crate::application::workflow::polling::WorkflowStepKind;
use crate::application::workflow::tools::CHILD_FAILED;
use serde_json::{Value, json};
use std::time::Duration;

/// A message-compatible destination ending in `answer`, with `output_schema` and extra steps.
fn responder(output_schema: Value, answer: Value) -> Value {
    let mut source = destination(None);
    source["output_schema"] = output_schema;
    source["steps"] = json!({"answer": answer});
    source
}

/// The result fields a version declares (p01 D3 bounds).
fn fields_schema() -> Value {
    json!({"type":"object","properties":{"fields":{"type":"object",
        "properties":{"why":{"type":"string","description":"Why the desk answered so"}}}}})
}

fn deny_destination() -> Value {
    responder(
        fields_schema(),
        json!({"type":"channel.deny","with":{"reason":{"literal":"Not ours"},
            "fields":{"literal":{"why":"scope"}}},"routes":{"success":"$end"}}),
    )
}

pub(super) fn no_reply_destination() -> Value {
    responder(
        json!({"type":"object"}),
        json!({"type":"channel.no_reply","with":{"value":{"literal":{"quiet":true}},
            "output_schema":{"literal":{"type":"object"}}},"routes":{"success":"$end"}}),
    )
}

fn hand_off_destination(run: Uuid) -> Value {
    responder(
        json!({"type":"object"}),
        json!({"type":"channel.hand_off","with":{"run_id":{"literal":run.to_string()}},
            "routes":{"success":"$end"}}),
    )
}

/// A destination answering `{answer}`, projected through `answered()`.
fn answer_destination(answer: &str) -> Value {
    let schema =
        json!({"type":"object","properties":{"answer":{"type":"string"}},"required":["answer"]});
    responder(
        schema.clone(),
        json!({"type":"data.map","with":{"value":{"literal":{"answer":answer}},
            "output_schema":{"literal":schema}},"routes":{"success":"$end"}}),
    )
}

fn answered() -> ChannelReplyContract {
    ChannelReplyContract {
        format_version: 1,
        subject: ReplyText::Literal {
            literal: "Status".into(),
        },
        body: ReplyText::Reference {
            r#ref: ReplyReference::try_from("/input/answer".to_owned()).unwrap(),
        },
    }
}

/// The call child's failure codes, as its run events recorded them.
async fn audited(s: &CallSetup, run: RunId, code: &str) -> bool {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workflow_run_events WHERE run_id=$1 AND event_kind=$2)",
    )
    .bind(run.as_uuid())
    .bind(code)
    .fetch_one(s.pool())
    .await
    .unwrap()
}

async fn stored_results(s: &CallSetup, caller: RunId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM workflow_channel_call_results WHERE run_id=$1")
        .bind(caller.as_uuid())
        .fetch_one(s.pool())
        .await
        .unwrap()
}

/// The child's final-response decision and the closure its proposal carries.
pub(super) async fn settled(s: &CallSetup, child: RunId) -> Option<(String, Option<String>)> {
    sqlx::query_as(
        "SELECT decision.disposition, proposal.closure FROM channel_reply_decisions AS decision \
         LEFT JOIN channel_reply_proposals AS proposal ON proposal.id=decision.proposal_id \
         WHERE decision.run_id=$1",
    )
    .bind(child.as_uuid())
    .fetch_optional(s.pool())
    .await
    .unwrap()
}

async fn proposals(s: &CallSetup, child: RunId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM channel_reply_proposals WHERE run_id=$1")
        .bind(child.as_uuid())
        .fetch_one(s.pool())
        .await
        .unwrap()
}

/// The desk reviewer: the destination's run-as member, whose principal the policy lists.
async fn reviewed(s: &CallSetup, seconds: u32) -> ReplyApprovalPolicy {
    let reviewer = s.tool.fixture.principal(s.run_as).await;
    let policy = ReplyApprovalPolicy::new(vec![reviewer], seconds).unwrap();
    s.select_on(
        s.desk,
        s.responder,
        Some(ReplySettingsRevision::try_from(1).unwrap()),
        Some(policy.clone()),
    )
    .await
    .unwrap();
    policy
}

/// The persistence as production composes it for review: final-response approvals notify.
fn reviewing(s: &CallSetup) -> PostgresPersistence {
    s.p()
        .clone()
        .with_channel_reply_notices(super::super::approval_test_support::notices(s.p()))
}

/// A call parked on a fresh claim, its child advanced to its final review.
async fn in_review(s: &CallSetup, key: &str) -> (ActivationRequest, ActivationRequest) {
    let claim = s.claim(key).await;
    let scope = claim.fence.scope;
    assert!(matches!(
        s.call(&claim, &s.arguments("Please review")).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let child = s.tool.child_of(scope).await;
    reviewing(s).advance_pure(child, budget()).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, Option<String>>(
            "SELECT waiting_reason FROM workflow_runs WHERE id=$1"
        )
        .bind(child.run.as_uuid())
        .fetch_one(s.pool())
        .await
        .unwrap()
        .as_deref(),
        Some("decision"),
        "the child waits for its final review"
    );
    (scope, child)
}

async fn review_decision(s: &CallSetup, child: ActivationRequest) -> DecisionId {
    DecisionId(
        sqlx::query_scalar(
            "SELECT id FROM workflow_decisions WHERE run_id=$1 AND reply_proposal_id IS NOT NULL",
        )
        .bind(child.run.as_uuid())
        .fetch_one(s.pool())
        .await
        .unwrap(),
    )
}

async fn submit(s: &CallSetup, child: ActivationRequest, choice: &str) {
    let decision = review_decision(s, child).await;
    let actor = WorkflowActor::authenticated(s.run_as).unwrap();
    let view = s
        .p()
        .read_decision(s.company(), actor, decision)
        .await
        .unwrap();
    s.p()
        .submit_decision(SubmitDecision {
            company: s.company(),
            actor,
            decision,
            command_key: DecisionCommandKey::parse(format!("call-final-{choice}")).unwrap(),
            expected_revision: view.revision,
            expected_artifact: view.artifact_digest,
            choice: choice.into(),
            feedback: String::new(),
            data: json!({}),
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn channel_call_response_owner_is_the_call() {
    let s = CallSetup::new().await;
    let claim = s.claim("response-a1").await;
    let scope = claim.fence.scope;
    assert!(matches!(
        s.call(&claim, &s.arguments("Own it")).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let record = s.record(scope.run).await;
    let owner: (String, Option<Uuid>, Option<Uuid>, Uuid) = sqlx::query_as(
        "SELECT owner_kind, intent_id, call_source_message_id, owner_id FROM channel_reply_runs WHERE run_id=$1",
    )
    .bind(record.child_run_id)
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(
        owner,
        (
            "call".to_owned(),
            None,
            Some(record.source_message_id),
            record.source_message_id
        )
    );
    // A second owner naming the call, for a run that is not its child, is refused by the guard.
    let forged = sqlx::query(
        "INSERT INTO channel_reply_runs(company_id,owner_kind,call_source_message_id,run_id,binding_id,binding_revision) \
         SELECT company_id,'call',source_message_id,run_id,binding_id,binding_revision FROM workflow_channel_calls WHERE run_id=$1",
    )
    .bind(scope.run.as_uuid())
    .execute(s.pool())
    .await
    .unwrap_err();
    assert!(
        forged
            .to_string()
            .contains("invalid channel response run ownership"),
        "{forged}"
    );
    // Both owner keys at once break the kind check.
    let both = sqlx::query(
        "INSERT INTO channel_reply_runs(company_id,owner_kind,intent_id,call_source_message_id,run_id,binding_id,binding_revision) \
         SELECT company_id,'call',gen_random_uuid(),source_message_id,child_run_id,binding_id,binding_revision FROM workflow_channel_calls WHERE run_id=$1",
    )
    .bind(scope.run.as_uuid())
    .execute(s.pool())
    .await
    .unwrap_err();
    assert_eq!(
        both.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    assert_eq!(s.desk_rows(s.desk).await.reply_contexts, 0);
}

#[tokio::test]
async fn channel_call_response_automatic_done() {
    let s = CallSetup::new().await;
    let arguments = s.arguments("Status?");
    let (scope, child) = finished(&s, "response-a2", &arguments).await;
    assert_eq!(
        settled(&s, child).await,
        Some(("automatic".to_owned(), Some("done".to_owned())))
    );
    let resumed = continued(&s, scope).await;
    let first = result(s.call(&resumed, &arguments).await.unwrap());
    assert_eq!(first, done(child));
    // D7: the disclosure is stored once and a replay returns it.
    assert_eq!(result(s.call(&resumed, &arguments).await.unwrap()), first);
    assert_eq!(stored_results(&s, scope.run).await, 1);
    let rows = s.desk_rows(s.desk).await;
    assert_eq!(
        (
            rows.intents,
            rows.deliveries,
            rows.reply_contexts,
            rows.external
        ),
        (0, 0, 0, 0)
    );
}

#[tokio::test]
async fn channel_call_response_no_reply() {
    let s = Box::pin(CallSetup::with(no_reply_destination())).await;
    let arguments = s.arguments("Quiet");
    let (scope, child) = finished(&s, "response-a3", &arguments).await;
    taint(&s, child).await;
    assert_eq!(
        settled(&s, child).await,
        Some(("suppressed".to_owned(), None))
    );
    let before = levels(&s, scope.run).await;
    let resumed = continued(&s, scope).await;
    assert_eq!(
        result(s.call(&resumed, &arguments).await.unwrap()),
        json!({"closure":"no_reply","child_run_id":child.as_uuid()})
    );
    assert_eq!(
        levels(&s, scope.run).await,
        before,
        "no_reply absorbs nothing"
    );
    assert_eq!(stored_results(&s, scope.run).await, 1);
}

#[tokio::test]
async fn channel_call_response_deny_and_hand_off() {
    let s = Box::pin(CallSetup::with(deny_destination())).await;
    let arguments = s.arguments("Refund please");
    let (scope, child) = finished(&s, "response-a4-deny", &arguments).await;
    taint(&s, child).await;
    assert_eq!(
        settled(&s, child).await,
        Some(("automatic".to_owned(), Some("denied".to_owned())))
    );
    let resumed = continued(&s, scope).await;
    assert_eq!(
        result(s.call(&resumed, &arguments).await.unwrap()),
        json!({"closure":"denied","code":DENIED,"reason":"Not ours","child_run_id":child.as_uuid(),
            "content":{"subject":"Re:","body":"Not ours"},"fields":{"why":"scope"}})
    );
    assert_eq!(
        levels(&s, scope.run).await,
        (2, false),
        "a denial is disclosed"
    );
    // A hand-off naming a run this child never called fails inside the child, before output.
    let s = Box::pin(CallSetup::with(hand_off_destination(Uuid::new_v4()))).await;
    let arguments = s.arguments("Pass it on");
    let (scope, child) = finished(&s, "response-a4-hand-off", &arguments).await;
    assert_eq!(run_state(s.p(), child).await, "failed");
    assert!(audited(&s, child, HAND_OFF_INVALID).await);
    assert_eq!(proposals(&s, child).await, 0);
    let resumed = continued(&s, scope).await;
    assert_eq!(
        code(&result(s.call(&resumed, &arguments).await.unwrap())),
        CHILD_FAILED
    );
}

#[tokio::test]
async fn channel_call_response_projection_refused() {
    let s = Box::pin(CallSetup::with(answer_destination("   "))).await;
    s.select_with(
        s.desk,
        s.responder,
        Some(ReplySettingsRevision::try_from(1).unwrap()),
        None,
        answered(),
    )
    .await
    .unwrap();
    let arguments = s.arguments("Answer blank");
    let (scope, child) = finished(&s, "response-a5", &arguments).await;
    assert_eq!(run_state(s.p(), child).await, "failed");
    assert_eq!(
        proposals(&s, child).await,
        0,
        "nothing reached review or publication"
    );
    let resumed = continued(&s, scope).await;
    assert_eq!(
        code(&result(s.call(&resumed, &arguments).await.unwrap())),
        CHILD_FAILED
    );
    // The same contract projects a real answer.
    let s = Box::pin(CallSetup::with(answer_destination("Forty-two"))).await;
    s.select_with(
        s.desk,
        s.responder,
        Some(ReplySettingsRevision::try_from(1).unwrap()),
        None,
        answered(),
    )
    .await
    .unwrap();
    let arguments = s.arguments("Answer");
    let (scope, child) = finished(&s, "response-a5-control", &arguments).await;
    let resumed = continued(&s, scope).await;
    assert_eq!(
        result(s.call(&resumed, &arguments).await.unwrap()),
        json!({"closure":"done","child_run_id":child.as_uuid(),
            "content":{"subject":"Status","body":"Forty-two"}})
    );
    // Result fields no call result could carry fail the child's terminal as well.
    let mut large = destination(None);
    large["steps"]["answer"]["with"]["value"] =
        json!({"literal": {"done": true, "fields": {"note": "x".repeat(140_000)}}});
    let s = Box::pin(CallSetup::with(large)).await;
    let arguments = s.arguments("Too large");
    let (scope, child) = finished(&s, "response-a5-fields", &arguments).await;
    assert_eq!(run_state(s.p(), child).await, "failed");
    assert_eq!(proposals(&s, child).await, 0);
    let resumed = continued(&s, scope).await;
    assert_eq!(
        code(&result(s.call(&resumed, &arguments).await.unwrap())),
        CHILD_FAILED
    );
}

/// A workflow-tool child and a manual run have no response owner: a response step there fails
/// with `response_step_unowned` and commits no output (D2a).
#[tokio::test]
async fn channel_call_response_steps_require_owner() {
    for step in [
        json!({"type":"channel.deny","with":{"reason":{"literal":"No"}},"routes":{"success":"$end"}}),
        json!({"type":"channel.hand_off","with":{"run_id":{"literal":Uuid::new_v4().to_string()}},
            "routes":{"success":"$end"}}),
    ] {
        let mut child = echo_child();
        child["steps"]["answer"] = step.clone();
        let setup = Box::pin(ToolSetup::new(child)).await;
        let claim = setup.claim("unowned-tool").await;
        let scope = claim.fence.scope;
        let call = setup
            .call(setup.initial_pin(), json!({"question": "Who?"}), &claim)
            .await;
        assert!(matches!(
            setup.dispatch(&claim, &call).await.unwrap(),
            AgentToolObservation::Parked(_)
        ));
        let tool_child = setup.finish_child(scope).await;
        assert_unowned(setup.persistence(), tool_child.run).await;
        // A manual run of the same procedure.
        let mut manual = destination(None);
        manual["steps"]["answer"] = step;
        let version = setup
            .fixture
            .publish_workflow(WorkflowId::new(Uuid::new_v4()), manual)
            .await;
        let target = bound(&setup.fixture, version).await;
        let run = admitted_as(
            &setup.fixture,
            target,
            RelatedAssociation::Company,
            "unowned-manual",
        )
        .await;
        setup
            .persistence()
            .advance_pure(run, budget())
            .await
            .unwrap();
        assert_unowned(setup.persistence(), run.run).await;
    }
}

/// A8 (gate case): a `workflow.call` child of a call child does not own the call's response;
/// only the call child does.
#[tokio::test]
async fn channel_call_response_steps_require_owner_in_static_child() {
    use crate::application::workflow::children::WorkflowChildren;
    let s = CallSetup::new().await;
    let leaf_id = WorkflowId::new(Uuid::new_v4());
    let mut leaf = destination(None);
    leaf["steps"]["answer"] = json!({"type":"channel.deny","with":{"reason":{"literal":"No"}},
        "routes":{"success":"$end"}});
    let leaf_version = s.tool.fixture.publish_workflow(leaf_id, leaf).await;
    let workflow = WorkflowId::new(Uuid::new_v4());
    let mut source = destination(None);
    source["workflow_id"] = json!(workflow.as_uuid());
    source["steps"]["answer"] = json!({"type":"workflow.call",
        "child_workflow_id":leaf_id.to_string(),"child_version_id":leaf_version.to_string(),
        "with":{"input":{"literal":{}}},"routes":{"success":"$end"}});
    let version = publish_agent_workflow(
        &s.tool.fixture,
        workflow,
        "static-child-destination",
        source.to_string(),
        step_builtins::StepBuiltinPublicationDirectory {
            persistence: s.p().clone(),
        },
    )
    .await
    .unwrap();
    let binding = s.responder_on(s.desk, version, s.run_as, json!({})).await;
    s.select(binding, Some(ReplySettingsRevision::try_from(1).unwrap()))
        .await
        .unwrap();
    let claim = s.claim("unowned-static").await;
    assert!(matches!(
        s.call(&claim, &s.arguments("Delegate")).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let child = s.tool.child_of(claim.fence.scope).await;
    s.p().park_child(child).await.unwrap().unwrap();
    let grandchild = s.tool.child_of(child).await;
    s.p().advance_pure(grandchild, budget()).await.unwrap();
    assert_unowned(s.p(), grandchild.run).await;
}

/// D8 across the subtree (AWC-P04-B1-F3): a `workflow.call` child of a call child cannot ask
/// either; it would otherwise reach the tree's root correspondent.
#[tokio::test]
async fn channel_call_response_ask_refused_in_static_child() {
    use crate::application::workflow::children::WorkflowChildren;
    let s = CallSetup::new().await;
    let leaf_id = WorkflowId::new(Uuid::new_v4());
    let message = crate::application::workflow::triggers::MessageTriggerInput::schema();
    let mut leaf = destination(None);
    leaf["input_schema"] = message.clone();
    leaf["steps"]["answer"] = json!({"type":"message.ask","with":{
        "source_message":{"ref":"/input/source_message"},
        "content":{"literal":{"body":"Which one?"}},
        "deadline":{"ref":"/run/deadline"},
        "payload_schema":{"literal":crate::application::workflow::triggers::MessageTriggerInput::schema()}},
        "routes":{"success":"$end"}});
    let leaf_version = s.tool.fixture.publish_with_builtins(leaf_id, leaf).await;
    let workflow = WorkflowId::new(Uuid::new_v4());
    let mut source = destination(None);
    source["workflow_id"] = json!(workflow.as_uuid());
    source["input_schema"] = message;
    source["steps"]["answer"] = json!({"type":"workflow.call",
        "child_workflow_id":leaf_id.to_string(),"child_version_id":leaf_version.to_string(),
        "with":{"input":{"ref":"/input"}},"routes":{"success":"$end"}});
    let version = publish_agent_workflow(
        &s.tool.fixture,
        workflow,
        "static-ask-destination",
        source.to_string(),
        step_builtins::StepBuiltinPublicationDirectory {
            persistence: s.p().clone(),
        },
    )
    .await
    .unwrap();
    let binding = s.responder_on(s.desk, version, s.run_as, json!({})).await;
    s.select(binding, Some(ReplySettingsRevision::try_from(1).unwrap()))
        .await
        .unwrap();
    let claim = s.claim("ask-static").await;
    assert!(matches!(
        s.call(&claim, &s.arguments("Delegate")).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let child = s.tool.child_of(claim.fence.scope).await;
    s.p().park_child(child).await.unwrap().unwrap();
    let grandchild = s.tool.child_of(child).await;
    let outcome = message_step(&s, grandchild, "message.ask").await;
    let WorkflowHandlerOutcome::Completed(Err(failure)) = outcome else {
        panic!("the ask is refused: {outcome:?}");
    };
    assert_eq!(failure.failure.code().as_str(), INTERACTIVE_UNSUPPORTED);
    let waits: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_waits WHERE run_id=$1")
        .bind(grandchild.run.as_uuid())
        .fetch_one(s.pool())
        .await
        .unwrap();
    assert_eq!(waits, 0, "no wait exists");
}

async fn assert_unowned(p: &PostgresPersistence, run: RunId) {
    assert_eq!(run_state(p, run).await, "failed");
    let (audited, output): (bool, i64) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM workflow_run_events WHERE run_id=$1 AND event_kind=$2), \
           (SELECT count(*) FROM workflow_executions WHERE run_id=$1 AND committed_output IS NOT NULL)",
    )
    .bind(run.as_uuid())
    .bind(RESPONSE_STEP_UNOWNED)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert!(audited, "{run} fails with {RESPONSE_STEP_UNOWNED}");
    assert_eq!(output, 0, "no output was committed");
}

/// B1, D4, D8: a reviewed desk answers under the policy its call captured, after approval, and
/// the reviewer is told the answer returns to the calling agent.
#[tokio::test]
async fn channel_call_review_approve_releases() {
    let s = CallSetup::new().await;
    let policy = reviewed(&s, 300).await;
    let (scope, child) = in_review(&s, "review-b1").await;
    // The desk's policy changes while the call waits; the captured one still governs (A10).
    s.select_on(
        s.desk,
        s.responder,
        Some(ReplySettingsRevision::try_from(2).unwrap()),
        None,
    )
    .await
    .unwrap();
    let decision = review_decision(&s, child).await;
    let actor = WorkflowActor::authenticated(s.run_as).unwrap();
    let view = s
        .p()
        .read_decision(s.company(), actor, decision)
        .await
        .unwrap();
    assert_eq!(view.reviewers, policy.reviewers().to_vec());
    assert_eq!(view.proposal["response"]["returns_to"], "calling_agent");
    assert_eq!(
        view.proposal["response"]["caller_run_id"],
        json!(scope.run.as_uuid())
    );
    assert_eq!(view.proposal["response"]["closure"], "done");
    // The view opens the call's desk thread, and its source is the call's own request there,
    // not the caller tree's message (AWC-P04-B1-F4).
    let record = s.record(scope.run).await;
    assert_eq!(view.context.thread, Some(record.thread_id));
    assert_eq!(view.context.trigger_message, Some(record.source_message_id));
    // The caller stays parked while the review is open.
    assert_eq!(run_state(s.p(), scope.run).await, "waiting");
    submit(&s, child, "approve").await;
    assert!(matches!(
        s.p().resume_finalization(child).await.unwrap(),
        FinalizationProgress::Completed(_)
    ));
    assert_eq!(run_state(s.p(), child.run).await, "succeeded");
    let resumed = continued(&s, scope).await;
    assert_eq!(
        result(
            s.call(&resumed, &s.arguments("Please review"))
                .await
                .unwrap()
        ),
        done(child.run)
    );
}

#[tokio::test]
async fn channel_call_review_reject_and_expire() {
    let s = CallSetup::new().await;
    reviewed(&s, 300).await;
    let (scope, child) = in_review(&s, "review-b2-reject").await;
    submit(&s, child, "reject").await;
    assert_eq!(run_state(s.p(), child.run).await, "failed");
    let resumed = continued(&s, scope).await;
    let answer = result(
        s.call(&resumed, &s.arguments("Please review"))
            .await
            .unwrap(),
    );
    assert_eq!(code(&answer), REVIEW_REJECTED);
    assert!(answer.get("content").is_none());
    // An unanswered review expires.
    let s = CallSetup::new().await;
    reviewed(&s, 1).await;
    let (scope, child) = in_review(&s, "review-b2-expire").await;
    tokio::time::sleep(Duration::from_millis(1100)).await;
    s.p().resume_wait(child).await.unwrap();
    assert_eq!(run_state(s.p(), child.run).await, "failed");
    let resumed = continued(&s, scope).await;
    assert_eq!(
        code(&result(
            s.call(&resumed, &s.arguments("Please review"))
                .await
                .unwrap()
        )),
        REVIEW_EXPIRED
    );
}

/// A rejection the procedure routes to a later step that fails reads `child_failed`: the
/// terminal execution holds no proposal (D3 "Failed").
#[tokio::test]
async fn channel_call_review_rejected_then_routed() {
    let mut source = destination(None);
    source["steps"]["answer"]["routes"] = json!({"success":"$end","error":"recover"});
    source["steps"]["recover"] = json!({"type":"data.map","with":{"value":{"literal":"not an object"},
        "output_schema":{"literal":{"type":"string"}}},"routes":{"success":"$end"}});
    let s = Box::pin(CallSetup::with(source)).await;
    reviewed(&s, 300).await;
    let (scope, child) = in_review(&s, "review-b2a").await;
    submit(&s, child, "reject").await;
    reviewing(&s)
        .advance_pure(s.tool.child_of(scope).await, budget())
        .await
        .unwrap();
    assert_eq!(run_state(s.p(), child.run).await, "failed");
    let resumed = continued(&s, scope).await;
    assert_eq!(
        code(&result(
            s.call(&resumed, &s.arguments("Please review"))
                .await
                .unwrap()
        )),
        CHILD_FAILED
    );
}

/// D7 with F8: a revoked replay discloses and stores nothing; once restored, the first
/// disclosure is stored and every later replay returns it.
#[tokio::test]
async fn channel_call_response_consumed_once() {
    let s = CallSetup::new().await;
    let arguments = s.arguments("Once");
    let (scope, child) = finished(&s, "response-d7", &arguments).await;
    let resumed = continued(&s, scope).await;
    desk_enabled(&s, false).await;
    assert_eq!(
        code(&result(s.call(&resumed, &arguments).await.unwrap())),
        REVOKED
    );
    assert_eq!(stored_results(&s, scope.run).await, 0);
    desk_enabled(&s, true).await;
    let first = result(s.call(&resumed, &arguments).await.unwrap());
    assert_eq!(first, done(child));
    // A stored disclosure is immutable, and later replays still re-check authority first.
    let changed =
        sqlx::query("UPDATE workflow_channel_call_results SET closure='error' WHERE run_id=$1")
            .bind(scope.run.as_uuid())
            .execute(s.pool())
            .await
            .unwrap_err();
    assert_eq!(
        changed.as_database_error().unwrap().constraint(),
        Some("workflow_channel_call_result_immutable")
    );
    desk_enabled(&s, false).await;
    assert_eq!(
        code(&result(s.call(&resumed, &arguments).await.unwrap())),
        REVOKED
    );
    desk_enabled(&s, true).await;
    assert_eq!(result(s.call(&resumed, &arguments).await.unwrap()), first);
    assert_eq!(stored_results(&s, scope.run).await, 1);
}

async fn desk_enabled(s: &CallSetup, enabled: bool) {
    sqlx::query("UPDATE channels SET enabled=$2 WHERE id=$1")
        .bind(s.desk)
        .bind(enabled)
        .execute(s.pool())
        .await
        .unwrap();
}

/// D8: `message.ask` inside a call child is refused before any wait exists.
#[tokio::test]
async fn channel_call_response_ask_refused() {
    let mut source = destination(None);
    source["input_schema"] = crate::application::workflow::triggers::MessageTriggerInput::schema();
    source["steps"]["answer"] = json!({"type":"message.ask","with":{
        "source_message":{"ref":"/input/source_message"},
        "content":{"literal":{"body":"Which one?"}},
        "deadline":{"ref":"/run/deadline"},
        "payload_schema":{"literal":crate::application::workflow::triggers::MessageTriggerInput::schema()}},
        "routes":{"success":"$end"}});
    let s = Box::pin(CallSetup::with(source)).await;
    let claim = s.claim("response-a11").await;
    let scope = claim.fence.scope;
    assert!(matches!(
        s.call(&claim, &s.arguments("Ask me")).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let child = s.tool.child_of(scope).await;
    let claimed = s
        .p()
        .claim_io(child, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .expect("the ask step is claimable");
    let outcome = Box::pin(
        super::support::runtime(s.p()).execute(&WorkflowStepKind("message.ask".into()), &claimed),
    )
    .await;
    let WorkflowHandlerOutcome::Completed(Err(failure)) = outcome else {
        panic!("the ask is refused: {outcome:?}");
    };
    assert_eq!(failure.failure.code().as_str(), INTERACTIVE_UNSUPPORTED);
    let waits: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_waits WHERE run_id=$1")
        .bind(child.run.as_uuid())
        .fetch_one(s.pool())
        .await
        .unwrap();
    assert_eq!(waits, 0, "no wait exists");
}

/// Publish every claimable final reply through the production publisher.
pub(super) async fn publish_all(s: &CallSetup) -> usize {
    use crate::application::workflow::channel_reply::{
        ChannelReplyPublisher, ReplyPublicationOutcome, ReplyPublicationPage,
        ReplyPublicationWorker, ReplyPublications,
    };
    let publisher = ChannelReplyPublisher::new(
        s.p().clone(),
        crate::application::workflow::message::MessagePlanner::new(
            super::super::approval_test_support::composer(s.p()),
            "agents.test".into(),
        ),
    );
    let claims = s
        .p()
        .claim_replies(
            ReplyPublicationWorker(Uuid::new_v4()),
            ReplyPublicationPage::try_from(10).unwrap(),
        )
        .await
        .unwrap();
    for claim in &claims {
        let outcome = publisher.publish_claim(*claim).await.unwrap();
        assert!(
            matches!(outcome, ReplyPublicationOutcome::Published(_)),
            "{outcome:?}"
        );
    }
    claims.len()
}

/// The desk thread's messages after the call's source: the published reply, as a note.
async fn desk_replies(s: &CallSetup, record: CallRecord) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT entry.entry_kind, message.clean_text_body FROM thread_messages AS entry \
         JOIN messages AS message ON message.company_id=entry.company_id AND message.id=entry.message_id \
         WHERE entry.thread_id=$1 AND entry.message_id<>$2 ORDER BY entry.created_at",
    )
    .bind(record.thread_id)
    .bind(record.source_message_id)
    .fetch_all(s.pool())
    .await
    .unwrap()
}

/// A2 and D6 (publisher reuse, U5): the destination's reply is published once as an internal
/// note in the call's desk thread, with no delivery; the result never waited for it.
#[tokio::test]
async fn channel_call_response_reply_note_published() {
    let s = CallSetup::new().await;
    let arguments = s.arguments("Note it");
    let (scope, _child) = finished(&s, "response-a2-note", &arguments).await;
    let record = s.record(scope.run).await;
    assert!(desk_replies(&s, record).await.is_empty());
    assert_eq!(publish_all(&s).await, 1);
    assert_eq!(
        desk_replies(&s, record).await,
        vec![("note".to_owned(), "Done".to_owned())]
    );
    assert_eq!(publish_all(&s).await, 0, "published exactly once");
    assert_eq!(s.desk_rows(s.desk).await.deliveries, 0);
}

/// Rows of the call tree that retention must remove with it.
pub(super) async fn call_rows(s: &CallSetup, caller: RunId, child: RunId) -> [i64; 5] {
    let count = |sql: &'static str, run: RunId| async move {
        sqlx::query_scalar::<_, i64>(sql)
            .bind(run.as_uuid())
            .fetch_one(s.pool())
            .await
            .unwrap()
    };
    [
        count(
            "SELECT count(*) FROM workflow_channel_calls WHERE run_id=$1",
            caller,
        )
        .await,
        count(
            "SELECT count(*) FROM workflow_channel_call_results WHERE run_id=$1",
            caller,
        )
        .await,
        count(
            "SELECT count(*) FROM channel_reply_runs WHERE run_id=$1",
            child,
        )
        .await,
        count(
            "SELECT count(*) FROM channel_reply_proposals WHERE run_id=$1",
            child,
        )
        .await,
        count(
            "SELECT count(*) FROM channel_reply_decisions WHERE run_id=$1",
            child,
        )
        .await,
    ]
}

/// Age a terminal tree past retention, expiring the calling agent's history payload through the
/// production history expiry first (the run rows' clocks are moved with triggers bypassed).
pub(super) async fn age(s: &CallSetup, caller: ActivationRequest) {
    crate::adapters::persistence::workflow::agent_history_retention_fixture::expire(s.p(), caller)
        .await;
    let mut tx = s.pool().begin().await.unwrap();
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE workflow_runs SET finished_at=clock_timestamp()-interval '91 days' \
         WHERE id IN (SELECT run_id FROM workflow_run_budgets WHERE root_run_id=$1)",
    )
    .bind(caller.run.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

pub(super) async fn retire(s: &CallSetup, caller: RunId) -> bool {
    use crate::application::workflow::storage::{RetentionRoot, WorkflowStorageMaintenance};
    let finished_at = sqlx::query_scalar("SELECT finished_at FROM workflow_runs WHERE id=$1")
        .bind(caller.as_uuid())
        .fetch_one(s.pool())
        .await
        .unwrap();
    s.p()
        .retire_tree(RetentionRoot {
            finished_at,
            company: s.company(),
            run: caller,
        })
        .await
        .unwrap()
        .is_some()
}

/// B5 (D5 r2): production retention deletes the caller and its call child in one statement, and
/// every call row goes with that tree, never before. A tree whose review is open is not eligible.
#[tokio::test]
async fn channel_call_privacy_retention() {
    // A finished tree: the child answered and its reply was published; the caller is cancelled
    // before its continuation is claimed. (A stored result row cascades with its record by FK.)
    let s = CallSetup::new().await;
    let arguments = s.arguments("Retain");
    let (scope, child) = finished(&s, "retention-finished", &arguments).await;
    publish_all(&s).await;
    s.tool.cancel(scope, "retention-finished-cancel").await;
    let record = s.record(scope.run).await;
    assert_eq!(call_rows(&s, scope.run, child).await, [1, 0, 1, 1, 1]);
    assert!(!retire(&s, scope.run).await, "a fresh tree is kept");
    age(&s, scope).await;
    assert!(retire(&s, scope.run).await);
    assert_eq!(call_rows(&s, scope.run, child).await, [0; 5]);
    let desk_history: i64 =
        sqlx::query_scalar("SELECT count(*) FROM thread_messages WHERE thread_id=$1")
            .bind(record.thread_id)
            .fetch_one(s.pool())
            .await
            .unwrap();
    // The desk thread is desk history, read under the desk's own grants; retention of the
    // workflow tree does not remove it.
    assert_eq!(
        desk_history, 2,
        "the call's source and the published reply remain"
    );
    // A tree with an open review is live (its caller waits on the call), so it is never
    // eligible. Cancelled during the review, the whole tree is, and nothing of the call remains.
    let s = CallSetup::new().await;
    reviewed(&s, 300).await;
    let (scope, child) = in_review(&s, "retention-review").await;
    assert_eq!(run_state(s.p(), scope.run).await, "waiting");
    s.tool.cancel(scope, "retention-review-cancel").await;
    assert_eq!(run_state(s.p(), child.run).await, "cancelled");
    age(&s, scope).await;
    assert!(retire(&s, scope.run).await);
    assert_eq!(call_rows(&s, scope.run, child.run).await, [0; 5]);
}

/// A message step's handler outcome inside the call child.
async fn message_step(
    s: &CallSetup,
    child: ActivationRequest,
    kind: &str,
) -> WorkflowHandlerOutcome {
    let claimed = s
        .p()
        .claim_io(child, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .expect("the message step is claimable");
    Box::pin(super::support::runtime(s.p()).execute(&WorkflowStepKind(kind.into()), &claimed)).await
}

/// As `message_step`, then the worker's commit of a completed step, as the task worker does.
async fn committed_step(s: &CallSetup, child: ActivationRequest, kind: &str) {
    use crate::application::workflow::completion::WorkflowCompletion;
    let claimed = s
        .p()
        .claim_io(child, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .expect("the message step is claimable");
    let WorkflowHandlerOutcome::Completed(Ok(output)) =
        Box::pin(super::support::runtime(s.p()).execute(&WorkflowStepKind(kind.into()), &claimed))
            .await
    else {
        panic!("the {kind} step completes");
    };
    s.p()
        .complete_io(crate::application::workflow::lease::FencedWorkflowResult {
            fence: claimed.fence,
            output,
            policy: lease(),
        })
        .await
        .unwrap()
        .expect("the step's result commits");
}

/// A destination whose first step is `step`, then the default answer.
fn stepped(step: Value) -> Value {
    let mut source = destination(None);
    source["input_schema"] = crate::application::workflow::triggers::MessageTriggerInput::schema();
    source["entry"] = json!("first");
    source["steps"]["first"] = step;
    source
}

/// A7: `message.reply` to the call's source inside the child files an internal note on the
/// call's desk thread; no recipient is guessed and nothing is delivered.
#[tokio::test]
async fn channel_call_response_reply_to_source() {
    let s = Box::pin(CallSetup::with(stepped(
        json!({"type":"message.reply","with":{
        "source_message":{"ref":"/input/source_message"},
        "content":{"literal":{"body":"Working on it"}}},"routes":{"success":"answer"}}),
    )))
    .await;
    let claim = s.claim("response-a7").await;
    let scope = claim.fence.scope;
    assert!(matches!(
        s.call(&claim, &s.arguments("Reply to me")).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let record = s.record(scope.run).await;
    let child = s.tool.child_of(scope).await;
    committed_step(&s, child, "message.reply").await;
    assert_eq!(
        desk_replies(&s, record).await,
        vec![("note".to_owned(), "Working on it".to_owned())]
    );
    let rows = s.desk_rows(s.desk).await;
    assert_eq!((rows.deliveries, rows.external), (0, 0));
    // The source reply does not settle the call: the child goes on to its answer, and the caller
    // then consumes that answer (doc 06 A scenario 3, "source reply").
    assert_eq!(run_state(s.p(), scope.run).await, "waiting");
    let next = s.tool.child_of(scope).await;
    s.p().advance_pure(next, budget()).await.unwrap();
    assert_eq!(run_state(s.p(), child.run).await, "succeeded");
    let resumed = continued(&s, scope).await;
    assert_eq!(
        result(s.call(&resumed, &s.arguments("Reply to me")).await.unwrap()),
        done(child.run)
    );
    let rows = s.desk_rows(s.desk).await;
    assert_eq!((rows.deliveries, rows.external), (0, 0));
}

/// A6: an explicit `message.send` inside the destination is its own effect, under its own
/// policy, beside the call's result.
#[tokio::test]
async fn channel_call_response_independent_effect() {
    let s = CallSetup::new().await;
    let other = super::support::new_channel(&s.tool.fixture, "ops").await;
    let version = s
        .tool
        .fixture
        .publish_with_builtins(
            WorkflowId::new(Uuid::new_v4()),
            stepped(json!({"type":"message.send","with":{
                "content":{"literal":{"subject":"FYI","body":"A call arrived"}},
                "destinations":{"literal":[{"kind":"channel","id":other.to_string()}]}},
                "routes":{"success":"answer"}})),
        )
        .await;
    let binding = s.responder_on(s.desk, version, s.run_as, json!({})).await;
    s.select(binding, Some(ReplySettingsRevision::try_from(1).unwrap()))
        .await
        .unwrap();
    let claim = s.claim("response-a6").await;
    let scope = claim.fence.scope;
    assert!(matches!(
        s.call(&claim, &s.arguments("Tell ops")).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let child = s.tool.child_of(scope).await;
    let outcome = message_step(&s, child, "message.send").await;
    let WorkflowHandlerOutcome::Completed(Ok(receipt)) = outcome else {
        panic!("the send is its own accepted effect: {outcome:?}");
    };
    assert_eq!(receipt["status"], "accepted");
    let (filed, actions, finals): (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM thread_messages WHERE channel_id=$1), \
           (SELECT count(*) FROM workflow_message_actions WHERE run_id=$2), \
           (SELECT count(*) FROM channel_reply_proposals WHERE run_id=$2)",
    )
    .bind(other)
    .bind(child.run.as_uuid())
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(
        (filed, actions, finals),
        (1, 1, 0),
        "the send is filed on its own desk under its own action; no final response yet"
    );
}

/// A9: an `agent.run` destination prepares under the call's ownership (the agent-preparation
/// call arm) and answers over the call through its final response.
#[tokio::test]
async fn channel_call_response_agent_destination_prepares() {
    use crate::application::workflow::completion::WorkflowCompletion;
    use crate::application::workflow::lease::FencedWorkflowResult;
    use crate::services::test_support::{LlmTurn, register_scripted_agent_base_url, scripted_llm};
    let s = CallSetup::new().await;
    // The caller's own published agent workflow answers the desk, run as the company owner.
    let binding = s
        .responder_on(s.desk, s.tool.parent, s.owner().user_id(), json!({}))
        .await;
    s.select(binding, Some(ReplySettingsRevision::try_from(1).unwrap()))
        .await
        .unwrap();
    let claim = s.claim("response-a9").await;
    let scope = claim.fence.scope;
    let arguments = s.arguments("What is the status?");
    assert!(matches!(
        s.call(&claim, &arguments).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let child = s.tool.child_of(scope).await;
    // The destination runs as the caller's own user, yet takes no personal scope from the
    // caller's private session: a call child is a fresh scope (AWC-P02-D1 D1.2).
    let personal: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_run_scopes WHERE run_id=$1 AND acting_principal_id IS NULL AND access='personal'",
    )
    .bind(child.run.as_uuid())
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(personal, 0);
    let llm = scripted_llm(vec![LlmTurn::text("\"All green\"")]).await;
    register_scripted_agent_base_url(s.tool.agent, &llm.base_url);
    let claimed = s
        .p()
        .claim_io(child, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .expect("the destination's agent step is claimable");
    let outcome = Box::pin(s.tool.runtime().execute(&kind(), &claimed)).await;
    let WorkflowHandlerOutcome::Completed(Ok(output)) = outcome else {
        panic!("the destination agent prepares and answers: {outcome:?}");
    };
    assert!(
        s.p()
            .complete_io(FencedWorkflowResult {
                fence: claimed.fence,
                output,
                policy: lease(),
            })
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(llm.finish().await.unwrap(), 1);
    assert_eq!(run_state(s.p(), child.run).await, "succeeded");
    assert_eq!(
        settled(&s, child.run).await,
        Some(("automatic".to_owned(), Some("done".to_owned())))
    );
    let resumed = continued(&s, scope).await;
    assert_eq!(
        result(s.call(&resumed, &arguments).await.unwrap()),
        done(child.run)
    );
}

/// B4: what the destination computed beyond its approved reply never reaches the caller.
#[tokio::test]
async fn channel_call_privacy_no_raw_leak() {
    const MARKER: &str = "CHILD-ONLY-7f3a";
    let schema = json!({"type":"object","properties":{"answer":{"type":"string"},
        "internal":{"type":"string"}},"required":["answer"]});
    let s = Box::pin(CallSetup::with(responder(
        schema.clone(),
        json!({"type":"data.map","with":{"value":{"literal":{"answer":"Public answer","internal":MARKER}},
            "output_schema":{"literal":schema}},"routes":{"success":"$end"}}),
    )))
    .await;
    s.select_with(
        s.desk,
        s.responder,
        Some(ReplySettingsRevision::try_from(1).unwrap()),
        None,
        answered(),
    )
    .await
    .unwrap();
    let arguments = s.arguments("Tell me");
    let (scope, child) = finished(&s, "privacy-b4", &arguments).await;
    let resumed = continued(&s, scope).await;
    let answer = result(s.call(&resumed, &arguments).await.unwrap());
    assert_eq!(answer["content"]["body"], "Public answer");
    assert!(!answer.to_string().contains(MARKER));
    publish_all(&s).await;
    // Every caller-side surface: the stored result, the caller's own executions and events,
    // notifications, and every message outside the call's own child.
    let leaked: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM workflow_channel_call_results WHERE run_id=$1 AND result::text LIKE '%'||$3||'%') \
          + (SELECT count(*) FROM workflow_executions WHERE run_id=$1 \
               AND (COALESCE(committed_output::text,'') LIKE '%'||$3||'%' OR COALESCE(frozen_inputs::text,'') LIKE '%'||$3||'%')) \
          + (SELECT count(*) FROM notifications WHERE company_id=$4 AND to_jsonb(notifications)::text LIKE '%'||$3||'%') \
          + (SELECT count(*) FROM messages WHERE company_id=$4 AND (subject LIKE '%'||$3||'%' OR clean_text_body LIKE '%'||$3||'%'))",
    )
    .bind(scope.run.as_uuid())
    .bind(child.as_uuid())
    .bind(MARKER)
    .bind(s.company().as_uuid())
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(leaked, 0, "the child-only value stays in the child");
}

fn approval(
    s: &CallSetup,
    decision: DecisionId,
    key: &str,
    choice: &str,
    revision: i64,
    artifact: String,
) -> SubmitDecision {
    SubmitDecision {
        company: s.company(),
        actor: WorkflowActor::authenticated(s.run_as).unwrap(),
        decision,
        command_key: DecisionCommandKey::parse(key).unwrap(),
        expected_revision: revision,
        expected_artifact: artifact,
        choice: choice.into(),
        feedback: String::new(),
        data: json!({}),
    }
}

/// B3: review races. Competing reviewers settle once and the caller reads the winner; a replay
/// is idempotent and a stale submission is refused; approval racing the caller's cancellation
/// releases nothing; approval racing revocation is held back at release (`revoked`).
#[tokio::test]
async fn channel_call_review_races() {
    let actor = |s: &CallSetup| WorkflowActor::authenticated(s.run_as).unwrap();
    // Competing approve and reject.
    let s = CallSetup::new().await;
    reviewed(&s, 300).await;
    let (scope, child) = in_review(&s, "race-compete").await;
    let decision = review_decision(&s, child).await;
    let view = s
        .p()
        .read_decision(s.company(), actor(&s), decision)
        .await
        .unwrap();
    let barrier = tokio::sync::Barrier::new(2);
    let settle = |key: &'static str, choice: &'static str| {
        let command = approval(
            &s,
            decision,
            key,
            choice,
            view.revision,
            view.artifact_digest.clone(),
        );
        let barrier = &barrier;
        let p = reviewing(&s);
        async move {
            barrier.wait().await;
            p.submit_decision(command).await
        }
    };
    let (approved, rejected) =
        tokio::join!(settle("race-a", "approve"), settle("race-r", "reject"));
    let (winner, key, choice) = match (approved, rejected) {
        (Ok(winner), Err(AppError::Conflict(_))) => (winner, "race-a", "approve"),
        (Err(AppError::Conflict(_)), Ok(winner)) => (winner, "race-r", "reject"),
        other => panic!("exactly one reviewer settles: {other:?}"),
    };
    // The winner's replay is idempotent; a later reviewer on the old revision is refused.
    let replay = reviewing(&s)
        .submit_decision(approval(
            &s,
            decision,
            key,
            choice,
            view.revision,
            view.artifact_digest.clone(),
        ))
        .await
        .unwrap();
    assert_eq!(replay.revision, winner.revision);
    assert!(matches!(
        reviewing(&s)
            .submit_decision(approval(
                &s,
                decision,
                "race-late",
                "approve",
                view.revision,
                view.artifact_digest.clone()
            ))
            .await,
        Err(AppError::Conflict(_))
    ));
    if choice == "approve" {
        assert!(matches!(
            s.p().resume_finalization(child).await.unwrap(),
            FinalizationProgress::Completed(_)
        ));
    }
    let resumed = continued(&s, scope).await;
    let answer = result(
        s.call(&resumed, &s.arguments("Please review"))
            .await
            .unwrap(),
    );
    if choice == "approve" {
        assert_eq!(answer, done(child.run));
    } else {
        assert_eq!(code(&answer), REVIEW_REJECTED);
    }
    assert_eq!(stored_results(&s, scope.run).await, 1);

    // Approval racing the caller's cancellation: the tree ends cancelled with nothing released.
    let s = CallSetup::new().await;
    reviewed(&s, 300).await;
    let (scope, child) = in_review(&s, "race-cancel").await;
    let decision = review_decision(&s, child).await;
    let view = s
        .p()
        .read_decision(s.company(), actor(&s), decision)
        .await
        .unwrap();
    let barrier = tokio::sync::Barrier::new(2);
    let command = approval(
        &s,
        decision,
        "race-cancel-a",
        "approve",
        view.revision,
        view.artifact_digest.clone(),
    );
    let approve = async {
        barrier.wait().await;
        reviewing(&s).submit_decision(command).await
    };
    let cancel = async {
        barrier.wait().await;
        s.tool.cancel(scope, "race-cancel-c").await;
    };
    let (approved, ()) = tokio::join!(approve, cancel);
    if let Err(error) = &approved {
        assert!(matches!(error, AppError::Conflict(_)), "{error:?}");
    }
    let _ = s.p().resume_finalization(child).await;
    assert_eq!(run_state(s.p(), scope.run).await, "cancelled");
    assert_eq!(run_state(s.p(), child.run).await, "cancelled");
    assert_eq!(stored_results(&s, scope.run).await, 0);

    // Approval racing revocation of the destination: whichever commits first, the approved
    // response is not released to a caller that lost its authority.
    let s = CallSetup::new().await;
    reviewed(&s, 300).await;
    let (scope, child) = in_review(&s, "race-revoke").await;
    let decision = review_decision(&s, child).await;
    let view = s
        .p()
        .read_decision(s.company(), actor(&s), decision)
        .await
        .unwrap();
    let barrier = tokio::sync::Barrier::new(2);
    let command = approval(
        &s,
        decision,
        "race-revoke-a",
        "approve",
        view.revision,
        view.artifact_digest.clone(),
    );
    let approve = async {
        barrier.wait().await;
        reviewing(&s).submit_decision(command).await
    };
    let revoke = async {
        barrier.wait().await;
        desk_enabled(&s, false).await;
    };
    let (approved, ()) = tokio::join!(approve, revoke);
    approved.unwrap();
    assert!(matches!(
        s.p().resume_finalization(child).await.unwrap(),
        FinalizationProgress::Completed(_)
    ));
    let resumed = continued(&s, scope).await;
    let answer = result(
        s.call(&resumed, &s.arguments("Please review"))
            .await
            .unwrap(),
    );
    assert_eq!(code(&answer), REVOKED);
    assert!(answer.get("content").is_none());
    assert_eq!(stored_results(&s, scope.run).await, 0);
}

/// How the destination's own nested call ends before its procedure hands off to it.
#[derive(Clone, Copy, PartialEq)]
enum Nested {
    /// The nested desk answers `done`.
    Done,
    /// The nested desk denies: a stored fact of another closure.
    Denied,
    /// The nested desk is disabled before the answer is consumed: no fact is stored.
    Revoked,
}

/// A caller parked on a desk whose agent workflow calls a second desk, takes `nested`'s
/// outcome, and then hands off to that nested run. Returns the caller's scope, the
/// destination child, the nested run and the caller's arguments.
async fn hand_off_through(
    s: &CallSetup,
    nested: Nested,
) -> (ActivationRequest, ActivationRequest, RunId, Value) {
    use crate::application::workflow::completion::WorkflowCompletion;
    let nested_version = match nested {
        Nested::Denied => {
            s.tool
                .fixture
                .publish_with_builtins(WorkflowId::new(Uuid::new_v4()), deny_destination())
                .await
        }
        Nested::Done | Nested::Revoked => s.version,
    };
    let (nested_desk, _) = s.add_desk("nested", nested_version, json!({})).await;
    // The desk's responder: the agent workflow, then a hand-off to the run its agent names.
    let mut source: Value = serde_json::from_str(
        &crate::application::workflow::registry::example("agent.run")
            .unwrap()
            .source,
    )
    .unwrap();
    let workflow = WorkflowId::new(Uuid::from_u128(0x0c0b));
    source["workflow_id"] = json!(workflow.as_uuid());
    source["steps"]["start"]["routes"] = json!({"success":"handoff"});
    source["steps"]["handoff"] = json!({"type":"channel.hand_off",
        "with":{"run_id":{"ref":"/steps/start/output"}},"routes":{"success":"$end"}});
    let version = publish_agent_workflow(
        &s.tool.fixture,
        workflow,
        "hand-off-destination",
        source.to_string(),
        step_builtins::StepBuiltinPublicationDirectory {
            persistence: s.p().clone(),
        },
    )
    .await
    .unwrap();
    let binding = s
        .responder_on(s.desk, version, s.owner().user_id(), json!({}))
        .await;
    s.select(binding, Some(ReplySettingsRevision::try_from(1).unwrap()))
        .await
        .unwrap();
    let claim = s.claim("response-hand-off").await;
    let scope = claim.fence.scope;
    let arguments = s.arguments("Who handles this?");
    assert!(matches!(
        s.call(&claim, &arguments).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    // The destination's agent asks the nested desk and takes its outcome.
    let child = s.tool.child_of(scope).await;
    let destination = s
        .p()
        .claim_io(child, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    let nested_arguments =
        json!({"channel_id": nested_desk.to_string(), "content": {"body": "Please take this"}});
    assert!(matches!(
        s.call(&destination, &nested_arguments).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let nested_run = s.tool.finish_child(child).await.run;
    if nested == Nested::Revoked {
        sqlx::query("UPDATE channels SET enabled=false WHERE id=$1")
            .bind(nested_desk)
            .execute(s.pool())
            .await
            .unwrap();
    }
    let destination = continued(s, child).await;
    let answer = result(s.call(&destination, &nested_arguments).await.unwrap());
    let closure: Option<String> = sqlx::query_scalar(
        "SELECT result->>'closure' FROM workflow_channel_call_results WHERE run_id=$1",
    )
    .bind(child.run.as_uuid())
    .fetch_optional(s.pool())
    .await
    .unwrap();
    match nested {
        Nested::Done => assert_eq!(answer, done(nested_run)),
        Nested::Denied => assert_eq!(answer["closure"], "denied"),
        Nested::Revoked => assert_eq!(code(&answer), REVOKED),
    }
    assert_eq!(
        closure.as_deref(),
        match nested {
            Nested::Done => Some("done"),
            Nested::Denied => Some("denied"),
            Nested::Revoked => None,
        },
        "the destination's stored fact for its nested call"
    );
    // Its agent answers with the nested run, and the procedure hands off to it.
    let handoff = s
        .p()
        .complete_io(crate::application::workflow::lease::FencedWorkflowResult {
            fence: destination.fence,
            output: json!(nested_run.as_uuid().to_string()),
            policy: lease(),
        })
        .await
        .unwrap()
        .and_then(|step| step.successor)
        .expect("the agent step routes to the hand-off");
    s.p().advance_pure(handoff, budget()).await.unwrap();
    (scope, child, nested_run, arguments)
}

/// A4 positive: a destination that called another desk and got `done` hands the caller off to
/// that nested run. The caller's result names the nested run and carries its answer.
#[tokio::test]
async fn channel_call_response_hand_off_names_the_nested_run() {
    let s = CallSetup::new().await;
    let (scope, child, nested, arguments) = hand_off_through(&s, Nested::Done).await;
    assert_eq!(run_state(s.p(), child.run).await, "succeeded");
    assert_eq!(
        settled(&s, child.run).await,
        Some(("automatic".to_owned(), Some("handed_off".to_owned())))
    );
    let resumed = continued(&s, scope).await;
    let answer = result(s.call(&resumed, &arguments).await.unwrap());
    assert_eq!(
        answer,
        json!({"closure":"handed_off","child_run_id":child.run.as_uuid(),"run_id":nested.as_uuid(),
            "content":{"subject":"Status","body":"Done"}})
    );
}

/// A4 refusals on a real nested call (gate F2): a nested call that closed `denied`, and one
/// revoked before its answer was consumed, cannot be handed off to.
#[tokio::test]
async fn channel_call_response_hand_off_needs_a_done_call() {
    for nested in [Nested::Denied, Nested::Revoked] {
        let s = CallSetup::new().await;
        let (scope, child, _, arguments) = hand_off_through(&s, nested).await;
        assert_eq!(run_state(s.p(), child.run).await, "failed");
        assert!(audited(&s, child.run, HAND_OFF_INVALID).await);
        assert_eq!(proposals(&s, child.run).await, 0);
        let resumed = continued(&s, scope).await;
        assert_eq!(
            code(&result(s.call(&resumed, &arguments).await.unwrap())),
            CHILD_FAILED
        );
    }
}

/// The platform's call notes filed in `thread`: (body, entry kind, role, audience).
async fn call_notes(s: &CallSetup, thread: Uuid) -> Vec<(String, String, String, String)> {
    sqlx::query_as(
        "SELECT message.clean_text_body, filed.entry_kind, message.role, message.audience \
         FROM thread_messages AS filed JOIN messages AS message ON message.company_id=filed.company_id AND message.id=filed.message_id \
         WHERE filed.thread_id=$1 AND message.subject LIKE 'Call to %' ORDER BY filed.created_at",
    )
    .bind(thread)
    .fetch_all(s.pool())
    .await
    .unwrap()
}

/// Every message filed in `thread`: (channel, body).
async fn filed(s: &CallSetup, thread: Uuid) -> Vec<(Uuid, String)> {
    sqlx::query_as(
        "SELECT filed.channel_id, message.clean_text_body FROM thread_messages AS filed \
         JOIN messages AS message ON message.company_id=filed.company_id AND message.id=filed.message_id \
         WHERE filed.thread_id=$1 ORDER BY filed.created_at",
    )
    .bind(thread)
    .fetch_all(s.pool())
    .await
    .unwrap()
}

/// A thread-associated caller's call, run to the caller's consumption; returns the caller's
/// thread, the call's desk thread, the child and the consumed result.
async fn trail_call(s: &CallSetup, key: &str) -> (Uuid, Uuid, RunId, Value) {
    let (claim, thread) = s.claim_in_thread(key).await;
    let scope = claim.fence.scope;
    let arguments = s.arguments("Is the refund due?");
    assert!(matches!(
        s.call(&claim, &arguments).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let child = s.tool.finish_child(scope).await.run;
    let resumed = continued(s, scope).await;
    let answer = result(s.call(&resumed, &arguments).await.unwrap());
    // A replayed settlement returns the stored fact and writes nothing twice.
    assert_eq!(result(s.call(&resumed, &arguments).await.unwrap()), answer);
    publish_all(s).await;
    (thread, s.record(scope.run).await.thread_id, child, answer)
}

/// C1: the call leaves one reply on the desk thread and one platform note on the caller's
/// thread; `no_reply` leaves only the note; `denied` publishes its reason; a caller without a
/// thread gets no note; a replay writes nothing twice; neither side quotes the other.
#[tokio::test]
async fn channel_call_trail_reply_and_note() {
    // A replied call.
    let s = CallSetup::new().await;
    let (thread, desk_thread, child, answer) = trail_call(&s, "trail-done").await;
    assert_eq!(answer, done(child));
    let notes = call_notes(&s, thread).await;
    assert_eq!(notes.len(), 1, "one note, however often the call replays");
    let (body, entry, role, audience) = &notes[0];
    assert_eq!(
        (entry.as_str(), role.as_str(), audience.as_str()),
        ("note", "system", "internal_only")
    );
    assert!(
        body.contains("finance") && body.contains(&child.as_uuid().to_string()),
        "{body}"
    );
    assert!(body.contains("closed: done"), "{body}");
    assert!(
        !body.contains("Done"),
        "the note never quotes the response: {body}"
    );
    let level: i16 = sqlx::query_scalar(
        "SELECT message.access_level::smallint FROM thread_messages AS filed JOIN messages AS message \
           ON message.company_id=filed.company_id AND message.id=filed.message_id \
         WHERE filed.thread_id=$1 AND message.subject LIKE 'Call to %'",
    )
    .bind(thread)
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(level, 0, "the note carries no classified content");
    let reentry: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM workflow_message_intents AS intent JOIN messages AS message \
                   ON message.company_id=intent.company_id AND message.id=intent.message_id \
                 WHERE message.subject LIKE 'Call to %') \
              + (SELECT count(*) FROM message_deliveries AS delivery JOIN messages AS message \
                   ON message.company_id=delivery.company_id AND message.id=delivery.message_id \
                 WHERE message.subject LIKE 'Call to %')",
    )
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(
        reentry, 0,
        "the note neither re-enters ingress nor is delivered"
    );
    let desk = filed(&s, desk_thread).await;
    assert_eq!(
        desk.iter()
            .map(|(_, body)| body.as_str())
            .collect::<Vec<_>>(),
        ["Is the refund due?", "Done"],
        "the desk reads question, then answer"
    );
    assert!(desk.iter().all(|(channel, _)| *channel == s.desk));
    let front = filed(&s, thread).await;
    assert!(front.iter().all(|(channel, _)| *channel != s.desk));
    assert!(
        front
            .iter()
            .all(|(_, body)| !body.contains("Is the refund due?"))
    );

    // `no_reply`: the note only.
    let s = Box::pin(CallSetup::with(no_reply_destination())).await;
    let (thread, desk_thread, _, _) = trail_call(&s, "trail-quiet").await;
    let notes = call_notes(&s, thread).await;
    assert_eq!(notes.len(), 1);
    assert!(notes[0].0.contains("closed: no_reply"), "{}", notes[0].0);
    assert_eq!(
        filed(&s, desk_thread).await.len(),
        1,
        "only the call's request"
    );

    // `denied`: the desk thread shows the reason; the note names the closure only.
    let s = Box::pin(CallSetup::with(deny_destination())).await;
    let (thread, desk_thread, _, _) = trail_call(&s, "trail-deny").await;
    let notes = call_notes(&s, thread).await;
    assert_eq!(notes.len(), 1);
    assert!(notes[0].0.contains("closed: denied"), "{}", notes[0].0);
    assert!(!notes[0].0.contains("Not ours"), "{}", notes[0].0);
    assert_eq!(filed(&s, desk_thread).await[1].1, "Not ours");

    // A caller without a thread records no note anywhere.
    let s = CallSetup::new().await;
    let arguments = s.arguments("Retain");
    let (scope, _) = finished(&s, "trail-threadless", &arguments).await;
    let resumed = continued(&s, scope).await;
    result(s.call(&resumed, &arguments).await.unwrap());
    let notes: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM messages WHERE company_id=$1 AND subject LIKE 'Call to %'",
    )
    .bind(s.company().as_uuid())
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(notes, 0);
}
