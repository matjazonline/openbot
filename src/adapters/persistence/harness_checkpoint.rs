//! Shared checkpoint decoding; authority and write ownership stay with each adapter.
use crate::{
    app_error::{AppError, AppResult},
    services::harness::runs::{RunCheckpoint, RunState},
};

pub(super) fn decode(value: serde_json::Value) -> AppResult<RunCheckpoint> {
    let run: RunCheckpoint = serde_json::from_value(value)
        .map_err(|_| AppError::BadRequest("Malformed harness checkpoint".into()))?;
    run.validate()?;
    if let Some(output) = &run.final_output
        && let Some(response) = &output.structured
    {
        response.verify(
            &output.content,
            &crate::adapters::response_schema::JsonResponseValidator,
        )?;
    }
    Ok(run)
}

pub(super) fn state_name(state: &RunState) -> &'static str {
    match state {
        RunState::Active => "active",
        RunState::InvalidOutput => "invalid_output",
        RunState::BudgetExceeded => "budget_exceeded",
        RunState::Completed => "completed",
    }
}
