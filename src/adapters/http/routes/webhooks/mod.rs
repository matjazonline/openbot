pub mod resend_api;
pub mod sendgrid;

use axum::Router;

use crate::adapters::http::app_state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(resend_api::router())
        .merge(sendgrid::router())
}
