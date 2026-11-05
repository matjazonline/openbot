//! File-level classification and pure library permissions; no transport or database types.
use super::{
    access_level::{AccessLevel, LevelActor},
    unicode_sanitization::is_invisible_or_malicious_unicode,
    value_objects::KnowledgeDocumentName,
};
use serde::Serialize;
use thiserror::Error;

pub const MAX_KNOWLEDGE_FILES: usize = 10;
pub const MAX_KNOWLEDGE_FILE_BYTES: usize = 1024 * 1024;
pub const MAX_COMPANY_KNOWLEDGE_BYTES: usize = 10 * MAX_KNOWLEDGE_FILE_BYTES;
pub const MAX_COMPANY_KNOWLEDGE_LAYERS: usize = 4096;
pub const EMBEDDING_DIMENSIONS: usize = 384;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeFileName {
    level: AccessLevel,
    name: KnowledgeDocumentName,
}
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("`{filename}`: {reason}; use a level 0–9 and `_` or `-`, for example `0_sales.md`")]
pub struct FileNameError {
    filename: String,
    reason: &'static str,
}
impl KnowledgeFileName {
    pub fn parse(raw: &str) -> Result<Self, FileNameError> {
        let malformed = |reason| FileNameError {
            filename: raw.chars().take(255).collect(),
            reason,
        };
        if raw.len() > 4096
            || raw
                .chars()
                .any(|c| c.is_control() || is_invisible_or_malicious_unicode(c))
        {
            return Err(malformed(
                "the filename contains unsupported characters or is too long",
            ));
        }
        let base = raw.rsplit(['/', '\\']).next().unwrap_or(raw).trim();
        let bytes = base.as_bytes();
        if bytes.len() < 3 || !bytes[0].is_ascii_digit() || !matches!(bytes[1], b'_' | b'-') {
            return Err(malformed("the name must start with a level prefix"));
        }
        let name = base[2..].trim();
        if name.is_empty() || name.chars().count() > 253 {
            return Err(malformed("the document name must contain 1–253 characters"));
        }
        Ok(Self {
            level: AccessLevel::try_from(i16::from(bytes[0] - b'0')).expect("one ASCII digit"),
            name: name.into(),
        })
    }
    pub fn level(&self) -> AccessLevel {
        self.level
    }
    pub fn name(&self) -> &KnowledgeDocumentName {
        &self.name
    }
    pub fn prefixed(&self) -> String {
        format!("{}_{}", self.level.as_i16(), self.name)
    }
    pub fn is_supported_text(&self) -> bool {
        self.name.rsplit_once('.').is_some_and(|(_, extension)| {
            extension.eq_ignore_ascii_case("txt") || extension.eq_ignore_ascii_case("md")
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("Only teammates may write documents at or below their own access level")]
pub struct DocumentWriteRefused;
pub fn may_write_document(
    actor: LevelActor,
    level: AccessLevel,
) -> Result<(), DocumentWriteRefused> {
    if actor.membership.is_team() && level <= actor.access_level {
        Ok(())
    } else {
        Err(DocumentWriteRefused)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct KnowledgeDocumentLayer {
    pub id: uuid::Uuid,
    pub name: KnowledgeDocumentName,
    pub access_level: AccessLevel,
    pub revision: i64,
    pub text_bytes: u64,
}
impl KnowledgeDocumentLayer {
    pub fn file_name(&self) -> String {
        format!("{}_{}", self.access_level.as_i16(), self.name)
    }
}

#[cfg(test)]
#[path = "knowledge_tests.rs"]
mod tests;
