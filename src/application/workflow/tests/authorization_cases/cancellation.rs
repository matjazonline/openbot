use super::*;

#[tokio::test]
async fn cancel_uses_stored_association_and_reader_errors_propagate() {
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
    let AdmissionResult::Created(run_id) = service(&store)
        .admit(admitted_request(
            company_id,
            version_id,
            "associated",
            related(&channel),
        ))
        .await
        .unwrap()
    else {
        panic!("created")
    };
    store.state.lock().unwrap().channels.remove(&channel.id);
    assert!(matches!(
        service(&store)
            .cancel(CancelWorkflowRequest {
                expected_revision: RunRevision(1),
                command_key: IdempotencyKey::parse("cancel").unwrap(),
                company_id,
                actor: actor(),
                run_id
            })
            .await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(
        store.head(company_id, run_id).await.unwrap().unwrap().state,
        RunState::Queued
    );
    store
        .state
        .lock()
        .unwrap()
        .channels
        .insert(channel.id, channel);
    cancellation_reader_failures(&store, company_id, run_id).await;
}

async fn cancellation_reader_failures(store: &MemoryStore, company_id: CompanyId, run_id: RunId) {
    for kind in 0..3 {
        {
            let mut state = store.state.lock().unwrap();
            state.fail_access = kind == 0;
            state.fail_channel = kind == 1;
            state.fail_thread = kind == 2;
        }
        let result = if kind == 2 {
            let thread = thread(
                store
                    .state
                    .lock()
                    .unwrap()
                    .channels
                    .values()
                    .next()
                    .unwrap()
                    .id,
            );
            let association = RelatedAssociation::Thread {
                channel_id: RelatedChannelId::new(thread.channel_id),
                thread_id: RelatedThreadId::new(thread.id),
            };
            store
                .state
                .lock()
                .unwrap()
                .runs
                .get_mut(&(company_id, run_id))
                .unwrap()
                .association = association;
            service(store)
                .cancel(CancelWorkflowRequest {
                    expected_revision: RunRevision(1),
                    command_key: IdempotencyKey::parse("cancel").unwrap(),
                    company_id,
                    actor: actor(),
                    run_id,
                })
                .await
        } else {
            service(store)
                .cancel(CancelWorkflowRequest {
                    expected_revision: RunRevision(1),
                    command_key: IdempotencyKey::parse("cancel").unwrap(),
                    company_id,
                    actor: actor(),
                    run_id,
                })
                .await
        };
        assert!(matches!(result, Err(AppError::Database(_))));
    }
    assert_eq!(
        store.head(company_id, run_id).await.unwrap().unwrap().state,
        RunState::Queued
    );
}
