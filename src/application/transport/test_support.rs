//! Builders for the canonical inbound vocabulary, so a test states the one fact it is about.
//!
//! Every field of an [`InboundEnvelope`] is validated, which is the point -- and would otherwise
//! make "a message from someone, saying something" twenty lines at each call site.

use crate::{
    entities::{
        auth::AuthVerdict,
        correlation::CorrelationId,
        email_message::EmailMessageMetadata,
        transport::{
            ChannelBindingId, ExternalMessageKey, ExternalThreadKey, IdentityNamespace,
            IdentitySubject, QualifiedIdentity, TransportKind,
        },
        value_objects::MessageId,
    },
    transport::{
        AddressedIdentity, BoundedVec, CanonicalContent, EmailIngressFacts, InboundDraft,
        InboundEnvelope, IngressDirectives, IngressPolicyFacts, ProtocolExtension, RecipientRole,
    },
};

/// One email address as the qualified identity the platform stores it as.
pub fn email_identity(address: &str) -> QualifiedIdentity {
    QualifiedIdentity::new(
        TransportKind::Email,
        IdentityNamespace::parse("email").expect("the constant namespace is valid"),
        IdentitySubject::parse(address.trim().to_ascii_lowercase())
            .expect("a test address is a valid subject"),
    )
}

/// A message that arrived over verified mail, with a DMARC pass and nothing else remarkable.
pub fn draft_from(sender: &str, subject: &str, body: &str) -> InboundDraft {
    let rfc_id = MessageId::from(format!("<{}@example.test>", uuid::Uuid::new_v4()));
    let metadata = EmailMessageMetadata::new(rfc_id.clone());
    InboundDraft {
        event_key: None,
        message_key: ExternalMessageKey::parse(rfc_id.as_str()).expect("a valid message key"),
        thread_key: ExternalThreadKey::parse(rfc_id.as_str()).expect("a valid thread key"),
        reply_message_keys: BoundedVec::empty(),
        reply_thread_keys: BoundedVec::empty(),
        author: email_identity(sender),
        addressed: BoundedVec::empty(),
        content: CanonicalContent::parse(subject, body).expect("bounded test content"),
        attachments: BoundedVec::empty(),
        directives: IngressDirectives::default(),
        policy: IngressPolicyFacts::Email(EmailIngressFacts {
            spf: AuthVerdict::Pass,
            dkim: AuthVerdict::Pass,
            dmarc: AuthVerdict::Pass,
            spam_score: None,
        }),
        correlation_id: CorrelationId::new(),
        extension: ProtocolExtension::email(metadata),
    }
}

/// The same message, already bound to an arbitrary interface.
pub fn envelope_from(sender: &str, subject: &str, body: &str) -> InboundEnvelope {
    draft_from(sender, subject, body).bind(ChannelBindingId::random())
}

/// Address a draft to one handle, so a test can exercise the recipient projection.
pub fn addressed_to(mut draft: InboundDraft, role: RecipientRole, address: &str) -> InboundDraft {
    let mut addressed = draft.addressed.into_inner();
    addressed.push(AddressedIdentity::new(role, email_identity(address)));
    draft.addressed = BoundedVec::parse("addressed identities", addressed)
        .expect("a test addresses fewer handles than the bound");
    draft
}
