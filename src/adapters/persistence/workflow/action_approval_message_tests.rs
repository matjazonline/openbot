use super::super::approval_test_support::{command, composer, notices};
use super::*;
use crate::adapters::workflow_http::NoLocal;
use crate::application::workflow::polling::WorkflowStepKind;
use crate::application::workflow::{actions::*, decisions::WorkflowDecisions, message::*};

/// Exercise a frozen protected request through the existing preparation port.
#[derive(Clone)]
struct ProtectedMessages(PostgresPersistence);
#[async_trait]
impl MessageActionPreparation for ProtectedMessages {
    async fn message_step_request(
        &self,
        claim: &ClaimedWorkflow,
        kind: &WorkflowStepKind,
    ) -> AppResult<ActionRequest> {
        let mut request = self.0.message_step_request(claim, kind).await?;
        request.context.approval_required = true;
        Ok(request)
    }
    async fn message_tool_request(
        &self,
        scope: ActionScope,
        kind: MessageKind,
        arguments: Value,
    ) -> AppResult<ActionRequest> {
        self.0.message_tool_request(scope, kind, arguments).await
    }
    async fn message_routes(
        &self,
        request: &ActionRequest,
        arguments: &MessageArguments,
    ) -> AppResult<MessageRoutes> {
        self.0.message_routes(request, arguments).await
    }
}
#[async_trait]
impl ActionIntents for ProtectedMessages {
    async fn prepare(
        &self,
        action: &FrozenAction,
        model_call: Option<&ModelToolCallId>,
    ) -> AppResult<ActionIntent> {
        self.0.prepare(action, model_call).await
    }
}

fn suspended(outcome: WorkflowHandlerOutcome) -> crate::domain::workflow::WaitId {
    match outcome {
        WorkflowHandlerOutcome::Suspended(wait) => wait.wait,
        WorkflowHandlerOutcome::Completed(result) => panic!("expected suspension: {result:?}"),
    }
}

fn protected() -> impl std::future::Future<Output = Setup> {
    setup_with_owner(|_, _, user, owner| {
        (
            "message.send",
            json!({
                "content":{"literal":{"subject":"Approved message","body":"Send after approval"}},
                "destinations":{"literal":[{"kind":"user","id":user}]},
                "approval":{"literal":{"reviewers":[{"kind":"user","id":owner}],
                    "deadline":(chrono::Utc::now()+chrono::Duration::minutes(20)).to_rfc3339()}}
            }),
        )
    })
}

fn approval_runtime(
    p: &PostgresPersistence,
) -> MessageActions<
    ProtectedMessages,
    action_dispatch::PostgresActionDispatch<message::MessageActionAuthority, NoLocal>,
> {
    let dispatch = action_dispatch::PostgresActionDispatch::new(
        p.clone(),
        message::MessageActionAuthority,
        NoLocal,
    )
    .with_decision_notices(notices(p));
    MessageActions::new(
        ProtectedMessages(p.clone()),
        dispatch,
        composer(p),
        APP_DOMAIN.into(),
        lease(),
    )
}

#[tokio::test]
async fn workflow_action_approval_message_then_effect_wait_preserves_both_histories() {
    let setup = Box::pin(protected()).await;
    let p = &setup.fixture.persistence;
    let runtime = approval_runtime(p);
    let approval = suspended(Box::pin(runtime.execute(&setup.kind, &setup.claim)).await);
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_message_actions")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(before, 0);
    let command = command(p, setup.scope(), setup.fixture.target.actor, "approve").await;
    p.submit_decision(command).await.unwrap();
    assert!(matches!(
        p.resume_wait(setup.scope()).await.unwrap(),
        WaitProgress::Refused
    ));
    let next = p
        .claim_io(setup.scope(), WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    let effect = suspended(Box::pin(runtime.execute(&setup.kind, &next)).await);
    assert_ne!(approval, effect);
    assert!(
        p.confirm_suspension(setup.claim.fence, approval)
            .await
            .unwrap()
    );
    assert!(p.confirm_suspension(next.fence, effect).await.unwrap());
    let waits: Vec<(String, String)> =
        sqlx::query_as("SELECT reason,state FROM workflow_waits ORDER BY reason")
            .fetch_all(p.pool())
            .await
            .unwrap();
    assert_eq!(
        waits,
        vec![
            ("decision".into(), "completed".into()),
            ("effect".into(), "waiting".into())
        ]
    );
    assert!(matches!(
        p.resume_wait(setup.scope()).await.unwrap(),
        WaitProgress::Pending
    ));
    let id = delivery(setup.pool(), setup.scope().run, 0).await;
    deliver(setup.pool(), id, "delivered").await;
    p.consume_message_receipts(setup.scope(), lease())
        .await
        .unwrap();
    assert!(matches!(
        p.resume_wait(setup.scope()).await.unwrap(),
        WaitProgress::Completed(_)
    ));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_action_dispatches")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn workflow_action_approval_race_message_lost_ack_resolves_park() {
    use crate::adapters::persistence::workflow::persistence_bound::recorder::Injected;
    use crate::adapters::persistence::workflow::tests::admission_tests::lease_tests::resolution_gate::{Abort, Gate, Stall, connect, resolution_behind};
    let _injected = Injected::mark();
    for applies in [true, false] {
        let setup = Box::pin(protected()).await;
        let p = &setup.fixture.persistence;
        let (mut observer, _) = connect(&setup).await;
        let mut gate = Gate::arm(
            &setup,
            "workflow_decisions",
            "INSERT",
            "true",
            Stall::Commit,
        )
        .await;
        let (runtime, kind, claim) = (approval_runtime(p), setup.kind.clone(), setup.claim.clone());
        let task = tokio::spawn(async move { runtime.execute(&kind, &claim).await });
        let stalled = gate
            .stalled(&mut observer, &[&|| !task.is_finished()])
            .await;
        if !applies {
            gate.abort_on_release(Abort::Backend(stalled)).await;
        }
        resolution_behind(&mut observer, stalled, lease(), &[&|| !task.is_finished()]).await;
        gate.release().await;
        let outcome = task.await.unwrap();
        if applies {
            let wait = suspended(outcome);
            assert!(p.confirm_suspension(setup.claim.fence, wait).await.unwrap());
        } else {
            assert!(matches!(outcome, WorkflowHandlerOutcome::Completed(Err(_))));
        }
        let counts: Value = sqlx::query_scalar("SELECT jsonb_build_object('decisions',(SELECT count(*) FROM workflow_decisions),'actions',(SELECT count(*) FROM workflow_message_actions),'dispatches',(SELECT count(*) FROM workflow_action_dispatches))")
            .fetch_one(p.pool()).await.unwrap();
        assert_eq!(
            counts,
            json!({"decisions":i64::from(applies),"actions":0,"dispatches":0})
        );
    }
}
