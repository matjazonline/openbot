//! Shared checkpoint decoding; authority and write ownership stay with each adapter.
use crate::{
    app_error::{AppError, AppResult},
    services::harness::runs::{RunCheckpoint, RunState},
};

pub(super) fn decode(run_id: uuid::Uuid, value: serde_json::Value) -> AppResult<RunCheckpoint> {
    let run: RunCheckpoint =
        serde_json::from_value(value).map_err(|error| invalid_checkpoint(run_id, error))?;
    run.validate()
        .map_err(|error| invalid_checkpoint(run_id, error))?;
    if let Some(output) = &run.final_output
        && let Some(response) = &output.structured
    {
        response
            .verify(
                &output.content,
                &crate::adapters::response_schema::JsonResponseValidator,
            )
            .map_err(|error| invalid_checkpoint(run_id, error))?;
    }
    Ok(run)
}

fn invalid_checkpoint(run_id: uuid::Uuid, error: impl std::fmt::Display) -> AppError {
    AppError::Internal(format!(
        "Invalid stored harness checkpoint {run_id}: {error}"
    ))
}

pub(super) fn state_name(state: &RunState) -> &'static str {
    match state {
        RunState::Active => "active",
        RunState::InvalidOutput => "invalid_output",
        RunState::BudgetExceeded => "budget_exceeded",
        RunState::Completed => "completed",
    }
}
