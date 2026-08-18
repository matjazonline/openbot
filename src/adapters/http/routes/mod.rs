pub mod agent;
pub mod approval;
pub mod assets;
pub mod channel;
pub mod company;
pub mod company_invite;
pub mod health;
pub mod monitoring;
pub mod onboarding;
pub mod task;
pub mod ui;
pub mod user;
pub mod webhooks;

use axum::{Router, middleware};

use crate::adapters::http::{app_state::AppState, auth};

pub fn router() -> Router<AppState> {
    let protected = Router::new()
        .merge(user::protected_router())
        .merge(company::router())
        .merge(company_invite::router())
        .merge(task::router())
        .merge(channel::router())
        .merge(agent::router())
        .merge(approval::router())
        .merge(monitoring::router())
        .merge(onboarding::router())
        .merge(ui::router())
        .route_layer(middleware::from_fn(auth::require_auth));

    Router::new()
        .merge(health::router())
        .merge(assets::router())
        .merge(user::public_router())
        .merge(webhooks::router())
        .merge(protected)
}
