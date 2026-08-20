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
}

/// One embedded PNG, served with the immutable cache headers every brand asset shares (Public).
async fn png(bytes: &'static [u8]) -> impl IntoResponse {
    (
        [
            (CONTENT_TYPE, "image/png"),
            (CACHE_CONTROL, IMMUTABLE_CACHE),
        ],
        bytes,
    )
}
