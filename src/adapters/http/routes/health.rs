use axum::{Router, response::Json, routing::get};
use serde_json::{Value, json};

use crate::adapters::http::app_state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/health", get(health))
}

/// Liveness probe for the Fly health check. Deliberately touches no
/// dependencies: a database blip should not make the platform recycle a
/// process that is otherwise serving fine.
async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}
