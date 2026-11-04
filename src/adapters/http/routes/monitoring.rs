use axum::{Router, extract::State, response::Json, routing::get};
use std::sync::Arc;

use crate::{
    adapters::http::app_state::AppState, app_error::AppResult,
    domain::monitoring::MonitoringService, entities::user::Caller,
    use_cases::caller::require_operator,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/monitoring/stats", get(get_monitoring_stats))
        .route("/metrics", get(get_monitoring_stats))
}

/// Process-wide metrics, for deployment operators only.
///
/// Checked here, on the [`Caller`] property the extractor decided, because no application use case
/// owns these reads: `MonitoringService` is the process's own counters, with no tenant in them.
async fn get_monitoring_stats(
    State(monitoring): State<Arc<dyn MonitoringService>>,
    caller: Caller,
) -> AppResult<Json<serde_json::Value>> {
    require_operator(&caller)?;
    Ok(Json(monitoring.get_stats_json()))
}

#[cfg(test)]
mod tests {
    use crate::{adapters::http::auth::caller_for, infra::config::AppConfig};

    #[test]
    fn metrics_require_deployment_operator() {
        let mut config = AppConfig::for_test();
        let mut user = crate::entities::user::User {
            auth_revision: 0,
            id: uuid::Uuid::new_v4(),
            username: "operator".into(),
            email: "OPERATOR@example.com".into(),
            email_verified_at: None,
            password_hash: String::new(),
            avatar_url: None,
            created_at: chrono::Utc::now(),
        };
        assert!(!caller_for(&user, &config).is_operator());
        config.operator_emails = vec!["operator@example.com".into()];
        assert!(!caller_for(&user, &config).is_operator());
        user.email_verified_at = Some(chrono::Utc::now());
        assert!(caller_for(&user, &config).is_operator());
    }
}
