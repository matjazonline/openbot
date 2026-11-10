//! Pure URL construction: this validates syntax and never grants access or fetches the URL.
use super::http::HttpOrigin;
use crate::application::app_error::{AppError, AppResult};
use serde_json::Value;

pub(crate) const MAX_SEGMENTS: usize = 16;
pub(crate) const MAX_SEGMENT_BYTES: usize = 128;
pub(crate) const MAX_URL_BYTES: usize = 8192;

struct UrlSegment<'a>(&'a str);

impl<'a> UrlSegment<'a> {
    fn parse(value: &'a Value) -> AppResult<Self> {
        let text = value.as_str().ok_or_else(invalid)?;
        if text.is_empty()
            || text.len() > MAX_SEGMENT_BYTES
            || matches!(text, "." | "..")
            || !text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
        {
            return Err(invalid());
        }
        Ok(Self(text))
    }
}

pub(crate) fn build(inputs: &Value) -> AppResult<Value> {
    let fields = inputs.as_object().ok_or_else(invalid)?;
    if fields.len() != 2 {
        return Err(invalid());
    }
    let origin = fields
        .get("origin")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?;
    let origin = HttpOrigin::parse(origin).map_err(|_| invalid())?;
    if !origin.as_str().starts_with("https://") {
        return Err(invalid());
    }
    let segments = fields
        .get("segments")
        .and_then(Value::as_array)
        .filter(|items| items.len() <= MAX_SEGMENTS)
        .ok_or_else(invalid)?;
    let mut url = url::Url::parse(origin.as_str()).map_err(|_| invalid())?;
    {
        let mut path = url.path_segments_mut().map_err(|_| invalid())?;
        for value in segments {
            path.push(UrlSegment::parse(value)?.0);
        }
    }
    let url = url.to_string();
    if url.len() > MAX_URL_BYTES {
        return Err(invalid());
    }
    Ok(Value::String(url))
}

fn invalid() -> AppError {
    AppError::BadRequest("Invalid HTTPS URL construction".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn data_url_normalizes_https_origin_without_replacing_authority() {
        for (origin, expected) in [
            ("https://example.test", "https://example.test/c/canvas-a"),
            (
                "HTTPS://EXAMPLE.TEST:443/",
                "https://example.test/c/canvas-a",
            ),
            (
                "https://127.0.0.1:8443",
                "https://127.0.0.1:8443/c/canvas-a",
            ),
            ("https://[::1]:8443", "https://[::1]:8443/c/canvas-a"),
        ] {
            assert_eq!(
                build(&json!({"origin":origin,"segments":["c","canvas-a"]})).unwrap(),
                expected
            );
        }
        assert_eq!(
            build(&json!({"origin":"https://example.test","segments":[]})).unwrap(),
            "https://example.test/"
        );
    }

    #[test]
    fn data_url_refuses_credential_selectors_paths_and_segment_escapes() {
        for origin in [
            "http://example.test",
            "javascript:alert(1)",
            "//example.test",
            "https://user:secret@example.test",
            "https://example.test/mcp",
            "https://example.test/?query=secret",
            "https://example.test/#fragment",
            "https://example.test:65536",
        ] {
            assert!(build(&json!({"origin":origin,"segments":["c"]})).is_err());
        }
        for segment in [
            "",
            ".",
            "..",
            "../x",
            "//evil.test",
            "a/b",
            "a\\b",
            "%2f",
            "a?x",
            "a#x",
            "a\n",
            "é",
        ] {
            assert!(build(&json!({"origin":"https://example.test","segments":[segment]})).is_err());
        }
        assert!(
            build(&json!({"origin":"https://example.test","segments":[],"token":"secret"}))
                .is_err()
        );
    }

    #[test]
    fn data_url_bounds_segments_before_construction() {
        let input = json!({"origin":"https://example.test","segments":vec!["x".repeat(MAX_SEGMENT_BYTES);MAX_SEGMENTS]});
        let value = build(&input).unwrap();
        let url = url::Url::parse(value.as_str().unwrap()).unwrap();
        assert_eq!(url.host_str(), Some("example.test"));
        assert_eq!(url.path_segments().unwrap().count(), MAX_SEGMENTS);
        assert!(value.as_str().unwrap().len() < MAX_URL_BYTES);
        assert!(
            build(&json!({"origin":"https://example.test","segments":vec!["x";MAX_SEGMENTS+1]}))
                .is_err()
        );
        assert!(build(&json!({"origin":"https://example.test","segments":["x".repeat(MAX_SEGMENT_BYTES+1)]})).is_err());
        assert!(
            build(&json!({"origin":format!("https://{}.test", "x".repeat(2048)),"segments":[]}))
                .is_err()
        );
    }
}
