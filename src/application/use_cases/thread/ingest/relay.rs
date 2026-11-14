//! Relay preparation operates on ingress delivery data. It does not expose stored message views.
//! Typed internal transport ingress, without a MIME round trip.
use super::super::*;
struct RelaySource {
    channel: Channel,
    company: Company,
}
#[async_trait]
impl InternalMailRelay for ThreadUseCases {
    async fn relay_internal(&self, mail: &InternalRelayMail<'_>) -> AppResult<RelayDisposition> {
        // A recipient outside this deployment, or one whose channel this source may not address,
        // is not a fault: it is the ordinary case, and the sender posts it over SMTP.
        let Some((selector, company_id)) = self.authorize_relay(mail).await? else {
            return Ok(RelayDisposition::NotInternal);
        };

        let message = self.internal_inbound_message(mail, &selector, company_id)?;
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
    async fn resolve_internal_destination(
        &self,
        source_channel_id: Uuid,
        selector: &ChannelSelector,
    ) -> AppResult<Option<RelaySource>> {
        let source = self
            .channel_persistence
            .get_by_id(source_channel_id)
            .await?
            .ok_or_else(|| AppError::Internal("Internal source channel was not found".into()))?;
        let target = match selector {
            ChannelSelector::CurrentCompany(channel_slug) => self
                .channel_persistence
                .list_by_company_id(source.company_id)
                .await?
                .into_iter()
                .find(|channel| channel.matches_slug(channel_slug)),
            ChannelSelector::Qualified { company, channel } => {
                self.channel_persistence
                    .get_by_company_slug_and_channel_slug(company, channel)
                    .await?
            }
        }
        .ok_or_else(|| {
            AppError::Internal(format!("Platform channel does not exist: {selector}"))
        })?;
        if source.company_id != target.company_id {
            return Err(AppError::Internal(
                "Cross-company internal channel delivery is not allowed".into(),
            ));
        }
        if source.id == target.id {
            return Err(AppError::Internal(
                "A channel cannot deliver an internal message to itself".into(),
            ));
        }
        if !target.enabled {
            return Err(AppError::Internal(format!(
                "Target channel '{}' is disabled",
                target.slug
            )));
        }
        let company = self
            .company_persistence
            .get_by_id(source.company_id)
            .await?
            .ok_or_else(|| AppError::Internal("Internal source company was not found".into()))?;
        if selector
            .company()
            .is_some_and(|selected| !company.slug.eq_ignore_ascii_case(selected))
        {
            return Err(AppError::Internal(
                "Internal recipient company does not match the source company".into(),
            ));
        }
        Ok(Some(RelaySource {
            channel: source,
            company,
        }))
    }

    /// The canonical inbound message one internal hop produces.
    ///
    /// Every provider key is derived from the relay's own RFC ids, so the receiving channel
    /// deduplicates a repeated hop exactly as it would a repeated delivery from outside.
    fn internal_inbound_message(
        &self,
        mail: &InternalRelayMail<'_>,
        selector: &ChannelSelector,
        company_id: Uuid,
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
                trace_channels: BoundedVec::parse("trace channels", mail.trace.clone())?,
                disposition,
                source_channel_id: Some(mail.source_channel_id),
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
            IngressOrigin::InternalChannel {
                company_id,
                channel_id: mail.source_channel_id,
                proof: mail.proof,
            },
        ))
    }

    /// Everything an internal hop has to be true for before it is ingested on the target channel.
    ///
    /// The sender address is checked against the channel the mail *claims* to come from, so a
    /// relayed message cannot borrow another channel's identity: the ingress guard trusts
    /// [`IngressOrigin::InternalChannel`] absolutely, and this is what earns that trust.
    async fn authorize_relay(
        &self,
        mail: &InternalRelayMail<'_>,
    ) -> AppResult<Option<(ChannelSelector, Uuid)>> {
        let Some(RelaySource {
            channel: source,
            company,
        }) = self
            .resolve_internal_destination(mail.source_channel_id, mail.target)
            .await?
        else {
            return Ok(None);
        };
        let expected_sender =
            Channel::address_for(&source.slug, &company.slug, &self.settings.mail_domain);
        if !mail.from.eq_ignore_ascii_case(&expected_sender) {
            return Err(AppError::Internal(
                "Internal sender address does not match its source channel".into(),
            ));
        }
        Ok(Some((mail.target.clone(), company.id)))
    }
}
