//! Live message notifications: Postgres `LISTEN`/`NOTIFY` fanned out to the SSE streams that keep
//! open `/ui` mailboxes up to date.
//!
//! The chain is `pg_notify` (from the `thread_messages_notify` trigger) → one [`PgListener`] per
//! process → a [`broadcast`] channel → one subscriber per open mailbox.
//!
//! Going through the database rather than a bare in-process channel is what makes this work at
//! all in this deployment: the writes that a reader most needs to see come from the `TaskWorker`
//! and the SMTP listener, which are separate tasks today and can be separate *machines* once more
//! than one is serving traffic. A process-local channel would only ever see a mailbox's own
//! compose/reply, which is the one case that already updates without help.
//!
//! Events carry identifiers, never bodies — subscribers re-query. See
//! [`crate::use_cases::thread::ThreadUseCases::get_thread_messages_after`].

use serde::Deserialize;
use sqlx::PgPool;
use sqlx::postgres::PgListener;
use tokio::sync::broadcast;
use tracing::{error, info, warn};
use uuid::Uuid;

/// The Postgres channel the `thread_messages_notify` trigger publishes committed messages on.
const MESSAGE_CHANNEL: &str = "thread_message";

/// The Postgres channel the `background_tasks_notify_activity` trigger publishes task status
/// changes on.
const ACTIVITY_CHANNEL: &str = "thread_activity";

/// How many events a slow subscriber may fall behind before it is marked lagged. Lag is not data
/// loss here — a lagged subscriber re-queries from its cursor and catches up in one round trip —
/// so this only needs to be large enough that lag is rare.
const BROADCAST_CAPACITY: usize = 256;

/// How long to wait before reopening a listener connection that failed outright.
const RECONNECT_BACKOFF: std::time::Duration = std::time::Duration::from_secs(5);

/// Which thread something happened in. Deliberately just identifiers: the payload is capped at
/// 8000 bytes, and a reader that re-queries gets the same result whether it missed one event or
/// ten.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct ThreadScope {
    pub thread_id: Uuid,
    pub channel_id: Uuid,
    pub company_id: Uuid,
}

/// Something a connected mailbox may need to redraw.
///
/// The two kinds are handled very differently downstream and must not be conflated: a committed
/// message is an *append* that a reader catches up on through a cursor, while an activity change
/// is a *state* that is re-read in full every time. Keeping them as one enum over one listener is
/// what lets a single `LISTEN` connection serve both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailboxEvent {
    MessageCommitted(ThreadScope),
    ActivityChanged(ThreadScope),
}

impl MailboxEvent {
    pub fn scope(&self) -> ThreadScope {
        match self {
            MailboxEvent::MessageCommitted(scope) | MailboxEvent::ActivityChanged(scope) => *scope,
        }
    }

    /// True for a message committed in `thread_id` -- the wake-up an appending reader wants.
    pub fn is_message_in_thread(&self, thread_id: Uuid) -> bool {
        matches!(self, MailboxEvent::MessageCommitted(scope) if scope.thread_id == thread_id)
    }

    /// True for a task status change on `thread_id`.
    pub fn is_activity_in_thread(&self, thread_id: Uuid) -> bool {
        matches!(self, MailboxEvent::ActivityChanged(scope) if scope.thread_id == thread_id)
    }

    /// True for a message committed anywhere in `channel_id`.
    pub fn is_message_in_channel(&self, channel_id: Uuid) -> bool {
        matches!(self, MailboxEvent::MessageCommitted(scope) if scope.channel_id == channel_id)
    }

    /// True for a task status change anywhere in `channel_id`.
    pub fn is_activity_in_channel(&self, channel_id: Uuid) -> bool {
        matches!(self, MailboxEvent::ActivityChanged(scope) if scope.channel_id == channel_id)
    }
}

/// The process-wide fan-out of [`MailboxEvent`]s. Cheap to clone; held in `AppState`.
#[derive(Clone)]
pub struct MailboxEvents {
    sender: broadcast::Sender<MailboxEvent>,
}

impl MailboxEvents {
    pub fn new() -> Self {
        Self {
            sender: broadcast::channel(BROADCAST_CAPACITY).0,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<MailboxEvent> {
        self.sender.subscribe()
    }

    /// Publish an event. A send with no subscribers is the normal case (nobody has a mailbox
    /// open), not an error, so the result is deliberately dropped.
    pub fn publish(&self, event: MailboxEvent) {
        let _ = self.sender.send(event);
    }
}

impl Default for MailboxEvents {
    fn default() -> Self {
        Self::new()
    }
}

/// Listen for committed messages until shutdown, republishing each onto `events`.
///
/// Errors are logged and retried rather than propagated: a mailbox that stops updating is a far
/// better failure than a server that stops serving, and every reader recovers on reconnect.
pub async fn run_mailbox_event_listener(
    pool: PgPool,
    events: MailboxEvents,
    mut shutdown: broadcast::Receiver<()>,
) {
    loop {
        tokio::select! {
            _ = shutdown.recv() => {
                info!("Thread message listener shutting down");
                return;
            }
            result = listen_until_error(&pool, &events) => {
                // `listen_until_error` only returns on failure; `PgListener` handles ordinary
                // reconnects internally without surfacing them here.
                if let Err(err) = result {
                    error!(error = %err, "Thread message listener failed, retrying");
                }
            }
        }

        tokio::select! {
            _ = shutdown.recv() => return,
            _ = tokio::time::sleep(RECONNECT_BACKOFF) => {}
        }
    }
}

async fn listen_until_error(pool: &PgPool, events: &MailboxEvents) -> Result<(), sqlx::Error> {
    let mut listener = PgListener::connect_with(pool).await?;
    // One connection serves both channels; `PgListener` re-issues every LISTEN on reconnect.
    listener
        .listen_all([MESSAGE_CHANNEL, ACTIVITY_CHANNEL])
        .await?;
    info!("Listening for mailbox notifications");

    loop {
        let notification = listener.recv().await?;
        let scope = match serde_json::from_str::<ThreadScope>(notification.payload()) {
            Ok(scope) => scope,
            // A payload we cannot read means a trigger and this struct have drifted apart. Skip it
            // rather than tearing down the listener over one bad row.
            Err(err) => {
                warn!(
                    error = %err,
                    channel = notification.channel(),
                    payload = notification.payload(),
                    "Ignoring unparseable mailbox notification"
                );
                continue;
            }
        };

        match notification.channel() {
            MESSAGE_CHANNEL => events.publish(MailboxEvent::MessageCommitted(scope)),
            ACTIVITY_CHANNEL => events.publish(MailboxEvent::ActivityChanged(scope)),
            other => warn!(
                channel = other,
                "Ignoring notification on an unknown channel"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(last: u8) -> Uuid {
        Uuid::parse_str(&format!("0a8f5f5e-0000-4000-8000-0000000000{last:02}")).unwrap()
    }

    fn scope() -> ThreadScope {
        ThreadScope {
            thread_id: id(1),
            channel_id: id(2),
            company_id: id(3),
        }
    }

    /// Guards both triggers' `json_build_object` shape against this struct. If a migration's key
    /// names change, this fails instead of the mailbox silently going quiet.
    #[test]
    fn parses_the_payload_the_triggers_build() {
        let payload = r#"{
            "thread_id": "0a8f5f5e-0000-4000-8000-000000000001",
            "channel_id": "0a8f5f5e-0000-4000-8000-000000000002",
            "company_id": "0a8f5f5e-0000-4000-8000-000000000003"
        }"#;

        assert_eq!(
            serde_json::from_str::<ThreadScope>(payload).unwrap(),
            scope()
        );
    }

    #[test]
    fn malformed_payloads_are_errors_not_panics() {
        for payload in ["", "{}", r#"{"thread_id": "not-a-uuid"}"#, "null"] {
            assert!(serde_json::from_str::<ThreadScope>(payload).is_err());
        }
    }

    /// The two kinds drive different machinery -- an append caught up through a cursor versus a
    /// state re-read in full -- so a subscriber must never mistake one for the other.
    #[test]
    fn the_two_event_kinds_are_matched_separately() {
        let message = MailboxEvent::MessageCommitted(scope());
        let activity = MailboxEvent::ActivityChanged(scope());

        assert!(message.is_message_in_thread(id(1)));
        assert!(!activity.is_message_in_thread(id(1)));

        assert!(message.is_message_in_channel(id(2)));
        assert!(!activity.is_message_in_channel(id(2)));

        assert!(activity.is_activity_in_channel(id(2)));
        assert!(!message.is_activity_in_channel(id(2)));

        // Another thread's or channel's event is never ours.
        assert!(!message.is_message_in_thread(id(9)));
        assert!(!activity.is_activity_in_channel(id(9)));
    }

    #[tokio::test]
    async fn published_events_reach_every_subscriber() {
        let events = MailboxEvents::new();
        let mut first = events.subscribe();
        let mut second = events.subscribe();

        let event = MailboxEvent::ActivityChanged(scope());
        events.publish(event);

        assert_eq!(first.recv().await.unwrap(), event);
        assert_eq!(second.recv().await.unwrap(), event);
    }

    #[test]
    fn publishing_with_no_subscribers_is_not_an_error() {
        // The common case: nobody has a mailbox open.
        MailboxEvents::new().publish(MailboxEvent::MessageCommitted(scope()));
    }
}
