use super::{Diagnostic, SourceSpan, parse_workflow};
use crate::domain::workflow::{VersionId, WorkflowId};

pub const MAX_SOURCE_BYTES: usize = 262_144;

/// Trusted bounded decoder boundary. Implementations preserve exact source and
/// byte spans and reject unsupported YAML, aliases, duplicate keys and budgets.
pub trait SourceDecoder: Send + Sync {
    fn decode(&self, source: &str) -> Result<super::DecodedSource, Diagnostic>;
}

/// Change only the located root scalar, then decode and type-check the result.
/// Child pins and identical text in literals/comments are never rewritten.
pub fn rebase_workflow_id(
    decoder: &impl SourceDecoder,
    source: &str,
    destination: WorkflowId,
) -> Result<String, Diagnostic> {
    let decoded = decoder.decode(source)?;
    let version = VersionId::new(uuid::Uuid::nil());
    let parsed = parse_workflow(&decoded, version)?;
    let span = parsed.locations["/workflow_id"];
    if destination.as_uuid().is_nil() || destination == parsed.definition.workflow_id {
        return Err(error(
            "copy.identity",
            "A fresh non-nil workflow identity is required",
            span,
        ));
    }
    let scalar = source.get(span.start..span.end).ok_or_else(|| {
        error(
            "copy.span",
            "Decoder returned an invalid scalar byte span",
            span,
        )
    })?;
    // Preserve ordinary quoted scalar style; unusual YAML scalar forms are
    // replaced as one complete scalar, never by searching for UUID text.
    let mut replacement = match scalar.as_bytes().first() {
        Some(b'\'') => format!("'{destination}'"),
        Some(b'"') => format!("\"{destination}\""),
        _ => destination.to_string(),
    };
    // Block-scalar spans include their final line break, while their YAML
    // indicator and indentation precede the span. Keep that token separator.
    if scalar.ends_with("\r\n") {
        replacement.push_str("\r\n");
    } else if scalar.ends_with('\n') {
        replacement.push('\n');
    }
    let size = source.len() - scalar.len() + replacement.len();
    if size > MAX_SOURCE_BYTES {
        return Err(error(
            "source.limit",
            "Rebased source exceeds 256 KiB",
            span,
        ));
    }
    let mut result = String::with_capacity(size);
    result.push_str(&source[..span.start]);
    result.push_str(&replacement);
    result.push_str(&source[span.end..]);
    let checked = decoder.decode(&result)?;
    if parse_workflow(&checked, version)?.definition.workflow_id != destination {
        return Err(error(
            "copy.identity",
            "Rebased root identity does not match destination",
            span,
        ));
    }
    Ok(result)
}

fn error(code: &'static str, message: &str, span: SourceSpan) -> Diagnostic {
    Diagnostic::at(code, message, "/workflow_id", span)
}
