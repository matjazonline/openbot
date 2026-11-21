pub(crate) fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Only this escaped HTML constructor can mint the marker. The HTTP boundary
/// still replaces arbitrary non-JSON failures with its redacted problem body.
#[derive(Clone)]
pub(crate) struct SafeError(());
pub(crate) fn error(
    status: axum::http::StatusCode,
    title: &str,
    message: &str,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let mut response=(status,axum::response::Html(format!("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{}</title></head><body><main><h1>{}</h1><p role=\"alert\">{}</p><a href=\"/\">Choose a company</a></main></body></html>",escape(title),escape(title),escape(message)))).into_response();
    response.extensions_mut().insert(SafeError(()));
    response
}
