use super::*;
use crate::application::workflow::checkpoints::WorkflowCheckpointUpdate as Update;
use crate::services::harness::AgentExecutionOutput;
use crate::services::harness::runs::{InvocationId, SavedToolCall};

fn output() -> AgentExecutionOutput {
    AgentExecutionOutput {
        content: "saved".into(),
        structured: None,
        metadata: None,
        token_usage: Default::default(),
    }
}

#[tokio::test]
async fn workflow_harness_update_settlement_approval_finishes_without_new_model_call() {
    let f = configured().await;
    let p = f.model.native.fixture.persistence();
    scheduling::exceed(&f).await;
    let paused = stored(&f.model.native.fixture, f.model.run.run_id.0).await;
    p.submit_decision(command(p, f.model.write.fence.scope, f.reviewer, "approve").await)
        .await
        .unwrap();
    let write = resumed(&f.model, &paused).await;
    let update = Update::FinalOutput(Box::new(output()));
    let (left, right) = tokio::join!(
        p.update_workflow_checkpoint(write, update.clone()),
        p.update_workflow_checkpoint(write, update.clone())
    );
    let outcomes = [left.unwrap(), right.unwrap()];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, WriteOutcome::Applied(_)))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, WriteOutcome::AlreadyApplied(_)))
            .count(),
        1
    );
    let saved = stored(&f.model.native.fixture, f.model.run.run_id.0).await;
    assert!(saved["model_blocked_decision"].is_null());
    assert_eq!(saved["checkpoint"]["final_output"]["content"], "saved");
    assert_eq!(accounting(&f.model).await["calls"], 1);
    assert!(matches!(
        p.update_workflow_checkpoint(write, update).await.unwrap(),
        WriteOutcome::AlreadyApplied(_)
    ));
}

#[tokio::test]
async fn workflow_harness_update_cannot_consume_reservation_approval() {
    let f = configured().await;
    let p = f.model.native.fixture.persistence();
    refusal::park(&f).await;
    let paused = stored(&f.model.native.fixture, f.model.run.run_id.0).await;
    p.submit_decision(command(p, f.model.write.fence.scope, f.reviewer, "approve").await)
        .await
        .unwrap();
    let write = resumed(&f.model, &paused).await;
    let before = stored(&f.model.native.fixture, f.model.run.run_id.0).await;
    assert!(
        p.update_workflow_checkpoint(write, Update::BindTools("a".repeat(64)))
            .await
            .is_err()
    );
    assert_eq!(
        stored(&f.model.native.fixture, f.model.run.run_id.0).await,
        before
    );
    assert_eq!(accounting(&f.model).await["calls"], 0);
}

fn call() -> SavedToolCall {
    let invocation_id = InvocationId(Uuid::new_v4());
    SavedToolCall {
        invocation_id,
        call_id: invocation_id.0.to_string(),
        item_id: None,
        tool_id: "saved_tool".into(),
        arguments: json!({}),
    }
}

async fn settled_child(f: &TokenFixture, calls: Vec<SavedToolCall>) -> WorkflowCheckpointWrite {
    let p = f.model.native.fixture.persistence();
    let write = refusal::child_checkpoint(f).await;
    let mut request = reservation();
    request.input_tokens = 50;
    request.output_tokens = 50;
    let WorkflowModelAdmission::New(permit) = p
        .reserve_workflow_model(write, model_request(&request), Some(&notices(p)))
        .await
        .unwrap()
    else {
        panic!("child not admitted")
    };
    let run = permit.consume();
    let write = WorkflowCheckpointWrite {
        expected_revision: run.revision,
        ..write
    };
    assert!(
        p.update_workflow_checkpoint(write, Update::InvalidOutput)
            .await
            .is_err()
    );
    let mut turn = response(&request);
    turn.calls = calls;
    let WorkflowModelSettlement::Saved(outcome) = p
        .settle_workflow_model(write, turn, Some(&notices(p)))
        .await
        .unwrap()
    else {
        panic!("child did not settle")
    };
    let WriteOutcome::Applied(run) = *outcome else {
        panic!("child not saved")
    };
    WorkflowCheckpointWrite {
        expected_revision: run.revision,
        ..write
    }
}

#[tokio::test]
async fn workflow_harness_update_retains_result_after_refusal_but_cannot_prepare_next() {
    let f = configured().await;
    let p = f.model.native.fixture.persistence();
    let first = call();
    let second = call();
    let write = settled_child(&f, vec![first.clone(), second.clone()]).await;
    let WriteOutcome::Applied(run) = p
        .update_workflow_checkpoint(write, Update::PrepareInvocation(first.invocation_id))
        .await
        .unwrap()
    else {
        panic!("first invocation not prepared")
    };
    let write = WorkflowCheckpointWrite {
        expected_revision: run.revision,
        ..write
    };
    refusal::park(&f).await;
    p.submit_decision(command(p, f.model.write.fence.scope, f.reviewer, "reject").await)
        .await
        .unwrap();
    let WriteOutcome::Applied(run) = p
        .update_workflow_checkpoint(
            write,
            Update::InvocationResult {
                invocation: first.invocation_id,
                result: json!({"received":true}),
            },
        )
        .await
        .unwrap()
    else {
        panic!("received result was discarded")
    };
    assert_eq!(run.invocations[0].result, Some(json!({"received":true})));
    let write = WorkflowCheckpointWrite {
        expected_revision: run.revision,
        ..write
    };
    assert!(
        p.update_workflow_checkpoint(write, Update::PrepareInvocation(second.invocation_id))
            .await
            .is_err()
    );
    let next = run
        .apply(Mutation::Prepare(second.invocation_id))
        .unwrap()
        .0;
    assert!(
        write_model(
            &mut p.pool().acquire().await.unwrap(),
            &next,
            write.fence,
            None
        )
        .await
        .is_err()
    );
    let saved: RunCheckpoint = serde_json::from_value(
        stored(&f.model.native.fixture, write.checkpoint.0).await["checkpoint"].clone(),
    )
    .unwrap();
    assert_eq!(saved.revision, run.revision);
    assert_eq!(saved.invocations.len(), 1);
}
