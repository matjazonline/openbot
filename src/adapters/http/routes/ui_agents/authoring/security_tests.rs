use super::*;
use axum::{
    http::{Request, StatusCode, header},
    middleware,
};
use tower::ServiceExt;

#[test]
fn autonomous_agent_authoring_routes_mount_in_the_authenticated_application_router() {
    let sessions = Arc::new(crate::adapters::http::session::SessionAuthority::for_test(
        &AppConfig::for_test(),
    ));
    let _ = crate::adapters::http::routes::router(sessions);
}

#[tokio::test]
async fn autonomous_agent_authoring_every_publication_write_refuses_cross_origin_cookie_posts() {
    let mut app = Router::new();
    for pattern in publication::WRITE_ROUTES {
        app = app.route(pattern, post(|| async { StatusCode::NO_CONTENT }));
    }
    let app = app.layer(middleware::from_fn_with_state(
        crate::adapters::http::routes::ui::stream_tests::test_config(),
        crate::adapters::http::security::browser_security,
    ));
    for pattern in publication::WRITE_ROUTES {
        let path = pattern
            .replace("{company_id}", &Uuid::new_v4().to_string())
            .replace("{agent_id}", &Uuid::new_v4().to_string());
        let request = |origin: &str| {
            Request::post(&path)
                .header(header::HOST, "localhost")
                .header(header::COOKIE, "session=signed")
                .header(header::ORIGIN, origin)
                .body(axum::body::Body::empty())
                .unwrap()
        };
        assert_eq!(
            app.clone()
                .oneshot(request("https://attacker.test"))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN,
            "{path}"
        );
        assert_eq!(
            app.clone()
                .oneshot(request("http://localhost"))
                .await
                .unwrap()
                .status(),
            StatusCode::NO_CONTENT,
            "{path}"
        );
    }
}
