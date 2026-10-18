use super::super::approval_test_support::{command, notices};
use super::*;
use crate::adapters::workflow_http::NoLocal;
use crate::application::workflow::decisions::WorkflowDecisions;

async fn protected() -> McpSetup {
    let server = mcp_server(Call::Json).await;
    let mut fixture = Fixture::new().await;
    fixture.persistence = PostgresPersistence::with_credential_cipher(
        fixture.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    let (connection, prior) = Box::pin(configured(&fixture, &server.url)).await;
    let mut protected = operation(fixture.target.company);
    protected.approval_required = true;
    let operation = fixture
        .persistence
        .register_mcp_operation(
            fixture.target.company,
            fixture.target.actor,
            McpOperationWrite {
                connection: connection.id,
                catalog_revision: connection.revision,
                expected_version: Some(prior),
                operation: protected,
            },
        )
        .await
        .unwrap();
    let mut source = source();
    source["steps"]["lookup"]["with"]["approval"] = json!({"literal":{
        "reviewers":[{"kind":"user","id":fixture.target.actor.user_id()}],
        "deadline":(chrono::Utc::now()+chrono::Duration::minutes(20)).to_rfc3339()
    }});
    let version = Box::pin(published_source(&fixture, connection.id, source)).await;
    let target = Box::pin(bound(&fixture, connection.id, version)).await;
    let client = guarded_client(&server.url);
    McpSetup {
        fixture,
        server,
        connection,
        operation,
        version,
        target,
        client,
    }
}

#[tokio::test]
async fn workflow_action_approval_mcp_never_opens_before_approval_then_calls_once() {
    let setup = Box::pin(protected()).await;
    let p = &setup.fixture.persistence;
    let first = claimed(&setup).await;
    let dispatch =
        action_dispatch::PostgresActionDispatch::new(p.clone(), mcp::McpActionAuthority, NoLocal)
            .with_decision_notices(notices(p));
    let runtime = McpActions::new(p.clone(), dispatch, setup.client.clone(), lease());
    let outcome = Box::pin(runtime.run(&kind(), &first)).await;
    assert!(matches!(outcome, WorkflowHandlerOutcome::Suspended(_)));
    assert!(
        setup.server.log().is_empty(),
        "no initialize, list or call before approval"
    );
    p.submit_decision(command(p, first.fence.scope, setup.actor(), "approve").await)
        .await
        .unwrap();
    let next = claim(p, first.fence.scope).await;
    let result = Box::pin(runtime.run(&kind(), &next))
        .await
        .completed_result();
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(setup.server.calls(), 1);
    assert_eq!(action_counts(p).await, counts(1, 1, 1));
    let repeated = Box::pin(runtime.run(&kind(), &next))
        .await
        .completed_result();
    assert!(repeated.is_ok(), "{repeated:?}");
    assert_eq!(setup.server.calls(), 1);
}

#[tokio::test]
async fn workflow_action_approval_not_applied_proof_never_grants_second_entry() {
    let setup = Box::pin(protected()).await;
    setup.server.set_call(Call::Fail);
    let p = &setup.fixture.persistence;
    let first = claimed(&setup).await;
    let dispatch =
        action_dispatch::PostgresActionDispatch::new(p.clone(), mcp::McpActionAuthority, NoLocal)
            .with_decision_notices(notices(p));
    let runtime = McpActions::new(p.clone(), dispatch, setup.client.clone(), lease());
    assert!(matches!(
        Box::pin(runtime.run(&kind(), &first)).await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    p.submit_decision(command(p, first.fence.scope, setup.actor(), "approve").await)
        .await
        .unwrap();
    let second = claim(p, first.fence.scope).await;
    let failure = Box::pin(runtime.run(&kind(), &second))
        .await
        .completed_result()
        .unwrap_err();
    assert_eq!(setup.server.calls(), 1);
    Box::pin(super::reconciliation_tests::settle_rejected(
        &setup, &second, failure,
    ))
    .await;
    let third = reclaim(p, first.fence.scope).await;
    let request = p.mcp_step_request(&third, &kind()).await.unwrap();
    let intent = ActionService::new(p.clone())
        .prepare_step(request)
        .await
        .unwrap();
    let request = ActionDispatchRequest {
        subject: intent.approval_subject(),
        fence: third.fence,
        lease: lease(),
    };
    assert_raw_second_entry_refused(p, &request).await;
    let failure = Box::pin(runtime.run(&kind(), &third))
        .await
        .completed_result()
        .unwrap_err();
    assert_eq!(failure.failure.code().as_str(), "mcp.approval_required");
    assert_eq!(setup.server.calls(), 1);
    assert_eq!(action_counts(p).await, counts(1, 1, 0));
    let current = setup.current().await;
    p.register_mcp_operation(
        setup.company(),
        setup.actor(),
        McpOperationWrite {
            connection: current.id,
            catalog_revision: current.revision,
            expected_version: Some(setup.operation),
            operation: operation(setup.company()),
        },
    )
    .await
    .unwrap();
    assert!(
        Box::pin(runtime.run(&kind(), &third))
            .await
            .completed_result()
            .is_err()
    );
    assert_eq!(
        setup.server.calls(),
        1,
        "removing protection cannot reuse consumed approval"
    );
}

async fn assert_raw_second_entry_refused(p: &PostgresPersistence, request: &ActionDispatchRequest) {
    let marker: Uuid = sqlx::query_scalar("SELECT id FROM workflow_action_dispatches")
        .fetch_one(p.pool())
        .await
        .unwrap();
    let proof: Option<Uuid> =
        sqlx::query_scalar("SELECT workflow_action_not_applied_available($1,$2)")
            .bind(request.scope().company.as_uuid())
            .bind(request.subject.invocation.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert!(
        proof.is_some(),
        "the otherwise-valid proof must exercise the alternative replay path"
    );
    let mut tx = p.pool().begin().await.unwrap();
    assert!(action_dispatch::PostgresActionDispatch::<mcp::McpActionAuthority,NoLocal>::reserve_entry_on(&mut tx,request,marker,Uuid::new_v4()).await.unwrap());
    let error = tx.commit().await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("workflow action remote entry lacks supported replay"),
        "{error}"
    );
}
