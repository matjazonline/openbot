use super::*;
use crate::{application::integrations::ExecutionContext, domain::integrations as integration};
use chrono::{Duration as ChronoDuration, Utc};
use tokio_util::sync::CancellationToken;
#[tokio::test]
async fn vault_dispatch_authority_approval_is_required_and_cannot_authorize_a_rebound_account() {
    let setup = Box::pin(protected("https://127.0.0.1:9", Entrance::Http)).await;
    let (vault, account) = super::super::vault_tests::account(
        &setup,
        setup.fixture.target.actor.user_id(),
        "protected-vault",
        "secret",
    )
    .await;
    super::super::vault_tests::attach(&setup, &vault, &account).await;
    let p = &setup.fixture.persistence;
    let request = p
        .http_step_request(&setup.claim, &setup.kind)
        .await
        .unwrap();
    let intent = ActionService::new(p.clone())
        .prepare_step(request)
        .await
        .unwrap();
    let dispatch = ActionDispatchRequest {
        subject: intent.approval_subject(),
        fence: setup.claim.fence,
        lease: lease(),
    };
    assert!(
        p.prepare_integration_invocation(
            dispatch,
            integration::DispatchId::new(Uuid::new_v4()),
            Utc::now() + ChronoDuration::seconds(20),
            CancellationToken::new()
        )
        .await
        .is_err()
    );
    let wait = parked(&setup).await;
    let decision = view(&setup).await;
    p.submit_decision(submission(&setup, &decision, "approve"))
        .await
        .unwrap();
    assert!(p.confirm_suspension(setup.claim.fence, wait).await.unwrap());
    let next = p
        .claim_io(
            setup.claim.fence.scope,
            WorkflowWorkerId(Uuid::new_v4()),
            lease(),
        )
        .await
        .unwrap()
        .unwrap();
    let request = p.http_step_request(&next, &setup.kind).await.unwrap();
    let intent = ActionService::new(p.clone())
        .prepare_step(request)
        .await
        .unwrap();
    let dispatch = ActionDispatchRequest {
        subject: intent.approval_subject(),
        fence: next.fence,
        lease: lease(),
    };
    let (invocation, context): (_, ExecutionContext) = p
        .prepare_integration_invocation(
            dispatch,
            integration::DispatchId::new(Uuid::new_v4()),
            Utc::now() + ChronoDuration::seconds(20),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    vault
        .final_dispatch_guard(&invocation, &context)
        .await
        .unwrap();
    super::super::vault_tests::attach(&setup, &vault, &account).await;
    assert!(
        vault
            .final_dispatch_guard(&invocation, &context)
            .await
            .is_err()
    );
    assert_eq!(
        action_counts(&setup.fixture).await,
        ActionCounts {
            dispatches: 0,
            entries: 0,
            receipts: 0
        }
    );
}
