use super::*;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InputCheck {
    Deadline,
    Approval,
    Http,
    Message,
}
impl InputCheck {
    /// Called for known literals at compilation and again for all resolved inputs.
    /// Missing inputs are checked by the input schema, never defaulted here.
    pub(crate) fn validate(
        &self,
        inputs: &Value,
        root: &str,
        span: SourceSpan,
    ) -> Result<(), Diagnostic> {
        match self {
            Self::Deadline => {
                if let Some(deadline) = inputs.get("deadline")
                    && deadline
                        .as_str()
                        .is_none_or(|s| chrono::DateTime::parse_from_rfc3339(s).is_err())
                {
                    return Err(error(
                        "registry.deadline",
                        "Use an RFC3339 deadline with a timezone",
                        root,
                        "deadline",
                        span,
                    ));
                }
            }
            Self::Approval => {
                if let Some(approval) = inputs.get("approval") {
                    Self::Deadline.validate(approval, &format!("{root}/approval"), span)?;
                }
            }
            Self::Http => check_http(inputs, root, span)?,
            Self::Message => check_message(inputs, root, span)?,
        }
        Ok(())
    }
}
fn error(
    code: &'static str,
    message: &str,
    root: &str,
    field: &str,
    span: SourceSpan,
) -> Diagnostic {
    Diagnostic::at(
        code,
        message,
        &format!("{root}/{}", field.replace('~', "~0").replace('/', "~1")),
        span,
    )
}
fn check_http(inputs: &Value, root: &str, span: SourceSpan) -> Result<(), Diagnostic> {
    if let Some(path) = inputs.get("path") {
        let valid = path.as_str().is_some_and(|s| {
            !s.is_empty()
                && !s.starts_with("//")
                && !s.contains("://")
                && !s.contains('\\')
                && !s.chars().any(|c| c.is_control() || c.is_whitespace())
                && !s.contains('#')
                && !s.split('/').next().unwrap_or_default().contains(':')
        });
        if !valid {
            return Err(error(
                "registry.http_path",
                "Use a relative HTTP path without an origin, fragment or control characters",
                root,
                "path",
                span,
            ));
        }
    }
    let Some(headers) = inputs.get("headers").and_then(Value::as_object) else {
        return Ok(());
    };
    for (name, value) in headers {
        let forbidden = [
            "authorization",
            "proxy-authorization",
            "cookie",
            "set-cookie",
            "host",
            "x-api-key",
            "api-key",
        ]
        .contains(&name.to_ascii_lowercase().as_str());
        if forbidden || value.as_str().is_some_and(|s| s.contains(['\r', '\n'])) {
            return Err(error(
                "registry.http_header",
                "Credentials, origin overrides and line breaks are forbidden in workflow headers",
                &format!("{root}/headers"),
                name,
                span,
            ));
        }
    }
    Ok(())
}

/// Canonical byte bounds and destination identity, checked for literals at compilation and for
/// every resolved input before the handler runs, so a violation never reaches a marker.
fn check_message(inputs: &Value, root: &str, span: SourceSpan) -> Result<(), Diagnostic> {
    use crate::application::transport::ingress::{MAX_BODY_BYTES, MAX_SUBJECT_BYTES};
    if let Some(content) = inputs.get("content").and_then(Value::as_object) {
        if content
            .get("subject")
            .and_then(Value::as_str)
            .is_some_and(|subject| subject.len() > MAX_SUBJECT_BYTES)
        {
            return Err(error(
                "registry.message_subject",
                "A message subject is at most 998 UTF-8 bytes",
                &format!("{root}/content"),
                "subject",
                span,
            ));
        }
        if content
            .get("body")
            .and_then(Value::as_str)
            .is_some_and(|body| body.len() > MAX_BODY_BYTES)
        {
            return Err(error(
                "registry.message_body",
                "A message body exceeds the canonical byte bound",
                &format!("{root}/content"),
                "body",
                span,
            ));
        }
    }
    let Some(destinations) = inputs.get("destinations").and_then(Value::as_array) else {
        return Ok(());
    };
    let mut seen = BTreeSet::new();
    for destination in destinations {
        let kind = destination.get("kind").and_then(Value::as_str);
        let id = destination
            .get("id")
            .and_then(Value::as_str)
            .and_then(|id| uuid::Uuid::parse_str(id).ok());
        let valid = matches!(kind, Some("user" | "channel")) && id.is_some();
        if !valid || !seen.insert((kind, id)) {
            return Err(error(
                "registry.message_destination",
                "Destinations are distinct users or channels named by UUID",
                root,
                "destinations",
                span,
            ));
        }
    }
    Ok(())
}
