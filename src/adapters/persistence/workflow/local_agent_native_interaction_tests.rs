//! Actual independent human input, UI permission denial and isolated login failure.
use super::*;
use crate::application::local_agents::management::LocalOperationView;

async fn waiting_input(rig: &NativeRig, id: OperationId) -> LocalOperationView {
    tokio::time::timeout(std::time::Duration::from_secs(45), async {
        loop {
            let view = rig
                .s
                .fixture
                .persistence
                .read_operation(
                    rig.s.fixture.target.actor.user_id(),
                    rig.s.scope.company.as_uuid(),
                    id,
                )
                .await
                .unwrap();
            assert!(
                view.operation.outcome().is_none(),
                "native permission scenario completed before requesting input"
            );
            if view.observed_turn_state == Some(TurnState::WaitingForInput) {
                return view;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap()
}

pub(super) async fn settled(rig: &NativeRig, id: OperationId) -> LocalOperationView {
    tokio::time::timeout(std::time::Duration::from_secs(120), async {
        loop {
            let view = rig
                .s
                .fixture
                .persistence
                .read_operation(
                    rig.s.fixture.target.actor.user_id(),
                    rig.s.scope.company.as_uuid(),
                    id,
                )
                .await
                .unwrap();
            if view.operation.outcome().is_some() {
                return view;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap()
}

fn turn(view: &LocalOperationView) -> CloudroomTurnId {
    match view.operation.dispatch() {
        DispatchEvidence::NativeIdentity {
            turn_id: Some(id), ..
        } => id.clone(),
        other => panic!("native interaction requires a positively accepted turn: {other:?}"),
    }
}

pub(super) struct PermissionWait {
    request: ControlSession,
    own: String,
    own_turn: CloudroomTurnId,
}

pub(super) async fn begin_permission(
    rig: &mut NativeRig,
    initial: &NativeInitial,
) -> PermissionWait {
    let policy = format!("OPENBOT_POLICY_{}", Uuid::new_v4().simple());
    rig.peer.command(json!({"command":"human","thread":initial.thread,"text":policy,"permissionMode":"accept-edits"})).await;
    rig.peer.event("human").await;
    rig.peer
        .command(json!({"command":"human_terminal","thread":initial.thread,"text":policy}))
        .await;
    rig.peer.event_timeout("human_terminal", 150).await;
    let own = format!("OPENBOT_PERMISSION_{}", Uuid::new_v4().simple());
    rig.peer
        .command(json!({"command":"permission_prompt","text":own}))
        .await;
    let prompt = rig.peer.event("permission_prompt").await;
    let request = ControlSession {
        actor: rig.s.fixture.target.actor.user_id(),
        company: rig.s.scope.company.as_uuid(),
        request_id: OperationId::new(Uuid::now_v7()).unwrap(),
        session_id: initial.session,
        command: SessionCommand::Send {
            prompt: LocalPrompt::try_from(prompt["prompt"].as_str().unwrap().to_owned()).unwrap(),
        },
    };
    rig.s
        .fixture
        .persistence
        .control_session(&request)
        .await
        .unwrap();
    let waiting = waiting_input(rig, request.request_id).await;
    let own_turn = turn(&waiting);
    PermissionWait {
        request,
        own,
        own_turn,
    }
}

pub(super) async fn finish_permission(
    rig: &mut NativeRig,
    initial: &NativeInitial,
    pending: PermissionWait,
) -> Value {
    let PermissionWait {
        request,
        own,
        own_turn,
    } = pending;
    let human = format!("OPENBOT_HUMAN_{}", Uuid::new_v4().simple());
    rig.peer
        .command(json!({"command":"human","thread":initial.thread,"text":human}))
        .await;
    rig.peer.event("human").await;
    let still_waiting = rig
        .s
        .fixture
        .persistence
        .read_operation(request.actor, request.company, request.request_id)
        .await
        .unwrap();
    assert!(still_waiting.operation.outcome().is_none());
    assert_eq!(turn(&still_waiting), own_turn);
    assert_eq!(
        still_waiting.observed_turn_state,
        Some(TurnState::WaitingForInput)
    );
    rig.peer
        .command(json!({"command":"deny","thread":initial.thread}))
        .await;
    assert_eq!(rig.peer.event_timeout("deny", 90).await["actualUi"], true);
    let result = settled(rig, request.request_id).await;
    assert_eq!(turn(&result), own_turn);
    assert!(matches!(
        result.operation.outcome(),
        Some(OperationOutcome::Succeeded | OperationOutcome::Failed | OperationOutcome::Cancelled)
    ));
    if result.operation.outcome() == Some(OperationOutcome::Succeeded) {
        assert!(result.output.as_ref().unwrap().as_str().contains(&own));
    }
    assert!(
        !result
            .output
            .as_ref()
            .is_some_and(|output| output.as_str().contains(&human))
    );
    rig.peer
        .command(json!({"command":"human_terminal","thread":initial.thread,"text":human}))
        .await;
    let foreign = rig.peer.event_timeout("human_terminal", 150).await;
    assert_ne!(foreign["turn"].as_str().unwrap(), own_turn.as_str());
    rig.peer
        .command(json!({"command":"journal","operation":request.request_id}))
        .await;
    let journal = rig.peer.event("journal").await;
    assert_eq!(journal["sdkCalls"], 1);
    assert_eq!(journal["turn"].as_str().unwrap(), own_turn.as_str());
    json!({"status":"PASS","actualUiPermissionDenied":true,"openbotWaitingForMatchingInput":true,
        "ownTurn":own_turn,"independentHumanTurn":foreign["turn"],"outcome":result.operation.outcome(),"sdkCalls":1})
}

pub(super) async fn logged_out(rig: &mut NativeRig) -> Value {
    rig.peer.command(json!({"command":"logged_out"})).await;
    let isolation = rig.peer.event_timeout("logged_out", 150).await;
    assert_eq!(isolation["isolatedProviderConfiguration"], true);
    assert_eq!(isolation["parentLoginUntouched"], true);
    let original = rig.s.scope;
    let actor = rig.s.fixture.target.actor;
    let inputs = json!({"profile_id":{"literal":rig.s.profile.id},"prompt":{"literal":"Disposable logged-out acceptance: reply exactly OPENBOT_AUTH_FAILURE_PROBE. Do not use tools, read/change files, use network, or start subagents."}});
    rig.s.scope = rig
        .s
        .activate(actor, inputs, RelatedAssociation::Company)
        .await;
    let workers = Workers::start(&rig.s.fixture.persistence);
    let state = super::super::early::terminal(&rig.s).await;
    workers.close().await;
    assert_eq!(
        state, "failed",
        "an isolated logged-out provider cannot produce workflow success"
    );
    let operation: Option<Uuid> = sqlx::query_scalar(
        "SELECT operation_id FROM workflow_local_agent_calls WHERE company_id=$1 AND run_id=$2 AND operation_id IS NOT NULL",
    ).bind(rig.s.scope.company.as_uuid()).bind(rig.s.scope.run.as_uuid())
        .fetch_optional(rig.s.fixture.persistence.pool()).await.unwrap();
    let mut sdk_calls = 0;
    if let Some(operation) = operation {
        let id = OperationId::new(operation).unwrap();
        let view = rig
            .s
            .fixture
            .persistence
            .read_operation(actor.user_id(), rig.s.scope.company.as_uuid(), id)
            .await
            .unwrap();
        assert!(matches!(
            view.operation.outcome(),
            Some(OperationOutcome::Failed | OperationOutcome::Uncertain)
        ));
        rig.peer
            .command(json!({"command":"journal","operation":id}))
            .await;
        sdk_calls = rig.peer.event("journal").await["sdkCalls"]
            .as_u64()
            .unwrap();
        assert!(
            sdk_calls <= 1,
            "authentication failure never permits a blind mutation retry"
        );
    }
    let successes: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_action_receipts WHERE company_id=$1 AND run_id=$2 AND result->>'status'='succeeded'",
    ).bind(rig.s.scope.company.as_uuid()).bind(rig.s.scope.run.as_uuid())
        .fetch_one(rig.s.fixture.persistence.pool()).await.unwrap();
    assert_eq!(successes, 0);
    rig.s.scope = original;
    state_timeout(&rig.s, "succeeded", 10).await;
    json!({"status":"PASS","isolation":isolation,"workflow":"failed","sdkCalls":sdk_calls,
        "nativeOperation":operation,"successfulReceipts":successes,"originalWorkflowUnchanged":true})
}
