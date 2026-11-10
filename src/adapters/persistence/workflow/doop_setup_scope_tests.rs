use super::*;

#[tokio::test]
async fn doop_connection_verification_cannot_bind_another_companys_same_slug_connection() {
    let setup = Box::pin(Verification::new()).await;
    let mut other = Fixture::in_database(setup.fixture._db.clone(), "-foreign-doop").await;
    other.persistence = PostgresPersistence::with_credential_cipher(
        other.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    let foreign = register(&other, &setup.server.url).await;
    assert_eq!(foreign.slug, setup.connection.slug);
    assert_eq!(foreign.endpoint, setup.connection.endpoint);
    let target = BindingTarget {
        binding: WorkflowBindingId::new(Uuid::new_v4()),
        ..setup.target
    };
    let configured = setup.engine.services().bindings().configure(ConfigureBinding {
        target, expected: None, association: RelatedAssociation::Company, version: setup.version,
        params: json!({"expected_bot_id":"user-bot-a","expected_workspace_id":"workspace-a","verification_canvas_id":"canvas-a"}),
        resources: BTreeMap::from([(ResourceName::parse("service").unwrap(), RuntimeResourceId::new(foreign.id))]),
        run_as: None, message_trigger: None,
    }).await;
    assert!(
        configured.is_err(),
        "foreign resource must not become dispatchable"
    );
    assert_eq!(setup.server.calls(), 0);
    setup.client.shutdown().await.unwrap();
}
