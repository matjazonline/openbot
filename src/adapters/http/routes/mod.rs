pub mod agent;
pub mod agent_library;
pub mod apple_auth;
pub mod approval;
pub mod assets;
pub mod channel;
pub mod company;
pub mod company_invite;
pub mod health;
mod live_updates;
pub mod monitoring;
pub mod onboarding;
pub mod schedule;
pub mod task;
pub mod ui;
pub mod ui_agents;
pub mod ui_attachments;
pub mod ui_channels;
pub mod ui_companies;
pub mod ui_dashboard;
pub mod ui_invites;
pub mod ui_message_diagnostics;
pub mod ui_outbox;
pub mod ui_profile;
pub mod ui_schedules;
pub mod ui_tasks;
pub mod ui_team;
pub mod ui_uploads;
pub mod user;
pub mod webhooks;

use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    extract::Request,
    http::{Method, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
};

use crate::adapters::http::{app_state::AppState, auth, pages, session::SessionAuthority};
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

/// Every route in the app.
///
/// Takes the [`SessionAuthority`] rather than reaching for it through `AppState`, because the auth
/// middleware is a layer rather than a handler: it is applied while the router is still stateless,
/// so what it needs to verify a cookie has to be handed to it here.
pub fn router(sessions: Arc<SessionAuthority>) -> Router<AppState> {
    let protected = Router::new()
        .merge(user::protected_router())
        .merge(apple_auth::protected_router())
        .merge(company::router())
        .merge(company_invite::router())
        .merge(task::router())
        .merge(channel::router())
        .merge(schedule::router())
        .merge(agent::router())
        .merge(agent_library::router())
        .merge(approval::router())
        .merge(monitoring::router())
        .merge(onboarding::router())
        .merge(ui::router())
        .merge(ui_agents::router())
        .merge(ui_attachments::router())
        .merge(ui_message_diagnostics::router())
        .merge(ui_channels::router())
        .merge(ui_schedules::router())
        .merge(ui_companies::router())
        .merge(ui_dashboard::router())
        .merge(ui_invites::router())
        .merge(ui_outbox::router())
        .merge(ui_profile::router())
        .merge(ui_tasks::router())
        .merge(ui_team::router())
        .merge(ui_uploads::router())
        .route_layer(middleware::from_fn_with_state(sessions, auth::require_auth))
        .layer(middleware::from_fn(ui_error_feedback));

    Router::new()
        .merge(health::router())
        .merge(assets::router())
        .merge(user::public_router())
        .merge(apple_auth::public_router())
        .merge(webhooks::router())
        .merge(protected)
}

/// How much of a failed answer's body is worth reading back to the reader. Error bodies here are
/// one short sentence; anything longer is a page or a dump, not a message.
const ERROR_BODY_LIMIT: usize = 4 * 1024;

/// Renders a failed `/ui` navigation as a page instead of a wall of plain text.
///
/// [`AppError`] and axum's own extractor rejections both answer in `text/plain`, which a browser
/// shows unstyled and htmx does not swap at all. htmx requests are left exactly as they are --
/// their status and body are what the shared error toast in the page shell reads -- so this only
/// rewrites the case that has no page left to put an alert on: a document navigation.
async fn ui_error_feedback(request: Request<Body>, next: Next) -> Response {
    let navigation = request.method() == Method::GET
        && request.uri().path().starts_with("/ui")
        && !auth::is_htmx_request(request.headers())
        && accepts_html(request.headers());

    let response = next.run(request).await;
    if !navigation || !response.status().is_client_error() && !response.status().is_server_error() {
        return response;
    }
    if is_html(response.headers()) {
        return response;
    }

    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), ERROR_BODY_LIMIT)
        .await
        .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_string())
        .unwrap_or_default();
    let reason = if body.is_empty() {
        status
            .canonical_reason()
            .unwrap_or("The request could not be completed.")
            .to_string()
    } else {
        body
    };

    (status, Html(pages::ui_error_page(status.as_u16(), &reason))).into_response()
}

fn accepts_html(headers: &header::HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|accept| accept.to_str().ok())
        .is_some_and(|accept| accept.contains("text/html") || accept.contains("*/*"))
}

fn is_html(headers: &header::HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|content_type| content_type.to_str().ok())
        .is_some_and(|content_type| content_type.starts_with("text/html"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, http::StatusCode, routing::get};
    use tower::ServiceExt;

    fn app() -> Router {
        Router::new()
            .route(
                "/ui/agents",
                get(|| async { (StatusCode::UNPROCESSABLE_ENTITY, "run_timeout_secs: bad") }),
            )
            .route(
                "/api/companies",
                get(|| async { (StatusCode::NOT_FOUND, "Company not found") }),
            )
            .route("/ui/dashboard", get(|| async { "the dashboard" }))
            .layer(middleware::from_fn(ui_error_feedback))
    }

    async fn body_of(response: Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn a_failed_ui_navigation_answers_with_the_reason_on_a_page() {
        let response = app()
            .oneshot(
                Request::get("/ui/agents")
                    .header(header::ACCEPT, "text/html")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = body_of(response).await;
        assert!(body.contains("alert alert-error"), "{body}");
        assert!(body.contains("run_timeout_secs: bad"), "{body}");
    }

    /// htmx has a page to put the toast on, so its answer must stay exactly what the handler said.
    #[tokio::test]
    async fn an_htmx_failure_is_left_for_the_toast_to_report() {
        let response = app()
            .oneshot(
                Request::get("/ui/agents")
                    .header(header::ACCEPT, "text/html")
                    .header("HX-Request", "true")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body_of(response).await, "run_timeout_secs: bad");
    }

    #[tokio::test]
    async fn an_api_failure_and_a_successful_page_are_both_untouched() {
        let api = app()
            .oneshot(
                Request::get("/api/companies")
                    .header(header::ACCEPT, "*/*")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(api.status(), StatusCode::NOT_FOUND);
        assert_eq!(body_of(api).await, "Company not found");

        let page = app()
            .oneshot(
                Request::get("/ui/dashboard")
                    .header(header::ACCEPT, "text/html")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(page.status(), StatusCode::OK);
        assert_eq!(body_of(page).await, "the dashboard");
    }
}
