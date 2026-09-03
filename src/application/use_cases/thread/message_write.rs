//! What a producer states when it stores a message, and how that message is correlated with the
//! provider that carried it.
//!
//! Deliberately not [`crate::entities::message::Message`]: the stored message has an association
//! id, a canonical id, a resolved author and a participant projection, none of which a producer
//! knows. It states handles and headers; resolving those to principals, identities and provider
//! keys happens inside the one transaction that writes the message.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    entities::{
        correlation::CorrelationId,
        email_message::EmailMessageMetadata,
        message::{AttachmentMetadata, MessageDirection, MessageParticipantKind, MessageRole},
        participant::{IdentityClaimMetadata, IdentityProvenance},
        transport::{PrincipalId, QualifiedIdentity},
    },
    use_cases::participant::IdentityObservation,
};

/// Who a message is attributed to, as its producer states it.
#[derive(Debug, Clone)]
pub enum MessageAuthorWrite {
    /// A transport handle. Resolved to the principal it names -- creating an external principal
    /// for a handle seen for the first time -- in the same transaction as the message, so a
    /// message never lands attributed to nobody.
    Observed(IdentityObservation),
    /// An actor already resolved: an agent answering, or a member acting through the application.
    Principal(PrincipalId),
    /// The platform itself. A schedule that runs for nobody in particular and an approval note are
    /// authored by the company's one system principal rather than by a fabricated mailbox.
    Platform,
}

/// One handle's part in a message. Position is assigned from the order these are given, per kind,
/// so a rendered `To:` header reproduces what the producer stated.
#[derive(Debug, Clone)]
pub struct MessageParticipantWrite {
    pub kind: MessageParticipantKind,
    pub identity: QualifiedIdentity,
}

impl MessageParticipantWrite {
    pub fn new(kind: MessageParticipantKind, identity: QualifiedIdentity) -> Self {
        Self { kind, identity }
    }
}

/// How this message is correlated with the provider that carried it.
///
/// The provider key is what makes redelivery idempotent, so this is stated rather than guessed:
/// a message with no transport behind it says so, instead of being given a synthetic key that a
/// later delivery could collide with.
#[derive(Debug, Clone)]
pub enum MessageCorrelation {
    /// Mail carried it. The RFC `Message-ID` is the provider key, on the channel's canonical email
    /// binding, and the rest of the headers become the message's email extension.
    Email(EmailMessageMetadata),
    /// No transport carried it: a schedule's prompt, an approval note, an agent's answer that is
    /// only ever read in the app.
    Internal,
}

/// One canonical message, and its association with one thread, as its producer states it.
#[derive(Debug, Clone)]
pub struct MessageWrite {
    pub thread_id: Uuid,
    pub author: MessageAuthorWrite,
    pub subject: String,
    pub clean_text_body: String,
    pub attachments: Vec<AttachmentMetadata>,
    pub direction: MessageDirection,
    pub role: MessageRole,
    /// The chain this message belongs to. Inherited from whatever caused it.
    pub correlation_id: CorrelationId,
    pub participants: Vec<MessageParticipantWrite>,
    pub correlation: MessageCorrelation,
    pub created_at: DateTime<Utc>,
}

impl MessageWrite {
    /// A message with no transport behind it, addressed to nobody: the shape a schedule prompt,
    /// an approval note or an in-app answer takes.
    pub fn internal(
        thread_id: Uuid,
        author: MessageAuthorWrite,
        subject: impl Into<String>,
        clean_text_body: impl Into<String>,
        direction: MessageDirection,
        role: MessageRole,
        correlation_id: CorrelationId,
    ) -> Self {
        Self {
            thread_id,
            author,
            subject: subject.into(),
            clean_text_body: clean_text_body.into(),
            attachments: Vec::new(),
            direction,
            role,
            correlation_id,
            participants: Vec::new(),
            correlation: MessageCorrelation::Internal,
            created_at: Utc::now(),
        }
    }

    pub fn with_participants(mut self, participants: Vec<MessageParticipantWrite>) -> Self {
        self.participants = participants;
        self
    }

    pub fn with_attachments(mut self, attachments: Vec<AttachmentMetadata>) -> Self {
        self.attachments = attachments;
        self
    }

    pub fn with_correlation(mut self, correlation: MessageCorrelation) -> Self {
        self.correlation = correlation;
        self
    }

    pub fn created_at(mut self, created_at: DateTime<Utc>) -> Self {
        self.created_at = created_at;
        self
    }

    /// The email headers this message carries, if mail carried it.
    pub fn email_metadata(&self) -> Option<&EmailMessageMetadata> {
        match &self.correlation {
            MessageCorrelation::Email(metadata) => Some(metadata),
            MessageCorrelation::Internal => None,
        }
    }

    /// How one of this message's recipients came to be known.
    ///
    /// A sighting on a message grants nothing; it only fixes which principal every later decision
    /// about that handle will name. The provenance records which path did the sighting, so an
    /// operator auditing an identity can tell an address that arrived in a header from one an
    /// account registered.
    pub fn participant_observation(&self, identity: QualifiedIdentity) -> IdentityObservation {
        IdentityObservation {
            identity,
            display_label: None,
            claim_metadata: IdentityClaimMetadata::observation(),
            provenance: match self.correlation {
                MessageCorrelation::Email(_) => IdentityProvenance::EmailIngress,
                MessageCorrelation::Internal => IdentityProvenance::System,
            },
        }
    }
}
