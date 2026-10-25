//! PostgreSQL is the sole owner of workflow authoring and runtime records.
//!
//! Authorized controls acquire company authority before the run.
//! Runtime transitions lock the run before its executions, jobs/attempts, waits
//! and events. Discovery reads grant no authority. Reacquiring an already-owned
//! run is safe. A parent's park or round transition creates its child run in its own
//! transaction (07-D1 §D1.2), and a cancel cascade locks parent then child, never the
//! reverse. Child terminal transitions append a child-owned wakeup fact; parent
//! consumption must use a separate transaction.
//! Foreign-key checks count as locks too: never insert a parent reference while
//! completing the child. Parent linkage is immutable and established at creation.
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
mod agent_preparation;
pub mod agent_publication;
mod association;
mod authoring_reads;
pub(crate) mod authority;
mod batch;
mod batch_commit;
mod binding_rows;
mod bindings;
mod budget;
mod capacity;
mod checkpoints;
pub mod child_publication;
mod child_waits;
mod children;
mod completion;
mod completion_job;
mod decisions;
mod definitions;
mod fair_polling;
mod fairness;
mod history_reader;
pub mod http;
mod lease;
mod lease_claim;
pub mod mcp;
pub mod message;
mod message_intents;
mod persistence_bound;
pub mod provisioning;
mod reads;
mod reply_waits;
mod resources;
mod rows;
mod run_views;
#[cfg(test)]
pub(crate) mod schedule_test_support;
mod schedules;
#[cfg(test)]
mod tests;
/// The uncertain HTTP action the reconciliation route tests start from (P08-D3).
#[cfg(test)]
pub(crate) use tests::reconciliation_fixture;
pub mod tool_publication;
mod wait_commit;
mod waits;
mod wakeups;
mod workflow_tool_calls;
mod workflow_tools;

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
mod message_outcomes;

mod token_budget;
