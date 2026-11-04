//! Trusted canonical ingress; transport syntax remains behind the renderer port.
use super::super::*;
use crate::transport::ingress::MAX_REPLY_CANDIDATES;
struct Target {
    company: Company,
    channel: Channel,
    thread: Option<Thread>,
}
struct EmailHeaders {
    message: ExternalMessageKey,
    thread: ExternalThreadKey,
    replies: BoundedVec<ExternalMessageKey, MAX_REPLY_CANDIDATES>,
    direct_parent: Option<ExternalMessageKey>,
    extension: ProtocolExtension,
}
impl ThreadUseCases {
    /// Validate tenant/conversation scope and ingest the canonical message through the same commit.
    pub async fn ingest_canonical(
        &self,
        ingress: CanonicalMessageIngress,
    ) -> AppResult<InboundIngestResult> {
        // Box lookup state at the phase boundary rather than growing the downstream ingest frame.
        let Some(target) = Box::pin(self.canonical_target(&ingress)).await? else {
            return Ok(InboundIngestResult::rejected(
                IngestRejection::UnknownRecipient,
            ));
        };
        let target_thread_id = target
            .thread
            .as_ref()
            .map(|thread| thread.id)
            .or(ingress.target_thread_id);
        let directives = canonical_directives(&ingress, target_thread_id);
        let subject = canonical_subject(ingress.subject, target.thread.as_ref());
        let parent = self
            .resolve_in_reply_to_rfc(
                target.company.id,
                target_thread_id,
                ingress.reply_to_message_id,
            )
            .await?;
        let headers = self.canonical_email_headers(
            &target.company.slug,
            parent.as_ref(),
            &ingress.text_body,
        )?;
        let address = target
            .channel
            .inbound_address(&target.company.slug, &self.settings.mail_domain);
        let recipient = qualified_email_identity(address)?;
        let selector = ChannelSelector::Qualified {
            company: target.company.slug,
            channel: target.channel.slug,
        };
        let routing =
            super::single_channel_routing(recipient.clone(), selector, ingress.disposition)?;
        let draft = InboundDraft {
            direct_parent_message_key: headers.direct_parent,
            event_key: None,
            message_key: headers.message,
            thread_key: headers.thread,
            reply_message_keys: headers.replies,
            reply_thread_keys: BoundedVec::empty(),
            author: ingress.author,
            addressed: BoundedVec::parse(
                "addressed identities",
                vec![AddressedIdentity::new(RecipientRole::To, recipient)],
            )?,
            content: CanonicalContent::parse(subject, ingress.text_body)?,
            attachments: BoundedVec::parse("attachments", ingress.attachments)?,
            directives,
            policy: IngressPolicyFacts::TrustedApplication,
            correlation_id: ingress.correlation_id.unwrap_or_else(CorrelationId::new),
            extension: headers.extension,
        };
        let message = InboundMessage {
            draft,
            routing,
            origin: ingress.origin,
            reply_delivery: ingress.reply_delivery,
            unusable_hints: Vec::new(),
        };
        self.ingest(message).await
    }
    async fn canonical_target(
        &self,
        ingress: &CanonicalMessageIngress,
    ) -> AppResult<Option<Target>> {
        let company = self
            .company_persistence
            .get_by_id(ingress.company_id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!("Company {} not found", ingress.company_id))
            })?;
        let channel = self
            .channel_persistence
            .get_by_id(ingress.channel_id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "Channel {} not found in company {}",
                    ingress.channel_id, ingress.company_id
                ))
            })?;
        if channel.company_id != company.id {
            return Ok(None);
        }
        let thread = self
            .resolve_target_thread(
                channel.id,
                ingress.target_thread_id,
                ingress.reply_to_message_id,
            )
            .await?;
        if (ingress.target_thread_id.is_some() || ingress.reply_to_message_id.is_some())
            && thread.is_none()
        {
            return Ok(None);
        }
        Ok(Some(Target {
            company,
            channel,
            thread,
        }))
    }
    fn canonical_email_headers(
        &self,
        company: &CompanySlug,
        parent: Option<&MessageId>,
        body: &str,
    ) -> AppResult<EmailHeaders> {
        let message = self
            .deliveries
            .renderer(crate::entities::transport::TransportKind::Email)?
            .application_message_key(company, Uuid::new_v4())?;
        let rfc = MessageId::from(message.as_str());
        let mut metadata =
            EmailMessageMetadata::new(rfc.clone()).raw_bodies(Some(body.to_string()), None);
        if let Some(parent) = parent {
            metadata = metadata
                .in_reply_to(Some(parent.clone()))
                .references(vec![parent.clone()]);
        }
        let direct_parent = metadata
            .direct_parent_id()
            .map(external_message_key)
            .transpose()?;
        let thread = external_thread_key(parent.unwrap_or(&rfc))?;
        let replies = match parent {
            Some(parent) => BoundedVec::parse(
                "reply message candidates",
                vec![external_message_key(parent)?],
            )?,
            None => BoundedVec::empty(),
        };
        Ok(EmailHeaders {
            message,
            thread,
            replies,
            direct_parent,
            extension: ProtocolExtension::email(metadata),
        })
    }
}
fn canonical_subject(subject: Option<String>, target: Option<&Thread>) -> String {
    match subject {
        Some(subject) if !subject.trim().is_empty() => subject,
        _ => target
            .map(Thread::reply_subject)
            .unwrap_or_else(|| "New conversation".into()),
    }
}
fn canonical_directives(
    ingress: &CanonicalMessageIngress,
    target: Option<Uuid>,
) -> IngressDirectives {
    IngressDirectives {
        hop_count: 0,
        trace_channels: BoundedVec::empty(),
        disposition: ingress.disposition,
        source_channel_id: None,
        target_thread_id: target,
        reply_to_message_id: ingress.reply_to_message_id,
        is_auto_reply: false,
        is_forwarded: false,
    }
}
