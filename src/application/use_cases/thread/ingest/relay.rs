//! Relay preparation operates on ingress delivery data. It does not expose stored message views.
//! Typed internal transport ingress, without a MIME round trip.
use super::super::*;
use crate::entities::address::AddressTarget;
#[async_trait]
impl InternalMailRelay for ThreadUseCases {
    async fn relay_internal(&self, mail: &InternalRelayMail<'_>) -> AppResult<RelayDisposition> {
        if mail.hop_count > crate::transport::MAX_INGRESS_HOPS {
            return Ok(RelayDisposition::Refused(
                IngestRejection::HopLimitReached.as_str().into(),
            ));
        }
        if mail.trace.len() > crate::transport::MAX_TRACE_TARGETS {
            return Ok(RelayDisposition::Refused(
                "Relay trace exceeds its hop budget".into(),
            ));
        }
        let (selector, source) = match self.authorize_relay(mail).await {
            Ok(value) => value,
            Err(AppError::Forbidden(reason)) => return Ok(RelayDisposition::Refused(reason)),
            Err(error) => return Err(error),
        };

        let message = self.internal_inbound_message(mail, &selector, source)?;
        let ingest = match self.ingest(message).await {
            Ok(ingest) => ingest,
            // The commit refused the hop's delivery proof. A claim that does not match the stored
            // part reads the same way on every attempt, and nothing was stored: definite, and
            // never handed to SMTP, which would deliver the same mail back as unmarked work.
            Err(AppError::Forbidden(reason)) => return Ok(RelayDisposition::Refused(reason)),
            Err(error) => return Err(error),
        };
        if ingest.accepted {
            return Ok(RelayDisposition::Relayed);
        }
        // A refusal by one of our own channels -- a spent hop budget, a disabled channel, an ACL
        // -- will read the same way on every retry, so it is reported as definite rather than left
        // to burn five attempts reaching the same verdict.
        Ok(RelayDisposition::Refused(
            ingest
                .reason()
                .unwrap_or("The receiving channel refused the message")
                .to_string(),
        ))
    }
}

impl ThreadUseCases {
    /// The canonical inbound message one internal hop produces.
    ///
    /// Every provider key is derived from the relay's own RFC ids, so the receiving channel
    /// deduplicates a repeated hop exactly as it would a repeated delivery from outside.
    fn internal_inbound_message(
        &self,
        mail: &InternalRelayMail<'_>,
        selector: &AddressSelector,
        source: crate::transport::AuthenticatedRelaySource,
    ) -> AppResult<InboundMessage> {
        let metadata = EmailMessageMetadata::new(mail.message_id.clone())
            .in_reply_to(mail.in_reply_to.cloned())
            .references(mail.references.to_vec())
            .raw_bodies(Some(mail.body_text.to_string()), None);
        let author = qualified_email_identity(mail.from.clone())?;
        let recipient_handle = qualified_email_identity(mail.recipient_to.clone())?;

        let quiet_body = crate::entities::channel::strip_quiet_prefix(mail.body_text);
        let disposition = if quiet_body.is_some() {
            MessageDisposition::FileOnly
        } else {
            MessageDisposition::Answer
        };
        let draft = InboundDraft {
            direct_parent_message_key: metadata
                .direct_parent_id()
                .map(external_message_key)
                .transpose()?,
            // No durable inbound event: the relay hands the message over in-process, and this
            // ingest is the only claim on it.
            event_key: None,
            message_key: external_message_key(&metadata.rfc_message_id)?,
            thread_key: external_thread_key(metadata.conversation_root_key())?,
            reply_message_keys: BoundedVec::parse(
                "reply message candidates",
                metadata
                    .reference_candidates()
                    .iter()
                    .map(external_message_key)
                    .collect::<AppResult<Vec<_>>>()?,
            )?,
            reply_thread_keys: BoundedVec::parse(
                "reply thread candidates",
                vec![external_thread_key(metadata.conversation_root_key())?],
            )?,
            author,
            addressed: BoundedVec::parse(
                "addressed identities",
                vec![AddressedIdentity::new(
                    RecipientRole::To,
                    recipient_handle.clone(),
                )],
            )?,
            content: CanonicalContent::parse(mail.subject, quiet_body.unwrap_or(mail.body_text))?,
            attachments: BoundedVec::empty(),
            directives: IngressDirectives {
                hop_count: mail.hop_count,
                trace_targets: BoundedVec::parse("trace channels", mail.trace.clone())?,
                disposition,
                source_target: Some(mail.source_target),
                target_thread_id: None,
                reply_to_message_id: None,
                // An agent answering another agent is machine-generated by construction; the guard
                // exempts the internal path precisely because this is expected here.
                is_auto_reply: true,
                is_forwarded: false,
            },
            // The relay is trusted because of who ran it, not because of a verdict it could
            // fabricate. There is no `AuthVerdict` on this path at all.
            policy: IngressPolicyFacts::TrustedApplication,
            // Carried, never re-minted: agent A's run and agent B's run are one chain.
            correlation_id: mail.correlation_id,
            extension: ProtocolExtension::email(metadata),
        };

        let routing =
            super::single_channel_routing(recipient_handle, selector.clone(), disposition)?;

        Ok(InboundMessage::arriving(
            draft,
            routing,
            IngressOrigin::Internal {
                company_id: source.company_id,
                source_target: source.source_target,
                agent_principal: source.agent_principal,
                proof: mail.proof,
            },
        ))
    }

    /// Everything an internal hop has to be true for before it is ingested on the target channel.
    ///
    /// The sender address is checked against the channel the mail *claims* to come from, so a
    /// relayed message cannot borrow another channel's identity: the ingress guard trusts
    /// [`IngressOrigin::Internal`] absolutely, and this is what earns that trust.
    async fn authorize_relay(
        &self,
        mail: &InternalRelayMail<'_>,
    ) -> AppResult<(AddressSelector, crate::transport::AuthenticatedRelaySource)> {
        let source = self
            .committer
            .authenticate_internal_relay(mail)
            .await?
            .ok_or_else(|| {
                AppError::Forbidden("Internal source proof does not match its delivery".into())
            })?;
        let company = self
            .company_persistence
            .get_by_id(source.company_id)
            .await?
            .ok_or_else(|| AppError::Forbidden("Internal source company was not found".into()))?;
        if !self.relay_sender_matches(mail, &source, &company).await? {
            return Err(AppError::Forbidden(
                "Internal sender address does not match its authority".into(),
            ));
        }
        // The sender and frozen source are authenticated before any recipient lookup.
        if mail
            .target
            .company()
            .is_some_and(|slug| !company.slug.eq_ignore_ascii_case(slug))
        {
            return Err(AppError::Forbidden(
                "Cross-company internal delivery is not allowed".into(),
            ));
        }
        let address = self
            .channel_persistence
            .address_targets(company.id, std::slice::from_ref(mail.target.channel()))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| AppError::Forbidden("Platform address does not exist".into()))?;
        if address.target == source.source_target {
            return Err(AppError::Forbidden(
                "An inbox cannot relay a message to itself".into(),
            ));
        }
        if let AddressTarget::Desk(id) = address.target {
            let target = self
                .channel_persistence
                .get_by_id(id)
                .await?
                .filter(|desk| desk.company_id == company.id && desk.enabled)
                .ok_or_else(|| AppError::Forbidden("Internal target desk is unavailable".into()))?;
            let _ = target;
        }
        Ok((mail.target.clone(), source))
    }

    async fn relay_sender_matches(
        &self,
        mail: &InternalRelayMail<'_>,
        source: &crate::transport::AuthenticatedRelaySource,
        company: &Company,
    ) -> AppResult<bool> {
        let mut targets = Vec::with_capacity(2);
        if let AddressTarget::Desk(_) = source.source_target {
            targets.push(source.source_target);
        }
        if let Some(agent) = source.agent_principal {
            targets.push(AddressTarget::Principal(agent));
        }
        for target in targets {
            if let Some(address) = self
                .channel_persistence
                .address_for_target(company.id, target)
                .await?
            {
                let expected =
                    Channel::address_for(&address.slug, &company.slug, &self.settings.mail_domain);
                if mail.from.eq_ignore_ascii_case(&expected) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}
