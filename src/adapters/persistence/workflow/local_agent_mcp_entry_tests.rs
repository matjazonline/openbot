//! Live native action entry, including a real reservation and real approval.
use super::*;
use crate::adapters::persistence::workflow::tests::approval_test_support::{command, notices};
use crate::adapters::persistence::workflow::{action_dispatch, mcp};
use crate::adapters::workflow_http::NoLocal;
use crate::application::workflow::actions::*;
use crate::application::workflow::decisions::WorkflowDecisions;
#[path = "local_agent_mcp_source_integration_tests.rs"]
mod integration_tests;

type Dispatch = action_dispatch::PostgresActionDispatch<mcp::McpActionAuthority, NoLocal>;

struct NativeAction {
    source: Source,
    run: RunId,
    claim: ClaimedWorkflow,
    request: ActionRequest,
}

async fn action(source: Source) -> NativeAction {
    let s = &source.local;
    let p = &s.fixture.persistence;
    let response = p.native_mcp(&request(s, &source.native)).await.unwrap();
    let claim = Box::pin(broker_claim(s, response.run_id)).await;
    let request = p
        .mcp_step_request(&claim, &WorkflowStepKind("mcp.call".into()))
        .await
        .unwrap();
    NativeAction {
        source,
        run: response.run_id,
        claim,
        request,
    }
}

fn dispatch(action: &NativeAction) -> Dispatch {
    let p = &action.source.local.fixture.persistence;
    action_dispatch::PostgresActionDispatch::new(p.clone(), mcp::McpActionAuthority, NoLocal)
        .with_decision_notices(notices(p))
}

async fn approved(action: &mut NativeAction) -> ActionDispatchRequest {
    let s = &action.source.local;
    let p = &s.fixture.persistence;
    let intent = ActionService::new(p.clone())
        .prepare_step(action.request.clone())
        .await
        .unwrap();
    let request = ActionDispatchRequest {
        subject: intent.approval_subject(),
        fence: action.claim.fence,
        lease: lease(),
    };
    assert!(matches!(
        dispatch(action)
            .reserve_remote(&request, None)
            .await
            .unwrap(),
        RemoteReservationResult::ApprovalParked(_)
    ));
    p.submit_decision(
        command(
            p,
            action.claim.fence.scope,
            s.fixture.target.actor,
            "approve",
        )
        .await,
    )
    .await
    .unwrap();
    action.claim = Box::pin(broker_claim(s, action.run)).await;
    action.request = p
        .mcp_step_request(&action.claim, &WorkflowStepKind("mcp.call".into()))
        .await
        .unwrap();
    let saved = ActionService::new(p.clone())
        .prepare_step(action.request.clone())
        .await
        .unwrap();
    assert_eq!(saved.approval_subject(), intent.approval_subject());
    ActionDispatchRequest {
        subject: saved.approval_subject(),
        fence: action.claim.fence,
        lease: lease(),
    }
}

#[tokio::test]
async fn native_mcp_workflow_fresh_action_and_credentials_use_original_account_prefix() {
    for index in 0..3 {
        let a = Box::pin(action(Box::pin(source()).await)).await;
        let p = &a.source.local.fixture.persistence;
        let service = ActionService::new(p.clone());
        let before = effects(&a.source.local).await;
        let blocker = competitor(&a.source, owners(&a.source)[index]).await;
        busy(service.prepare_step(a.request.clone())).await;
        busy(p.mcp_use(&a.request)).await;
        assert_eq!(effects(&a.source.local).await, before);
        blocker.rollback().await.unwrap();
        let intent = service.prepare_step(a.request.clone()).await.unwrap();
        assert!(!intent.replayed);
        p.mcp_use(&a.request).await.unwrap();
        let blocker = competitor(&a.source, owners(&a.source)[index]).await;
        let replay = service.prepare_step(a.request.clone()).await.unwrap();
        assert!(
            replay.replayed,
            "structural replay needs no new live source authority"
        );
        assert_eq!(replay.approval_subject(), intent.approval_subject());
        blocker.rollback().await.unwrap();
    }
}

#[tokio::test]
async fn native_mcp_workflow_outer_preparation_never_waits_for_broker_run() {
    let a = Box::pin(action(Box::pin(source()).await)).await;
    let p = &a.source.local.fixture.persistence;
    let before = effects(&a.source.local).await;
    let competitor = competitor(&a.source, Owner::Run(a.run.as_uuid())).await;
    busy(p.mcp_step_request(&a.claim, &WorkflowStepKind("mcp.call".into()))).await;
    busy(ActionService::new(p.clone()).prepare_step(a.request.clone())).await;
    busy(p.mcp_use(&a.request)).await;
    assert_eq!(effects(&a.source.local).await, before);
    competitor.rollback().await.unwrap();
    p.mcp_step_request(&a.claim, &WorkflowStepKind("mcp.call".into()))
        .await
        .unwrap();
    ActionService::new(p.clone())
        .prepare_step(a.request.clone())
        .await
        .unwrap();
    p.mcp_use(&a.request).await.unwrap();
}

#[tokio::test]
async fn native_mcp_workflow_dispatch_rechecks_source_after_approval_and_reservation() {
    for index in 0..3 {
        let mut a = Box::pin(action(Box::pin(source()).await)).await;
        let request = Box::pin(approved(&mut a)).await;
        let writer = dispatch(&a);
        let before = effects(&a.source.local).await;
        let blocker = competitor(&a.source, owners(&a.source)[index]).await;
        busy(writer.reserve_remote(&request, None)).await;
        assert_eq!(effects(&a.source.local).await, before);
        blocker.rollback().await.unwrap();
        let RemoteReservationResult::Reserved(reservation) =
            writer.reserve_remote(&request, None).await.unwrap()
        else {
            panic!("a real approved native action must reserve");
        };
        let reserved = effects(&a.source.local).await;
        assert_eq!(reserved.dispatches, before.dispatches + 1);
        let blocker = competitor(&a.source, owners(&a.source)[index]).await;
        busy(writer.enter_remote(*reservation)).await;
        assert_eq!(
            effects(&a.source.local).await,
            reserved,
            "the existing reservation cannot exempt actual gateway entry from live authority"
        );
        blocker.rollback().await.unwrap();
        a.source
            .local
            .fixture
            .persistence
            .mcp_use(&a.request)
            .await
            .unwrap();
        assert_eq!(effects(&a.source.local).await, reserved);
        // A failed entry consumed its opaque permit. Reusing it would bypass the dispatch protocol.
    }
}

#[tokio::test]
async fn native_mcp_workflow_return_authority_rechecks_original_accounts() {
    let mut a = Box::pin(action(Box::pin(source()).await)).await;
    let request = Box::pin(approved(&mut a)).await;
    let writer = dispatch(&a);
    let RemoteReservationResult::Reserved(reservation) =
        writer.reserve_remote(&request, None).await.unwrap()
    else {
        panic!("a real approved native action must reserve");
    };
    let entry = writer.enter_remote(*reservation).await.unwrap();
    for owner in owners(&a.source) {
        let before = effects(&a.source.local).await;
        let competitor = competitor(&a.source, owner).await;
        busy(writer.return_authorized(&entry)).await;
        assert_eq!(effects(&a.source.local).await, before);
        competitor.rollback().await.unwrap();
        assert!(writer.return_authorized(&entry).await.unwrap());
    }
}
