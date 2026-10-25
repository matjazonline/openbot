//! Private thread notes.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

use crate::entities::transport::PrincipalId;

/// Notes are intentionally much smaller than transport messages. They are collaboration context,
/// not an alternate upload path; attachments remain unsupported until their security lifecycle is
/// defined end to end.
pub const MAX_INTERNAL_NOTE_BYTES: usize = 65_536;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InternalNoteProvenance {
    HumanUi,
    Api,
    Integration,
    EmailQuietIngress,
}

impl InternalNoteProvenance {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HumanUi => "human_ui",
            Self::Api => "api",
            Self::Integration => "integration",
            Self::EmailQuietIngress => "email_quiet_ingress",
        }
    }
}

impl FromStr for InternalNoteProvenance {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "human_ui" => Ok(Self::HumanUi),
            "api" => Ok(Self::Api),
            "integration" => Ok(Self::Integration),
            "email_quiet_ingress" => Ok(Self::EmailQuietIngress),
            _ => Err(format!("Unknown internal note provenance: {value}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddInternalNote {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Uuid,
    pub text: String,
    pub command_id: Uuid,
    pub supersedes_note_id: Option<Uuid>,
    pub provenance: InternalNoteProvenance,
}

impl AddInternalNote {
    pub fn validate(&self) -> Result<(), String> {
        let text = self.text.trim();
        if text.is_empty() {
            return Err("An internal note cannot be empty.".into());
        }
        if text.len() > MAX_INTERNAL_NOTE_BYTES {
            return Err(format!(
                "An internal note cannot exceed {MAX_INTERNAL_NOTE_BYTES} bytes."
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TombstoneInternalNote {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Uuid,
    pub note_id: Uuid,
    pub command_id: Uuid,
}

/// Lifecycle metadata for one note as a mailbox renders it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InternalNoteView {
    pub id: Uuid,
    pub author_principal_id: PrincipalId,
    pub provenance: InternalNoteProvenance,
    pub supersedes_note_id: Option<Uuid>,
    pub superseded_by_note_id: Option<Uuid>,
    pub tombstoned_at: Option<DateTime<Utc>>,
}

impl InternalNoteView {
    pub const fn is_active(&self) -> bool {
        self.superseded_by_note_id.is_none() && self.tombstoned_at.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(text: String) -> AddInternalNote {
        AddInternalNote {
            company_id: Uuid::new_v4(),
            channel_id: Uuid::new_v4(),
            thread_id: Uuid::new_v4(),
            text,
            command_id: Uuid::new_v4(),
            supersedes_note_id: None,
            provenance: InternalNoteProvenance::Api,
        }
    }

    #[test]
    fn note_text_is_nonempty_and_bounded_in_bytes() {
        assert!(note(" \n ".into()).validate().is_err());
        assert!(note("a".repeat(MAX_INTERNAL_NOTE_BYTES)).validate().is_ok());
        assert!(
            note("é".repeat(MAX_INTERNAL_NOTE_BYTES / 2 + 1))
                .validate()
                .is_err()
        );
    }
}
