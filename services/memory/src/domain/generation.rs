//! Observation generation rules (04A, D9), pure: privacy redaction, the provider-safe output
//! schema, the bounded local validation of provider output, and the spend reservation. Labels
//! come from `access_level::derive_label` over trusted inputs only; nothing here reads a level,
//! namespace or session from model output.
use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::OnceLock};

/// The versioned redaction pattern set, recorded with what it produced.
pub const REDACTION_VERSION: i32 = 1;
/// A window holds at most 512 events and 512 KiB of event text (D9).
pub const MAX_WINDOW_BYTES: usize = 512 * 1024;
/// A client-supplied description of the activity: untrusted data, at most 1 KiB.
pub const MAX_DESCRIPTION_BYTES: usize = 1024;
/// At most 16 observations per generation.
pub const MAX_GENERATED_ITEMS: usize = 16;
/// Provider output text above this is malformed without being parsed.
pub const MAX_OUTPUT_BYTES: usize = 256 * 1024;
const TITLE_CHARS: usize = 256;
const TITLE_BYTES: usize = 1024;
const BODY_CHARS: usize = 16_384;
const BODY_BYTES: usize = 64 * 1024;
/// The reservation's fixed allowance for structured-output overhead (D9).
pub const RESERVE_OVERHEAD_TOKENS: u64 = 4_096;
/// Dispatches per operation: a definite rejection (or nothing sent) is retried with backoff
/// until the third, then the operation fails (D9).
pub const MAX_DISPATCH_ATTEMPTS: i32 = 3;
/// The retry backoff base: 30 s × 2ⁿ⁻¹ after the n-th dispatch.
pub const RETRY_BACKOFF_SECS: f64 = 30.0;
/// Generation inputs and raw evidence are kept this long after the operation that used them
/// finished, or after they were recorded when no operation used them (D9, 06A.1).
pub const RETENTION_DAYS: u32 = 30;

/// `2 × prompt bytes + 4,096 + max_output_tokens` (D9): the spend reserved before dispatch.
pub const fn reservation(prompt_bytes: u64, max_output_tokens: u32) -> u64 {
    2 * prompt_bytes + RESERVE_OVERHEAD_TOKENS + max_output_tokens as u64
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redacted {
    pub text: String,
    /// `<private>` spans removed (an unclosed one runs to the end of the text).
    pub private_spans: u32,
    /// Kinds of secrets replaced, in order of replacement.
    pub secrets: Vec<&'static str>,
}

struct Pattern {
    kind: &'static str,
    regex: Regex,
    /// Replacement text; `$1`-style groups keep a label such as `password=` in place.
    replacement: &'static str,
    /// Whether a match really is a secret (prose and earlier markers are not).
    accept: fn(&regex::Captures<'_>) -> bool,
}

fn always(_: &regex::Captures<'_>) -> bool {
    true
}

/// Whether `token` looks like a credential rather than a word: at least `min` bytes with a
/// digit or one of `+ / =`. The patterns never let a token end in `.` or `-`, so sentence
/// punctuation and hyphenated prose ("the basic responsibilities.", "bearer
/// instrumentation-heavy") stay words.
fn shaped(token: &str, min: usize) -> bool {
    token.len() >= min
        && token
            .chars()
            .any(|c| c.is_ascii_digit() || "+/=".contains(c))
}

/// A token after `Bearer`/`Basic` in prose ("the basic configuration") is a word; a
/// credential is at least 16 bytes and credential-shaped.
fn credential_shaped(caps: &regex::Captures<'_>) -> bool {
    shaped(&caps[3], 16)
}

/// The value after an `Authorization:` header name is a credential when credential-shaped
/// ("Authorization: required for admins" is prose).
fn authorization_shaped(caps: &regex::Captures<'_>) -> bool {
    shaped(&caps[2], 8)
}

/// A `password=` value is redacted unless the whole value already is a marker, so the
/// function stays idempotent without letting `token=abc[REDACTED:x]def` through.
fn not_a_marker(caps: &regex::Captures<'_>) -> bool {
    !marker().is_match(&caps[3])
}

fn marker() -> &'static Regex {
    static MARKER: OnceLock<Regex> = OnceLock::new();
    MARKER.get_or_init(|| Regex::new(r"^\[REDACTED:[a-z_]+\]$").expect("marker pattern"))
}

fn patterns() -> &'static [Pattern] {
    static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        let pattern = |kind, regex: &str, replacement, accept| Pattern {
            kind,
            regex: Regex::new(regex).expect("redaction pattern"),
            replacement,
            accept,
        };
        vec![
            pattern(
                "private_key",
                r"(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----.*?(?:-----END [A-Z0-9 ]*PRIVATE KEY-----|\z)",
                "[REDACTED:private_key]",
                always,
            ),
            pattern(
                "api_key",
                r"\b(?:sk-(?:ant-)?[A-Za-z0-9_\-]{16,}|gh[pousr]_[A-Za-z0-9]{20,}|xox[abposr]-[A-Za-z0-9\-]{10,}|AKIA[0-9A-Z]{16}|msk_(?:adm|cli)_[A-Za-z0-9_\-]{8,})",
                "[REDACTED:api_key]",
                always,
            ),
            // A credential-shaped value after an `Authorization:` header name.
            pattern(
                "credential",
                r"(?i)\b(authorization\s*[:=]\s*(?:(?:bearer|basic|token)\s+)?)([A-Za-z0-9._~+/=\-]*[A-Za-z0-9_~+/=])",
                "${1}[REDACTED:credential]",
                authorization_shaped,
            ),
            pattern(
                "credential",
                r"(?i)\b(bearer|basic)(\s+)([A-Za-z0-9._~+/=\-]*[A-Za-z0-9_~+/=])",
                "${1}${2}[REDACTED:credential]",
                credential_shaped,
            ),
            // URL userinfo passwords: `postgres://user:hunter2@db`.
            pattern(
                "secret",
                r"(?i)\b([a-z][a-z0-9+.\-]*://[^\s:/@]+)(:)([^\s@/]+)(@)",
                "${1}${2}[REDACTED:secret]${4}",
                not_a_marker,
            ),
            // `password=`-style pairs, also behind a prefixed key (`DB_PASSWORD=`,
            // `client_secret:`, `OPENAI_API_KEY=`, `AWS_SECRET_ACCESS_KEY=`); the whole key stays.
            // A quoted value takes what follows it up to the next separator, so a second pass
            // finds exactly a marker and changes nothing.
            pattern(
                "secret",
                r#"(?i)\b((?:[a-z0-9]+[_\-])*(?:password|passwd|pwd|secret|secret[_\-]access[_\-]key|api[_\-]?key|access[_\-]?token|auth[_\-]?token|token))(\s*[:=]\s*)("[^"]*"[^\s,;]*|'[^']*'[^\s,;]*|[^\s,;]+)"#,
                "${1}${2}[REDACTED:secret]",
                not_a_marker,
            ),
        ]
    })
}

/// Whether `value` (an identifier: a source ref, revision or event id) holds anything the
/// privacy function would remove. Identifiers are stored and sent as given, so a
/// secret-shaped one is refused rather than rewritten.
pub fn holds_secret(value: &str) -> bool {
    let redacted = redact(value);
    redacted.private_spans > 0 || !redacted.secrets.is_empty()
}

fn private_pattern() -> &'static Regex {
    static PRIVATE: OnceLock<Regex> = OnceLock::new();
    PRIVATE
        .get_or_init(|| Regex::new(r"(?is)<private>.*?(?:</private>|\z)").expect("private pattern"))
}

/// The D9 privacy function: strip `<private>` spans, then replace secrets. It is idempotent,
/// so it runs again on loaded text at the generation boundary.
pub fn redact(text: &str) -> Redacted {
    let private_spans = private_pattern().find_iter(text).count() as u32;
    let mut current = private_pattern().replace_all(text, "").into_owned();
    let mut secrets = Vec::new();
    for pattern in patterns() {
        let mut found = 0usize;
        let replaced = pattern
            .regex
            .replace_all(&current, |caps: &regex::Captures<'_>| {
                if !(pattern.accept)(caps) {
                    return caps[0].to_owned();
                }
                found += 1;
                let mut out = String::new();
                caps.expand(pattern.replacement, &mut out);
                out
            })
            .into_owned();
        secrets.extend(std::iter::repeat_n(pattern.kind, found));
        current = replaced;
    }
    Redacted {
        text: current,
        private_spans,
        secrets,
    }
}

/// The provider-safe output schema (D9): every property required, `additionalProperties: false`,
/// no length, count or numeric constraints, and no level, namespace or session fields.
pub fn provider_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["outcome", "observations", "summary"],
        "properties": {
            "outcome": {"type": "string", "enum": ["save", "nothing_to_save"]},
            "observations": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["kind", "title", "body", "event_ids"],
                    "properties": {
                        "kind": {"type": "string",
                                 "enum": ["fact", "decision", "discovery", "fix", "outcome"]},
                        "title": {"type": "string"},
                        "body": {"type": "string"},
                        "event_ids": {"type": "array", "items": {"type": "string"}}
                    }
                }
            },
            "summary": {"type": ["string", "null"]}
        }
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneratedKind {
    Fact,
    Decision,
    Discovery,
    Fix,
    Outcome,
    /// Never chosen by the model: the session summary.
    #[serde(skip)]
    Summary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedObservation {
    pub kind: GeneratedKind,
    pub title: String,
    pub body: String,
    /// Window event ids, in window order, deduplicated; at least one.
    pub event_ids: Vec<String>,
}

/// A validated provider answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidOutput {
    /// A valid empty result: the window holds nothing worth keeping.
    NothingToSave,
    /// At least one observation, or a summary.
    Save(Vec<GeneratedObservation>),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OutputError {
    #[error("the provider output is empty")]
    Empty,
    #[error("the provider output is malformed: {0}")]
    Malformed(&'static str),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawObservation {
    kind: GeneratedKind,
    title: String,
    body: String,
    event_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOutput {
    outcome: String,
    observations: Vec<RawObservation>,
    summary: Option<String>,
}

fn bounded_text(value: &str, min: usize, chars: usize, bytes: usize) -> bool {
    let count = value.chars().count();
    !value.contains('\0') && count >= min && count <= chars && value.len() <= bytes
}

/// The stricter local validation (D9): the shape (unknown fields, including any level or
/// namespace the model invents, are malformed), at most 16 observations, the D6 title and body
/// bounds, event ids within what was sent, and the summary present exactly when requested.
/// `window` lists the event ids sent, in window order.
pub fn validate_output(
    text: &str,
    window: &[String],
    summary_requested: bool,
) -> Result<ValidOutput, OutputError> {
    if text.trim().is_empty() {
        return Err(OutputError::Empty);
    }
    if text.len() > MAX_OUTPUT_BYTES {
        return Err(OutputError::Malformed("output too large"));
    }
    let raw: RawOutput =
        serde_json::from_str(text).map_err(|_| OutputError::Malformed("not the schema"))?;
    let sent: BTreeSet<&str> = window.iter().map(String::as_str).collect();
    match raw.outcome.as_str() {
        "nothing_to_save" => {
            if raw.observations.is_empty() && raw.summary.is_none() {
                Ok(ValidOutput::NothingToSave)
            } else {
                Err(OutputError::Malformed("nothing_to_save with content"))
            }
        }
        "save" => {
            if raw.observations.len() > MAX_GENERATED_ITEMS {
                return Err(OutputError::Malformed("too many observations"));
            }
            if raw.summary.is_some() != summary_requested {
                return Err(OutputError::Malformed(
                    "summary presence differs from the request",
                ));
            }
            let mut items = Vec::with_capacity(raw.observations.len() + 1);
            for observation in raw.observations {
                if !bounded_text(&observation.title, 1, TITLE_CHARS, TITLE_BYTES)
                    || !bounded_text(&observation.body, 0, BODY_CHARS, BODY_BYTES)
                {
                    return Err(OutputError::Malformed("text out of bounds"));
                }
                let cited: BTreeSet<&str> =
                    observation.event_ids.iter().map(String::as_str).collect();
                if cited.is_empty() || !cited.is_subset(&sent) {
                    return Err(OutputError::Malformed("event ids outside the window"));
                }
                items.push(GeneratedObservation {
                    kind: observation.kind,
                    title: observation.title,
                    body: observation.body,
                    event_ids: window
                        .iter()
                        .filter(|id| cited.contains(id.as_str()))
                        .cloned()
                        .collect(),
                });
            }
            if let Some(summary) = raw.summary {
                if !bounded_text(&summary, 1, BODY_CHARS, BODY_BYTES) {
                    return Err(OutputError::Malformed("summary out of bounds"));
                }
                // A session summary draws on the whole window it was given.
                items.push(GeneratedObservation {
                    kind: GeneratedKind::Summary,
                    title: "Session summary".to_owned(),
                    body: summary,
                    event_ids: window.to_vec(),
                });
            }
            if items.is_empty() {
                return Err(OutputError::Malformed("save without content"));
            }
            Ok(ValidOutput::Save(items))
        }
        _ => Err(OutputError::Malformed("unknown outcome")),
    }
}

#[cfg(test)]
#[path = "generation_tests.rs"]
mod tests;
