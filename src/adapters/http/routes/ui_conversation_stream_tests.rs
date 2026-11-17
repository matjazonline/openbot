use super::*;
use crate::application::thread_activity::{ThreadRunActivity, ThreadRuns};

#[tokio::test]
async fn both_runs_reconcile_on_connect_wakeup_and_reconnect() {
    let Some(h) = Harness::new().await else {
        return;
    };
    let thread = h.thread("Two runs").await;
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    h.activity.runs.lock().unwrap().insert(
        thread,
        ThreadRuns {
            inspectable: Default::default(),
            items: vec![
                ThreadRunActivity {
                    run_id: first,
                    title: "First".into(),
                    activity: ThreadActivity::Working,
                },
                ThreadRunActivity {
                    run_id: second,
                    title: "Second".into(),
                    activity: ThreadActivity::AwaitingReview,
                },
            ],
            has_more: false,
            message_runs: vec![crate::application::thread_activity::MessageRuns {
                canonical_id: Uuid::new_v4(),
                run_ids: vec![first, second],
                direction: crate::entities::message::MessageDirection::Inbound,
            }],
            response_outcomes: vec![],
            responses_truncated: false,
        },
    );
    // This wakeup predates subscription; the first read still recovers both identities.
    h.publish_activity_change(thread);
    let mut frames = h.message_stream(thread).await;
    let initial = frames.next_named("activity").await.data;
    for id in [first, second] {
        assert!(initial.contains(&id.to_string()), "{initial}");
    }
    assert!(initial.contains("hx-swap-oob=\"innerHTML\""));
    assert!(initial.contains("Started"));
    assert!(initial.contains("Agent working") && initial.contains("Awaiting review"));
    h.activity
        .runs
        .lock()
        .unwrap()
        .get_mut(&thread)
        .unwrap()
        .items
        .remove(0);
    h.publish_activity_change(thread);
    let refreshed = frames.next_named("activity").await.data;
    assert!(!refreshed.contains(&format!("data-run-id=\"{first}\"")));
    assert!(refreshed.contains(&second.to_string()));
    drop(frames);
    let mut connected = h.message_stream(thread).await;
    assert_eq!(connected.next_named("activity").await.data, refreshed);
}

#[tokio::test]
async fn conversation_reads_refuse_foreign_scope_and_private_channels() {
    let Some(h) = Harness::new().await else {
        return;
    };
    let Some(other) = Harness::new().await else {
        return;
    };
    let thread = h.thread("Private").await;
    crate::adapters::persistence::test_support::restrict_channel_to_configured_grants(
        h.persistence.pool(),
        h.channel_id,
    )
    .await;
    crate::adapters::persistence::test_support::insert_root_member(
        h.persistence.pool(),
        crate::domain::workflow::CompanyId::new(h.company_id),
        other.owner.user_id,
        crate::entities::company_member::CompanyAccessRole::Admin,
        crate::entities::access_level::AccessLevel::PUBLIC,
    )
    .await;
    for (viewer, company_id) in [
        (h.owner.clone(), other.company_id),
        (other.owner.clone(), h.company_id),
    ] {
        let result = thread_message_stream(
            State(h.channels.clone()),
            State(h.threads.clone()),
            State(h.reader()),
            State(h.events.clone()),
            viewer.clone(),
            HeaderMap::new(),
            Query(ThreadStreamQuery {
                company_id,
                inbox: crate::adapters::http::routes::ui::InboxSelection {
                    channel_id: Some(h.channel_id),
                    principal_id: None,
                },
                thread_id: thread,
                after: None,
            }),
        )
        .await;
        assert!(matches!(result, Err(AppError::NotFound(_))));
        let result = message_pane_fragment(
            State(h.channels.clone()),
            State(h.threads.clone()),
            State(h.reader()),
            viewer,
            Query(ThreadQuery {
                company_id,
                inbox: crate::adapters::http::routes::ui::InboxSelection {
                    channel_id: Some(h.channel_id),
                    principal_id: None,
                },
                thread_id: thread,
            }),
        )
        .await;
        assert!(matches!(result, Err(AppError::NotFound(_))));
    }
}
