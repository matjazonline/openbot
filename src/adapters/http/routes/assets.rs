//! Static brand assets, embedded in the binary.
//!
//! `include_bytes!` keeps the deployed image a single artifact — there is no asset directory to
//! ship or mount at runtime — at the cost of a rebuild whenever an asset changes.

use std::sync::LazyLock;

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

/// Immutable because the URL changes whenever the asset does — a vendored file carries its version
/// in the name, and the first-party bundles carry [`fingerprint`] of their bytes in the query.
const IMMUTABLE_CACHE: &str = "public, max-age=31536000, immutable";

/// A content hash (FNV-1a) that makes [`IMMUTABLE_CACHE`] safe on a fixed asset path.
///
/// `app.css`, `app.js`, and `theme-init.js` keep the same path across builds, so `immutable` alone
/// pins whatever a browser fetched first for a year: a Tailwind rebuild that adds a utility class
/// leaves every returning visitor rendering the new markup against the old stylesheet.
fn fingerprint(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

static APP_CSS_URL: LazyLock<String> =
    LazyLock::new(|| format!("/assets/app.css?v={}", fingerprint(APP_CSS)));
static LOGO_LIGHT_URL: LazyLock<String> = LazyLock::new(|| {
    format!(
        "/assets/busybots-logo-light.png?v={}",
        fingerprint(LOGO_LIGHT_PNG)
    )
});
static APP_JS_URL: LazyLock<String> = LazyLock::new(|| {
    format!(
        "/assets/app.js?v={}",
        fingerprint(crate::adapters::http::pages::application_javascript().as_bytes())
    )
});
static THEME_INIT_JS_URL: LazyLock<String> = LazyLock::new(|| {
    format!(
        "/assets/theme-init.js?v={}",
        fingerprint(crate::adapters::http::pages::theme_init_javascript().as_bytes())
    )
});

/// The `<link>`/`<script>` URLs the page shells must use, fingerprinted so a rebuild invalidates.
pub fn app_css_url() -> &'static str {
    &APP_CSS_URL
}

pub fn logo_light_url() -> &'static str {
    &LOGO_LIGHT_URL
}

pub fn app_js_url() -> &'static str {
    &APP_JS_URL
}

pub fn theme_init_js_url() -> &'static str {
    &THEME_INIT_JS_URL
}

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
        [
            (CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (CACHE_CONTROL, IMMUTABLE_CACHE),
        ],
        crate::adapters::http::pages::application_javascript(),
    )
}

async fn theme_init_javascript() -> impl IntoResponse {
    (
        [
            (CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (CACHE_CONTROL, IMMUTABLE_CACHE),
        ],
        crate::adapters::http::pages::theme_init_javascript(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of the query string: different bytes, different URL.
    #[test]
    fn the_fingerprint_tracks_the_asset_bytes() {
        assert_eq!(fingerprint(b"body{}"), fingerprint(b"body{}"));
        assert_ne!(fingerprint(b"body{}"), fingerprint(b"body{ }"));
    }

    #[test]
    fn first_party_asset_urls_carry_a_version() {
        for url in [
            app_css_url(),
            logo_light_url(),
            app_js_url(),
            theme_init_js_url(),
        ] {
            let (path, version) = url.split_once("?v=").expect("fingerprinted asset url");
            assert!(path.starts_with("/assets/"));
            assert_eq!(version.len(), 16, "{url}");
        }
    }
}
