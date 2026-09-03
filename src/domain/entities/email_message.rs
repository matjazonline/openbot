//! The email protocol extension of a canonical message.
//!
//! These are the fields only mail has: the RFC 5322 threading headers, the MAPI `Thread-Index`
//! some clients send instead, and the raw bodies a reply needs in order to strip quoted history.
//! They live here rather than on [`crate::entities::message::Message`] so that a Slack post, a
//! schedule prompt or an agent's answer is a complete message without inventing any of them.
//!
//! Nothing in this module parses email syntax. Adapters own that; this is the validated result.

use serde::{Deserialize, Serialize};

use crate::entities::value_objects::{MessageId, ThreadIndex};

/// The most `References` entries retained for one message.
///
/// The whole array is read into memory to build a threading lookup key, so it is bounded at the
/// boundary rather than trusted. `email_message_metadata_references_check` enforces the same
/// number in the database.
pub const MAX_RETAINED_REFERENCES: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailMessageMetadata {
    pub rfc_message_id: MessageId,
    pub in_reply_to: Option<MessageId>,
    pub references: Vec<MessageId>,
    pub thread_index: Option<ThreadIndex>,
    pub raw_text_body: Option<String>,
    pub raw_html_body: Option<String>,
}

impl EmailMessageMetadata {
    /// The headers of a message that threads onto nothing.
    pub fn new(rfc_message_id: MessageId) -> Self {
        Self {
            rfc_message_id,
            in_reply_to: None,
            references: Vec::new(),
            thread_index: None,
            raw_text_body: None,
            raw_html_body: None,
        }
    }

    pub fn in_reply_to(mut self, in_reply_to: Option<MessageId>) -> Self {
        self.in_reply_to = in_reply_to;
        self
    }

    /// Sets the `References` chain, dropping anything past [`MAX_RETAINED_REFERENCES`].
    ///
    /// Truncated rather than rejected: a client with a pathological chain still has a deliverable
    /// message, and the entries that matter for threading are the ones nearest the root.
    pub fn references(mut self, references: Vec<MessageId>) -> Self {
        let mut references = references;
        references.truncate(MAX_RETAINED_REFERENCES);
        self.references = references;
        self
    }

    pub fn thread_index(mut self, thread_index: Option<ThreadIndex>) -> Self {
        self.thread_index = thread_index;
        self
    }

    pub fn raw_bodies(mut self, text: Option<String>, html: Option<String>) -> Self {
        self.raw_text_body = text;
        self.raw_html_body = html;
        self
    }

    /// The Message-IDs that name *other* messages in this conversation, nearest first.
    ///
    /// This is the lookup key for "which thread does this belong to", built in one place because
    /// assembling it ad hoc at each call site is how the pre-canonical code ended up with four
    /// subtly different copies of it.
    pub fn reference_candidates(&self) -> Vec<MessageId> {
        let mut ids = Vec::with_capacity(self.references.len() + 1);
        if let Some(in_reply_to) = self.in_reply_to.as_ref() {
            ids.push(in_reply_to.clone());
        }
        for reference in &self.references {
            if !ids.contains(reference) {
                ids.push(reference.clone());
            }
        }
        ids
    }

    /// The conversation this message belongs to, as a stable provider key.
    ///
    /// RFC 5322 puts the root of a conversation first in `References`, so a reply and the root it
    /// answers derive the same key -- which is what lets a reply that arrives *before* its root
    /// create the external thread the root then joins, rather than the two starting two
    /// conversations.
    pub fn conversation_root_key(&self) -> &MessageId {
        self.references
            .first()
            .or(self.in_reply_to.as_ref())
            .unwrap_or(&self.rfc_message_id)
    }

    /// The reference candidates plus this message's own id: what locates the thread a newly
    /// arrived message belongs to, including a redelivery of one already stored.
    pub fn thread_lookup_candidates(&self) -> Vec<MessageId> {
        let mut ids = vec![self.rfc_message_id.clone()];
        for candidate in self.reference_candidates() {
            if !ids.contains(&candidate) {
                ids.push(candidate);
            }
        }
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_candidates_lead_with_the_message_itself_then_its_ancestors() {
        let metadata = EmailMessageMetadata::new(MessageId::from("<self@example.com>"))
            .in_reply_to(Some(MessageId::from("<parent@example.com>")))
            .references(vec![
                MessageId::from("<root@example.com>"),
                MessageId::from("<parent@example.com>"),
            ]);

        assert_eq!(
            metadata.thread_lookup_candidates(),
            vec![
                MessageId::from("<self@example.com>"),
                MessageId::from("<parent@example.com>"),
                MessageId::from("<root@example.com>"),
            ],
            "each id appears once, nearest ancestor first"
        );
    }

    /// The invariant reply-before-root depends on: the reply and the root it answers name the
    /// same conversation, so whichever arrives first creates the external thread the other joins.
    #[test]
    fn a_reply_and_its_root_derive_the_same_conversation_key() {
        let root = EmailMessageMetadata::new(MessageId::from("<root@example.com>"));
        let reply = EmailMessageMetadata::new(MessageId::from("<reply@example.com>"))
            .in_reply_to(Some(MessageId::from("<root@example.com>")))
            .references(vec![MessageId::from("<root@example.com>")]);
        let deep_reply = EmailMessageMetadata::new(MessageId::from("<deep@example.com>"))
            .in_reply_to(Some(MessageId::from("<reply@example.com>")))
            .references(vec![
                MessageId::from("<root@example.com>"),
                MessageId::from("<reply@example.com>"),
            ]);

        assert_eq!(root.conversation_root_key().as_str(), "<root@example.com>");
        assert_eq!(reply.conversation_root_key().as_str(), "<root@example.com>");
        assert_eq!(
            deep_reply.conversation_root_key().as_str(),
            "<root@example.com>"
        );
    }

    /// A client that sends `In-Reply-To` and no `References` at all is common enough that falling
    /// back to it is the difference between one conversation and two.
    #[test]
    fn a_reply_without_a_references_chain_falls_back_to_what_it_answers() {
        let reply = EmailMessageMetadata::new(MessageId::from("<reply@example.com>"))
            .in_reply_to(Some(MessageId::from("<root@example.com>")));
        assert_eq!(reply.conversation_root_key().as_str(), "<root@example.com>");
    }

    #[test]
    fn a_pathological_reference_chain_is_truncated_rather_than_stored_whole() {
        let metadata = EmailMessageMetadata::new(MessageId::from("<self@example.com>")).references(
            (0..MAX_RETAINED_REFERENCES + 50)
                .map(|index| MessageId::from(format!("<{index}@example.com>")))
                .collect(),
        );
        assert_eq!(metadata.references.len(), MAX_RETAINED_REFERENCES);
    }
}
