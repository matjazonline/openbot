use super::*;
use crate::adapters::persistence::workflow::knowledge;
use crate::entities::access_level::AccessLevel;
#[tokio::test]
async fn knowledge_run_level_mcp_step_and_tool_refuse_without_provider_entry() {
    let setup = Box::pin(mcp_setup(Call::Json)).await;
    let p = &setup.fixture.persistence;
    let claim = claimed(&setup).await;
    let scope = claim.fence.scope;
    let before = setup.server.log();
    let mut tx = p.pool().begin().await.unwrap();
    knowledge::raise_content_level_on(&mut tx, scope.company, scope.run, AccessLevel::L5)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let runtime = setup.runtime();
    let failure = runtime
        .run(&kind(), &claim)
        .await
        .completed_result()
        .unwrap_err();
    assert_eq!(
        code(&failure),
        crate::app_error::KNOWLEDGE_AUDIENCE_BELOW_CONTENT
    );
    assert!(matches!(
        runtime
            .call_tool(
                claim.fence,
                tool_call("private-tool"),
                &CancellationToken::new()
            )
            .await,
        Err(AppError::KnowledgeRelease { .. })
    ));
    assert_eq!(setup.server.log(), before);
    assert_eq!(setup.server.calls(), 0);
    assert_eq!(action_counts(p).await, counts(0, 0, 0));
}

#[tokio::test]
async fn knowledge_run_level_mcp_level_change_preserves_operation_and_credentials() {
    let setup = Box::pin(mcp_setup(Call::Json)).await;
    let p = &setup.fixture.persistence;
    let claim = claimed(&setup).await;
    let scope = claim.fence.scope;
    let current = setup.current().await;
    p.set_mcp_level(crate::use_cases::connection_level::ConnectionLevelCommand {
        company: scope.company,
        actor: setup.actor(),
        connection: current.id,
        expected_revision: current.revision,
        access_level: AccessLevel::L5,
    })
    .await
    .unwrap();
    let mut tx = p.pool().begin().await.unwrap();
    knowledge::raise_content_level_on(&mut tx, scope.company, scope.run, AccessLevel::L5)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(
        setup
            .runtime()
            .run(&kind(), &claim)
            .await
            .completed_result()
            .is_ok()
    );
    assert_eq!(setup.server.calls(), 1);
    assert_eq!(action_counts(p).await, counts(1, 1, 1));
}
