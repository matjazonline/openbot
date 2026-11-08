use super::*;

pub(super) async fn publish(fixture: &Fixture, source: Value) -> VersionId {
    let version = VersionId::new(Uuid::new_v4());
    let service = DefinitionService::new(
        Preflight,
        fixture.persistence.clone(),
        WorkflowSourceDecoder,
        step_builtins::StepBuiltinPublicationDirectory {
            persistence: fixture.persistence.clone(),
        },
    );
    fixture
        .publish_source(
            &service,
            fixture.target.workflow,
            source.to_string(),
            version,
        )
        .await
        .unwrap();
    version
}
