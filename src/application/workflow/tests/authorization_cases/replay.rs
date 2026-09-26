use super::*;

#[tokio::test]
async fn replay_reauthorizes_and_association_is_part_of_dedup() {
    let company_id = company();
    let version_id = version();
    let store = MemoryStore::new(owned(company_id, version_id, 128));
    let channel = channel(company_id, ChannelAccessMode::Team);
    store
        .state
        .lock()
        .unwrap()
        .channels
        .insert(channel.id, channel.clone());
    let request = || admitted_request(company_id, version_id, "same", related(&channel));
    let AdmissionResult::Created(run_id) = service(&store).admit(request()).await.unwrap() else {
        panic!("created")
    };
    assert_eq!(
        service(&store).admit(request()).await.unwrap(),
        AdmissionResult::Replayed(run_id)
    );
    let other_actor = WorkflowActor::authenticated(Uuid::new_v4()).unwrap();
    store.state.lock().unwrap().access.insert(
        (company_id, other_actor.user_id()),
        PrincipalAccessContext {
            principal_id: None,
            membership: CompanyMembership::Owner,
        },
    );
    let mut same_operation = request();
    same_operation.actor = other_actor;
    assert_eq!(
        service(&store).admit(same_operation).await.unwrap(),
        AdmissionResult::Replayed(run_id)
    );
    assert_eq!(
        service(&store)
            .admit(admitted_request(
                company_id,
                version_id,
                "same",
                RelatedAssociation::Company
            ))
            .await
            .unwrap(),
        AdmissionResult::Conflict
    );
    set_membership(&store, company_id, CompanyMembership::Member, None);
    assert!(matches!(
        service(&store).admit(request()).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(store.state.lock().unwrap().runs.len(), 1);
}
