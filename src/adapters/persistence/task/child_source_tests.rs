use super::*;

pub(super) async fn create_sibling_source_task(
    persistence: &PostgresPersistence,
    fixture: &DelegationFixture,
    source_message_id: crate::entities::message::CanonicalMessageId,
) -> BackgroundTask {
    let channel = ChannelPersistence::create(
        persistence,
        fixture.company.id,
        ChannelWrite {
            name: "Sibling pipeline".into(),
            slug: "sibling-pipeline".into(),
            enabled: false,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    persistence
        .enqueue_task(NewTask {
            targets: Vec::new(),
            source: TaskSource::Message(source_message_id),
            company_id: fixture.company.id,
            channel_id: channel.id,
            thread_id: None,
            task_type: "sibling-pipeline".into(),
            payload: serde_json::json!({}),
            correlation_id: fixture.task.correlation_id,
        })
        .await
        .unwrap()
}

pub(super) async fn assert_internal_child_projection(
    persistence: &PostgresPersistence,
    fixture: &DelegationFixture,
    child_id: Uuid,
    sibling: &BackgroundTask,
) {
    let summary = persistence
        .get_collaboration_summary(
            CollaborationReadScope {
                company_id: fixture.company.id,
                visible_channel_ids: &[fixture.task.channel_id, sibling.channel_id],
            },
            fixture.task.id,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(summary.children.len(), 1);
    assert_eq!(
        summary.children[0].child.as_ref().unwrap().task_id,
        child_id,
        "both collaboration queries follow the internal target's channel to its child"
    );
}
