use super::*;

#[tokio::test]
async fn related_ids_and_thread_channel_must_match_loaded_records() {
    let company_id = company();
    let version_id = version();
    let store = MemoryStore::new(owned(company_id, version_id, 128));
    let channel = channel(company_id, ChannelAccessMode::Team);
    reject_mismatched_channel_records(&store, company_id, version_id, &channel).await;
    reject_mismatched_thread_records(&store, company_id, version_id, &channel).await;
}

async fn reject_mismatched_channel_records(
    store: &MemoryStore,
    company_id: CompanyId,
    version_id: VersionId,
    channel: &Channel,
) {
    let mut wrong = channel.clone();
    wrong.company_id = company().as_uuid();
    store
        .state
        .lock()
        .unwrap()
        .channels
        .insert(channel.id, wrong);
    assert!(matches!(
        service(store)
            .admit(admitted_request(
                company_id,
                version_id,
                "foreign",
                related(channel)
            ))
            .await,
        Err(AppError::NotFound(_))
    ));
    let mut wrong = channel.clone();
    wrong.id = Uuid::new_v4();
    store
        .state
        .lock()
        .unwrap()
        .channels
        .insert(channel.id, wrong);
    assert!(matches!(
        service(store)
            .admit(admitted_request(
                company_id,
                version_id,
                "wrong-id",
                related(channel)
            ))
            .await,
        Err(AppError::NotFound(_))
    ));
    store
        .state
        .lock()
        .unwrap()
        .channels
        .insert(channel.id, channel.clone());
}

async fn reject_mismatched_thread_records(
    store: &MemoryStore,
    company_id: CompanyId,
    version_id: VersionId,
    channel: &Channel,
) {
    let thread = thread(channel.id);
    let association = RelatedAssociation::Thread {
        channel_id: RelatedChannelId::new(channel.id),
        thread_id: RelatedThreadId::new(thread.id),
    };
    assert!(matches!(
        service(store)
            .admit(admitted_request(
                company_id,
                version_id,
                "missing-thread",
                association
            ))
            .await,
        Err(AppError::NotFound(_))
    ));
    let mut wrong = thread.clone();
    wrong.id = Uuid::new_v4();
    store.state.lock().unwrap().threads.insert(thread.id, wrong);
    assert!(matches!(
        service(store)
            .admit(admitted_request(
                company_id,
                version_id,
                "wrong-thread-id",
                association
            ))
            .await,
        Err(AppError::NotFound(_))
    ));
    let mut wrong = thread.clone();
    wrong.channel_id = Uuid::new_v4();
    store.state.lock().unwrap().threads.insert(thread.id, wrong);
    assert!(matches!(
        service(store)
            .admit(admitted_request(
                company_id,
                version_id,
                "wrong-channel",
                association
            ))
            .await,
        Err(AppError::NotFound(_))
    ));
    store
        .state
        .lock()
        .unwrap()
        .threads
        .insert(thread.id, thread);
    assert!(matches!(
        service(store)
            .admit(admitted_request(
                company_id,
                version_id,
                "valid-thread",
                association
            ))
            .await
            .unwrap(),
        AdmissionResult::Created(_)
    ));
}
