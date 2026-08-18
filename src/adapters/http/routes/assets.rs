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

const LOGO_PNG: &[u8] = include_bytes!("../../../../assets/argo-inbox-logo.png");

/// Immutable because the URL changes whenever the asset does — the file name is part of the build.
const IMMUTABLE_CACHE: &str = "public, max-age=31536000, immutable";

pub fn router() -> Router<AppState> {
    Router::new().route("/assets/argo-inbox-logo.png", get(logo))
}

/// GET /assets/argo-inbox-logo.png - The wordmark shown in the mailbox top bar (Public).
async fn logo() -> impl IntoResponse {
    (
        [
            (CONTENT_TYPE, "image/png"),
            (CACHE_CONTROL, IMMUTABLE_CACHE),
        ],
        LOGO_PNG,
    )
}
