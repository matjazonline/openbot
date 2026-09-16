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

/// The open thread's wake-ups: its messages, its activity, and its channel's held replies.
///
/// The handoff term is **channel**-scoped even here, and deliberately so. [`AttentionScope`]'s
/// `source_id` is the handoff's own id, not the thread's, so this stream cannot tell whether a
/// change was on its own thread without querying -- and narrowing the payload would mean editing
/// `notify_attention_changed`, a trigger shared by six other sources, for a latency optimisation.
/// The cost is one handoff re-read per handoff change in the channel while a thread is open, and
/// its result is an identical `innerHTML` swap when nothing relevant moved. That is the
/// "SSE is a wake-up only, readers re-query" contract; do not "fix" it with a thread-scoped
/// notification channel.
///
/// [`AttentionScope`]: crate::infra::events::AttentionScope
pub(super) fn mailbox_thread_wake_ups(
    events: &MailboxEvents,
    label: &'static str,
    thread_id: Uuid,
    channel_id: Uuid,
) -> impl Stream<Item = Wake> + Send + use<> {
    wake_ups(events, label, move |event| {
        event.is_message_in_thread(thread_id)
            || event.is_activity_in_thread(thread_id)
            || event.is_thread_handoff_in_channel(channel_id)
    })
}

/// The mailbox column's wake-ups: messages, activity, and held replies.
///
/// A separate function rather than a widened [`channel_wake_ups`]: that one is also the
/// schedule-run stream's, and [`thread_wake_ups`] is also the simulation stream's. Neither of those
/// renders a handoff, so widening them would wake two unrelated streams into a pointless re-query
/// on every claim.
pub(super) fn mailbox_channel_wake_ups(
    events: &MailboxEvents,
    label: &'static str,
    channel_id: Uuid,
) -> impl Stream<Item = Wake> + Send + use<> {
    wake_ups(events, label, move |event| {
        event.is_message_in_channel(channel_id)
            || event.is_activity_in_channel(channel_id)
            || event.is_thread_handoff_in_channel(channel_id)
    })
}

pub(super) fn task_chain_wake_ups(
    events: &MailboxEvents,
    label: &'static str,
    company_id: Uuid,
) -> impl Stream<Item = Wake> + Send + use<> {
    wake_ups(events, label, move |event| {
        event.is_task_chain_in_company(company_id)
    })
}

pub(super) fn task_count_wake_ups(
    events: &MailboxEvents,
    label: &'static str,
    company_id: Uuid,
) -> impl Stream<Item = Wake> + Send + use<> {
    wake_ups(events, label, move |event| {
        event.is_task_attention_in_company(company_id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::events::{AttentionScope, AttentionWakeSource, ThreadScope};

    #[test]
    fn counts_only_wake_for_task_attention_in_the_same_company() {
        let company_id = Uuid::new_v4();
        for source_kind in [
            AttentionWakeSource::Task,
            AttentionWakeSource::ThreadHandoff,
            AttentionWakeSource::Handoff,
            AttentionWakeSource::Delivery,
            AttentionWakeSource::ResponseReview,
            AttentionWakeSource::Delegation,
        ] {
            let event = MailboxEvent::AttentionChanged(AttentionScope {
                company_id,
                channel_id: Uuid::new_v4(),
                source_kind,
                source_id: Uuid::new_v4(),
            });
            assert_eq!(
                event.is_task_attention_in_company(company_id),
                source_kind == AttentionWakeSource::Task
            );
            assert!(!event.is_task_attention_in_company(Uuid::new_v4()));
        }
    }

    fn held_reply_changed(company_id: Uuid, channel_id: Uuid) -> MailboxEvent {
        MailboxEvent::AttentionChanged(AttentionScope {
            company_id,
            channel_id,
            source_kind: AttentionWakeSource::ThreadHandoff,
            source_id: Uuid::new_v4(),
        })
    }

    /// Case 19. The regression guard for "add new predicates, do not widen the existing ones".
    ///
    /// [`thread_wake_ups`] is the simulation stream's and [`channel_wake_ups`] is the schedule-run
    /// stream's. Neither renders a held reply, so a claim must not wake either of them -- and the
    /// mailbox's own two must wake for exactly the same event. The message published after the
    /// handoff change is what makes "did not wake" an assertion rather than a timeout: the old
    /// predicates' first wake-up is the message, so the handoff change was skipped, not merely
    /// slow.
    #[tokio::test]
    async fn a_held_reply_change_wakes_the_mailbox_streams_and_neither_of_the_other_two() {
        let events = MailboxEvents::new();
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let thread_id = Uuid::new_v4();
        let scope = ThreadScope {
            thread_id,
            channel_id,
            company_id,
        };

        let mut simulation = Box::pin(thread_wake_ups(&events, "test", thread_id));
        let mut schedule_runs = Box::pin(channel_wake_ups(&events, "test", channel_id));
        let mut mailbox_thread = Box::pin(mailbox_thread_wake_ups(
            &events, "test", thread_id, channel_id,
        ));
        let mut mailbox_column = Box::pin(mailbox_channel_wake_ups(&events, "test", channel_id));

        let handoff = held_reply_changed(company_id, channel_id);
        events.publish(handoff);
        events.publish(MailboxEvent::MessageCommitted(scope));

        for woken in [mailbox_thread.next().await, mailbox_column.next().await] {
            assert!(
                matches!(woken, Some(Wake::Event(event)) if event == handoff),
                "the mailbox redraws its badge and banner on a held-reply change"
            );
        }
        for skipped in [simulation.next().await, schedule_runs.next().await] {
            assert!(
                matches!(
                    skipped,
                    Some(Wake::Event(MailboxEvent::MessageCommitted(_)))
                ),
                "a held-reply change was filtered out before it reached this stream"
            );
        }
    }

    /// A handoff change in another channel is nobody's news, including the mailbox's.
    #[tokio::test]
    async fn a_held_reply_change_in_another_channel_wakes_nothing() {
        let events = MailboxEvents::new();
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let thread_id = Uuid::new_v4();

        let mut mailbox_thread = Box::pin(mailbox_thread_wake_ups(
            &events, "test", thread_id, channel_id,
        ));
        let mut mailbox_column = Box::pin(mailbox_channel_wake_ups(&events, "test", channel_id));

        events.publish(held_reply_changed(company_id, Uuid::new_v4()));
        events.publish(MailboxEvent::MessageCommitted(ThreadScope {
            thread_id,
            channel_id,
            company_id,
        }));

        for woken in [mailbox_thread.next().await, mailbox_column.next().await] {
            assert!(matches!(
                woken,
                Some(Wake::Event(MailboxEvent::MessageCommitted(_)))
            ));
        }
    }
}
