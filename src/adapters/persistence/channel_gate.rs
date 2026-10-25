//! The channel gate: one transaction-scoped advisory lock per `(company, channel)`.
//!
//! Routing decides whether a channel takes a message -- the enabled switch, the access mode, the
//! allowlist, the addresses -- from reads made before the inbound commit begins, and nothing in
//! that decision writes the channel row, so no row lock can protect it. The gate and the channel's
//! settings revision do:
//!
//! - a settings edit takes the gate **exclusively**, before it locks the channel row, and moves
//!   `channels.settings_revision` on with the settings
//!   ([`crate::adapters::persistence::channel_edit`]);
//! - routing reads the revision in the statement that reads the settings it decides on;
//! - the inbound commit takes the gate **shared** for every channel it admits into, before its
//!   first row lock, and then refuses the message as [`AppError::Superseded`] if any of those
//!   channels has moved past the revision it was routed under.
//!
//! So a message is stored under the settings it was routed against, with any edit waiting for the
//! commit, or not at all -- its transport retries it and routing sees the new settings. Never
//! under a mix of the two.
//!
//! Under `READ COMMITTED` the revision check being a statement of its own, started after the gate
//! is held, is what makes the gate mean anything: a statement that waited for the lock inside
//! itself would still read with the snapshot it started with.
//!
//! The keys are hashed, so two unrelated channels can collide; a collision only serializes them,
//! it never skips a lock.

use sqlx::PgConnection;
use uuid::Uuid;

use crate::app_error::{AppError, AppResult};

/// One channel's gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ChannelGateKey {
    pub(crate) company_id: Uuid,
    pub(crate) channel_id: Uuid,
}

impl ChannelGateKey {
    pub(crate) const fn new(company_id: Uuid, channel_id: Uuid) -> Self {
        Self {
            company_id,
            channel_id,
        }
    }
}

/// How a transaction takes the gate, which is also why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChannelGateAccess {
    /// Admitting a message through the channel's current settings. Any number may hold it at once.
    Admit,
    /// Changing those settings. Waits for every admission in flight, and holds new ones back
    /// until it commits.
    ChangeSettings,
}

/// Take the gate for every key, in `(company_id, channel_id)` order.
///
/// A multi-channel writer must pass all of its keys in one call: taking them one at a time in the
/// order it happens to meet them is how two such writers deadlock each other.
pub(crate) async fn acquire_channel_gates_on(
    conn: &mut PgConnection,
    keys: impl IntoIterator<Item = ChannelGateKey>,
    access: ChannelGateAccess,
) -> AppResult<()> {
    let mut keys: Vec<ChannelGateKey> = keys.into_iter().collect();
    keys.sort_unstable();
    keys.dedup();
    let function = match access {
        ChannelGateAccess::Admit => "pg_advisory_xact_lock_shared",
        ChannelGateAccess::ChangeSettings => "pg_advisory_xact_lock",
    };
    let statement = format!(
        "SELECT {function}(hashtextextended('channel-gate:' || $1::text || ':' || $2::text, 0))"
    );
    for key in keys {
        sqlx::query(&statement)
            .bind(key.company_id)
            .bind(key.channel_id)
            .execute(&mut *conn)
            .await
            .map_err(AppError::from)?;
    }
    Ok(())
}

/// [`acquire_channel_gates_on`] for one channel.
pub(crate) async fn acquire_channel_gate_on(
    conn: &mut PgConnection,
    key: ChannelGateKey,
    access: ChannelGateAccess,
) -> AppResult<()> {
    acquire_channel_gates_on(conn, [key], access).await
}
