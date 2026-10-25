//! Which accepted messages a binding admits (p08 D1.2). Eligibility belongs to the binding's
//! configuration, never to channel execution settings or a hidden dispatch branch.

use crate::domain::entities::transport::RecipientRole;
use std::fmt;
use std::str::FromStr;

/// The most bindings with a message trigger one channel may have active at once, so the
/// admission work one accepted message causes stays bounded (p08 D1.2).
pub const MAX_MESSAGE_BINDINGS_PER_CHANNEL: i64 = 8;

/// A binding's message admission rule. A binding without one is never admitted by messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageTrigger {
    /// Addressed in `To`, or mentioned.
    Direct,
    /// Addressed in `To` or `Cc`, or mentioned.
    Any,
    /// Mentioned by name, alias, address or handle.
    Mentioned,
    /// Mentioned, or a direct reply to a message a workflow run published in the thread.
    MentionedOrReply,
}

/// What ingress established about one accepted message on one channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageFacts {
    pub role: RecipientRole,
    pub mentioned: bool,
    pub replies_to_workflow: bool,
}

impl MessageTrigger {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Any => "any",
            Self::Mentioned => "mentioned",
            Self::MentionedOrReply => "mentioned_or_reply",
        }
    }

    pub const fn admits(self, facts: MessageFacts) -> bool {
        match self {
            Self::Direct => matches!(facts.role, RecipientRole::To) || facts.mentioned,
            Self::Any => true,
            Self::Mentioned => facts.mentioned,
            Self::MentionedOrReply => facts.mentioned || facts.replies_to_workflow,
        }
    }
}

impl fmt::Display for MessageTrigger {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for MessageTrigger {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "direct" => Ok(Self::Direct),
            "any" => Ok(Self::Any),
            "mentioned" => Ok(Self::Mentioned),
            "mentioned_or_reply" => Ok(Self::MentionedOrReply),
            other => Err(format!("unknown message trigger {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(role: RecipientRole, mentioned: bool, replies_to_workflow: bool) -> MessageFacts {
        MessageFacts {
            role,
            mentioned,
            replies_to_workflow,
        }
    }

    #[test]
    fn message_trigger_admission_matrix() {
        use MessageTrigger::*;
        use RecipientRole::{Cc, To};
        // (trigger, role, mentioned, replies_to_workflow, admitted)
        let cases = [
            (Direct, To, false, false, true),
            (Direct, Cc, false, false, false),
            (Direct, Cc, true, false, true),
            (Direct, Cc, false, true, false),
            (Any, To, false, false, true),
            (Any, Cc, false, false, true),
            (Mentioned, To, false, false, false),
            (Mentioned, Cc, true, false, true),
            (Mentioned, To, false, true, false),
            (MentionedOrReply, To, false, false, false),
            (MentionedOrReply, Cc, false, true, true),
            (MentionedOrReply, Cc, true, false, true),
        ];
        for (trigger, role, mentioned, reply, admitted) in cases {
            assert_eq!(
                trigger.admits(facts(role, mentioned, reply)),
                admitted,
                "{trigger} {role:?} mentioned={mentioned} reply={reply}"
            );
        }
    }

    #[test]
    fn message_trigger_round_trips_its_stored_name() {
        for trigger in [
            MessageTrigger::Direct,
            MessageTrigger::Any,
            MessageTrigger::Mentioned,
            MessageTrigger::MentionedOrReply,
        ] {
            assert_eq!(trigger.as_str().parse::<MessageTrigger>(), Ok(trigger));
        }
        assert!("always".parse::<MessageTrigger>().is_err());
    }
}
