use axum::{Router, extract::State, response::Json, routing::get};
use std::sync::Arc;

use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser},
    app_error::{AppError, AppResult},
    domain::monitoring::MonitoringService,
    infra::config::AppConfig,
    use_cases::user::UserUseCases,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/monitoring/stats", get(get_monitoring_stats))
        .route("/metrics", get(get_monitoring_stats))
}

async fn get_monitoring_stats(
    State(monitoring): State<Arc<dyn MonitoringService>>,
    State(users): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
) -> AppResult<Json<serde_json::Value>> {
    let user = users
        .get_user_by_id(user.id)
        .await?
        .ok_or_else(|| AppError::NotFound("Operator not found".into()))?;
    if !config.is_operator(&user) {
        return Err(AppError::NotFound("Operator not found".into()));
    }
    Ok(Json(monitoring.get_stats_json()))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(!config.is_operator(&user));
        config.operator_emails = vec!["operator@example.com".into()];
        assert!(!config.is_operator(&user));
        user.email_verified_at = Some(chrono::Utc::now());
        assert!(config.is_operator(&user));
    }
}
