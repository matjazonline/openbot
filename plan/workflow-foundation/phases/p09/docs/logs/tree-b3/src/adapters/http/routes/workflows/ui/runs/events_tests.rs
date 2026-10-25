//! The workspace's event stream: what wakes it, for whom, and the periodic access re-check.
use super::*;

/// The stream's names: `resync` first (a change before it subscribed is covered), `runs` and
/// `run-<id>` for a run the viewer can see, nothing for another company's or a hidden channel's
/// run, and `resync` on the listener's reconnect.
#[tokio::test]
async fn the_event_stream_wakes_with_resync_first_and_names_only_visible_runs() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let events = MailboxEvents::new();
    let receiver = events.subscribe();
    // A change published after subscribing but before the first read is still delivered.
    let visible = Uuid::new_v4();
    let board = f.channel("Board", "board", true).await;
    events.publish(MailboxEvent::WorkflowRunChanged(RunScope {
        company_id: f.other,
        channel_id: None,
        thread_id: None,
        run_id: Uuid::new_v4(),
    }));
    events.publish(MailboxEvent::WorkflowRunChanged(RunScope {
        company_id: f.company,
        channel_id: Some(board),
        thread_id: None,
        run_id: Uuid::new_v4(),
    }));
    events.publish(MailboxEvent::WorkflowRunChanged(RunScope {
        company_id: f.company,
        channel_id: None,
        thread_id: None,
        run_id: visible,
    }));
    events.publish(MailboxEvent::Resync);
    let stream = events::run_event_stream(
        receiver,
        f.ui(f.admin),
        f.company,
        Duration::from_secs(3600),
    );
    tokio::pin!(stream);
    let mut names = Vec::new();
    while names.len() < 4 {
        let event = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("an event")
            .expect("open")
            .unwrap();
        // Axum's Event renders as SSE text; the name is its `event:` line.
        let text = format!("{event:?}");
        names.push(text);
    }
    let rendered = names.join("\n");
    assert!(names[0].contains("resync"), "{rendered}");
    assert!(
        names[1].contains("runs") && names[2].contains(&format!("run-{visible}")),
        "{rendered}"
    );
    assert!(names[3].contains("resync"), "{rendered}");
    assert!(!rendered.contains(&board.to_string()));
}

/// The periodic re-check: while the viewer may still manage the company it sends `tick` (which
/// only an unfinished run's pane reloads on), and once they may not, the stream ends.
#[tokio::test]
async fn the_event_stream_ticks_while_access_holds_and_ends_when_it_is_lost() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let events = MailboxEvents::new();
    let stream = events::run_event_stream(
        events.subscribe(),
        f.ui(f.admin),
        f.company,
        Duration::from_millis(50),
    );
    tokio::pin!(stream);
    let next = next_event;
    assert!(next(&mut stream).await.unwrap().contains("resync"));
    let tick = next(&mut stream).await.unwrap();
    assert!(tick.contains("tick") && !tick.contains("resync"), "{tick}");
    sqlx::query("DELETE FROM company_members WHERE company_id = $1 AND user_id = $2")
        .bind(f.company)
        .bind(f.admin)
        .execute(f.persistence.pool())
        .await
        .unwrap();
    assert_eq!(
        next(&mut stream).await,
        None,
        "the stream ends once access is gone"
    );
}

/// A listener that falls behind the broadcast cannot know what it missed: the stream says
/// `resync`, so every view reads again in full, and goes on with the events after the gap.
#[tokio::test]
async fn a_lagging_stream_asks_every_view_to_read_again() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let (sender, receiver) = tokio::sync::broadcast::channel(1);
    let stream = events::run_event_stream(
        receiver,
        f.ui(f.owner),
        f.company,
        Duration::from_secs(3600),
    );
    tokio::pin!(stream);
    let next = next_event;
    assert!(next(&mut stream).await.unwrap().contains("resync"));
    // Three changes into a channel that holds one: the receiver lags past two of them.
    let runs: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
    for run in &runs {
        sender
            .send(MailboxEvent::WorkflowRunChanged(RunScope {
                company_id: f.company,
                channel_id: None,
                thread_id: None,
                run_id: *run,
            }))
            .unwrap();
    }
    let lagged = next(&mut stream).await.unwrap();
    assert!(lagged.contains("resync"), "{lagged}");
    assert!(next(&mut stream).await.unwrap().contains("runs"));
    let newest = next(&mut stream).await.unwrap();
    assert!(newest.contains(&format!("run-{}", runs[2])), "{newest}");
}

/// The stream's next event as SSE text, or `None` once it has ended.
async fn next_event(
    stream: &mut (
             impl tokio_stream::Stream<
        Item = Result<axum::response::sse::Event, std::convert::Infallible>,
    > + Unpin
         ),
) -> Option<String> {
    tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("the stream answers")
        .map(|event| format!("{:?}", event.unwrap()))
}
