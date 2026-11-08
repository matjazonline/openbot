use super::*;
#[tokio::test]
async fn vault_workflow_http_native_valid_oversized_projection_is_terminal_before_credentials() {
    let (mut setup, _vault, _account) = Box::pin(fixture("static-bearer")).await;
    setup.fixture.target.workflow = WorkflowId::new(Uuid::new_v4());
    let p = setup.fixture.persistence.clone();
    let directory = http::HttpPublicationDirectory {
        persistence: p.clone(),
        selections: vec![http::HttpPublicationSelection {
            connection: setup.connection.id,
            slot: Some(ResourceName::parse("service").unwrap()),
            operation: TypeName::parse("http.request").unwrap(),
        }],
    };
    let definitions =
        DefinitionService::new(Preflight, p.clone(), WorkflowSourceDecoder, directory);
    let mut source: Value = serde_json::from_str(
        &crate::application::workflow::registry::example("http.request")
            .unwrap()
            .source,
    )
    .unwrap();
    source["workflow_id"] = json!(setup.fixture.target.workflow);
    source["steps"]["start"]["with"]["body"] = json!({"literal":{"message":"x".repeat(17_000)}});
    setup.version = VersionId::new(Uuid::new_v4());
    setup
        .fixture
        .publish_source(
            &definitions,
            setup.fixture.target.workflow,
            source.to_string(),
            setup.version,
        )
        .await
        .unwrap();
    let target = Box::pin(bound(
        &setup.fixture,
        &setup.connection,
        setup.version,
        Entrance::Http,
        RelatedAssociation::Company,
    ))
    .await;
    setup.claim = Box::pin(
        crate::adapters::persistence::workflow::tests::http_tests::support::claimed_with_key(
            &setup.fixture,
            target,
            RelatedAssociation::Company,
            "http-projection",
        ),
    )
    .await;
    let runtime = HttpRuntime::with_integrations(
        p,
        HttpDestinationPolicy::default(),
        lease(),
        None,
        Arc::new(
            crate::adapters::persistence::workflow::tests::projection_test_support::NoExecution,
        ),
    );
    let failure = Box::pin(runtime.run(&setup.kind, &setup.claim))
        .await
        .completed_result()
        .unwrap_err();
    assert_eq!(
        failure.failure.class(),
        crate::domain::workflow::FailureClass::Terminal
    );
    assert_eq!(failure.failure.code().as_str(), "http.integration_invalid");
    assert_eq!(
        action_counts(&setup.fixture).await,
        ActionCounts {
            dispatches: 0,
            entries: 0,
            receipts: 0
        }
    );
}
