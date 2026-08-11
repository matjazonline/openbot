pub mod agent;
pub mod approval;
pub mod company;
pub mod company_invite;
pub mod task;
pub mod user;
pub mod webhooks;
pub mod workflow;

use axum::Router;

use crate::adapters::http::app_state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(user::router())
        .merge(company::router())
        .merge(company_invite::router())
        .merge(task::router())
        .merge(workflow::router())
        .merge(agent::router())
        .merge(webhooks::router())
        .merge(approval::router())
}
