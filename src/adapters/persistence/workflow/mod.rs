//! PostgreSQL is the sole owner of workflow authoring and runtime records.
mod admission;
mod admission_binding;
mod admission_history;
mod admission_replay;
mod admission_source;
mod admission_write;
mod association;
mod authority;
mod binding_rows;
mod bindings;
mod definitions;
mod resources;
mod rows;
#[cfg(test)]
mod tests;

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
