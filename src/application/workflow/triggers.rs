//! The fixed inputs automatic triggers hand a workflow run (p08 D1.2, D1.9). One constructor and
//! one schema each, so admission, binding validation and the reserved `message.reply` wait
//! payload can never disagree about the shape.

use crate::application::transport::ingress::MAX_SUBJECT_BYTES;
use crate::domain::entities::message::CanonicalMessageId;
use serde_json::{Value, json};

/// The body bytes a message-triggered run receives; longer bodies are cut at a char boundary.
pub const MESSAGE_INPUT_BODY_BYTES: usize = 16 * 1024;
/// The sender label bound (an RFC 5321 path is at most 256 octets; labels are shorter).
pub const MESSAGE_INPUT_SENDER_BYTES: usize = 320;
/// The reserved event a reply to a workflow-published message is delivered as (p08 D1.5).
pub const MESSAGE_REPLY_EVENT: &str = "message.reply";

/// What an accepted message gives the run it admits, and the payload of a correlated reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageTriggerInput {
    pub source_message: CanonicalMessageId,
    pub message: String,
    pub subject: String,
    pub sender: String,
}

impl MessageTriggerInput {
    /// Bound every field the way the schema does, cutting at char boundaries.
    pub fn new(
        source_message: CanonicalMessageId,
        body: &str,
        subject: &str,
        sender: &str,
    ) -> Self {
        Self {
            source_message,
            message: truncate(body, MESSAGE_INPUT_BODY_BYTES).to_string(),
            subject: truncate(subject, MAX_SUBJECT_BYTES).to_string(),
            sender: truncate(sender, MESSAGE_INPUT_SENDER_BYTES).to_string(),
        }
    }

    pub fn to_value(&self) -> Value {
        json!({
            "source_message": self.source_message.as_uuid().to_string(),
            "message": self.message,
            "subject": self.subject,
            "sender": self.sender,
        })
    }

    /// The instance configure/activate validate a message-triggered binding's input schema with.
    pub fn representative() -> Value {
        Self::new(
            CanonicalMessageId::new(uuid::Uuid::nil()),
            "A representative message body.",
            "A representative subject",
            "sender@example.com",
        )
        .to_value()
    }

    /// The registry-owned schema; a `message.reply` wait must declare exactly this payload.
    pub fn schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "source_message": {
                    "type": "string",
                    "pattern": "^[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}$"
                },
                "message": {"type": "string", "maxLength": MESSAGE_INPUT_BODY_BYTES},
                "subject": {"type": "string", "maxLength": MAX_SUBJECT_BYTES},
                "sender": {"type": "string", "maxLength": MESSAGE_INPUT_SENDER_BYTES}
            },
            "required": ["source_message", "message", "subject", "sender"],
            "additionalProperties": false
        })
    }
}

/// What a materialized schedule slot gives the run it admits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduleTriggerInput {
    pub scheduled_for: chrono::DateTime<chrono::Utc>,
}

impl ScheduleTriggerInput {
    pub fn to_value(self) -> Value {
        json!({"scheduled_for": self.scheduled_for.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)})
    }

    pub fn representative() -> Value {
        Self {
            scheduled_for: chrono::DateTime::UNIX_EPOCH,
        }
        .to_value()
    }
}

fn truncate(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_trigger_input_is_bounded_at_char_boundaries_and_matches_its_schema() {
        let body = "é".repeat(MESSAGE_INPUT_BODY_BYTES); // two bytes each
        let input = MessageTriggerInput::new(
            CanonicalMessageId::new(uuid::Uuid::new_v4()),
            &body,
            "subject",
            "a@example.com",
        );
        assert!(input.message.len() <= MESSAGE_INPUT_BODY_BYTES);
        assert!(input.message.len() >= MESSAGE_INPUT_BODY_BYTES - 1);
        let schema = jsonschema::validator_for(&MessageTriggerInput::schema()).unwrap();
        assert!(schema.is_valid(&input.to_value()));
        assert!(schema.is_valid(&MessageTriggerInput::representative()));
        let mut extra = input.to_value();
        extra["other"] = json!(1);
        assert!(!schema.is_valid(&extra));
    }

    #[test]
    fn schedule_trigger_input_renders_utc_seconds() {
        assert_eq!(
            ScheduleTriggerInput::representative(),
            json!({"scheduled_for": "1970-01-01T00:00:00Z"})
        );
    }
}
