use tokio_stream::{
    Stream, StreamExt,
    wrappers::{BroadcastStream, errors::BroadcastStreamRecvError},
};
use tracing::debug;
use uuid::Uuid;

use crate::infra::events::{MailboxEvent, MailboxEvents};

/// Why a live UI stream woke up.
pub(super) enum Wake {
    Event(MailboxEvent),
    Lagged,
}

/// Wake-ups filtered after authorization, with lag converted into a database reconciliation.
fn wake_ups<Matches>(
    events: &MailboxEvents,
    label: &'static str,
    matches: Matches,
) -> impl Stream<Item = Wake> + Send + use<Matches>
where
    Matches: Fn(&MailboxEvent) -> bool + Send + 'static,
{
    BroadcastStream::new(events.subscribe()).filter_map(move |event| match event {
        // A listener reconnect may have lost notifications: reconcile in full, as on lag.
        Ok(event) if event.is_resync() => {
            debug!(
                stream = label,
                "Live stream resync after a listener reconnect"
            );
            Some(Wake::Lagged)
        }
        Ok(event) => matches(&event).then_some(Wake::Event(event)),
        Err(BroadcastStreamRecvError::Lagged(missed)) => {
            debug!(missed, stream = label, "Live stream lagged, catching up");
            Some(Wake::Lagged)
        }
    })
}

pub(super) fn thread_wake_ups(
    events: &MailboxEvents,
    label: &'static str,
    thread_id: Uuid,
) -> impl Stream<Item = Wake> + Send + use<> {
    wake_ups(events, label, move |event| {
        event.is_message_in_thread(thread_id) || event.is_activity_in_thread(thread_id)
    })
}

/// The simulation's wake-ups (p08 D4.6): the thread's messages and activity, and any run of the
/// thread changing state. A resync arrives as `Lagged`, which re-renders the view like any wake.
pub(super) fn simulation_wake_ups(
    events: &MailboxEvents,
    label: &'static str,
    thread_id: Uuid,
) -> impl Stream<Item = Wake> + Send + use<> {
    wake_ups(events, label, move |event| {
        event.is_message_in_thread(thread_id)
            || event.is_activity_in_thread(thread_id)
            || matches!(event, MailboxEvent::WorkflowRunChanged(scope) if scope.thread_id == Some(thread_id))
    })
}

/// The mailbox column's wake-ups: a message or a run's activity on any of the channel's threads.
pub(super) fn channel_wake_ups(
    events: &MailboxEvents,
    label: &'static str,
    channel_id: Uuid,
) -> impl Stream<Item = Wake> + Send + use<> {
    wake_ups(events, label, move |event| {
        event.is_message_in_channel(channel_id) || event.is_activity_in_channel(channel_id)
    })
}

/// The run counts' wake-ups: any run of the company changing state. The counts are re-read for
/// the viewer's channels, so a change elsewhere costs one unchanged, suppressed pass.
pub(super) fn run_count_wake_ups(
    events: &MailboxEvents,
    label: &'static str,
    company_id: Uuid,
) -> impl Stream<Item = Wake> + Send + use<> {
    wake_ups(events, label, move |event| {
        event.run_scope(company_id).is_some()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::events::{AttentionScope, AttentionWakeSource, ThreadScope};

    /// p08 D4.4: the counts wake for a run of their company, and for a resync, and for nothing
    /// else.
    #[tokio::test]
    async fn counts_wake_for_a_run_of_their_company_and_a_resync() {
        let events = MailboxEvents::new();
        let company_id = Uuid::new_v4();
        let mut wakes = Box::pin(run_count_wake_ups(&events, "run-counts", company_id));
        let run = |company_id| {
            MailboxEvent::WorkflowRunChanged(crate::infra::events::RunScope {
                company_id,
                channel_id: None,
                thread_id: None,
                run_id: Uuid::new_v4(),
            })
        };
        events.publish(run(Uuid::new_v4()));
        events.publish(MailboxEvent::AttentionChanged(AttentionScope {
            company_id,
            channel_id: Uuid::new_v4(),
            source_kind: AttentionWakeSource::Delivery,
            source_id: Uuid::new_v4(),
        }));
        events.publish(run(company_id));
        events.publish(MailboxEvent::Resync);
        assert!(matches!(
            wakes.next().await,
            Some(Wake::Event(MailboxEvent::WorkflowRunChanged(scope))) if scope.company_id == company_id
        ));
        assert!(matches!(wakes.next().await, Some(Wake::Lagged)));
    }

    /// p08 D4.6: the simulation wakes for a run of its own thread, and for a resync; another
    /// thread's run, or a run with no thread, is not its news.
    #[tokio::test]
    async fn the_simulation_wakes_for_its_threads_runs_and_a_resync() {
        let events = MailboxEvents::new();
        let company_id = Uuid::new_v4();
        let thread_id = Uuid::new_v4();
        let mut wakes = Box::pin(simulation_wake_ups(&events, "simulation", thread_id));
        let run = |thread_id| {
            MailboxEvent::WorkflowRunChanged(crate::infra::events::RunScope {
                company_id,
                channel_id: Some(Uuid::new_v4()),
                thread_id,
                run_id: Uuid::new_v4(),
            })
        };
        events.publish(run(Some(Uuid::new_v4())));
        events.publish(run(None));
        events.publish(run(Some(thread_id)));
        events.publish(MailboxEvent::Resync);
        assert!(matches!(
            wakes.next().await,
            Some(Wake::Event(MailboxEvent::WorkflowRunChanged(scope))) if scope.thread_id == Some(thread_id)
        ));
        assert!(matches!(wakes.next().await, Some(Wake::Lagged)));
    }

    /// The mailbox streams wake for their own thread's and channel's messages and run activity;
    /// an attention change -- a decision opening, say -- is the attention page's news, not theirs.
    /// The message published last is what makes "did not wake" an assertion rather than a timeout.
    #[tokio::test]
    async fn the_mailbox_streams_wake_for_messages_and_activity_and_not_for_attention() {
        let events = MailboxEvents::new();
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let thread_id = Uuid::new_v4();
        let scope = ThreadScope {
            thread_id,
            channel_id,
            company_id,
        };

        let mut thread = Box::pin(thread_wake_ups(&events, "test", thread_id));
        let mut column = Box::pin(channel_wake_ups(&events, "test", channel_id));

        events.publish(MailboxEvent::AttentionChanged(AttentionScope {
            company_id,
            channel_id,
            source_kind: AttentionWakeSource::Delivery,
            source_id: Uuid::new_v4(),
        }));
        events.publish(MailboxEvent::ActivityChanged(ThreadScope {
            thread_id: Uuid::new_v4(),
            channel_id: Uuid::new_v4(),
            company_id,
        }));
        events.publish(MailboxEvent::ActivityChanged(scope));
        events.publish(MailboxEvent::MessageCommitted(scope));

        for woken in [thread.next().await, column.next().await] {
            assert!(
                matches!(woken, Some(Wake::Event(MailboxEvent::ActivityChanged(woken))) if woken == scope),
                "the first wake-up is this thread's run activity"
            );
        }
        for woken in [thread.next().await, column.next().await] {
            assert!(matches!(
                woken,
                Some(Wake::Event(MailboxEvent::MessageCommitted(_)))
            ));
        }
    }
}
