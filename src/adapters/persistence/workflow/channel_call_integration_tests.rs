//! agent-workflow-call p06 Batch A (doc 06 A): end-to-end proof over the real persistence and
//! agent runtime with a scripted model. Each test names the doc 06 scenario and gap it closes.
use super::channel_call_support::*;
use super::workflow_tool_support::*;
use super::*;
use crate::application::workflow::activation::{ActivationRequest, WorkflowJobId};
use crate::application::workflow::batch::WorkflowBatch;
use crate::application::workflow::channel_call::ChannelCallDispatch;
use crate::application::workflow::channel_reply::{
    ChannelReplyContract, FinalizationProgress, ReplyReference, ReplySettingsRevision, ReplyText,
    WorkflowFinalizations,
};
use crate::application::workflow::decisions::{
    DecisionCommandKey, DecisionId, SubmitDecision, WorkflowDecisions,
};
use crate::domain::entities::correlation::CorrelationId;
use crate::services::test_support::{LlmTurn, register_scripted_agent_base_url, scripted_llm};
use serde_json::{Value, json};

const DECLARED_CALL: &str = "channel__call";

fn revision(value: u64) -> ReplySettingsRevision {
    ReplySettingsRevision::try_from(value).unwrap()
}

/// The persistence as production composes it for review: final-response approvals notify.
fn reviewing(s: &CallSetup) -> PostgresPersistence {
    s.p()
        .clone()
        .with_channel_reply_notices(super::super::approval_test_support::notices(s.p()))
}

/// A destination whose `decision.human` step lets `reviewer` replace the draft; its result's
/// `data.body` is the edited answer.
fn edited_destination(reviewer: Uuid) -> Value {
    let mut source: Value =
        serde_json::from_str(&registry::example("decision.human").unwrap().source).unwrap();
    source["input_schema"] = json!({"type":"object"});
    source["output_schema"] = json!({"type":"object","required":["data"],"properties":{
        "data":{"type":"object","required":["body"],
            "properties":{"body":{"type":"string","maxLength":200}}}}});
    let with = &mut source["steps"]["start"]["with"];
    with["proposal"] = json!({"literal":{"body":"B drafted: order 7 is late"}});
    with["data_schema"] = json!({"literal":{"type":"object","additionalProperties":false,
        "required":["body"],"properties":{"body":{"type":"string","maxLength":200}}}});
    with["feedback_required"] = json!({"literal":[]});
    with["reviewers"] = json!({"literal":[{"kind":"principal","id":reviewer.to_string()}]});
    with["deadline"] =
        json!({"literal":(chrono::Utc::now() + chrono::Duration::minutes(20)).to_rfc3339()});
    source
}

/// The reply contract answering with the reviewed result's edited body.
fn edited_contract() -> ChannelReplyContract {
    ChannelReplyContract {
        format_version: 1,
        subject: ReplyText::Literal {
            literal: "Status".into(),
        },
        body: ReplyText::Reference {
            r#ref: ReplyReference::try_from("/input/data/body".to_owned()).unwrap(),
        },
    }
}

/// The reviewer submits the child's human decision with `data`; returns its successor.
async fn decide(s: &CallSetup, child: ActivationRequest, data: Value) -> Option<ActivationRequest> {
    s.p()
        .park_decision(child, &super::super::approval_test_support::notices(s.p()))
        .await
        .unwrap()
        .expect("the child parks on its human decision");
    let decision = DecisionId(
        sqlx::query_scalar("SELECT id FROM workflow_decisions WHERE run_id=$1")
            .bind(child.run.as_uuid())
            .fetch_one(s.pool())
            .await
            .unwrap(),
    );
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
            command_key: DecisionCommandKey::parse("integration-edit").unwrap(),
            expected_revision: view.revision,
            expected_artifact: view.artifact_digest,
            choice: "continue".into(),
            feedback: String::new(),
            data,
        })
        .await
        .unwrap()
        .successor
}

/// The model request at `index`, as text, with the tool results it carries.
fn request_text(requests: &[Value], index: usize) -> String {
    serde_json::to_string(&requests[index]).unwrap()
}

/// G1 + G2 (scenario 1; scenario 3 "edited approval" and "human decision"): one agent execution
/// calls desk B, waits while B's human step edits the draft, consumes the edited answer, calls desk C (another version, params and intake),
/// consumes C's answer and finishes. The model's later requests carry exactly those approved
/// bodies. Neither destination is a registered workflow tool.
#[tokio::test]
async fn channel_call_integration_two_desks_in_one_execution() {
    let s = CallSetup::new().await;
    // Desk B: a human step lets the responder's run-as member replace the draft, and the reply
    // contract answers with the edited body.
    let reviewer = s.tool.fixture.principal(s.run_as).await;
    let b_version = s
        .tool
        .fixture
        .publish_with_builtins(
            WorkflowId::new(Uuid::from_u128(DESTINATION + 2)),
            edited_destination(reviewer),
        )
        .await;
    let b_binding = s.responder_on(s.desk, b_version, s.run_as, json!({})).await;
    s.select_with(
        s.desk,
        b_binding,
        Some(revision(1)),
        None,
        edited_contract(),
    )
    .await
    .unwrap();
    // Desk C: another destination version, with params and an intake, and its own body.
    let intake = json!({"type":"object","additionalProperties":false,
        "properties":{"order":{"type":"string","maxLength":8,"description":"The order number"}}});
    let c_version = s
        .tool
        .fixture
        .publish_with_builtins(
            WorkflowId::new(Uuid::from_u128(DESTINATION + 1)),
            destination(Some(intake)),
        )
        .await;
    assert_ne!(c_version, s.version);
    let (legal, legal_binding) = s
        .add_desk("legal", c_version, json!({"tone":"formal"}))
        .await;
    s.select_with(
        legal,
        legal_binding,
        Some(revision(1)),
        None,
        literal("C approved the refund for order 7"),
    )
    .await
    .unwrap();
    let registered: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_tool_registrations WHERE company_id=$1 AND workflow_id = ANY($2)",
    )
    .bind(s.company().as_uuid())
    .bind(vec![Uuid::from_u128(DESTINATION + 1), Uuid::from_u128(DESTINATION + 2)])
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(registered, 0, "neither destination is a workflow tool");
    let control: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_tool_registrations WHERE company_id=$1 AND workflow_id=$2",
    )
    .bind(s.company().as_uuid())
    .bind(Uuid::from_u128(CHILD))
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(control, 1, "the query sees the agent's own registered tool");

    let claim = s.claim("integration-g1").await;
    let scope = claim.fence.scope;
    let mut llm = scripted_llm(vec![
        LlmTurn::tool_call(DECLARED_CALL, s.arguments("Where is order 7?")),
        LlmTurn::tool_call(
            DECLARED_CALL,
            json!({"channel_id": legal.to_string(), "content": {"body": "Refund order 7?"},
                "fields": {"order": "7"}}),
        ),
        LlmTurn::text("\"Order 7 ships Monday; refund approved\""),
    ])
    .await;
    register_scripted_agent_base_url(s.tool.agent, &llm.base_url);
    let runtime = s.tool.runtime();
    let WorkflowHandlerOutcome::Suspended(_) = Box::pin(runtime.execute(&kind(), &claim)).await
    else {
        panic!("the call to B parks the agent");
    };
    // B's child parks on its human step; the reviewer replaces the draft and the child ends.
    let child_b = s.tool.child_of(scope).await;
    if let Some(next) = decide(
        &s,
        child_b,
        json!({"body":"Edited by review: order 7 ships Monday"}),
    )
    .await
    {
        reviewing(&s).advance_pure(next, budget()).await.unwrap();
    }
    if run_state(s.p(), child_b.run).await != "succeeded" {
        assert!(matches!(
            s.p().resume_finalization(child_b).await.unwrap(),
            FinalizationProgress::Completed(_)
        ));
    }
    assert_eq!(run_state(s.p(), child_b.run).await, "succeeded");
    let resumed = continued(&s, scope).await;
    assert_eq!(resumed.fence.scope.execution, scope.execution);
    let WorkflowHandlerOutcome::Suspended(_) = Box::pin(runtime.execute(&kind(), &resumed)).await
    else {
        panic!("the agent consumes B and its call to C parks it");
    };
    let child_c = s.tool.finish_child(scope).await;
    assert_ne!(child_c.run, child_b.run);
    let resumed = continued(&s, scope).await;
    assert_eq!(resumed.fence.scope.execution, scope.execution);
    let WorkflowHandlerOutcome::Completed(Ok(output)) =
        Box::pin(runtime.execute(&kind(), &resumed)).await
    else {
        panic!("the agent consumes C and finishes");
    };
    assert_eq!(output, json!("Order 7 ships Monday; refund approved"));
    // What the model saw: B's approved (edited) body after the first call, never the draft;
    // then C's body as well.
    let requests = llm.observed();
    assert_eq!(requests.len(), 3);
    assert!(!request_text(&requests, 0).contains("order 7 ships Monday"));
    let second = request_text(&requests, 1);
    assert!(
        second.contains("Edited by review: order 7 ships Monday"),
        "{second}"
    );
    assert!(
        !second.contains("B drafted"),
        "the draft never reaches the model"
    );
    assert!(!second.contains("C approved the refund"));
    let third = request_text(&requests, 2);
    assert!(third.contains("Edited by review: order 7 ships Monday"));
    assert!(
        third.contains("C approved the refund for order 7"),
        "{third}"
    );
    assert_eq!(llm.finish().await.unwrap(), 3);
    // Two children, one per desk, each with its own responder and version; two stored results.
    let children: Vec<(Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT call.channel_id, child.binding_id, result.closure FROM workflow_channel_calls AS call \
           JOIN workflow_runs AS child ON child.company_id=call.company_id AND child.id=call.child_run_id \
           JOIN workflow_channel_call_results AS result ON result.company_id=call.company_id \
                AND result.run_id=call.run_id AND result.call_id=call.call_id \
          WHERE call.run_id=$1 ORDER BY call.created_at",
    )
    .bind(scope.run.as_uuid())
    .fetch_all(s.pool())
    .await
    .unwrap();
    assert_eq!(
        children,
        vec![
            (s.desk, b_binding.as_uuid(), "done".to_owned()),
            (legal, legal_binding.as_uuid(), "done".to_owned()),
        ]
    );
    assert_eq!(run_state(s.p(), child_b.run).await, "succeeded");
    assert_eq!(run_state(s.p(), child_c.run).await, "succeeded");
}

/// G3 (scenario 6): a run on a desk cannot call that desk, and a call child cannot call back a
/// desk its chain already answers (A → B → A). Both answer `channel_call.cycle` with no record or
/// child. A legal nested call from the child stays inside the caller's tree budget.
#[tokio::test]
async fn channel_call_integration_cycles_and_nested_budget() {
    use crate::application::workflow::channel_call::CYCLE;
    let s = CallSetup::new().await;
    let (home, _) = s.add_desk("home", s.version, json!({})).await;
    let (legal, _) = s.add_desk("legal", s.version, json!({})).await;
    // A self-call: the caller answers desk A and calls desk A.
    let own = s.claim_from(home, "integration-g3-self").await;
    let answer = result(
        s.call(
            &own,
            &json!({"channel_id": home.to_string(), "content": {"body": "Me again"}}),
        )
        .await
        .unwrap(),
    );
    assert_eq!(code(&answer), CYCLE);
    assert_eq!(s.records().await, 0);
    s.tool.cancel(own.fence.scope, "integration-g3-self").await;
    // Desk B is answered by the calling agent's own workflow, so B's child can call too.
    let agent_binding = s
        .responder_on(s.desk, s.tool.parent, s.owner().user_id(), json!({}))
        .await;
    s.select(agent_binding, Some(revision(1))).await.unwrap();
    let claim = s.claim_from(home, "integration-g3").await;
    let scope = claim.fence.scope;
    assert!(matches!(
        s.call(&claim, &s.arguments("Ask B")).await.unwrap(),
        crate::application::workflow::agent::AgentToolObservation::Parked(_)
    ));
    let child = s.tool.child_of(scope).await;
    // B's agent calls desk A (refused as a cycle), then desk C (a legal nested call).
    let mut llm = scripted_llm(vec![
        LlmTurn::tool_call(
            DECLARED_CALL,
            json!({"channel_id": home.to_string(), "content": {"body": "Back to A"}}),
        ),
        LlmTurn::tool_call(
            DECLARED_CALL,
            json!({"channel_id": legal.to_string(), "content": {"body": "On to C"}}),
        ),
    ])
    .await;
    register_scripted_agent_base_url(s.tool.agent, &llm.base_url);
    let claimed = s
        .p()
        .claim_io(child, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .expect("B's agent step is claimable");
    let WorkflowHandlerOutcome::Suspended(_) =
        Box::pin(s.tool.runtime().execute(&kind(), &claimed)).await
    else {
        panic!("B's nested call to C parks B");
    };
    let requests = llm.observed();
    assert_eq!(requests.len(), 2);
    assert!(
        request_text(&requests, 1).contains(CYCLE),
        "B's agent saw the cycle refusal"
    );
    // Exactly two records: the caller's call to B and B's call to C. Nothing for A.
    let calls: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT run_id, channel_id FROM workflow_channel_calls WHERE company_id=$1 ORDER BY created_at",
    )
    .bind(s.company().as_uuid())
    .fetch_all(s.pool())
    .await
    .unwrap();
    assert_eq!(
        calls,
        vec![(scope.run.as_uuid(), s.desk), (child.run.as_uuid(), legal)]
    );
    // The grandchild charges the caller's tree: its budget root is the caller's root.
    let roots: Vec<Uuid> = sqlx::query_scalar(
        "SELECT budget.root_run_id FROM workflow_run_budgets AS budget \
           JOIN workflow_channel_calls AS call ON call.company_id=budget.company_id AND call.child_run_id=budget.run_id \
          WHERE call.company_id=$1 ORDER BY call.created_at",
    )
    .bind(s.company().as_uuid())
    .fetch_all(s.pool())
    .await
    .unwrap();
    assert_eq!(roots, vec![scope.run.as_uuid(), scope.run.as_uuid()]);
}

/// G5 (scenario 5): a process restart after the desk's final review settled loses nothing. A
/// fresh persistence finds the wakeup, the caller replays the call once and the model sees the
/// approved answer; one child, one result, no retry.
#[tokio::test]
async fn channel_call_integration_restart_after_review_settlement() {
    use crate::application::workflow::polling::{PollWork, WorkflowPolling};
    use crate::application::workflow::waits::{WaitProgress, WorkflowWaits};
    let s = CallSetup::new().await;
    let reviewer = s.tool.fixture.principal(s.run_as).await;
    s.select_with(
        s.desk,
        s.responder,
        Some(revision(1)),
        Some(
            crate::application::workflow::channel_reply::ReplyApprovalPolicy::new(
                vec![reviewer],
                600,
            )
            .unwrap(),
        ),
        literal("Approved after review"),
    )
    .await
    .unwrap();
    let claim = s.claim("integration-g5").await;
    let scope = claim.fence.scope;
    let mut llm = scripted_llm(vec![
        LlmTurn::tool_call(DECLARED_CALL, s.arguments("Review this")),
        LlmTurn::text("\"reviewed\""),
    ])
    .await;
    register_scripted_agent_base_url(s.tool.agent, &llm.base_url);
    let WorkflowHandlerOutcome::Suspended(_) =
        Box::pin(s.tool.runtime().execute(&kind(), &claim)).await
    else {
        panic!("parks");
    };
    let child = s.tool.child_of(scope).await;
    reviewing(&s).advance_pure(child, budget()).await.unwrap();
    let decision = DecisionId(
        sqlx::query_scalar(
            "SELECT id FROM workflow_decisions WHERE run_id=$1 AND reply_proposal_id IS NOT NULL",
        )
        .bind(child.run.as_uuid())
        .fetch_one(s.pool())
        .await
        .unwrap(),
    );
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
            command_key: DecisionCommandKey::parse("integration-g5").unwrap(),
            expected_revision: view.revision,
            expected_artifact: view.artifact_digest,
            choice: "approve".into(),
            feedback: String::new(),
            data: json!({}),
        })
        .await
        .unwrap();
    // Restart between the approval and its finalization, then again before the wakeup.
    let restarted = PostgresPersistence::new(s.pool().clone());
    assert!(matches!(
        restarted.resume_finalization(child).await.unwrap(),
        FinalizationProgress::Completed(_)
    ));
    let restarted = PostgresPersistence::new(s.pool().clone());
    let work: Vec<PollWork> = restarted
        .poll_work(None, 100)
        .await
        .unwrap()
        .candidates
        .into_iter()
        .filter(|candidate| candidate.scope.run == scope.run)
        .map(|candidate| candidate.work)
        .collect();
    assert_eq!(work, vec![PollWork::Wait]);
    assert_eq!(
        restarted.resume_wait(scope).await.unwrap(),
        WaitProgress::Continued
    );
    let resumed = PostgresPersistence::new(s.pool().clone())
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .expect("the caller's continued job");
    let WorkflowHandlerOutcome::Completed(Ok(output)) =
        Box::pin(s.tool.runtime().execute(&kind(), &resumed)).await
    else {
        panic!("the caller consumes the approved answer and finishes");
    };
    assert_eq!(output, json!("reviewed"));
    let requests = llm.observed();
    assert!(request_text(&requests, 1).contains("Approved after review"));
    assert_eq!(llm.finish().await.unwrap(), 2);
    let (records, results, children, retries): (i64, i64, i64, i32) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM workflow_channel_calls WHERE run_id=$1), \
           (SELECT count(*) FROM workflow_channel_call_results WHERE run_id=$1), \
           (SELECT count(*) FROM workflow_runs WHERE parent_run_id=$1), \
           (SELECT retry_count FROM background_tasks WHERE id=$2)",
    )
    .bind(scope.run.as_uuid())
    .bind(scope.job.0)
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!((records, results, children, retries), (1, 1, 1, 0));
}

/// G4 (scenario 4): two consumptions of one call's result race; exactly one stored result, and
/// every answer either side receives is that result.
#[tokio::test]
async fn channel_call_integration_competing_consumptions() {
    let s = CallSetup::new().await;
    let claim = s.claim("integration-g4-consume").await;
    let scope = claim.fence.scope;
    let arguments = s.arguments("Consume me once");
    assert!(matches!(
        s.call(&claim, &arguments).await.unwrap(),
        crate::application::workflow::agent::AgentToolObservation::Parked(_)
    ));
    let child = s.tool.finish_child(scope).await;
    taint(&s, child.run).await;
    let resumed = continued(&s, scope).await;
    let saved = s.tool.channel_call(&arguments, &resumed).await;
    let other = s.tool.channel_call(&arguments, &resumed).await;
    let (first, second) = tokio::join!(
        s.p().call_channel(resumed.fence, saved),
        s.p().call_channel(resumed.fence, other)
    );
    let answers: Vec<Value> = [first, second]
        .into_iter()
        .filter_map(|answer| answer.ok().map(result))
        .collect();
    assert!(!answers.is_empty(), "at least one consumer answers");
    for answer in &answers {
        assert_eq!(answer, &done(child.run));
    }
    let stored: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_channel_call_results WHERE run_id=$1")
            .bind(scope.run.as_uuid())
            .fetch_one(s.pool())
            .await
            .unwrap();
    assert_eq!(stored, 1);
    assert_eq!(levels(&s, scope.run).await, (2, false), "absorbed once");
}

/// G4 (scenario 4): launch races the agent's grant being revoked on a members-only desk, through
/// the production port. Either the call launched with its whole record, or it was refused with
/// no row at all.
#[tokio::test]
async fn channel_call_integration_launch_races_revocation() {
    use crate::application::workflow::agent::AgentToolObservation;
    use crate::application::workflow::channel_call::TARGET_UNAVAILABLE;
    for round in 0..3 {
        let s = CallSetup::new().await;
        sqlx::query("UPDATE channels SET access_mode='members' WHERE id=$1")
            .bind(s.desk)
            .execute(s.pool())
            .await
            .unwrap();
        s.participate(true).await;
        let claim = s.claim(&format!("integration-g4-revoke-{round}")).await;
        let arguments = s.arguments("Race the grant");
        let saved = s.tool.channel_call(&arguments, &claim).await;
        let (launched, ()) =
            tokio::join!(s.p().call_channel(claim.fence, saved), s.participate(false));
        match launched.unwrap() {
            AgentToolObservation::Parked(_) => {
                assert_eq!(s.records().await, 1, "round {round}");
                assert_eq!(children(s.p(), claim.fence.scope.run).await, 1);
            }
            AgentToolObservation::Completed(answer) => {
                assert_eq!(code(&answer), TARGET_UNAVAILABLE, "round {round}");
                assert_no_call_rows(&s).await;
            }
            other => panic!("round {round}: {other:?}"),
        }
    }
}

/// What a call record pinned: responder binding, its revision, the desk's settings revision and
/// the calling agent's revision.
async fn pins(s: &CallSetup, caller: RunId) -> (Uuid, i64, i64, Uuid) {
    sqlx::query_as(
        "SELECT binding_id, binding_revision, settings_revision, agent_revision_id \
         FROM workflow_channel_calls WHERE company_id=$1 AND run_id=$2",
    )
    .bind(s.company().as_uuid())
    .bind(caller.as_uuid())
    .fetch_one(s.pool())
    .await
    .unwrap()
}

async fn version_of(s: &CallSetup, run: RunId) -> Uuid {
    sqlx::query_scalar("SELECT version_id FROM workflow_runs WHERE id=$1")
        .bind(run.as_uuid())
        .fetch_one(s.pool())
        .await
        .unwrap()
}

/// G6 (scenario 2): while call 1 waits, the desk's responder is reselected to another version
/// whose run calls a static `workflow.call` child, and the calling agent is republished keeping
/// `channel.call`. Call 1 stays pinned (binding, settings, agent revision, version, contract)
/// and consumes its own answer; a later distinct call takes the current configuration, and its
/// static grandchild settles it with the new contract's answer.
#[tokio::test]
async fn channel_call_integration_pinned_while_waiting_later_call_current() {
    use crate::application::workflow::agent::AgentToolObservation;
    use crate::application::workflow::children::WorkflowChildren;
    use crate::application::workflow::waits::{WaitProgress, WorkflowWaits};
    let s = CallSetup::new().await;
    let first = s.claim("integration-g6-first").await;
    let first_scope = first.fence.scope;
    let first_arguments = s.arguments("First");
    assert!(matches!(
        s.call(&first, &first_arguments).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let first_pins = pins(&s, first_scope.run).await;
    assert_eq!(first_pins.0, s.responder.as_uuid());
    // Reselect: a destination calling a static leaf, under another contract.
    let leaf_id = WorkflowId::new(Uuid::new_v4());
    let leaf_version = s
        .tool
        .fixture
        .publish_workflow(leaf_id, destination(None))
        .await;
    let workflow = WorkflowId::new(Uuid::new_v4());
    let mut source = destination(None);
    source["workflow_id"] = json!(workflow.as_uuid());
    source["steps"]["answer"] = json!({"type":"workflow.call",
        "child_workflow_id":leaf_id.to_string(),"child_version_id":leaf_version.to_string(),
        "with":{"input":{"literal":{}}},"routes":{"success":"$end"}});
    let static_version = publish_agent_workflow(
        &s.tool.fixture,
        workflow,
        "integration-static-destination",
        source.to_string(),
        step_builtins::StepBuiltinPublicationDirectory {
            persistence: s.p().clone(),
        },
    )
    .await
    .unwrap();
    let binding = s
        .responder_on(s.desk, static_version, s.run_as, json!({}))
        .await;
    s.select_with(
        s.desk,
        binding,
        Some(revision(1)),
        None,
        literal("Delegated"),
    )
    .await
    .unwrap();
    // Republish the calling agent with another prompt; its tool bindings stay as they are.
    crate::application::use_cases::agent::AgentPersistence::update(
        s.p(),
        s.tool.agent,
        crate::application::use_cases::agent::AgentWrite {
            name: "Assistant".into(),
            slug: "assistant".into(),
            provider: Some(crate::services::test_support::SCRIPTED_PROVIDER.into()),
            model: Some(crate::services::test_support::SCRIPTED_MODEL.into()),
            system_prompt: Some("Ask the desk again, then answer.".into()),
            tool_bindings: None,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    s.tool.fixture.publish_agent(s.tool.agent).await;
    // A later distinct call selects what is current now.
    let second = s.claim("integration-g6-second").await;
    let second_scope = second.fence.scope;
    let second_arguments = s.arguments("Second");
    assert!(matches!(
        s.call(&second, &second_arguments).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let second_pins = pins(&s, second_scope.run).await;
    assert_eq!(second_pins.0, binding.as_uuid());
    assert_eq!(second_pins.2, 2, "the reselection's settings revision");
    assert_eq!(first_pins.2, 1);
    assert_ne!(
        second_pins.3, first_pins.3,
        "the republished agent revision"
    );
    assert_eq!(
        pins(&s, first_scope.run).await,
        first_pins,
        "call 1 is unchanged"
    );
    // Call 1 settles on its pinned version and contract, after the republish.
    let first_child = s.tool.finish_child(first_scope).await;
    assert_eq!(version_of(&s, first_child.run).await, s.version.as_uuid());
    let resumed = continued(&s, first_scope).await;
    assert_eq!(
        result(s.call(&resumed, &first_arguments).await.unwrap()),
        done(first_child.run)
    );
    // Call 2: its child runs the current version and parks on the static leaf, which settles it.
    let second_child = s.tool.child_of(second_scope).await;
    assert_eq!(
        version_of(&s, second_child.run).await,
        static_version.as_uuid()
    );
    s.p().park_child(second_child).await.unwrap().unwrap();
    let grandchild = s.tool.child_of(second_child).await;
    assert_eq!(version_of(&s, grandchild.run).await, leaf_version.as_uuid());
    s.p().advance_pure(grandchild, budget()).await.unwrap();
    // The leaf's terminal fact completes the static step, and with it the call child.
    assert!(matches!(
        s.p().resume_wait(second_child).await.unwrap(),
        WaitProgress::Completed(_)
    ));
    assert_eq!(run_state(s.p(), second_child.run).await, "succeeded");
    let resumed = continued(&s, second_scope).await;
    assert_eq!(
        result(s.call(&resumed, &second_arguments).await.unwrap()),
        json!({"closure": "done", "child_run_id": second_child.run.as_uuid(),
            "content": {"subject": "Status", "body": "Delegated"}})
    );
}

/// G7 (scenario 7): a foreign, missing and members-only desk without a grant each answer the
/// same `target_unavailable` through the production port, with no rows. A non-operations caller
/// (admitted by an admin since demoted) never reaches the call: its agent step is rejected
/// before the model. The owner's run is the positive control; its child runs as the desk's
/// `run_as`.
#[tokio::test]
async fn channel_call_integration_member_caller_and_unavailable_desks() {
    use crate::adapters::persistence::workflow::tests::action_outcome_support::CompletedOutcome;
    use crate::application::workflow::channel_call::TARGET_UNAVAILABLE;
    let s = CallSetup::new().await;
    // Refusals first, so "no rows" is company-wide.
    let foreign = foreign_channel(&s).await;
    let private = new_channel(&s.tool.fixture, "private-desk").await;
    sqlx::query("UPDATE channels SET enabled=true, access_mode='members' WHERE id=$1")
        .bind(private)
        .execute(s.pool())
        .await
        .unwrap();
    let mut answers = Vec::new();
    for (index, desk) in [foreign, Uuid::new_v4(), private].into_iter().enumerate() {
        let claim = s.claim(&format!("integration-g7-refused-{index}")).await;
        let mut arguments = s.arguments("Anyone there?");
        arguments["channel_id"] = json!(desk);
        let answer = result(s.call(&claim, &arguments).await.unwrap());
        assert_eq!(code(&answer), TARGET_UNAVAILABLE, "{index}: {answer}");
        assert_eq!(s.desk_rows(desk).await, DeskRows::NONE, "{index}");
        answers.push(answer);
        // Free the company's job slot (two at once by default).
        s.tool
            .cancel(claim.fence.scope, &format!("integration-g7-cancel-{index}"))
            .await;
    }
    assert!(
        answers.windows(2).all(|pair| pair[0] == pair[1]),
        "indistinguishable: {answers:?}"
    );
    assert_no_call_rows(&s).await;
    // Admission needs operations authority, so the non-operations caller is a run an admin
    // admitted, who is demoted to a plain member before the call.
    let plain = member(&s.tool.fixture, "admin").await;
    let p = s.p();
    let admitted = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight)
        .admit(AdmitWorkflowRequest {
            history_session: None,
            company_id: s.company(),
            actor: WorkflowActor::authenticated(plain).unwrap(),
            association: RelatedAssociation::Company,
            trigger: TriggerRef::new(
                s.company(),
                TriggerId::new(Uuid::new_v4()),
                TriggerSource::Manual,
            )
            .unwrap(),
            correlation_id: CorrelationId::new(),
            binding_id: s.tool.target.binding,
            idempotency_key: IdempotencyKey::parse("integration-g7-member").unwrap(),
            input: json!({}),
            rerun_of: None,
            delivery_policy: crate::application::workflow::DeliveryPolicy::Deliver,
            expected_binding_revision: None,
            admission_claim: None,
        })
        .await
        .unwrap();
    let AdmissionResult::Created(run) = admitted else {
        panic!("{admitted:?}")
    };
    let (execution, job): (Uuid, Uuid) = sqlx::query_as(
        "SELECT execution.id,job.id FROM workflow_executions AS execution \
         JOIN background_tasks AS job ON job.workflow_execution_id=execution.id WHERE execution.run_id=$1",
    )
    .bind(run.as_uuid())
    .fetch_one(s.pool())
    .await
    .unwrap();
    let scope = ActivationRequest {
        company: s.company(),
        run,
        execution: ExecutionId::new(execution),
        job: WorkflowJobId(job),
    };
    let actor: Uuid = sqlx::query_scalar("SELECT actor_id FROM workflow_runs WHERE id=$1")
        .bind(run.as_uuid())
        .fetch_one(s.pool())
        .await
        .unwrap();
    assert_eq!(actor, plain, "the caller's person is the admitting admin");
    crate::use_cases::company_invite::CompanyInvitePersistence::update_member_role(
        p,
        crate::use_cases::company_invite::MemberRoleChange {
            actor_id: s.owner().user_id(),
            company_id: s.company().as_uuid(),
            member_user_id: plain,
            role: crate::entities::company_member::CompanyAccessRole::Member,
        },
    )
    .await
    .unwrap()
    .expect("the member is demoted");
    let claim = p
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    // The agent step refuses the demoted person before the model is asked (the run's own
    // authority, as for every tool): no saved call, so no `channel.call` and no call rows.
    let mut llm = scripted_llm(vec![LlmTurn::tool_call(
        DECLARED_CALL,
        s.arguments("From a member"),
    )])
    .await;
    register_scripted_agent_base_url(s.tool.agent, &llm.base_url);
    let failure = Box::pin(s.tool.runtime().execute(&kind(), &claim))
        .await
        .completed_result()
        .unwrap_err();
    assert_eq!(failure.failure.code().as_str(), "agent.rejected");
    assert!(llm.observed().is_empty(), "the model is never asked");
    assert_no_call_rows(&s).await;
    // The positive control: the owner's run on the same public desk launches and settles.
    let (scope, child) = finished(&s, "integration-g7-owner", &s.arguments("From the owner")).await;
    let record = s.record(scope.run).await;
    let child_actor: Uuid = sqlx::query_scalar("SELECT actor_id FROM workflow_runs WHERE id=$1")
        .bind(record.child_run_id)
        .fetch_one(s.pool())
        .await
        .unwrap();
    assert_eq!(child_actor, s.run_as, "the child runs as the desk's run_as");
    let resumed = continued(&s, scope).await;
    assert_eq!(
        result(
            s.call(&resumed, &s.arguments("From the owner"))
                .await
                .unwrap()
        ),
        done(child)
    );
}

/// Every row of the call machinery left in the company.
async fn company_call_rows(s: &CallSetup) -> [i64; 5] {
    let mut rows = [0; 5];
    for (row, table) in [
        "workflow_channel_calls",
        "workflow_channel_call_results",
        "workflow_waits",
        "run_dependency_manifests",
        "channel_reply_runs",
    ]
    .into_iter()
    .enumerate()
    {
        rows[row] = count(
            s.pool(),
            &format!("SELECT count(*) FROM {table} WHERE company_id=$1"),
            s.company(),
        )
        .await;
    }
    rows
}

async fn run_rows(s: &CallSetup, table: &str, run: RunId) -> i64 {
    sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE run_id=$1"))
        .bind(run.as_uuid())
        .fetch_one(s.pool())
        .await
        .unwrap()
}

/// G8 (scenario 8): retention retires a tree whose caller consumed (so stored) its result: the
/// record, the stored result, the wait, the child's reply rows and the call child's manifest all
/// go with the tree. Company teardown removes a waiting call's whole tree, its desk thread and
/// source, without a dangling reference or the immutability guard refusing it.
#[tokio::test]
async fn channel_call_integration_retention_and_teardown() {
    use super::channel_call_response_tests::{age, call_rows, publish_all, retire};
    // Retention, with a stored result.
    let s = CallSetup::new().await;
    let arguments = s.arguments("Retain the result");
    let (scope, child) = finished(&s, "integration-g8-retain", &arguments).await;
    publish_all(&s).await;
    let resumed = continued(&s, scope).await;
    assert_eq!(
        result(s.call(&resumed, &arguments).await.unwrap()),
        done(child)
    );
    s.tool.cancel(scope, "integration-g8-retain-cancel").await;
    assert_eq!(
        call_rows(&s, scope.run, child).await[..2],
        [1, 1],
        "record and stored result"
    );
    assert!(
        run_rows(&s, "run_dependency_manifests", child).await > 0,
        "the call child's scope manifest"
    );
    assert!(
        run_rows(&s, "workflow_waits", scope.run).await > 0,
        "the caller's call wait"
    );
    age(&s, scope).await;
    assert!(retire(&s, scope.run).await);
    assert_eq!(call_rows(&s, scope.run, child).await, [0; 5]);
    assert_eq!(run_rows(&s, "run_dependency_manifests", child).await, 0);
    assert_eq!(run_rows(&s, "run_dependency_manifests", scope.run).await, 0);
    assert_eq!(run_rows(&s, "workflow_waits", scope.run).await, 0);
    // Teardown, with a call still waiting on its child.
    let s = CallSetup::new().await;
    let claim = s.claim("integration-g8-teardown").await;
    assert!(matches!(
        s.call(&claim, &s.arguments("Mid-flight")).await.unwrap(),
        crate::application::workflow::agent::AgentToolObservation::Parked(_)
    ));
    let record = s.record(claim.fence.scope.run).await;
    let before = company_call_rows(&s).await;
    assert_eq!(before[0], 1);
    assert!(before[2] > 0 && before[3] > 0, "{before:?}");
    crate::application::use_cases::company::CompanyPersistence::delete(
        s.p(),
        s.company().as_uuid(),
    )
    .await
    .unwrap();
    assert_eq!(company_call_rows(&s).await, [0; 5]);
    for (table, column, id) in [
        ("workflow_runs", "company_id", s.company().as_uuid()),
        ("threads", "id", record.thread_id),
        ("thread_messages", "id", record.source_message_id),
    ] {
        let left: i64 =
            sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE {column}=$1"))
                .bind(id)
                .fetch_one(s.pool())
                .await
                .unwrap();
        assert_eq!(left, 0, "{table}");
    }
}

/// G2 (scenario 3, "parent response suppression remains independent"): desk B's responder is an
/// agent workflow that itself calls desk C, whose run answers `channel.no_reply`. C's reply is
/// suppressed and B's agent receives `no_reply`; B's own response is still decided and released
/// to the caller as `done` with B's contract body.
#[tokio::test]
async fn channel_call_integration_nested_no_reply_keeps_parent_response() {
    use super::channel_call_response_tests::{no_reply_destination, settled};
    use crate::application::workflow::agent::AgentToolObservation;
    use crate::application::workflow::completion::WorkflowCompletion;
    let s = CallSetup::new().await;
    let quiet = s
        .tool
        .fixture
        .publish_with_builtins(WorkflowId::new(Uuid::new_v4()), no_reply_destination())
        .await;
    let (quiet_desk, _) = s.add_desk("quiet", quiet, json!({})).await;
    // Desk B answers with an agent workflow whose agent may call desk C.
    let mut source: Value =
        serde_json::from_str(&registry::example("agent.run").unwrap().source).unwrap();
    let workflow = WorkflowId::new(Uuid::new_v4());
    source["workflow_id"] = json!(workflow.as_uuid());
    let version = publish_agent_workflow(
        &s.tool.fixture,
        workflow,
        "integration-nested-quiet",
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
    s.select(binding, Some(revision(1))).await.unwrap();
    let claim = s.claim("integration-g2-suppression").await;
    let scope = claim.fence.scope;
    let arguments = s.arguments("Ask C for me");
    assert!(matches!(
        s.call(&claim, &arguments).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    // B's agent calls C; C's run answers no_reply, and its own response is suppressed.
    let child = s.tool.child_of(scope).await;
    let destination = s
        .p()
        .claim_io(child, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    let nested_arguments =
        json!({"channel_id": quiet_desk.to_string(), "content": {"body": "Anything?"}});
    assert!(matches!(
        s.call(&destination, &nested_arguments).await.unwrap(),
        AgentToolObservation::Parked(_)
    ));
    let nested = s.tool.finish_child(child).await.run;
    assert_eq!(
        settled(&s, nested).await,
        Some(("suppressed".to_owned(), None))
    );
    let destination = continued(&s, child).await;
    assert_eq!(
        result(s.call(&destination, &nested_arguments).await.unwrap()),
        json!({"closure":"no_reply","child_run_id":nested.as_uuid()})
    );
    // B's agent finishes; B's run ends and its own response is decided, not suppressed.
    let step = s
        .p()
        .complete_io(crate::application::workflow::lease::FencedWorkflowResult {
            fence: destination.fence,
            output: json!("C had nothing to add"),
            policy: lease(),
        })
        .await
        .unwrap()
        .expect("B's agent step commits");
    assert!(step.successor.is_none(), "B's run ends after its agent");
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
