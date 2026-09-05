//! Removing credentials from anything a run is about to log or store.
//!
//! It lives beside the harness ports rather than inside the `ai-agents` adapter because both
//! sides of the boundary need it: the adapter redacts the compiled config and the provider's
//! response, and the runner redacts the error it records against the task. Two copies of a
//! redaction list is one copy that stops being updated.
//!
//! The patterns are deliberately broader than this platform's own keys. What passes through here
//! is whatever a provider echoed back or a model wrote, so the reason to match `access_token` is
//! not that we ever set one -- it is that something upstream might.

use regex::Regex;
use std::sync::LazyLock;

static URL_KEY_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)([\?&](?:api_key|apikey|key|access_token|token|secret|secret_key|private_key|app_key|app_secret|auth|authorization|password|bearer)=)([^&\s"'`<>\)]+)"#).unwrap()
});

static JSON_KEY_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)("(?:api_key|apikey|access_token|token|secret|secret_key|private_key|app_secret|password)"\s*:\s*")([^"]+)(")"#).unwrap()
});

static YAML_KEY_QUOTED_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(\b(?:api_key|apikey|access_token|token|secret|secret_key|private_key|app_secret|password)\s*:\s*')([^']+)(')"#).unwrap()
});

static YAML_KEY_UNQUOTED_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(\b(?:api_key|apikey|access_token|token|secret|secret_key|private_key|app_secret|password)\s*:\s*)([^\s\n"']+)"#).unwrap()
});

/// `input` with every credential-shaped value replaced, plus `secret_key` itself wherever it
/// appears literally.
///
/// The literal replacement is the one that matters: the compiled agent config carries the
/// company's provider credential in a field this platform chose the name of, and a pattern list
/// alone would only catch it by luck.
pub fn sanitize_text(input: &str, secret_key: Option<&str>) -> String {
    let mut result = URL_KEY_REGEX
        .replace_all(input, "${1}[REDACTED]")
        .into_owned();
    result = JSON_KEY_REGEX
        .replace_all(&result, "${1}[REDACTED]${3}")
        .into_owned();
    result = YAML_KEY_QUOTED_REGEX
        .replace_all(&result, "${1}[REDACTED]${3}")
        .into_owned();
    result = YAML_KEY_UNQUOTED_REGEX
        .replace_all(&result, "${1}[REDACTED]")
        .into_owned();

    if let Some(key) = secret_key {
        let trimmed = key.trim();
        // Short enough to be a fragment of ordinary prose rather than a credential: replacing it
        // would redact the message instead of the key.
        if trimmed.len() >= 4 {
            result = result.replace(trimmed, "[REDACTED]");
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The four shapes a credential arrives in, plus the literal replacement that catches the one
    /// this platform chose the field name of.
    #[test]
    fn a_credential_is_hidden_in_urls_configs_and_prose() {
        let raw_url = "Fetched https://api.service.com/v1/data?key=12345SECRET&other=val and https://other.org/ep?api_key=98765SECRET";
        let sanitized_url = sanitize_text(raw_url, Some("12345SECRET"));
        assert!(!sanitized_url.contains("12345SECRET"));
        assert!(!sanitized_url.contains("98765SECRET"));
        assert!(sanitized_url.contains("key=[REDACTED]"));
        assert!(sanitized_url.contains("api_key=[REDACTED]"));

        let json_config = r#"{"llm": {"provider": "openai", "api_key": "my_secret_key_123"}}"#;
        let sanitized_json = sanitize_text(json_config, Some("my_secret_key_123"));
        assert!(!sanitized_json.contains("my_secret_key_123"));
        assert!(sanitized_json.contains(r#""api_key": "[REDACTED]""#));

        let yaml_config = "llm:\n  provider: google\n  api_key: secret_abc_123\n";
        let sanitized_yaml = sanitize_text(yaml_config, Some("secret_abc_123"));
        assert!(!sanitized_yaml.contains("secret_abc_123"));
        assert!(sanitized_yaml.contains("api_key: [REDACTED]"));

        let msg_with_secret = "The secret token is secret_abc_123.";
        assert_eq!(
            sanitize_text(msg_with_secret, Some("secret_abc_123")),
            "The secret token is [REDACTED]."
        );

        let err_with_url_key = "Failed to fetch URL https://api.service.com/v1/exec?api_key=SECRET_API_KEY_999: HTTP 500 Internal Server Error";
        let sanitized_err = sanitize_text(err_with_url_key, Some("SECRET_API_KEY_999"));
        assert!(!sanitized_err.contains("SECRET_API_KEY_999"));
        assert!(sanitized_err.contains("api_key=[REDACTED]"));

        let err_with_raw_key = "Authentication failed for key sk-proj-SECRET123456789";
        assert_eq!(
            sanitize_text(err_with_raw_key, Some("sk-proj-SECRET123456789")),
            "Authentication failed for key [REDACTED]"
        );
    }

    /// A value too short to be a credential is not replaced: doing so would redact the message
    /// instead of the key.
    #[test]
    fn a_short_secret_is_not_treated_as_one() {
        assert_eq!(
            sanitize_text("a cat sat on the mat", Some("at")),
            "a cat sat on the mat"
        );
        assert_eq!(sanitize_text("nothing to hide", None), "nothing to hide");
    }
}
