//! The channel assignment gate: one transaction-scoped advisory lock per `(company, channel)`.
//!
//! Row locks settle races over rows that already exist. They cannot stop a concurrent transaction
//! from inserting a task that a just-removed agent will own, or from handing an existing task to
//! that agent, because until then there is no row to lock. So:
//!
//! - an assignment change takes the gate **exclusively**, before it reads the assignments it is
//!   replacing or locks the channel row;
//! - every writer that admits work through an assignment -- creating a task, starting one by hand,
//!   transferring one to an agent, resuming one, publishing one's output or asking outreach on its
//!   behalf -- takes it **shared**,
//!   before its first row lock, and reads eligibility in a statement that starts after it is held.
//!
//! Under `READ COMMITTED` that last point is what makes the gate mean anything: a statement that
//! waited for the lock inside itself would still read with the snapshot it started with.
//!
//! Never take the gate while holding a task, outreach, draft or delivery row lock: the assignment
//! change holds the gate and then takes those, in that order, and the reverse is a deadlock. The
//! keys are hashed, so two unrelated channels can collide; a collision only serializes them, it
//! never skips a lock.

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
    /// Admitting work through the channel's current assignments. Any number may hold it at once.
    Admit,
    /// Changing the assignments themselves. Waits for every admission in flight, and holds new
    /// ones back until it commits.
    ChangeAssignments,
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
        ChannelGateAccess::ChangeAssignments => "pg_advisory_xact_lock",
    };
    let statement = format!(
        "SELECT {function}(hashtextextended('channel-assignment:' || $1::text || ':' || $2::text, 0))"
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

/// The gate a task's primary channel is guarded by, read without taking any lock.
///
/// Read first and revalidated after: the caller takes the gate with this key and only then locks
/// the task row, so it must not trust anything else this read returned.
pub(crate) async fn task_gate_key_on(
    conn: &mut PgConnection,
    task_id: Uuid,
) -> AppResult<Option<ChannelGateKey>> {
    let row: Option<(Uuid, Uuid)> =
        sqlx::query_as("SELECT company_id, channel_id FROM background_tasks WHERE id = $1")
            .bind(task_id)
            .fetch_optional(&mut *conn)
            .await
            .map_err(AppError::from)?;
    Ok(row.map(|(company_id, channel_id)| ChannelGateKey::new(company_id, channel_id)))
}
