//! Static brand assets, embedded in the binary.
//!
//! `include_bytes!` keeps the deployed image a single artifact — there is no asset directory to
//! ship or mount at runtime — at the cost of a rebuild whenever an asset changes.

use axum::{
    Router,
    http::header::{CACHE_CONTROL, CONTENT_TYPE},
    response::IntoResponse,
    routing::get,
};

use crate::adapters::http::app_state::AppState;

/// The dark-ink wordmark, for a light background.
const LOGO_DARK_PNG: &[u8] = include_bytes!("../../../../assets/busybots-logo-dark-hor.png");

/// The light-ink wordmark, for a dark background.
const LOGO_LIGHT_PNG: &[u8] = include_bytes!("../../../../assets/busybots-logo-light.png");
const APP_CSS: &[u8] = include_bytes!("../../../../assets/app.css");
const HTMX_JS: &[u8] = include_bytes!("../../../../assets/htmx-2.0.4.min.js");
const HTMX_SSE_JS: &[u8] = include_bytes!("../../../../assets/htmx-ext-sse-2.2.3.js");

/// Immutable because the URL changes whenever the asset does — the file name is part of the build.
const IMMUTABLE_CACHE: &str = "public, max-age=31536000, immutable";

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/assets/busybots-logo-dark-hor.png",
            get(|| png(LOGO_DARK_PNG)),
        )
        .route(
            "/assets/busybots-logo-light.png",
            get(|| png(LOGO_LIGHT_PNG)),
        )
        .route(
            "/assets/app.css",
            get(|| asset("text/css; charset=utf-8", APP_CSS)),
        )
        .route(
            "/assets/htmx-2.0.4.min.js",
            get(|| asset("text/javascript; charset=utf-8", HTMX_JS)),
        )
        .route(
            "/assets/htmx-ext-sse-2.2.3.js",
            get(|| asset("text/javascript; charset=utf-8", HTMX_SSE_JS)),
        )
        .route("/assets/app.js", get(app_javascript))
        .route("/assets/theme-init.js", get(theme_init_javascript))
}

/// One embedded PNG, served with the immutable cache headers every brand asset shares (Public).
async fn png(bytes: &'static [u8]) -> impl IntoResponse {
    asset("image/png", bytes).await
}

async fn asset(content_type: &'static str, bytes: &'static [u8]) -> impl IntoResponse {
    (
        [
            (CONTENT_TYPE, content_type),
            (CACHE_CONTROL, IMMUTABLE_CACHE),
        ],
        bytes,
    )
}

async fn app_javascript() -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "text/javascript; charset=utf-8")],
        crate::adapters::http::pages::application_javascript(),
    )
}

async fn theme_init_javascript() -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "text/javascript; charset=utf-8")],
        crate::adapters::http::pages::theme_init_javascript(),
    )
}
