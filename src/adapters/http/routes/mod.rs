pub mod agent;
pub mod approval;
pub mod assets;
pub mod channel;
pub mod company;
pub mod company_invite;
pub mod health;
pub mod monitoring;
pub mod onboarding;
pub mod schedule;
pub mod task;
pub mod ui;
pub mod ui_agents;
pub mod ui_channels;
pub mod ui_companies;
pub mod ui_dashboard;
pub mod ui_outbox;
pub mod ui_schedules;
pub mod ui_tasks;
pub mod ui_team;
pub mod ui_uploads;
pub mod user;
pub mod webhooks;

use axum::{Router, middleware};

use crate::adapters::http::{app_state::AppState, auth};
use crate::app_error::AppError;

/// What an htmx fragment shows when the company behind a request cannot be loaded.
///
/// These routes answer 200 with an alert body because htmx does not swap a 4xx response, so the
/// status code cannot carry the distinction. The match is what keeps a database fault from being
/// reported as a routine "not found" — which is what the `.ok().flatten()` these callers used to
/// do would have done.
pub(super) fn company_load_error(error: &AppError) -> String {
    match error {
        AppError::NotFound(message) => message.clone(),
        other => format!("Failed to load company: {other}"),
    }
}

pub fn router() -> Router<AppState> {
    let protected = Router::new()
        .merge(user::protected_router())
        .merge(company::router())
        .merge(company_invite::router())
        .merge(task::router())
        .merge(channel::router())
        .merge(schedule::router())
        .merge(agent::router())
        .merge(approval::router())
        .merge(monitoring::router())
        .merge(onboarding::router())
        .merge(ui::router())
        .merge(ui_agents::router())
        .merge(ui_channels::router())
        .merge(ui_schedules::router())
        .merge(ui_companies::router())
        .merge(ui_dashboard::router())
        .merge(ui_outbox::router())
        .merge(ui_tasks::router())
        .merge(ui_team::router())
        .merge(ui_uploads::router())
        .route_layer(middleware::from_fn(auth::require_auth));

    Router::new()
        .merge(health::router())
        .merge(assets::router())
        .merge(user::public_router())
        .merge(webhooks::router())
        .merge(protected)
}
