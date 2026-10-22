//! `flow.repeat` rounds (07-D1 §D1.4, §D1.6, §D1.9). Each round's child is a `wait.event` whose
//! payload stands in for the human result, so rounds are scripted without a model or reviewer.
use super::*;
use crate::application::workflow::children::WorkflowChildren;
use crate::application::workflow::completion::CommitDisposition;
use crate::application::workflow::lease::WorkflowWorkerId;
use crate::application::workflow::polling::WorkflowFairPolling;
use crate::application::workflow::waits::*;

fn choice(name: &str) -> RouteSelection {
    RouteSelection::Choice(ChoiceName::parse(name).unwrap())
}

fn round_output() -> Value {
    json!({
        "type": "object",
        "properties": {
            "choice": {"type": "string", "enum": ["accept", "revise", "cancel"]},
            "artifact": {"type": "object", "properties": {"body": {"type": "string"}},
                "required": ["body"], "additionalProperties": false},
            "feedback": {"type": "string"}
        },
        "required": ["choice", "artifact", "feedback"],
        "additionalProperties": false
    })
}

/// One round: wait for the reviewer's result and return it as the round's output.
pub(super) fn round_child() -> Value {
    json!({
        "format_version": 1,
        "workflow_id": Uuid::nil().to_string(),
        "input_schema": {"type": "object", "properties": {
            "previous_body": {"type": "string"}, "feedback": {"type": "string", "maxLength": 32}},
            "required": ["previous_body", "feedback"], "additionalProperties": false},
        "parameter_schema": true,
        "output_schema": round_output(),
        "resources": [],
        "entry": "review",
        "steps": {"review": {
            "type": "wait.event",
            "with": {
                "event": {"literal": "review.completed"},
                "correlation": {"literal": "round"},
                "deadline": {"literal": "2099-01-01T00:00:00Z"},
                "payload_schema": {"literal": round_output()}
            },
            "routes": {"success": "$end"}
        }},
        "limits": {"max_steps": 10, "max_context_bytes": 65536}
    })
}

/// Repeat the round until it is not a revision, then deliver the final artifact once.
pub(super) fn repeat_parent(child: WorkflowId, version: VersionId, rounds: u32) -> Value {
    json!({
        "format_version": 1,
        "workflow_id": Uuid::nil().to_string(),
        "input_schema": true,
        "parameter_schema": true,
        "output_schema": true,
        "resources": [],
        "entry": "revisions",
        "steps": {
            "revisions": {
                "type": "flow.repeat",
                "child_workflow_id": child.to_string(),
                "child_version_id": version.to_string(),
                "max_iterations": rounds,
                "with": {"input": {"object": {
                    "previous_body": {"literal": ""}, "feedback": {"literal": ""}}}},
                "repeat": {
                    "next_input": {"object": {
                        "previous_body": {"ref": "/steps/revisions/output/result/artifact/body"},
                        "feedback": {"ref": "/steps/revisions/output/result/feedback"}}},
                    "exit_when": {"ne": [{"ref": "/steps/revisions/output/result/choice"}, {"literal": "revise"}]}
                },
                "routes": {"choices": {"done": "deliver", "exhausted": "escalate"}, "error": "invalid"}
            },
            "deliver": {"type": "data.map", "with": {
                "value": {"ref": "/steps/revisions/output/result/artifact"},
                "output_schema": {"literal": true}}, "routes": {"success": "$end"}},
            "escalate": {"type": "data.map", "with": {
                "value": {"literal": "escalated"}, "output_schema": {"literal": true}},
                "routes": {"success": "$end"}},
            "invalid": {"type": "data.map", "with": {
                "value": {"literal": "invalid"}, "output_schema": {"literal": true}},
                "routes": {"success": "$end"}}
        },
        "limits": {"max_steps": 20, "max_context_bytes": 65536,
            "root_budget": {"activations": 100, "model_calls": 2, "repetitions": 10}}
    })
}

async fn repeating(rounds: u32) -> (AdmissionFixture, ActivationRequest) {
    AdmissionFixture::with_child(round_child(), |child, version| {
        repeat_parent(child, version, rounds)
    })
    .await
}

fn review(round: ActivationRequest, choice: &str, body: &str, feedback: &str) -> WorkflowEvent {
    WorkflowEvent {
        company: round.company,
        run: round.run,
        id: WorkflowEventId(Uuid::new_v4()),
        name: WorkflowEventName("review.completed".into()),
        correlation: WorkflowEventCorrelation("round".into()),
        payload: json!({"choice": choice, "artifact": {"body": body}, "feedback": feedback}),
    }
}

/// Answer the parent's current round and let the parent consume it.
async fn answer(
    f: &AdmissionFixture,
    parent: ActivationRequest,
    choice: &str,
    body: &str,
    feedback: &str,
) -> (ActivationRequest, WaitProgress) {
    let p = f.persistence();
    let round = f.current_child(parent).await.expect("a live round");
    p.park_wait(round).await.unwrap().unwrap();
    assert_eq!(
        p.record_event(review(round, choice, body, feedback))
            .await
            .unwrap(),
        EventIngress::Recorded
    );
    assert!(matches!(
        p.resume_wait(round).await.unwrap(),
        WaitProgress::Completed(_)
    ));
    (round, p.resume_wait(parent).await.unwrap())
}

async fn row(f: &AdmissionFixture, query: &str, id: Uuid) -> Value {
    sqlx::query_scalar(query)
        .bind(id)
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}

async fn receipts(f: &AdmissionFixture, scope: ActivationRequest) -> Vec<String> {
    sqlx::query_scalar("SELECT reservation_key||':'||disposition FROM workflow_budget_receipts WHERE run_id=$1 AND execution_id=$2 AND resource='repetition' ORDER BY created_at, reservation_key")
        .bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .fetch_all(f.persistence().pool()).await.unwrap()
}

#[tokio::test]
async fn workflow_repeat_feedback_reaches_next_round_and_last_round_accept_is_done() {
    let (f, parent) = repeating(3).await;
    let p = f.persistence();
    p.park_child(parent).await.unwrap().unwrap();
    assert_eq!(p.resume_wait(parent).await.unwrap(), WaitProgress::Pending);
    let (first, progress) = answer(&f, parent, "revise", "Draft 1", "Shorter").await;
    assert_eq!(progress, WaitProgress::NextRound);
    let second = f.current_child(parent).await.unwrap();
    // The revision reaches the next round as explicit input; round one stays as it was.
    assert_eq!(
        row(
            &f,
            "SELECT input FROM workflow_runs WHERE id=$1",
            second.run.as_uuid()
        )
        .await,
        json!({"previous_body": "Draft 1", "feedback": "Shorter"})
    );
    let first_output = row(
        &f,
        "SELECT committed_output FROM workflow_executions WHERE id=$1",
        first.execution.as_uuid(),
    )
    .await;
    assert_eq!(first_output["artifact"]["body"], "Draft 1");
    let (_, progress) = answer(&f, parent, "revise", "Draft 2", "Clearer").await;
    assert_eq!(progress, WaitProgress::NextRound);
    let (last, progress) = answer(&f, parent, "accept", "Draft 3", "").await;
    let WaitProgress::Completed(done) = progress else {
        panic!("{progress:?}")
    };
    // Acceptance in the last permitted round leaves through done, with the final result.
    assert_eq!(done.disposition, CommitDisposition::Committed);
    assert_eq!(done.route, choice("done"));
    assert_eq!(done.output["rounds"], 3);
    assert_eq!(done.output["result"]["artifact"]["body"], "Draft 3");
    assert_eq!(
        receipts(&f, parent).await,
        ["round:1:granted", "round:2:granted", "round:3:granted"]
    );
    let rounds: Vec<i32> = sqlx::query_scalar("SELECT child_ordinal FROM workflow_waits WHERE run_id=$1 AND reason='child_run' AND state='completed' ORDER BY child_ordinal")
        .bind(parent.run.as_uuid()).fetch_all(p.pool()).await.unwrap();
    assert_eq!(rounds, [1, 2, 3]);
    // A replay answers the same completion; nothing is created again.
    let WaitProgress::Completed(replay) = p.resume_wait(parent).await.unwrap() else {
        panic!("replay")
    };
    assert_eq!(replay.disposition, CommitDisposition::Replayed);
    assert_eq!(replay.output, done.output);
    // Only the final artifact is delivered, once.
    let deliver = done.successor.unwrap();
    p.advance_pure(deliver, budget(4)).await.unwrap();
    let delivered: Vec<Value> = sqlx::query_scalar(
        "SELECT committed_output FROM workflow_executions WHERE run_id=$1 AND step_id='deliver'",
    )
    .bind(parent.run.as_uuid())
    .fetch_all(p.pool())
    .await
    .unwrap();
    assert_eq!(delivered, [json!({"body": "Draft 3"})]);
    // A late result for an older round cannot settle anything: its round is over.
    assert_eq!(
        p.record_event(review(first, "accept", "Draft 1", ""))
            .await
            .unwrap(),
        EventIngress::Unmatched
    );
    assert_eq!(
        p.record_event(review(last, "accept", "Draft 3", ""))
            .await
            .unwrap(),
        EventIngress::Unmatched
    );
}

#[tokio::test]
async fn workflow_repeat_exhaustion_takes_its_route_and_never_approves() {
    let (f, parent) = repeating(2).await;
    let p = f.persistence();
    p.park_child(parent).await.unwrap().unwrap();
    assert_eq!(
        answer(&f, parent, "revise", "Draft 1", "More").await.1,
        WaitProgress::NextRound
    );
    let WaitProgress::Completed(exhausted) =
        answer(&f, parent, "revise", "Draft 2", "More").await.1
    else {
        panic!("exhausted")
    };
    assert_eq!(exhausted.route, choice("exhausted"));
    assert_eq!(exhausted.output["rounds"], 2);
    assert_eq!(exhausted.output["result"]["choice"], "revise");
    let next = exhausted.successor.unwrap();
    let step: String = sqlx::query_scalar("SELECT step_id FROM workflow_executions WHERE id=$1")
        .bind(next.execution.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(step, "escalate");
    assert_eq!(
        receipts(&f, parent).await,
        ["round:1:granted", "round:2:granted"]
    );
}

#[tokio::test]
async fn workflow_repeat_budget_exhaustion_fails_unrouted_at_park_and_between_rounds() {
    // Between rounds: the root grants two repetitions to a three-round step.
    let (f, parent) = AdmissionFixture::with_child(round_child(), |child, version| {
        let mut source = repeat_parent(child, version, 3);
        source["limits"]["root_budget"]["repetitions"] = json!(2);
        source
    })
    .await;
    let p = f.persistence();
    p.park_child(parent).await.unwrap().unwrap();
    answer(&f, parent, "revise", "Draft 1", "More").await;
    let progress = answer(&f, parent, "revise", "Draft 2", "More").await.1;
    assert_eq!(
        progress,
        WaitProgress::Failed {
            disposition: CommitDisposition::Committed,
            reason: WaitFailureReason::RootBudgetExhausted
        }
    );
    let run = row(
        &f,
        "SELECT to_jsonb(run) FROM workflow_runs AS run WHERE id=$1",
        parent.run.as_uuid(),
    )
    .await;
    assert_eq!(run["state"], "failed");
    assert_eq!(
        run["terminal_execution_id"],
        json!(parent.execution.as_uuid())
    );
    assert!(f.current_child(parent).await.is_none());
    assert_eq!(
        receipts(&f, parent).await,
        ["round:1:granted", "round:2:granted", "round:3:exhausted"]
    );
    assert_eq!(
        p.resume_wait(parent).await.unwrap(),
        WaitProgress::Failed {
            disposition: CommitDisposition::Replayed,
            reason: WaitFailureReason::RootBudgetExhausted
        }
    );
    // Settled: two consecutive polls never offer the parent again (no hot loop).
    for _ in 0..2 {
        let page = p
            .poll_fair(WorkflowWorkerId(Uuid::new_v4()), 128)
            .await
            .unwrap();
        assert!(page.candidates.iter().all(|c| c.scope.run != parent.run));
    }

    // At park: another reservation already spent the only repetition.
    let (f, parent) = AdmissionFixture::with_child(round_child(), |child, version| {
        let mut source = repeat_parent(child, version, 3);
        source["limits"]["root_budget"]["repetitions"] = json!(1);
        source
    })
    .await;
    let p = f.persistence();
    sqlx::query("INSERT INTO workflow_budget_receipts(company_id,run_id,execution_id,root_run_id,resource,reservation_key,quantity,disposition) VALUES($1,$2,$3,$2,'repetition','elsewhere',1,'granted')")
        .bind(parent.company.as_uuid()).bind(parent.run.as_uuid()).bind(parent.execution.as_uuid())
        .execute(p.pool()).await.unwrap();
    assert!(p.park_child(parent).await.unwrap().is_none());
    let run = row(
        &f,
        "SELECT to_jsonb(run) FROM workflow_runs AS run WHERE id=$1",
        parent.run.as_uuid(),
    )
    .await;
    assert_eq!(run["state"], "failed");
    let children: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE parent_run_id=$1")
            .bind(parent.run.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(children, 0);
    for _ in 0..2 {
        let page = p
            .poll_fair(WorkflowWorkerId(Uuid::new_v4()), 128)
            .await
            .unwrap();
        assert!(page.candidates.iter().all(|c| c.scope.run != parent.run));
    }
}

#[tokio::test]
async fn workflow_repeat_undecidable_round_takes_the_error_route() {
    let (f, parent) = repeating(3).await;
    let p = f.persistence();
    p.park_child(parent).await.unwrap().unwrap();
    // The feedback is too long for the next round's input: Terminal workflow.invalid_input.
    let long = "x".repeat(40);
    let WaitProgress::Completed(routed) = answer(&f, parent, "revise", "Draft 1", &long).await.1
    else {
        panic!("routed")
    };
    assert_eq!(routed.route, RouteSelection::FinalError);
    let next = routed.successor.unwrap();
    let step: String = sqlx::query_scalar("SELECT step_id FROM workflow_executions WHERE id=$1")
        .bind(next.execution.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(step, "invalid");
    assert!(f.current_child(parent).await.is_none());
    assert_eq!(receipts(&f, parent).await, ["round:1:granted"]);
    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_run_events WHERE run_id=$1 AND event_kind='workflow.invalid_input'")
        .bind(parent.run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(audit, 1);
}
