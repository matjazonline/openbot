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

pub(super) fn channel_wake_ups(
    events: &MailboxEvents,
    label: &'static str,
    channel_id: Uuid,
) -> impl Stream<Item = Wake> + Send + use<> {
    wake_ups(events, label, move |event| {
        event.is_message_in_channel(channel_id) || event.is_activity_in_channel(channel_id)
    })
}
