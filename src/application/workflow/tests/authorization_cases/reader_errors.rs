use super::*;

#[tokio::test]
async fn admission_reader_failures_propagate_without_writes() {
    let company_id = company();
    let version_id = version();
    let store = MemoryStore::new(owned(company_id, version_id, 128));
    let channel = channel(company_id, ChannelAccessMode::Team);
    let thread = thread(channel.id);
    {
        let mut state = store.state.lock().unwrap();
        state.channels.insert(channel.id, channel.clone());
        state.threads.insert(thread.id, thread.clone());
    }
    let association = RelatedAssociation::Thread {
        channel_id: RelatedChannelId::new(channel.id),
        thread_id: RelatedThreadId::new(thread.id),
    };
    for kind in 0..3 {
        {
            let mut state = store.state.lock().unwrap();
            state.fail_access = kind == 0;
            state.fail_channel = kind == 1;
            state.fail_thread = kind == 2;
        }
        assert!(matches!(
            service(&store)
                .admit(admitted_request(
                    company_id,
                    version_id,
                    &format!("error-{kind}"),
                    association
                ))
                .await,
            Err(AppError::Database(_))
        ));
        let state = store.state.lock().unwrap();
        assert!(state.runs.is_empty());
        assert!(state.jobs.is_empty());
        assert!(state.admissions.is_empty());
    }
}
