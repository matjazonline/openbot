//! Frozen-mail recording fixture for application relay policy tests.
use super::*;

/// One inter-channel hop, as the email sender hands it to the relay.
///
/// The relay is reached through [`InternalMailRelay`] with the mail already frozen. The shape this
/// replaces rendered an `OutboundEmail` to RFC 5322 first, purely so the ingress path had
/// something email-shaped to read back -- so a test had to build a full outbound email to exercise
/// a hop budget.
pub(super) struct InternalHop {
    pub(super) source_channel_id: Uuid,
    pub(super) from: EmailAddress,
    pub(super) to: EmailAddress,
    pub(super) subject: String,
    pub(super) body: String,
    pub(super) message_id: MessageId,
    pub(super) in_reply_to: Option<MessageId>,
    pub(super) references: Vec<MessageId>,
    /// The hop count this message *carries*, which is what the sender hands the relay: the
    /// renderer has already added one for the hop the mail itself is.
    pub(super) hop_count: u32,
    pub(super) trace_targets: Vec<crate::entities::address::AddressTarget>,
    pub(super) correlation_id: CorrelationId,
    /// The delivery part the hop claims. These tests commit through a recording double, which
    /// stores the claim without proving it; the stored-part proof is exercised against Postgres.
    pub(super) proof: crate::transport::InternalRelayProof,
}

impl InternalHop {
    pub(super) fn new(
        source_channel_id: Uuid,
        from: &str,
        to: &str,
        subject: &str,
        body: &str,
    ) -> Self {
        Self {
            source_channel_id,
            from: EmailAddress::from(from),
            to: EmailAddress::from(to),
            subject: subject.to_string(),
            body: body.to_string(),
            message_id: MessageId::from(format!("<{}@mailagents.com>", Uuid::new_v4())),
            in_reply_to: None,
            references: Vec::new(),
            hop_count: 0,
            trace_targets: Vec::new(),
            correlation_id: CorrelationId::new(),
            proof: crate::transport::test_support::relay_proof(Uuid::new_v4()),
        }
    }
}

/// Hand one hop to the relay, the way the email sender does before reaching SMTP.
pub(super) async fn relay_hop(
    use_cases: &ThreadUseCases,
    hop: &InternalHop,
) -> crate::transport::RelayDisposition {
    let target = crate::adapters::protocols::email::EmailAddressParser::new("mailagents.com")
        .parse(hop.to.as_str())
        .expect("the test recipient is internal")
        .primary()
        .clone();
    // Freeze this recording fixture's claim with the source desk's actual tenant.
    // PostgreSQL tests prove the delivery part itself; this double only records it.
    let proof = match use_cases
        .channel_persistence
        .get_by_id(hop.source_channel_id)
        .await
        .unwrap()
    {
        Some(source) => crate::transport::InternalRelayProof::claiming(
            source.company_id,
            hop.proof.delivery_id(),
            hop.proof.part_index(),
        ),
        None => hop.proof,
    };
    use_cases
        .relay_internal(&crate::transport::InternalRelayMail {
            target: &target,
            from: &hop.from,
            recipient_to: &hop.to,
            subject: &hop.subject,
            body_text: &hop.body,
            message_id: &hop.message_id,
            in_reply_to: hop.in_reply_to.as_ref(),
            references: &hop.references,
            source_target: crate::entities::address::AddressTarget::Desk(hop.source_channel_id),
            hop_count: hop.hop_count,
            trace: hop.trace_targets.clone(),
            correlation_id: hop.correlation_id,
            proof,
        })
        .await
        .expect("the relay reached a verdict")
}

/// The refusal a relayed hop carries, or `None` when it was accepted.
pub(super) fn refusal(disposition: &crate::transport::RelayDisposition) -> Option<&str> {
    match disposition {
        crate::transport::RelayDisposition::Refused(reason) => Some(reason.as_str()),
        _ => None,
    }
}
