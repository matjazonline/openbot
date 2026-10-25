//! Live message notifications: Postgres `LISTEN`/`NOTIFY` fanned out to the SSE streams that keep
//! open `/ui` mailboxes up to date.
//!
//! The chain is `pg_notify` (from the `thread_messages_notify` trigger) → one [`PgListener`] per
//! process → a [`broadcast`] channel → one subscriber per open mailbox.
//!
//! Going through the database rather than a bare in-process channel is what makes this work at
//! all in this deployment: the writes that a reader most needs to see come from the workflow
//! worker and the SMTP listener, which are separate tasks today and can be separate *machines* once more
//! than one is serving traffic. A process-local channel would only ever see a mailbox's own
//! compose/reply, which is the one case that already updates without help.
//!
//! Events carry identifiers, never bodies — subscribers re-query. See
//! [`crate::use_cases::thread::ThreadUseCases::get_thread_messages_after`].

use crate::services::inbound_event_worker::InboundEventWakeups;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use sqlx::postgres::PgListener;
use tokio::sync::broadcast;
use tracing::{error, info, warn};
use uuid::Uuid;

/// The Postgres channel the `thread_messages_notify` trigger publishes committed messages on.
const MESSAGE_CHANNEL: &str = "thread_message";

/// The Postgres channel a workflow run on a thread announces its state changes on
/// (`workflow_runs_notify_activity_*`, P08-D4 §D4.1).
const ACTIVITY_CHANNEL: &str = "thread_activity";

/// The channel the `inbound_events_notify_ready` trigger announces claimable inbox rows on.
///
/// This carries no payload the worker needs: it re-reads and claims durably, so a dropped or
/// duplicated notification costs at most one poll interval. It shares this connection rather than
/// opening its own because a pure latency hint does not justify a second dedicated listener.
const INBOUND_EVENT_CHANNEL: &str = "inbound_event_ready";
const ATTENTION_CHANNEL: &str = "attention_changed";
/// Run-level workflow transitions (`notify_workflow_run_changed`, P08-D4 §D4.1).
const WORKFLOW_RUN_CHANNEL: &str = "workflow_run_changed";
const ACTIONABLE_NOTIFICATION_CHANNEL: &str = "actionable_notification_changed";

/// How many events a slow subscriber may fall behind before it is marked lagged. Lag is not data
/// loss here — a lagged subscriber re-queries from its cursor and catches up in one round trip —
/// so this only needs to be large enough that lag is rare.
const BROADCAST_CAPACITY: usize = 256;

/// [`BROADCAST_CAPACITY`], for a test that has to produce a lag on purpose.
#[cfg(test)]
pub const fn broadcast_capacity_for_tests() -> usize {
    BROADCAST_CAPACITY
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct AttentionScope {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub source_kind: AttentionWakeSource,
    pub source_id: Uuid,
}

/// A workflow run whose state or waiting reason changed, or that was admitted. A run without a
/// channel is company-wide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct RunScope {
    pub company_id: Uuid,
    pub channel_id: Option<Uuid>,
    pub thread_id: Option<Uuid>,
    pub run_id: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct NotificationScope {
    pub company_id: Uuid,
    pub recipient_user_id: Uuid,
    pub notification_id: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionWakeSource {
    /// `message_deliveries_notify_attention`, the one trigger that still raises the wake (p08
    /// D4.3).
    Delivery,
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
    AttentionChanged(AttentionScope),
    NotificationChanged(NotificationScope),
    WorkflowRunChanged(RunScope),
    /// The listener reconnected, so notifications may have been lost meanwhile: every view
    /// reconciles in full, as on a broadcast lag (P08-D4 F077, F084).
    Resync,
}

impl MailboxEvent {
    /// Whether a view must reconcile in full regardless of scope.
    pub fn is_resync(&self) -> bool {
        matches!(self, MailboxEvent::Resync)
    }

    /// A workflow run change in `company_id`, for the views that follow runs.
    pub fn run_scope(&self, company_id: Uuid) -> Option<RunScope> {
        match self {
            Self::WorkflowRunChanged(scope) if scope.company_id == company_id => Some(*scope),
            _ => None,
        }
    }

    pub fn thread_scope(&self) -> Option<ThreadScope> {
        match self {
            MailboxEvent::MessageCommitted(scope) | MailboxEvent::ActivityChanged(scope) => {
                Some(*scope)
            }
            MailboxEvent::AttentionChanged(_)
            | MailboxEvent::NotificationChanged(_)
            | MailboxEvent::WorkflowRunChanged(_)
            | MailboxEvent::Resync => None,
        }
    }

    /// True for a message committed in `thread_id` -- the wake-up an appending reader wants.
    pub fn is_message_in_thread(&self, thread_id: Uuid) -> bool {
        matches!(self, MailboxEvent::MessageCommitted(scope) if scope.thread_id == thread_id)
    }

    /// True for a run's state change on `thread_id`.
    pub fn is_activity_in_thread(&self, thread_id: Uuid) -> bool {
        matches!(self, MailboxEvent::ActivityChanged(scope) if scope.thread_id == thread_id)
    }

    /// True for a message committed anywhere in `channel_id`.
    pub fn is_message_in_channel(&self, channel_id: Uuid) -> bool {
        matches!(self, MailboxEvent::MessageCommitted(scope) if scope.channel_id == channel_id)
    }

    /// True for a run's state change on any thread of `channel_id`.
    pub fn is_activity_in_channel(&self, channel_id: Uuid) -> bool {
        matches!(self, MailboxEvent::ActivityChanged(scope) if scope.channel_id == channel_id)
    }

    pub fn attention_scope(&self, company_id: Uuid) -> Option<AttentionScope> {
        match self {
            Self::AttentionChanged(scope) if scope.company_id == company_id => Some(*scope),
            _ => None,
        }
    }

    pub fn notification_scope(&self, user_id: Uuid) -> Option<NotificationScope> {
        match self {
            Self::NotificationChanged(scope) if scope.recipient_user_id == user_id => Some(*scope),
            _ => None,
        }
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
    inbound_events: InboundEventWakeups,
    mut shutdown: broadcast::Receiver<()>,
) {
    loop {
        tokio::select! {
            _ = shutdown.recv() => {
                info!("Thread message listener shutting down");
                return;
            }
            result = listen_until_error(&pool, &events, &inbound_events) => {
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

async fn listen_until_error(
    pool: &PgPool,
    events: &MailboxEvents,
    inbound_events: &InboundEventWakeups,
) -> Result<(), sqlx::Error> {
    let mut listener = PgListener::connect_with(pool).await?;
    // One connection serves every channel; `PgListener` re-issues each LISTEN on reconnect.
    listener
        .listen_all([
            MESSAGE_CHANNEL,
            ACTIVITY_CHANNEL,
            INBOUND_EVENT_CHANNEL,
            ATTENTION_CHANNEL,
            ACTIONABLE_NOTIFICATION_CHANNEL,
            WORKFLOW_RUN_CHANNEL,
        ])
        .await?;
    info!("Listening for mailbox notifications");
    // A new listener, first or rebuilt after a failure, may have missed notifications: views
    // reconcile (F084).
    events.publish(MailboxEvent::Resync);

    loop {
        // `try_recv` surfaces the reconnect `recv` would hide; the notifications sent while the
        // connection was down are lost, so views reconcile (F077).
        let Some(notification) = listener.try_recv().await? else {
            warn!("Mailbox listener reconnected; views resync");
            events.publish(MailboxEvent::Resync);
            continue;
        };
        // The inbox wake-up is not a `MailboxEvent` and has no payload to parse: it only shortens
        // the idle poll of a worker that may be in another process entirely.
        if notification.channel() == INBOUND_EVENT_CHANNEL {
            inbound_events.notify();
            continue;
        }
        let parsed = match notification.channel() {
            MESSAGE_CHANNEL => serde_json::from_str::<ThreadScope>(notification.payload())
                .map(MailboxEvent::MessageCommitted),
            ACTIVITY_CHANNEL => serde_json::from_str::<ThreadScope>(notification.payload())
                .map(MailboxEvent::ActivityChanged),
            ATTENTION_CHANNEL => serde_json::from_str::<AttentionScope>(notification.payload())
                .map(MailboxEvent::AttentionChanged),
            ACTIONABLE_NOTIFICATION_CHANNEL => {
                serde_json::from_str::<NotificationScope>(notification.payload())
                    .map(MailboxEvent::NotificationChanged)
            }
            WORKFLOW_RUN_CHANNEL => serde_json::from_str::<RunScope>(notification.payload())
                .map(MailboxEvent::WorkflowRunChanged),
            other => {
                warn!(
                    channel = other,
                    "Ignoring notification on an unknown channel"
                );
                continue;
            }
        };
        match parsed {
            Ok(event) => events.publish(event),
            Err(err) => warn!(
                error = %err,
                channel = notification.channel(),
                "Ignoring unparseable mailbox notification"
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

        // The `message_deliveries_notify_attention` trigger's argument, which is the only thing
        // standing between a renamed source kind and a queue that silently stops updating.
        let attention_payload = r#"{
            "company_id": "0a8f5f5e-0000-4000-8000-000000000003",
            "channel_id": "0a8f5f5e-0000-4000-8000-000000000002",
            "source_kind": "delivery",
            "source_id": "0a8f5f5e-0000-4000-8000-000000000006"
        }"#;
        assert_eq!(
            serde_json::from_str::<AttentionScope>(attention_payload).unwrap(),
            AttentionScope {
                company_id: id(3),
                channel_id: id(2),
                source_kind: AttentionWakeSource::Delivery,
                source_id: id(6),
            }
        );
        // The legacy runtime's sources are gone with their triggers.
        for retired in [
            "task",
            "handoff",
            "thread_handoff",
            "response_review",
            "delegation",
        ] {
            let payload = attention_payload.replace("\"delivery\"", &format!("\"{retired}\""));
            assert!(
                serde_json::from_str::<AttentionScope>(&payload).is_err(),
                "{retired}"
            );
        }

        let notification_payload = r#"{
            "company_id": "0a8f5f5e-0000-4000-8000-000000000003",
            "recipient_user_id": "0a8f5f5e-0000-4000-8000-000000000004",
            "notification_id": "0a8f5f5e-0000-4000-8000-000000000005"
        }"#;
        assert_eq!(
            serde_json::from_str::<NotificationScope>(notification_payload).unwrap(),
            NotificationScope {
                company_id: id(3),
                recipient_user_id: id(4),
                notification_id: id(5),
            }
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

        // An attention change is a third kind, and no predicate above may answer for it.
        let attention = MailboxEvent::AttentionChanged(attention(AttentionWakeSource::Delivery));
        assert!(!attention.is_message_in_channel(id(2)));
        assert!(!attention.is_activity_in_channel(id(2)));
    }

    fn attention(source_kind: AttentionWakeSource) -> AttentionScope {
        AttentionScope {
            company_id: id(3),
            channel_id: id(2),
            source_kind,
            source_id: id(7),
        }
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
    async fn next_event(received: &mut broadcast::Receiver<MailboxEvent>) -> MailboxEvent {
        tokio::time::timeout(std::time::Duration::from_secs(20), received.recv())
            .await
            .expect("an event arrives")
            .unwrap()
    }

    /// p08 D4.1 (F077, F084) on a real database of the test's own: a new listener publishes
    /// `Resync`; a malformed `workflow_run_changed` payload is dropped and the listener carries
    /// on; and killing its backend makes it reconnect, which publishes `Resync` again -- through
    /// `PgListener`'s own reconnect or the outer rebuild, whichever the failure takes.
    #[tokio::test]
    async fn the_listener_resyncs_on_start_and_reconnect_and_drops_a_bad_run_payload() {
        let Some(db) = crate::adapters::persistence::test_support::own_database().await else {
            return;
        };
        let pool = db.pool.clone();
        let events = MailboxEvents::new();
        let mut received = events.subscribe();
        let (shutdown, shutdown_rx) = broadcast::channel(1);
        let listener = tokio::spawn(run_mailbox_event_listener(
            pool.clone(),
            events.clone(),
            InboundEventWakeups::new(),
            shutdown_rx,
        ));
        assert_eq!(next_event(&mut received).await, MailboxEvent::Resync);

        let run = RunScope {
            company_id: id(3),
            channel_id: None,
            thread_id: None,
            run_id: id(4),
        };
        sqlx::query("SELECT pg_notify('workflow_run_changed', 'not json')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("SELECT pg_notify('workflow_run_changed', $1)")
            .bind(
                serde_json::json!({"company_id": run.company_id, "channel_id": null,
                    "thread_id": null, "run_id": run.run_id, "state": "queued",
                    "waiting_reason": null})
                .to_string(),
            )
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            next_event(&mut received).await,
            MailboxEvent::WorkflowRunChanged(run),
            "the malformed payload was dropped and the next one still arrives"
        );

        let terminated: bool = sqlx::query_scalar(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE datname = current_database() AND pid <> pg_backend_pid() \
               AND query LIKE '%LISTEN \"workflow_run_changed\"%'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            terminated,
            "the listener's backend was found and terminated"
        );
        assert_eq!(next_event(&mut received).await, MailboxEvent::Resync);

        shutdown.send(()).unwrap();
        listener.await.unwrap();
    }
}
