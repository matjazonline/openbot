//! One inbound event's identity, carried through every stage that event causes.
//!
//! A message arriving over SMTP, through the SendGrid webhook, or composed in the mailbox is the
//! start of a causal chain: it is stored, it enqueues a task, that task runs an agent, the agent
//! calls tools, and something goes out through the outbox. Each of those stages already writes a
//! durable row, but until now nothing tied the rows together, so answering "what happened to this
//! email?" meant joining five tables by hand and hoping the timestamps lined up.
//!
//! A correlation id is minted once at ingress and copied -- never re-minted -- onto every row and
//! every span downstream. `WHERE correlation_id = $1` then returns the whole trail.
//!
//! It is deliberately *not* the RFC 5322 `Message-ID`: that is chosen by the sender, is not unique
//! in practice, and says nothing about work we started on our own (a schedule firing, an approval
//! resuming). See `src/AGENTS.md`, "Make operations traceable without leaking data".

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The header an inbound message may carry its own correlation id in, and the one outbound mail
/// is stamped with so a reply can be tied back to the exchange that caused it.
///
/// Named like the other `X-MailAgents-*` headers the parser already understands
/// (`x-mailagents-channel-id`, `x-mailagents-hop-count`, `x-mailagents-trace`). Its most useful
/// case is inter-channel: agent A emails agent B, B's reply comes back carrying the same id, and
/// what was two chains reads as one.
pub const CORRELATION_HEADER: &str = "X-MailAgents-Correlation-ID";

/// Deliberately not `Default`. `Default::default()` reads as "empty" or "zero" everywhere else,
/// and a type whose default silently mints a *new* chain would be picked up by `..Default::default()`
/// and by `#[serde(default)]` exactly where inheriting an existing chain was the point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CorrelationId(Uuid);

#[allow(
    clippy::new_without_default,
    reason = "a defaulted correlation id would mint a new chain where inheriting one was meant"
)]
impl CorrelationId {
    /// Mints an id for a chain that starts here. Call this at ingress and nowhere else -- every
    /// stage after it inherits rather than mints, which is the whole point of the type.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Adopts an id a caller supplied, so a chain that began in another system (or in another
    /// channel of this one) stays one chain.
    ///
    /// `None` for anything that is not a UUID: a malformed header is not worth failing an inbound
    /// message over, and the caller mints a fresh id instead.
    pub fn parse(value: &str) -> Option<Self> {
        Uuid::parse_str(value.trim()).ok().map(Self)
    }

    /// Adopts a supplied id, or mints one when there was none to adopt.
    pub fn parse_or_new(value: Option<&str>) -> Self {
        match value.and_then(Self::parse) {
            Some(existing) => existing,
            None => Self::new(),
        }
    }

    /// The wrapped value, for binding to a `UUID` column.
    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl From<Uuid> for CorrelationId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl From<CorrelationId> for Uuid {
    fn from(value: CorrelationId) -> Self {
        value.0
    }
}

impl fmt::Display for CorrelationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_supplied_id_is_adopted_rather_than_replaced() {
        let existing = CorrelationId::new();
        assert_eq!(
            CorrelationId::parse_or_new(Some(&existing.to_string())),
            existing,
            "a chain that arrives with an id must stay one chain"
        );
    }

    #[test]
    fn a_malformed_header_mints_instead_of_failing() {
        assert_eq!(CorrelationId::parse("not-a-uuid"), None);
        // The point is that ingress still gets a usable id.
        assert_ne!(
            CorrelationId::parse_or_new(Some("not-a-uuid")),
            CorrelationId::parse_or_new(Some("not-a-uuid")),
            "each fallback mints its own"
        );
    }

    #[test]
    fn surrounding_whitespace_from_a_header_is_tolerated() {
        let id = CorrelationId::new();
        assert_eq!(
            CorrelationId::parse(&format!("  {id}  ")),
            Some(id),
            "header values arrive with the folding whitespace still attached"
        );
    }
}
