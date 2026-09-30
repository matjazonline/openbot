//! PostgreSQL is the sole owner of workflow authoring and runtime records.
//!
//! Authorized controls acquire company authority before the run, matching child admission.
//! Runtime transitions lock the run before its executions, jobs/attempts, waits
//! and events. Discovery reads grant no authority. Reacquiring an already-owned
//! run is safe. Child admission/replay may lock parent then child, never the
//! reverse. Runtime transitions own only one run. Child terminal transitions append a
//! child-owned wakeup fact; parent consumption must use a separate transaction.
//! Foreign-key checks count as locks too: never insert a parent reference while
//! completing the child. Parent linkage is immutable and established at admission.
mod action_authority;
pub mod action_dispatch;
mod action_receipts;
pub mod action_reconciliation;
mod action_uncertainty;
mod actions;
mod activation;
mod admission;
mod admission_binding;
mod admission_history;
mod admission_replay;
mod admission_source;
mod admission_write;
mod association;
mod authority;
mod batch;
mod batch_commit;
mod binding_rows;
mod bindings;
mod budget;
mod capacity;
mod completion;
mod completion_job;
mod definitions;
mod fair_polling;
mod fairness;
mod lease;
mod lease_claim;
// Exact historical owner is compiled only for pre-episode migration fixtures.
mod resources;
mod rows;
#[cfg(test)]
mod tests;
#[cfg(test)]
#[path = "action_reconciliation_upgrade_legacy_claim.rs"]
mod upgrade_legacy_claim;
mod wait_commit;
mod waits;
mod wakeups;

use super::PostgresPersistence;
use crate::app_error::{AppError, AppResult};
use crate::application::workflow::{lifecycle::*, publication::*, templates::*, *};
use crate::domain::workflow::*;
use async_trait::async_trait;
use sqlx::{PgConnection, Postgres, Transaction};
use std::sync::Arc;
use uuid::Uuid;

fn conflict() -> AppError {
    AppError::Conflict("Workflow lifecycle revision or command conflict".into())
}
fn missing() -> AppError {
    AppError::NotFound("Workflow resource".into())
}
fn invalid() -> AppError {
    AppError::Database("Invalid stored workflow record".into())
}
fn revision(value: u64) -> AppResult<i64> {
    i64::try_from(value).map_err(|_| conflict())
}
fn write_error(error: sqlx::Error) -> AppError {
    if error
        .as_database_error()
        .is_some_and(|error| error.is_unique_violation())
    {
        conflict()
    } else {
        error.into()
    }
}

mod polling;

mod recovery;

mod pending_recovery;

mod maintenance;

mod control_retry;
mod controls;
mod inspection;
