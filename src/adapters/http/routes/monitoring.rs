use axum::{Router, extract::State, response::Json, routing::get};
use std::sync::Arc;

use crate::{adapters::http::app_state::AppState, domain::monitoring::MonitoringService};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/monitoring/stats", get(get_monitoring_stats))
        .route("/metrics", get(get_monitoring_stats))
}

async fn get_monitoring_stats(
    State(monitoring): State<Arc<dyn MonitoringService>>,
) -> Json<serde_json::Value> {
    Json(monitoring.get_stats_json())
}
