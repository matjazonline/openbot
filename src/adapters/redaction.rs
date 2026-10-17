//! One redaction rule for provider responses that may echo an active credential (HTTP
//! actions and MCP tool results): occurrences of the secret in strings, object keys and
//! scalars, plus values under credential-bearing field names.
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};

const MAX_DEPTH: usize = 32;
pub(crate) const REDACTED: &str = "[REDACTED]";

pub(crate) fn redact_text(value: &str, secret: Option<&SecretString>) -> String {
    match secret {
        Some(secret) => value.replace(secret.expose_secret(), REDACTED),
        None => value.to_owned(),
    }
}

fn credential_field(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "authorization" | "cookie" | "set-cookie" | "x-api-key" | "api-key" | "proxy-authorization"
    )
}

/// Redacts in place. Returns false (and leaves a partially redacted value that the caller
/// must discard) when nesting exceeds the bound.
#[must_use]
pub(crate) fn redact_json(value: &mut Value, secret: Option<&SecretString>) -> bool {
    redact_at(value, secret, 0)
}

fn redact_at(value: &mut Value, secret: Option<&SecretString>, depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    match value {
        Value::String(text) => *text = redact_text(text, secret),
        Value::Array(values) => {
            for value in values {
                if !redact_at(value, secret, depth + 1) {
                    return false;
                }
            }
        }
        Value::Object(values) => {
            let old = std::mem::take(values);
            for (name, mut value) in old {
                if credential_field(&name) {
                    value = json!(REDACTED);
                } else if !redact_at(&mut value, secret, depth + 1) {
                    return false;
                }
                values.insert(redact_text(&name, secret), value);
            }
        }
        Value::Number(_) | Value::Bool(_) | Value::Null => {
            // A bearer token can itself be a JSON scalar (for example a numeric PIN).
            // Parsing a provider's echo must not make it escape string redaction.
            if secret.is_some_and(|secret| value.to_string().contains(secret.expose_secret())) {
                *value = json!(REDACTED);
            }
        }
    }
    true
}
