use super::*;
use crate::application::agent::AgentPublicationScope;
use crate::application::workflow::checkpoints::WorkflowCheckpointUpdate;
use crate::entities::agent_capability::AgentId;
use crate::services::harness::runs::{InvocationId, SavedToolCall};
use std::future::Future;

async fn blocked_by(p: &PostgresPersistence, publisher: i32) {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity AS waiting
                 WHERE waiting.datname=current_database() AND waiting.wait_event_type='Lock'
                   AND $1=ANY(pg_blocking_pids(waiting.pid)))",
            )
            .bind(publisher)
            .fetch_one(p.pool())
            .await
            .unwrap();
            if waiting {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the checkpoint must actually wait on the preview's company lock");
}

async fn preview_race<T>(f: &ModelFixture, checkpoint: impl Future<Output = AppResult<T>>) -> T {
    let p = f.native.fixture.persistence();
    let target = f.native.fixture.binding.target;
    let scope = AgentPublicationScope {
        company: target.company,
        actor: target.actor,
        agent: AgentId::new(f.run.identity.agent_id),
    };
    let mut publishing = p.pool().begin().await.unwrap();
    let publisher: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *publishing)
        .await
        .unwrap();
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE id=$1 FOR SHARE")
        .bind(scope.actor.user_id())
        .fetch_one(&mut *publishing)
        .await
        .unwrap();
    crate::adapters::persistence::workflow::authority::authorize_company(
        &mut publishing,
        scope.company,
        scope.actor,
    )
    .await
    .unwrap();
    // The real publication witness requests agent UPDATE only once the actual
    // checkpoint is queued on our company lock. Old agent→company order deadlocks.
    let preview = async {
        blocked_by(p, publisher).await;
        let preview =
            crate::adapters::persistence::agent_publication::LockedAgentPublication::lock(
                &mut publishing,
                scope,
            )
            .await
            .unwrap()
            .preview()
            .await
            .unwrap();
        assert_eq!(preview.definition.agent_id, scope.agent);
        publishing.commit().await.unwrap();
    };
    // Keep the checkpoint→agent future off the stock test thread stack.
    let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(Box::pin(checkpoint), preview)
    })
    .await
    .expect("preview and checkpoint must serialize without deadlock");
    result.unwrap()
}

#[tokio::test]
async fn autonomous_agent_authority_checkpoint_model_reservation_serializes_with_preview() {
    let f = opened().await;
    let p = f.native.fixture.persistence();
    let outcome = preview_race(
        &f,
        p.reserve_workflow_model(f.write, model_request(&reservation()), None),
    )
    .await;
    assert!(matches!(outcome, WorkflowModelAdmission::New(_)));
    assert_eq!(accounting(&f).await["calls"], 1);
}

async fn saved_call(f: &ModelFixture) -> (WorkflowCheckpointWrite, InvocationId) {
    let p = f.native.fixture.persistence();
    let request = reservation();
    let WorkflowModelAdmission::New(permit) = p
        .reserve_workflow_model(f.write, model_request(&request), None)
        .await
        .unwrap()
    else {
        panic!("model request must be admitted")
    };
    let mut write = WorkflowCheckpointWrite {
        expected_revision: permit.consume().revision,
        ..f.write
    };
    let invocation = InvocationId(Uuid::new_v4());
    let mut turn = response(&request);
    turn.calls = vec![SavedToolCall {
        invocation_id: invocation,
        call_id: invocation.0.to_string(),
        item_id: None,
        tool_id: "saved_tool".into(),
        arguments: json!({}),
    }];
    let WorkflowModelSettlement::Saved(outcome) =
        p.settle_workflow_model(write, turn, None).await.unwrap()
    else {
        panic!("model response must be saved")
    };
    let WriteOutcome::Applied(run) = *outcome else {
        panic!("model response must advance the checkpoint")
    };
    write.expected_revision = run.revision;
    (write, invocation)
}

#[tokio::test]
async fn autonomous_agent_authority_checkpoint_saved_call_serializes_with_preview() {
    let f = opened().await;
    let p = f.native.fixture.persistence();
    let (write, invocation) = saved_call(&f).await;
    let outcome = preview_race(
        &f,
        p.update_workflow_checkpoint(
            write,
            WorkflowCheckpointUpdate::PrepareInvocation(invocation),
        ),
    )
    .await;
    let WriteOutcome::Applied(run) = outcome else {
        panic!("the saved invocation must advance after preview releases company")
    };
    assert_eq!(run.invocations.len(), 1);
    assert!(run.invocations[0].result.is_none());
}
