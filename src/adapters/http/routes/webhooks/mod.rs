pub mod resend;
pub mod sendgrid;

use axum::Router;

use crate::adapters::http::app_state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(resend::router())
        .merge(sendgrid::router())
}
