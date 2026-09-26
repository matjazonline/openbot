use super::*;

#[tokio::test]
async fn unresolved_resources_reject_before_run_write() {
    let company_id = company();
    let version_id = version();
    let store = MemoryStore::new(owned(company_id, version_id, 128));
    let mut changed = owned(company_id, version_id, 128);
    let mut definition = changed.definition.definition().clone();
    definition.resources.push(ResourceRequirement {
        slot: ResourceName::parse("calendar").unwrap(),
        kind: TypeName::parse("mcp.connection").unwrap(),
        contract: None,
    });
    changed.workflow_id = definition.workflow_id;
    changed.definition = validate(definition).unwrap();
    store
        .state
        .lock()
        .unwrap()
        .versions
        .insert((company_id, version_id), changed);
    assert!(matches!(
        service(&store)
            .admit(admitted_request(
                company_id,
                version_id,
                "resources",
                RelatedAssociation::Company
            ))
            .await,
        Err(AppError::BadRequest(_))
    ));
    assert!(store.state.lock().unwrap().runs.is_empty());
}
