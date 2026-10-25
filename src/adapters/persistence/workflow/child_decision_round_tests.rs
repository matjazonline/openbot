//! Revision rounds with real human decisions (plan 07 §Acceptance, 07-D1 §D1.10): each round is
//! a `decision.human` child whose reviewer comes from the child's `/params`, supplied by the
//! parent's static `with.params`; the parent loads thread context once, before its repeat step.
//! Everything runs through the worker and fair poll; no model is involved.
use super::*;
use crate::adapters::persistence::workflow::tests::admission_tests::admission_history_tests::Conversation;
use crate::adapters::persistence::workflow::tests::message_tests::support::member;
use crate::adapters::protocols::email::EmailRenderer;
use crate::application::transport::{DeliveryComposer, TransportRenderers};
use crate::application::workflow::decisions::*;
use crate::application::workflow::history::WorkflowHistoryRuntime;
use crate::application::workflow::lease::{LeasePolicy, WorkflowWorkerId};
use crate::application::workflow::polling::{PollPolicy, WorkflowFairPolling};
use crate::application::workflow::worker::{WorkflowHandler, WorkflowWorker};

fn artifact() -> Value {
    json!({"type":"object","properties":{"body":{"type":"string","maxLength":1024}},"required":["body"],"additionalProperties":false})
}

fn round_result() -> Value {
    json!({"type":"object","properties":{"choice":{"type":"string","enum":["accept","revise"]},"artifact":artifact(),"feedback":{"type":"string"}},"required":["choice","artifact","feedback"],"additionalProperties":false})
}

/// One round: a draft from the previous artifact and feedback, a human decision by the reviewer
/// in the round's own parameters, and that decision as the round's result.
fn round() -> Value {
    json!({
        "format_version": 1,
        "workflow_id": Uuid::nil().to_string(),
        "input_schema": {"type":"object","properties":{"previous_body":{"type":"string"},"feedback":{"type":"string"}},"required":["previous_body","feedback"],"additionalProperties":false},
        "parameter_schema": {"type":"object","properties":{"reviewer":{"type":"string","maxLength":64},"deadline":{"type":"string","maxLength":64}},"required":["reviewer","deadline"],"additionalProperties":false},
        "output_schema": round_result(),
        "resources": [],
        "entry": "draft",
        "steps": {
            "draft": {"type":"data.map","with":{
                "value":{"object":{"body":{"concat":[{"literal":"Draft from '"},{"ref":"/input/previous_body"},{"literal":"' after '"},{"ref":"/input/feedback"},{"literal":"'"}]}}},
                "output_schema":{"literal":artifact()}},
                "routes":{"success":"review"}},
            "review": {"type":"decision.human","with":{
                "proposal":{"ref":"/steps/draft/output"},
                "reviewers":{"array":[{"object":{"kind":{"literal":"user"},"id":{"ref":"/params/reviewer"}}}]},
                "deadline":{"ref":"/params/deadline"},
                "data_schema":{"literal":artifact()},
                "feedback_required":{"literal":["revise"]}},
                "routes":{"choices":{"accept":"outcome","revise":"outcome"}}},
            "outcome": {"type":"data.map","with":{
                "value":{"object":{
                    "choice":{"ref":"/steps/review/output/choice"},
                    "artifact":{"ref":"/steps/review/output/data"},
                    "feedback":{"ref":"/steps/review/output/feedback"}}},
                "output_schema":{"literal":round_result()}},
                "routes":{"success":"$end"}}
        },
        "limits": {"max_steps": 10, "max_context_bytes": 65536}
    })
}

/// Load the thread context once, repeat the round until it is not a revision, deliver once.
fn parent(child: WorkflowId, version: VersionId) -> Value {
    json!({
        "format_version": 1,
        "workflow_id": Uuid::nil().to_string(),
        "input_schema": true,
        "parameter_schema": {"type":"object","properties":{"reviewer":{"type":"string","maxLength":64},"deadline":{"type":"string","maxLength":64}},"required":["reviewer","deadline"],"additionalProperties":false},
        "output_schema": true,
        "resources": [],
        "entry": "context",
        "steps": {
            "context": {"type":"context.history","with":{"max_tokens":{"literal":1000}},"routes":{"success":"revisions"}},
            "revisions": {
                "type": "flow.repeat",
                "child_workflow_id": child.to_string(),
                "child_version_id": version.to_string(),
                "max_iterations": 3,
                "with": {
                    "input": {"object": {"previous_body": {"literal": ""}, "feedback": {"literal": ""}}},
                    "params": {"object": {"reviewer": {"ref": "/params/reviewer"}, "deadline": {"ref": "/params/deadline"}}}
                },
                "repeat": {
                    "next_input": {"object": {
                        "previous_body": {"ref": "/steps/revisions/output/result/artifact/body"},
                        "feedback": {"ref": "/steps/revisions/output/result/feedback"}}},
                    "exit_when": {"ne": [{"ref": "/steps/revisions/output/result/choice"}, {"literal": "revise"}]}
                },
                "routes": {"choices": {"done": "deliver", "exhausted": "escalate"}}
            },
            "deliver": {"type":"data.map","with":{"value":{"ref":"/steps/revisions/output/result/artifact"},"output_schema":{"literal":true}},"routes":{"success":"$end"}},
            "escalate": {"type":"data.map","with":{"value":{"literal":"escalated"},"output_schema":{"literal":true}},"routes":{"success":"$end"}}
        },
        "limits": {"max_steps": 20, "max_context_bytes": 65536,
            "root_budget": {"activations": 100, "model_calls": 2, "repetitions": 10}}
    })
}

/// Poll and process until nothing is offered: every live run then waits on a person. A fair
/// page can be empty while the company's cursor wraps, so rest means several empty pages.
async fn drive<H: WorkflowHandler>(
    f: &AdmissionFixture,
    worker: &WorkflowWorker<'_, PostgresPersistence, H>,
) {
    let cancel = tokio_util::sync::CancellationToken::new();
    let mut empty = 0;
    for _ in 0..64 {
        let page = f.persistence().poll_fair(worker.worker, 16).await.unwrap();
        if page.candidates.is_empty() {
            empty += 1;
            if empty == 3 {
                return;
            }
            continue;
        }
        empty = 0;
        for candidate in page.candidates {
            worker.process(candidate, &cancel).await.unwrap();
        }
    }
    panic!("the workflow did not come to rest");
}

/// The run of the round the root is currently waiting on.
async fn live_round(f: &AdmissionFixture, root: RunId) -> RunId {
    RunId::new(
        sqlx::query_scalar("SELECT child_run_id FROM workflow_waits WHERE run_id=$1 AND reason='child_run' AND state='waiting'")
            .bind(root.as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .expect("the root waits on a live round"),
    )
}

async fn decision_of(f: &AdmissionFixture, round: RunId) -> DecisionId {
    DecisionId(
        sqlx::query_scalar("SELECT id FROM workflow_decisions WHERE run_id=$1")
            .bind(round.as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap(),
    )
}

async fn decision_row(f: &AdmissionFixture, decision: DecisionId) -> Value {
    sqlx::query_scalar("SELECT to_jsonb(decision) FROM workflow_decisions AS decision WHERE id=$1")
        .bind(decision.0)
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}

struct Answer<'a> {
    key: &'a str,
    choice: &'a str,
    body: &'a str,
    feedback: &'a str,
}

fn submission(
    company: CompanyId,
    reviewer: WorkflowActor,
    view: &DecisionView,
    answer: Answer<'_>,
) -> SubmitDecision {
    SubmitDecision {
        company,
        actor: reviewer,
        decision: view.decision,
        command_key: DecisionCommandKey::parse(answer.key).unwrap(),
        expected_revision: view.revision,
        expected_artifact: view.artifact_digest.clone(),
        choice: answer.choice.into(),
        feedback: answer.feedback.into(),
        data: json!({"body": answer.body}),
    }
}

async fn count(f: &AdmissionFixture, query: &str, run: RunId) -> i64 {
    sqlx::query_scalar(query)
        .bind(run.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn workflow_revision_rounds_with_human_decisions_keep_old_rounds_and_load_context_once() {
    let fixture = Fixture::new().await;
    let reviewer = member(&fixture, "admin").await;
    let workflow = WorkflowId::new(Uuid::from_u128(0x0701));
    let version = fixture.publish_workflow(workflow, round()).await;
    let mut root = parent(workflow, version);
    root["workflow_id"] = json!(fixture.target.workflow.to_string());
    let mut binding = BindingFixture::from_fixture(fixture, root).await;
    let deadline = (chrono::Utc::now() + chrono::Duration::minutes(30)).to_rfc3339();
    binding.params = json!({"reviewer": reviewer.to_string(), "deadline": deadline});
    let f = AdmissionFixture::with_binding(binding).await;
    let p = f.persistence();
    let company = f.binding.target.company;
    let c = Conversation::new(&f).await;
    let command = f.prepare(c.request(&f, "revision-root", c.message)).await;
    p.admit(&command).await.unwrap();
    let root = command.proposed_run_id();
    let renderers = Arc::new(
        TransportRenderers::new()
            .register(Arc::new(EmailRenderer::new("agents.test")))
            .unwrap(),
    );
    let notices = DecisionNotices::new(
        DeliveryComposer::new(renderers, Arc::new(p.clone())),
        "https://agents.test",
        "agents.test",
    );
    let history = WorkflowHistoryRuntime::new(p.clone());
    let worker = WorkflowWorker {
        port: p,
        handler: &history,
        notices: Some(&notices),
        worker: WorkflowWorkerId(Uuid::new_v4()),
        lease: LeasePolicy::new(std::time::Duration::from_secs(30)).unwrap(),
        poll: PollPolicy::new(std::time::Duration::from_millis(50), 16).unwrap(),
        active_jobs: None,
    };
    let actor = WorkflowActor::authenticated(reviewer).unwrap();
    let read = |decision| async move { p.read_decision(company, actor, decision).await.unwrap() };
    let submit = |view: &DecisionView, key, choice, body, feedback| {
        submission(
            company,
            actor,
            view,
            Answer {
                key,
                choice,
                body,
                feedback,
            },
        )
    };

    // Context loads, then round one parks on its decision for the reviewer from its /params.
    drive(&f, &worker).await;
    let first = live_round(&f, root).await;
    let first_decision = decision_of(&f, first).await;
    let first_view = read(first_decision).await;
    assert_eq!(first_view.reviewers, [reviewer]);
    assert_eq!(
        first_view.proposal,
        json!({"body": "Draft from '' after ''"})
    );
    let applied = p
        .submit_decision(submit(
            &first_view,
            "round-1",
            "revise",
            "Edited one",
            "Shorter",
        ))
        .await
        .unwrap();
    assert_eq!(applied.disposition, DecisionDisposition::Applied);

    // The revision reaches round two as explicit input; round one stays as it was.
    drive(&f, &worker).await;
    let second = live_round(&f, root).await;
    assert_ne!(second, first);
    let second_view = read(decision_of(&f, second).await).await;
    assert_eq!(second_view.reviewers, [reviewer]);
    assert_eq!(
        second_view.proposal,
        json!({"body": "Draft from 'Edited one' after 'Shorter'"})
    );
    let decided = decision_row(&f, first_decision).await;
    // Accepting the older draft cannot authorize the newer one, with the old or a fresh view.
    let refreshed = read(first_decision).await;
    assert_ne!(refreshed.state, "waiting");
    for (key, view) in [("late-old", &first_view), ("late-fresh", &refreshed)] {
        assert!(
            p.submit_decision(submit(view, key, "accept", "Edited one", ""))
                .await
                .is_err(),
            "{key}"
        );
    }
    assert_eq!(decision_row(&f, first_decision).await, decided);
    assert_eq!(read(decision_of(&f, second).await).await.state, "waiting");
    assert_eq!(live_round(&f, root).await, second);

    // Acceptance in round two ends the repetition; only that artifact is delivered, once.
    p.submit_decision(submit(&second_view, "round-2", "accept", "Final", ""))
        .await
        .unwrap();
    drive(&f, &worker).await;
    let state: String = sqlx::query_scalar("SELECT state FROM workflow_runs WHERE id=$1")
        .bind(root.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(state, "succeeded");
    let delivered: Vec<Value> = sqlx::query_scalar(
        "SELECT committed_output FROM workflow_executions WHERE run_id=$1 AND step_id='deliver'",
    )
    .bind(root.as_uuid())
    .fetch_all(p.pool())
    .await
    .unwrap();
    assert_eq!(delivered, [json!({"body": "Final"})]);
    // The context load and the repeat step each ran once, across both rounds.
    for step in ["context", "revisions"] {
        assert_eq!(
            count(
                &f,
                &format!(
                    "SELECT count(*) FROM workflow_executions WHERE run_id=$1 AND step_id='{step}'"
                ),
                root,
            )
            .await,
            1,
            "{step}"
        );
    }
    assert_eq!(
        count(
            &f,
            "SELECT count(*) FROM workflow_history_members WHERE run_id=$1",
            root
        )
        .await,
        1
    );
    // Two rounds, two decisions, each still holding its own submission.
    assert_eq!(
        count(
            &f,
            "SELECT count(*) FROM workflow_runs WHERE parent_run_id=$1",
            root
        )
        .await,
        2
    );
    assert_eq!(decision_row(&f, first_decision).await, decided);
    let first_result: Value = sqlx::query_scalar(
        "SELECT committed_output FROM workflow_executions WHERE run_id=$1 AND step_id='outcome'",
    )
    .bind(first.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(
        first_result,
        json!({"choice":"revise","artifact":{"body":"Edited one"},"feedback":"Shorter"})
    );
}
