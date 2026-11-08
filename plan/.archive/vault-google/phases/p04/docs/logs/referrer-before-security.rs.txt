use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{Method, StatusCode, header},
    middleware::Next,
    response::Response,
};

use crate::infra::config::AppConfig;

const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self'; form-action 'self'; object-src 'none'; base-uri 'self'; frame-ancestors 'none'";

pub async fn browser_security(
    State(config): State<Arc<AppConfig>>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    validate_same_origin(&request, config.secure_cookies)?;
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_SECURITY_POLICY, CSP.parse().unwrap());
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
    headers.insert(header::REFERRER_POLICY, "same-origin".parse().unwrap());
    headers.insert(
        "permissions-policy",
        "camera=(), microphone=(), geolocation=()".parse().unwrap(),
    );
    headers.insert("x-frame-options", "DENY".parse().unwrap());
    if config.secure_cookies {
        headers.insert(
            header::STRICT_TRANSPORT_SECURITY,
            "max-age=31536000; includeSubDomains".parse().unwrap(),
        );
    }
    Ok(response)
}

fn validate_same_origin(request: &Request, secure: bool) -> Result<(), StatusCode> {
    if matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
    ) || request.uri().path().starts_with("/webhooks/")
        || request.headers().contains_key(header::AUTHORIZATION)
        || !request.headers().contains_key(header::COOKIE)
    {
        return Ok(());
    }

    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .ok_or(StatusCode::FORBIDDEN)?;
    let expected_scheme = if secure { "https" } else { "http" };
    let expected = url::Url::parse(&format!("{expected_scheme}://{host}"))
        .map_err(|_| StatusCode::FORBIDDEN)?;
    let matches = |value: &str| {
        url::Url::parse(value).is_ok_and(|url| {
            url.scheme() == expected.scheme()
                && url.host_str() == expected.host_str()
                && url.port_or_known_default() == expected.port_or_known_default()
        })
    };

    if let Some(origin) = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    {
        return matches(origin).then_some(()).ok_or(StatusCode::FORBIDDEN);
    }
    request
        .headers()
        .get(header::REFERER)
        .and_then(|value| value.to_str().ok())
        .filter(|referer| matches(referer))
        .map(|_| ())
        .ok_or(StatusCode::FORBIDDEN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;

    #[test]
    fn cookie_mutations_require_an_exact_origin() {
        let request = |origin: Option<&str>| {
            let mut builder = Request::builder()
                .method(Method::POST)
                .uri("/ui/profile")
                .header(header::HOST, "app.example:443")
                .header(header::COOKIE, "session=signed");
            if let Some(origin) = origin {
                builder = builder.header(header::ORIGIN, origin);
            }
            builder.body(Body::empty()).unwrap()
        };
        assert!(validate_same_origin(&request(Some("https://app.example:443")), true).is_ok());
        assert_eq!(
            validate_same_origin(&request(Some("https://evil.example")), true),
            Err(StatusCode::FORBIDDEN)
        );
        assert_eq!(
            validate_same_origin(&request(None), true),
            Err(StatusCode::FORBIDDEN)
        );
    }
}
