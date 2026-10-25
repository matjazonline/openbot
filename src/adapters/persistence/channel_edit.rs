//! Saving a channel edit under the channel's gate.
//!
//! One transaction, in this order, so a failure anywhere leaves the channel exactly as it was:
//!
//! 1. the channel's gate, exclusively ([`crate::adapters::persistence::channel_gate`]), so no
//!    inbound commit admits a message against settings this edit is replacing;
//! 2. the channel row, re-checked against the tenant;
//! 3. the channel settings, slugs, email binding and allowlist grants, moving the settings
//!    revision on so a message routed against the old ones is refused at its commit.
//!
//! The whole operation runs under an explicit time budget, so an edit that cannot take its locks
//! is refused with nothing changed rather than left waiting.

use std::time::{Duration, Instant};

use sqlx::{PgPool, Postgres, Transaction};
use tracing::warn;

use super::{
    channel::write_channel_settings_on,
    channel_gate::{ChannelGateAccess, ChannelGateKey, acquire_channel_gate_on},
};
use crate::{
    app_error::{AppError, AppResult},
    use_cases::channel::{ChannelUpdate, channel_not_found},
};

/// The bounds one channel edit must finish inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EditBudget {
    pub(crate) lock_timeout: Duration,
    /// For the whole transaction, not per statement: a long series of individually quick
    /// statements must not be able to run on indefinitely.
    pub(crate) deadline: Duration,
}

impl EditBudget {
    pub(crate) const DEFAULT: Self = Self {
        lock_timeout: Duration::from_secs(2),
        deadline: Duration::from_secs(30),
    };
}

/// Save the edit, inside `budget`.
pub(crate) async fn update_channel_within(
    pool: &PgPool,
    request: ChannelUpdate,
    budget: EditBudget,
) -> AppResult<()> {
    let started = Instant::now();
    let (company_id, channel_id) = (request.company_id, request.channel_id);
    let mut tx = pool.begin().await.map_err(AppError::from)?;
    let applied = tokio::time::timeout(
        budget.deadline,
        update_channel_on(&mut tx, &request, budget),
    )
    .await;
    let duration_ms = started.elapsed().as_millis() as u64;

    let error = match applied {
        Ok(Ok(())) => return tx.commit().await.map_err(AppError::from),
        // A statement or lock wait reaching the bounds `bound_statements_on` set.
        Ok(Err(AppError::DatabaseTimeout(detail))) => AppError::Timeout(format!(
            "Saving this channel waited too long for work already in progress, so nothing was \
             changed. Try again shortly. ({detail})"
        )),
        Ok(Err(error)) => error,
        Err(_elapsed) => AppError::Timeout(format!(
            "Saving this channel took longer than {} seconds, so nothing was changed. \
             Try again shortly.",
            budget.deadline.as_secs()
        )),
    };
    // Explicit, rather than left to the drop: the caller is told the edit did not happen, and a
    // rollback that itself fails is worth knowing about.
    if let Err(rollback) = tx.rollback().await {
        warn!(%company_id, %channel_id, error = %rollback, "Could not roll back a failed channel edit");
    }
    warn!(
        %company_id,
        %channel_id,
        outcome = edit_failure_kind(&error),
        duration_ms,
        "A channel edit was refused and rolled back"
    );
    Err(error)
}

/// The log label for a refused edit.
fn edit_failure_kind(error: &AppError) -> &'static str {
    match error {
        AppError::Timeout(_) => "timeout",
        AppError::Conflict(_) => "refused",
        AppError::NotFound(_) => "not_found",
        _ => "error",
    }
}

/// The edit itself, on the caller's transaction. Commits nothing.
pub(crate) async fn update_channel_on(
    tx: &mut Transaction<'_, Postgres>,
    request: &ChannelUpdate,
    budget: EditBudget,
) -> AppResult<()> {
    bound_statements_on(tx, budget).await?;
    acquire_channel_gate_on(
        tx,
        ChannelGateKey::new(request.company_id, request.channel_id),
        ChannelGateAccess::ChangeSettings,
    )
    .await?;
    lock_channel_on(tx, request).await?;
    write_channel_settings_on(tx, request.company_id, request.channel_id, &request.write).await
}

/// Bound every statement and lock wait in this transaction.
///
/// `set_config(.., true)` is `SET LOCAL`, so the bounds end with the transaction. The statement
/// bound is the whole deadline: the transaction-wide deadline is enforced around the future, and
/// this makes the server stop a statement that outlives it rather than finish it for nobody.
async fn bound_statements_on(
    tx: &mut Transaction<'_, Postgres>,
    budget: EditBudget,
) -> AppResult<()> {
    sqlx::query(
        "SELECT set_config('lock_timeout', $1, true), set_config('statement_timeout', $2, true)",
    )
    .bind(format!("{}ms", budget.lock_timeout.as_millis()))
    .bind(format!("{}ms", budget.deadline.as_millis()))
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(())
}

/// Lock the channel row within its tenant.
async fn lock_channel_on(
    tx: &mut Transaction<'_, Postgres>,
    request: &ChannelUpdate,
) -> AppResult<()> {
    sqlx::query_scalar::<_, i32>(
        "SELECT 1 FROM channels WHERE company_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(request.company_id)
    .bind(request.channel_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?
    .map(|_| ())
    .ok_or_else(channel_not_found)
}

#[cfg(test)]
#[path = "channel_edit_tests.rs"]
mod tests;
