//! What one inbound provider message looks like once its adapter is finished with it, and what
//! the application asks persistence to commit for it.
//!
//! Nothing here is email-shaped. An [`InboundEnvelope`] carries qualified identities, canonical
//! content and *typed* policy facts; the fields that only mail has live in
//! [`IngressPolicyFacts::Email`] and [`ProtocolExtension::Email`], so a Slack message is a
//! complete envelope without a DMARC verdict, a spam score or an RFC Message-ID -- and cannot be
//! given a defaulted one by accident.

use uuid::Uuid;

use serde::{Deserialize, Serialize};

use crate::{
    entities::{
        auth::AuthVerdict,
        correlation::CorrelationId,
        email_message::EmailMessageMetadata,
        message::{AttachmentMetadata, CanonicalMessageId},
        transport::{
            ChannelBindingId, ChannelSelector, ExternalEventKey, ExternalMessageKey,
            ExternalThreadKey, InboundEventId, InboundSource, QualifiedIdentity, RecipientRole,
            ReplyMessageKeyCandidate, ReplyThreadKeyCandidate,
        },
        value_objects::{CompanySlug, EmailAddress},
    },
    transport::{
        bounded::{BoundedVec, BoundsError, bounded_text},
        delivery::DeliveryIntent,
        lease::ExecutionLease,
    },
};

/// The longest subject the canonical message will hold. RFC 5322's unfolded-line limit, which is
/// also comfortably more than any chat provider's title.
pub const MAX_SUBJECT_BYTES: usize = 998;

/// The longest canonical body. Larger than any legitimate message people write, and small enough
/// that a worker holding several in memory at once is not a memory-exhaustion path.
pub const MAX_BODY_BYTES: usize = 1_000_000;

/// The most `To`/`Cc`-style identities one message may address.
pub const MAX_ADDRESSED_IDENTITIES: usize = 100;

/// The most attachments one message may carry metadata for.
pub const MAX_ATTACHMENTS: usize = 50;

/// The most candidate keys a message may offer for locating the conversation it belongs to.
pub const MAX_REPLY_CANDIDATES: usize = 100;

/// The most channels one inbound message may be associated with in a single commit.
pub const MAX_THREAD_ASSOCIATIONS: usize = 20;

/// How many times one message may be relayed from one channel of this platform into another before
/// the chain is refused.
///
/// Transport-neutral by nature: any transport whose reply can re-enter another channel's ingress
/// can cycle, so the bound belongs beside the directives that carry the count rather than inside
/// the mail parser that used to own it.
pub const MAX_INGRESS_HOPS: u32 = 5;

/// Inline images below this size are signature decoration rather than content, and are left out of
/// the agent prompt.
///
/// A statement about what a prompt is worth spending tokens on, which is why it is application
/// policy: the mail parser only reports the size it measured.
pub const SMALL_INLINE_IMAGE_BYTES: usize = 10_000;

/// The most recipients one inbound message may name before it is refused.
///
/// Distinct from [`MAX_ADDRESSED_IDENTITIES`], which bounds what an accepted envelope may carry:
/// this bounds what an adapter may allocate while deciding, so an oversized `To:` line is rejected
/// before the addresses are parsed rather than after.
pub const MAX_ADDRESSED_TARGETS: usize = MAX_ADDRESSED_IDENTITIES;

/// One identity a message was addressed to, and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressedIdentity {
    pub role: RecipientRole,
    pub identity: QualifiedIdentity,
}

impl AddressedIdentity {
    pub const fn new(role: RecipientRole, identity: QualifiedIdentity) -> Self {
        Self { role, identity }
    }
}

/// What was said, bounded, with no protocol syntax left in it.
///
/// Constructed only through [`CanonicalContent::parse`], so an oversized subject or body is
/// refused at the adapter boundary rather than at the `INSERT`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalContent {
    subject: String,
    body_text: String,
}

impl CanonicalContent {
    pub fn parse(
        subject: impl Into<String>,
        body_text: impl Into<String>,
    ) -> Result<Self, BoundsError> {
        let subject = subject.into();
        let body_text = body_text.into();
        bounded_text("subject", &subject, MAX_SUBJECT_BYTES)?;
        bounded_text("body", &body_text, MAX_BODY_BYTES)?;
        Ok(Self { subject, body_text })
    }

    pub fn subject(&self) -> &str {
        &self.subject
    }

    pub fn body_text(&self) -> &str {
        &self.body_text
    }
}

/// Whether the agent's reply to a message should actually leave the building.
///
/// A property of the *message*, not of the run, which is why it survives into the durable payload:
/// the worker that eventually answers is a different process from the one that took the message
/// in, and it has no other way to know what was asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplyDelivery {
    /// Answer for real. Every message that arrived over a transport, and every internal hop.
    Send,
    /// Run the agent but keep the answer in the app -- a send the user marked as a test.
    InAppOnly,
}

/// Whether this message asks for an answer at all.
///
/// A `+quiet` address suffix and a `[[quiet]]` body marker both mean "file it on the thread, do
/// not run the agent". Modelled as a disposition rather than an `is_context_only: bool` because
/// the two states have names and a bool at a call site does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageDisposition {
    /// Run the channel's agent and reply.
    Answer,
    /// Record it as context; no agent run, no reply.
    FileOnly,
}

impl MessageDisposition {
    pub const fn answers(self) -> bool {
        matches!(self, Self::Answer)
    }
}

/// Transport-neutral routing facts: loop protection and what the sender asked for.
///
/// Hop count and trace are not email concepts. Any transport that can deliver one channel's reply
/// into another channel's ingress can cycle, so both live on the envelope rather than in the email
/// policy facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngressDirectives {
    pub hop_count: u32,
    pub trace_channels: Vec<Uuid>,
    pub disposition: MessageDisposition,
    /// The channel this message provably came from, when a trusted internal transport carried it.
    pub source_channel_id: Option<Uuid>,
    /// Whether the message announced itself as machine-generated -- a vacation reply, a bounce.
    ///
    /// Here rather than in [`EmailIngressFacts`] because it is a *content* marker, not a verdict:
    /// a transport with no such marker states `false` truthfully, which is exactly what a
    /// fabricated `AuthVerdict::Pass` would not be.
    pub is_auto_reply: bool,
    /// Whether the body is a forwarded conversation rather than one this sender wrote.
    pub is_forwarded: bool,
}

impl Default for IngressDirectives {
    /// The directives of a message that has taken no hops, traced through no channel, asked for an
    /// answer, and came from outside rather than from another channel of this platform.
    ///
    /// Every one of those is the honest value for a first-hop external message rather than a
    /// placeholder -- unlike a defaulted DMARC verdict, which is why the *policy* facts have no
    /// `Default` at all.
    fn default() -> Self {
        Self {
            hop_count: 0,
            trace_channels: Vec::new(),
            disposition: MessageDisposition::Answer,
            source_channel_id: None,
            is_auto_reply: false,
            is_forwarded: false,
        }
    }
}

/// What the email boundary established about a message, as facts rather than as defaults.
///
/// Every field here is meaningless for a transport that is not email, which is why they are inside
/// [`IngressPolicyFacts::Email`]: a Slack message cannot be handed an [`AuthVerdict`] at all, let
/// alone a `Pass` it did not earn.
#[derive(Debug, Clone, PartialEq)]
pub struct EmailIngressFacts {
    pub spf: AuthVerdict,
    pub dkim: AuthVerdict,
    pub dmarc: AuthVerdict,
    /// `None` when no scanner ran, which is not the same as a score of zero.
    pub spam_score: Option<f64>,
}

/// How this message was authenticated, and therefore which policy questions can be asked of it.
#[derive(Debug, Clone, PartialEq)]
pub enum IngressPolicyFacts {
    /// Arrived over SMTP or a mail webhook. The verdicts are the boundary's, never the headers'.
    Email(EmailIngressFacts),
    /// Composed through an authenticated application route -- the mailbox, a simulation run. The
    /// signed-in principal is the authentication; there is no transport verdict to fabricate.
    TrustedApplication,
    /// Arrived from a provider conversation this company has installed and bound. Membership of
    /// that conversation is the disclosure boundary; it is not an authentication verdict.
    InstalledConversation,
}

impl IngressPolicyFacts {
    /// The email boundary's findings, for the decisions only email has.
    ///
    /// Deliberately `Option` rather than a defaulted struct: a caller that needs a DMARC verdict
    /// from a Slack message has a bug, and this is where it surfaces.
    pub const fn email(&self) -> Option<&EmailIngressFacts> {
        match self {
            Self::Email(facts) => Some(facts),
            Self::TrustedApplication | Self::InstalledConversation => None,
        }
    }
}

/// The provider-specific facts that survive alongside the canonical message.
///
/// Versioned and discriminated for the same reason [`crate::entities::message::MessageAttachments`]
/// is: it is persisted JSON read back by a process that may be older or newer than the one that
/// wrote it, so an unknown shape must fail to decode rather than be guessed at.
///
/// [`ProtocolExtension::StoredEvent`] is the escape hatch for providers whose payload is large or
/// sensitive: the bounded raw event is already durable in its own row at ingress, so this names it
/// instead of copying it into anything a task or a queue would carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProtocolExtension {
    Email {
        version: u8,
        metadata: EmailMessageMetadata,
    },
    StoredEvent {
        version: u8,
        binding_id: ChannelBindingId,
        event_key: ExternalEventKey,
    },
    /// Nothing the transport added: a schedule's prompt, an approval note.
    None { version: u8 },
}

impl ProtocolExtension {
    pub const fn email(metadata: EmailMessageMetadata) -> Self {
        Self::Email {
            version: 1,
            metadata,
        }
    }

    pub const fn stored_event(binding_id: ChannelBindingId, event_key: ExternalEventKey) -> Self {
        Self::StoredEvent {
            version: 1,
            binding_id,
            event_key,
        }
    }

    pub const fn none() -> Self {
        Self::None { version: 1 }
    }

    pub const fn email_metadata(&self) -> Option<&EmailMessageMetadata> {
        match self {
            Self::Email { metadata, .. } => Some(metadata),
            Self::StoredEvent { .. } | Self::None { .. } => None,
        }
    }
}

/// The keys that might identify the conversation this message continues.
///
/// Both lists are binding-qualified, because the same provider key text may legitimately occur in
/// another binding and mean another conversation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplyCandidates {
    pub messages: BoundedVec<ReplyMessageKeyCandidate, MAX_REPLY_CANDIDATES>,
    pub threads: BoundedVec<ReplyThreadKeyCandidate, MAX_REPLY_CANDIDATES>,
}

impl ReplyCandidates {
    pub fn parse(
        messages: Vec<ReplyMessageKeyCandidate>,
        threads: Vec<ReplyThreadKeyCandidate>,
    ) -> Result<Self, BoundsError> {
        Ok(Self {
            messages: BoundedVec::parse("reply message candidates", messages)?,
            threads: BoundedVec::parse("reply thread candidates", threads)?,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty() && self.threads.is_empty()
    }
}

/// One inbound message as its adapter parsed it, before the interface that carried it is resolved.
///
/// Email is what forces this split. A mail arrives at the deployment's MX addressed to
/// `support+billing@acme.example.com`, and each of those channels owns its own binding: *which*
/// binding is the source is a routing conclusion, not a parsing one. A transport whose event
/// arrives on one already-known binding -- a Slack event names its installation and conversation --
/// builds the envelope directly and never holds a draft.
///
/// The provider keys here are deliberately unqualified. [`InboundDraft::bind`] is the only way to
/// qualify them, so there is no path that stores a bare provider key as if it were unique on its
/// own.
#[derive(Debug, Clone)]
pub struct InboundDraft {
    pub event_key: Option<ExternalEventKey>,
    pub message_key: ExternalMessageKey,
    pub thread_key: ExternalThreadKey,
    pub reply_message_keys: BoundedVec<ExternalMessageKey, MAX_REPLY_CANDIDATES>,
    pub reply_thread_keys: BoundedVec<ExternalThreadKey, MAX_REPLY_CANDIDATES>,
    pub author: QualifiedIdentity,
    pub addressed: BoundedVec<AddressedIdentity, MAX_ADDRESSED_IDENTITIES>,
    pub content: CanonicalContent,
    pub attachments: BoundedVec<AttachmentMetadata, MAX_ATTACHMENTS>,
    pub directives: IngressDirectives,
    pub policy: IngressPolicyFacts,
    pub correlation_id: CorrelationId,
    pub extension: ProtocolExtension,
}

impl InboundDraft {
    /// Bind this message to the interface that carried it, qualifying every provider key by it.
    pub fn bind(self, binding_id: ChannelBindingId) -> InboundEnvelope {
        let qualify_message = |message_key| ReplyMessageKeyCandidate {
            binding_id,
            message_key,
        };
        let qualify_thread = |thread_key| ReplyThreadKeyCandidate {
            binding_id,
            thread_key,
        };
        InboundEnvelope {
            source: InboundSource {
                binding_id,
                event_key: self.event_key,
                message_key: self.message_key,
                thread_key: self.thread_key,
            },
            author: self.author,
            addressed: self.addressed,
            content: self.content,
            attachments: self.attachments,
            reply_candidates: ReplyCandidates {
                messages: self.reply_message_keys.map(qualify_message),
                threads: self.reply_thread_keys.map(qualify_thread),
            },
            directives: self.directives,
            policy: self.policy,
            correlation_id: self.correlation_id,
            extension: self.extension,
        }
    }
}

/// One inbound provider message, as the application understands it.
///
/// Every field is already validated: the identities are qualified, the content is bounded, the
/// lists cannot exceed their limits, and the policy facts say which questions may be asked. There
/// is deliberately no `Serialize` -- an envelope carries provider-derived content, and the one
/// thing that must never happen to it is being written into a durable task payload. See
/// [`crate::transport::InboundTaskPayloadV1`] for what a task carries instead.
#[derive(Debug, Clone)]
pub struct InboundEnvelope {
    /// Which binding this arrived on, and the provider's keys for the event, message and thread.
    pub source: InboundSource,
    /// Who sent it, qualified by the namespace their subject is unique in.
    pub author: QualifiedIdentity,
    /// Whom it was addressed to, for the transports that address recipients by name. Empty for a
    /// transport that posts into a conversation instead.
    pub addressed: BoundedVec<AddressedIdentity, MAX_ADDRESSED_IDENTITIES>,
    pub content: CanonicalContent,
    pub attachments: BoundedVec<AttachmentMetadata, MAX_ATTACHMENTS>,
    pub reply_candidates: ReplyCandidates,
    pub directives: IngressDirectives,
    pub policy: IngressPolicyFacts,
    /// The chain this message belongs to: adopted when the provider carried one, minted at ingress
    /// when it did not. Never re-minted downstream.
    pub correlation_id: CorrelationId,
    pub extension: ProtocolExtension,
}

impl InboundEnvelope {
    /// The identities addressed in one role, in the order the message carried them.
    pub fn addressed_in(&self, role: RecipientRole) -> impl Iterator<Item = &QualifiedIdentity> {
        self.addressed
            .iter()
            .filter(move |addressed| addressed.role == role)
            .map(|addressed| &addressed.identity)
    }
}

/// A reserved platform address that the platform answers itself instead of routing to a channel.
///
/// The leading underscore is what makes this namespace safe to reserve: `channel_slugs_format` and
/// `companies_slug_format` both constrain slugs to `^[a-z0-9]...`, so no customer can create a
/// channel or company that shadows one of these names. Adding a variant needs no blocklist and no
/// migration.
///
/// Declared with the ingress vocabulary rather than in the mail adapter because *answering* one is
/// application work: the adapter only recognises which reserved name an address carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemAddress {
    /// `_help@{company}.{app_domain}` -- replies with the sender's channels and the address syntax.
    Help,
}

impl SystemAddress {
    /// Every reserved local part, so tests can assert the whole set at once.
    pub const ALL: &'static [SystemAddress] = &[SystemAddress::Help];

    pub const fn local_part(self) -> &'static str {
        match self {
            Self::Help => "_help",
        }
    }

    /// Match a raw local part, exactly. Must be given the local part *before* any pipeline or
    /// context-suffix handling, or a future `_msg` would be eaten by suffix stripping.
    pub fn parse(local_part: &str) -> Option<Self> {
        let candidate = local_part.trim().to_lowercase();
        Self::ALL
            .iter()
            .copied()
            .find(|system| system.local_part() == candidate)
    }
}

/// What one recipient of an inbound message addresses.
///
/// The adapter classifies; the application routes. An email adapter reads
/// `support+billing.quiet@acme.example.com` and states "an ordered two-channel pipeline in `acme`,
/// filed rather than answered"; a chat adapter states the one conversation the post arrived in.
/// Neither hands the application a string to re-parse, which is what keeps address syntax in the
/// one place that owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressedTarget {
    /// An ordered pipeline of business channels, in the order the address named them.
    Channels(Vec<ChannelSelector>),
    /// A reserved platform address that the platform answers itself.
    System {
        company: CompanySlug,
        address: SystemAddress,
    },
    /// Someone with no channel behind them: a person copied on the message.
    Outsider,
}

/// One recipient, as the adapter classified it.
///
/// `handle` is kept because two decisions still speak in it -- whether a Cc'd channel was mentioned
/// in the body, and what a bounce reports -- but nothing routes on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressedRecipient {
    pub role: RecipientRole,
    pub handle: QualifiedIdentity,
    pub target: AddressedTarget,
    /// What this particular address asked for. An address may ask to be filed even when its
    /// siblings ask for an answer, so the disposition is per recipient and folded afterwards.
    pub disposition: MessageDisposition,
}

/// Everything an adapter states about where one inbound message was addressed.
///
/// Separate from [`InboundEnvelope`] because it is a *routing* statement rather than message
/// content: the envelope is what gets stored, this is what decides where.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboundRouting {
    pub recipients: Vec<AddressedRecipient>,
}

impl InboundRouting {
    pub fn parse(recipients: Vec<AddressedRecipient>) -> Result<Self, BoundsError> {
        if recipients.len() > MAX_ADDRESSED_TARGETS {
            return Err(BoundsError::TooMany {
                field: "recipients",
                actual: recipients.len(),
                max: MAX_ADDRESSED_TARGETS,
            });
        }
        Ok(Self { recipients })
    }

    /// Every channel pipeline named, `To:` before `Cc:`, in address order.
    pub fn channel_pipelines(
        &self,
    ) -> impl Iterator<Item = (&AddressedRecipient, &[ChannelSelector])> {
        self.recipients
            .iter()
            .filter_map(|recipient| match &recipient.target {
                AddressedTarget::Channels(selectors) => Some((recipient, selectors.as_slice())),
                AddressedTarget::System { .. } | AddressedTarget::Outsider => None,
            })
    }

    /// Every reserved address named, with the company it belongs to.
    pub fn system_addresses(&self) -> impl Iterator<Item = (&CompanySlug, SystemAddress)> {
        self.recipients
            .iter()
            .filter_map(|recipient| match &recipient.target {
                AddressedTarget::System { company, address } => Some((company, *address)),
                AddressedTarget::Channels(_) | AddressedTarget::Outsider => None,
            })
    }

    /// The handles of everyone addressed who is not a platform interface.
    pub fn outsiders(&self) -> impl Iterator<Item = &QualifiedIdentity> {
        self.recipients.iter().filter_map(|recipient| {
            matches!(recipient.target, AddressedTarget::Outsider).then_some(&recipient.handle)
        })
    }

    /// Whether any address asked for the message to be filed rather than answered.
    pub fn any_files_only(&self) -> bool {
        self.recipients
            .iter()
            .any(|recipient| !recipient.disposition.answers())
    }
}

/// Which thread an association targets: one that exists, or one this commit must open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadTarget {
    Existing(Uuid),
    Create { subject: String },
}

/// Where one match sits in a multi-channel pipeline.
///
/// A named pair rather than two adjacent `usize` parameters, which is the argument-swap bug
/// `src/AGENTS.md` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineStep {
    pub index: usize,
    pub total: usize,
}

impl PipelineStep {
    pub const fn only() -> Self {
        Self { index: 0, total: 1 }
    }
}

/// One thread this message must be associated with, and the interface it reached it through.
///
/// `binding_id` is stated rather than derived inside the commit: one mail addressed to three
/// channels is three bindings, and a committer that resolved them itself would have to know that
/// email's binding is "the channel's one deployment binding" -- an email rule in the one place
/// that must stay transport-neutral.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadAssociation {
    pub channel_id: Uuid,
    pub binding_id: ChannelBindingId,
    pub target: ThreadTarget,
    pub role: RecipientRole,
    pub step: PipelineStep,
    /// The handles this message adds to the thread's participant projection.
    ///
    /// Stated rather than derived inside the commit: whether the sender joins at all, and whether
    /// the outsiders they copied are pulled in, is channel policy -- `add_3rd_party` and the
    /// sender's trust -- and a transaction is not where policy is decided. For a thread this
    /// commit opens, it is the complete starting set.
    pub participants: Vec<EmailAddress>,
}

/// One channel an agent-dispatch run drives.
///
/// Named by channel rather than by thread because the thread may not exist yet: a message that
/// opens a conversation has its thread created by the same commit that creates this task, and the
/// committer resolves the pair from the association it just wrote. A target with no association is
/// therefore unrepresentable rather than merely unlikely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InboundTaskTarget {
    pub channel_id: Uuid,
    pub role: RecipientRole,
}

/// The agent-dispatch task this message should create, if any.
///
/// It names the channels the run drives, first one primary; the canonical message id is not known
/// until the commit assigns one, which is exactly why the committer -- not the caller -- builds the
/// durable payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundTaskRequest {
    pub task_type: String,
    /// In pipeline order. The first is the run's own channel and thread.
    pub targets: Vec<InboundTaskTarget>,
}

impl InboundTaskRequest {
    pub fn primary(&self) -> Option<InboundTaskTarget> {
        self.targets.first().copied()
    }
}

/// Everything one accepted inbound message must make durable, together or not at all.
///
/// The whole point of one named struct is that these rows agree: a canonical message visible
/// without its provider mapping is a message a redelivery will duplicate, and one visible without
/// its task is a message no agent will ever answer.
#[derive(Debug, Clone)]
pub struct InboundCommitRequest {
    pub company_id: Uuid,
    pub envelope: InboundEnvelope,
    /// The inbound-event row this commit completes, for a transport whose ingress is claimed from
    /// a durable inbox. `None` for direct ingress, which has nothing to fence.
    pub claimed_event: Option<ExecutionLease<InboundEventId>>,
    pub associations: BoundedVec<ThreadAssociation, MAX_THREAD_ASSOCIATIONS>,
    pub task: Option<InboundTaskRequest>,
    pub deliveries: Vec<DeliveryIntent>,
    /// Whether the answer this message earns should actually be sent.
    ///
    /// Part of the commit because it has to reach the worker, and the durable task payload is
    /// written here: a run that had to guess would either mail a user's test or swallow a real
    /// reply.
    pub reply_delivery: ReplyDelivery,
}

/// Whether this commit stored a new message or recognised one already stored.
///
/// A redelivery is not an error and not a second message: it returns the identifiers of the first
/// delivery and enqueues nothing further.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitDisposition {
    Created,
    Duplicate,
}

impl CommitDisposition {
    pub const fn is_duplicate(self) -> bool {
        matches!(self, Self::Duplicate)
    }
}

/// What the commit made durable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundCommitOutcome {
    pub disposition: CommitDisposition,
    pub message_id: CanonicalMessageId,
    /// The threads the message is now associated with, in association order.
    pub thread_ids: Vec<Uuid>,
    pub task_id: Option<Uuid>,
    /// The deliveries this commit created. Empty for a duplicate, which must not fan out twice.
    pub delivery_ids: Vec<crate::entities::transport::DeliveryId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_is_refused_rather_than_truncated_when_it_exceeds_its_bound() {
        assert!(CanonicalContent::parse("subject", "body").is_ok());
        assert!(CanonicalContent::parse("s".repeat(MAX_SUBJECT_BYTES), "body").is_ok());
        assert!(CanonicalContent::parse("s".repeat(MAX_SUBJECT_BYTES + 1), "body").is_err());
        assert!(CanonicalContent::parse("subject", "b".repeat(MAX_BODY_BYTES + 1)).is_err());
    }

    /// The reason the facts are an enum: there is no way to spell "this Slack message passed
    /// DMARC", because there is no way to attach a verdict to anything but the email variant.
    #[test]
    fn only_email_carries_authentication_verdicts() {
        let email = IngressPolicyFacts::Email(EmailIngressFacts {
            spf: AuthVerdict::Pass,
            dkim: AuthVerdict::Pass,
            dmarc: AuthVerdict::Pass,
            spam_score: None,
        });
        assert_eq!(
            email.email().map(|facts| facts.dmarc),
            Some(AuthVerdict::Pass)
        );
        assert!(IngressPolicyFacts::TrustedApplication.email().is_none());
        assert!(IngressPolicyFacts::InstalledConversation.email().is_none());
    }

    #[test]
    fn a_protocol_extension_decodes_only_from_a_shape_it_knows() {
        let stored = ProtocolExtension::email(EmailMessageMetadata::new(
            crate::entities::value_objects::MessageId::from("<a@example.com>"),
        ));
        let encoded = serde_json::to_value(&stored).unwrap();
        assert_eq!(encoded["kind"], "email");
        assert_eq!(encoded["version"], 1);
        assert_eq!(
            serde_json::from_value::<ProtocolExtension>(encoded).unwrap(),
            stored
        );

        let unknown = serde_json::json!({ "kind": "carrier_pigeon", "version": 1 });
        assert!(serde_json::from_value::<ProtocolExtension>(unknown).is_err());
    }
}
