use super::*;
#[tokio::test]
async fn vault_workflow_mcp_native_valid_oversized_projection_is_terminal_before_credentials() {
    let mut f = Box::pin(fixture(Delivery::Accepted)).await;
    let p = f.setup.fixture.persistence.clone();
    let current = f.setup.current().await;
    let mut write = crate::application::use_cases::mcp::McpConnectionWrite {
        slug: current.slug,
        endpoint: current.endpoint,
        enabled: current.enabled,
        auth: current.auth,
        discovered_tools: current.discovered_tools,
        tool_grants: current.tool_grants,
    };
    write.discovered_tools[0].input_schema["properties"]["customer_id"]["maxLength"] =
        json!(20_000);
    let current = p
        .update_mcp_connection(
            f.setup.company().as_uuid(),
            current.id,
            current.revision,
            write,
        )
        .await
        .unwrap();
    let mut operation = operation(f.setup.company());
    operation.tool.contract.input_schema["properties"]["customer_id"]["maxLength"] = json!(20_000);
    f.setup.operation = p
        .register_mcp_operation(
            f.setup.company(),
            f.setup.actor(),
            McpOperationWrite {
                connection: current.id,
                catalog_revision: current.revision,
                expected_version: Some(f.setup.operation),
                operation,
            },
        )
        .await
        .unwrap();
    f.setup.fixture.target.workflow = WorkflowId::new(Uuid::new_v4());
    let mut definition = source();
    definition["workflow_id"] = json!(f.setup.fixture.target.workflow);
    definition["steps"]["lookup"]["with"]["arguments"] =
        json!({"literal":{"customer_id":"x".repeat(17_000)}});
    f.setup.version = Box::pin(publish_unique(&f.setup.fixture, current.id, definition)).await;
    f.setup.target = Box::pin(bound(&f.setup.fixture, current.id, f.setup.version)).await;
    let scope = Box::pin(admitted(&f.setup, "mcp-projection")).await;
    let claim = Box::pin(claim(&p, scope)).await;
    let runtime = crate::adapters::workflow_mcp::McpRuntime::with_integrations(
        p.clone(),
        f.client.clone(),
        lease(),
        None,
        Arc::new(
            crate::adapters::persistence::workflow::tests::projection_test_support::NoExecution,
        ),
    );
    let failure = Box::pin(runtime.run(&kind(), &claim))
        .await
        .completed_result()
        .unwrap_err();
    assert_eq!(
        failure.failure.class(),
        crate::domain::workflow::FailureClass::Terminal
    );
    assert_eq!(failure.failure.code().as_str(), "mcp.integration_invalid");
    assert_eq!(f.client.counts(), (0, 0, 0));
    assert_eq!(action_counts(&p).await, counts(0, 0, 0));
    f.client.shutdown().await.unwrap();
}
