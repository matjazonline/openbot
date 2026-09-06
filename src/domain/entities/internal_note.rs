//! Private thread notes and the explicit commands that make an agent consume them.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

use crate::entities::{message::CanonicalMessageId, transport::PrincipalId};

/// Notes are intentionally much smaller than transport messages. They are collaboration context,
/// not an alternate upload path; attachments remain unsupported until their security lifecycle is
/// defined end to end.
pub const MAX_INTERNAL_NOTE_BYTES: usize = 65_536;
pub const MAX_SELECTED_NOTES: usize = 50;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskOwnerToAct {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Uuid,
    pub task_id: Uuid,
    pub expected_ownership_version: u64,
    pub note_ids: Vec<Uuid>,
    pub command_id: Uuid,
}

impl AskOwnerToAct {
    pub fn validate(&self) -> Result<(), String> {
        if self.expected_ownership_version == 0 || self.expected_ownership_version > i64::MAX as u64
        {
            return Err("The expected ownership version is invalid.".into());
        }
        if self.note_ids.is_empty() {
            return Err("Select at least one active internal note.".into());
        }
        if self.note_ids.len() > MAX_SELECTED_NOTES {
            return Err(format!("Select at most {MAX_SELECTED_NOTES} notes."));
        }
        let mut unique = self.note_ids.clone();
        unique.sort_unstable();
        unique.dedup();
        if unique.len() != self.note_ids.len() {
            return Err("The selected internal notes must be unique.".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartAgentTask {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Uuid,
    pub note_ids: Vec<Uuid>,
    pub command_id: Uuid,
}

impl StartAgentTask {
    pub fn validate(&self) -> Result<(), String> {
        if self.note_ids.is_empty() {
            return Err("Select at least one active internal note.".into());
        }
        if self.note_ids.len() > MAX_SELECTED_NOTES {
            return Err(format!("Select at most {MAX_SELECTED_NOTES} notes."));
        }
        let mut unique = self.note_ids.clone();
        unique.sort_unstable();
        unique.dedup();
        if unique.len() != self.note_ids.len() {
            return Err("The selected internal notes must be unique.".into());
        }
        Ok(())
    }
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

/// A selected note as it crosses the prompt boundary. No transport fields are present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInstructionNote {
    pub note_id: Uuid,
    pub message_id: CanonicalMessageId,
    pub author_display: String,
    pub created_at: DateTime<Utc>,
    pub supersedes_note_id: Option<Uuid>,
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskOwnerOutcome {
    Queued,
    Requeued,
    Parked,
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

    #[test]
    fn selected_notes_are_required_unique_and_bounded() {
        let selected = Uuid::new_v4();
        let mut ask = AskOwnerToAct {
            company_id: Uuid::new_v4(),
            channel_id: Uuid::new_v4(),
            thread_id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            expected_ownership_version: 1,
            note_ids: Vec::new(),
            command_id: Uuid::new_v4(),
        };
        assert!(ask.validate().is_err());
        ask.expected_ownership_version = 0;
        ask.note_ids = vec![selected];
        assert!(ask.validate().is_err());
        ask.expected_ownership_version = 1;
        ask.note_ids = vec![selected, selected];
        assert!(ask.validate().is_err());
        ask.note_ids = (0..=MAX_SELECTED_NOTES).map(|_| Uuid::new_v4()).collect();
        assert!(ask.validate().is_err());
        ask.note_ids.pop();
        assert!(ask.validate().is_ok());

        let start = StartAgentTask {
            company_id: ask.company_id,
            channel_id: ask.channel_id,
            thread_id: ask.thread_id,
            note_ids: ask.note_ids,
            command_id: Uuid::new_v4(),
        };
        assert!(start.validate().is_ok());
    }
}
