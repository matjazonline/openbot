use axum::{Router, http, middleware};
use http::header::{AUTHORIZATION, CONTENT_TYPE};
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    trace::TraceLayer,
};
use uuid::Uuid;

use crate::adapters::{
    self,
    http::{app_state::AppState, security},
};

pub fn create_app(app_state: AppState) -> Router {
    let allowed_origins: Vec<http::HeaderValue> = app_state
        .config
        .cors_allowed_origins
        .iter()
        .map(|origin| {
            origin.parse().unwrap_or_else(|_| {
                panic!("CORS_ALLOWED_ORIGINS contains an invalid origin: {origin}")
            })
        })
        .collect();

    // Credentials are allowed, so the origin can never be a wildcard; every
    // accepted origin has to be listed explicitly.
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(allowed_origins))
        .allow_methods([http::Method::POST, http::Method::GET])
        .allow_headers([CONTENT_TYPE, AUTHORIZATION])
        .allow_credentials(true);

    let security_config = app_state.config.clone();
    Router::new()
        .merge(adapters::http::routes::router(app_state.sessions.clone()))
        .with_state(app_state)
        .layer(cors)
        .layer(middleware::from_fn_with_state(
            security_config,
            security::browser_security,
        ))
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &http::Request<_>| {
                let request_id = Uuid::new_v4();
                tracing::info_span!(
                    "http-request",
                    method = %request.method(),
                    uri = %request.uri(),
                    version = ?request.version(),
                    request_id = %request_id
                )
            }),
        )
}
