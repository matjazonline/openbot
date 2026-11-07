//! Shared canonical message planning; provider sends belong to the delivery worker.
use super::*;
use crate::application::ports::thread::{
    MessageAuthorWrite, MessageCorrelation, MessageParticipantWrite, qualified_email_identity,
};
use crate::application::transport::{
    DeliveryComposer, DeliveryContext, DeliveryPurpose, DeliveryRequest, EmailDeliveryContext,
    EmailRelayTrace, EmailThreading,
};
use crate::domain::workflow::ActionInvocationId;
use crate::entities::channel::Channel;
use crate::entities::message::{
    MessageDirection, MessageParticipantKind, MessageRole, ThreadEntryKind,
};

#[derive(Debug, Clone, Copy)]
pub struct MessagePublicationIdentity {
    pub company: CompanyId,
    pub invocation: ActionInvocationId,
}

struct MessageDomain(String);

/// Explicit actions and channel finals use the same routing and rendering rules.
pub struct MessagePlanner {
    composer: DeliveryComposer,
    mail_domain: MessageDomain,
}

impl MessagePlanner {
    pub fn new(composer: DeliveryComposer, mail_domain: String) -> Self {
        Self {
            composer,
            mail_domain: MessageDomain(mail_domain),
        }
    }

    pub(crate) async fn plan(
        &self,
        identity: MessagePublicationIdentity,
        arguments: &MessageArguments,
        routes: &MessageRoutes,
    ) -> AppResult<MessagePlan> {
        let content = arguments.content();
        let source_key = format!("invocation:{}", identity.invocation.as_uuid());
        let mut destinations = Vec::with_capacity(routes.targets.len());
        let ids: Vec<(DestinationKind, Uuid)> = match arguments {
            MessageArguments::Send { destinations, .. } => destinations
                .iter()
                .map(|destination| (destination.kind, destination.id))
                .collect(),
            MessageArguments::Reply { source, .. } => {
                vec![(DestinationKind::Reply, source.as_uuid())]
            }
        };
        if ids.len() != routes.targets.len() {
            return Err(invalid_message());
        }
        for ((kind, destination_id), target) in ids.into_iter().zip(&routes.targets) {
            let planned = match target {
                RouteTarget::Channel { channel } => {
                    plan_channel(kind, destination_id, *channel, content, routes)
                }
                RouteTarget::ReplyInternal {
                    source,
                    thread,
                    channel,
                } => plan_internal_reply(*source, *thread, *channel, content, routes),
                RouteTarget::User { .. } | RouteTarget::ReplyEmail { .. }
                    if routes.delivery_policy
                        == crate::application::workflow::DeliveryPolicy::InAppOnly =>
                {
                    plan_in_app(kind, destination_id, target, content, routes)?
                }
                RouteTarget::User { .. } | RouteTarget::ReplyEmail { .. } => {
                    self.plan_email(
                        identity,
                        (kind, destination_id),
                        target,
                        content,
                        routes,
                        &source_key,
                    )
                    .await?
                }
            };
            destinations.push(planned);
        }
        Ok(MessagePlan { destinations })
    }

    async fn plan_email(
        &self,
        identity: MessagePublicationIdentity,
        (kind, destination_id): (DestinationKind, Uuid),
        target: &RouteTarget,
        content: &CanonicalContent,
        routes: &MessageRoutes,
        source_key: &str,
    ) -> AppResult<PlannedDestination> {
        let email = EmailPlan::of(target, content)?;
        let from = Channel::address_for(
            &email.sender.slug,
            &routes.company_slug,
            &self.mail_domain.0,
        );
        let message_id = CanonicalMessageId::random();
        let composed = self
            .composer
            .compose(DeliveryRequest {
                company_id: identity.company.as_uuid(),
                channel_id: email.sender.id,
                message_id,
                correlation_id: routes.correlation_id,
                purpose: email.purpose,
                source_key: source_key.to_string(),
                content,
                context: DeliveryContext::Email(EmailDeliveryContext {
                    from: from.clone(),
                    from_name: Some(email.sender.name.clone()),
                    recipient_to: email.recipient.clone(),
                    recipients_cc: Vec::new(),
                    threading: email.threading.clone(),
                    relay: Some(email.relay.clone()),
                }),
            })
            .await?;
        let message =
            email.message_write(routes, content, from, message_id, composed.correlation)?;
        Ok(PlannedDestination {
            kind,
            destination_id,
            new_thread: email.new_thread,
            channel_id: email.sender.id,
            thread_id: email.thread_id,
            message,
            delivery: Some(composed.delivery),
            principal_id: email.principal,
            recipient: Some(email.recipient),
            check: email.check,
        })
    }
}

/// The email-specific facts of one destination, decided without I/O.
struct EmailPlan {
    sender: ChannelRoute,
    recipient: EmailAddress,
    threading: EmailThreading,
    relay: EmailRelayTrace,
    purpose: DeliveryPurpose,
    thread_id: Uuid,
    new_thread: Option<NewThread>,
    principal: Option<Uuid>,
    check: RouteCheck,
}
impl EmailPlan {
    fn message_write(
        &self,
        routes: &MessageRoutes,
        content: &CanonicalContent,
        from: EmailAddress,
        message_id: CanonicalMessageId,
        correlation: crate::transport::ports::RenderedCorrelation,
    ) -> AppResult<MessageWrite> {
        let provider_key = correlation.provider_key.ok_or_else(|| {
            AppError::BadRequest("Email delivery has no frozen Message-ID".into())
        })?;
        let crate::application::ports::thread::ProtocolExtension::Email {
            version: 1,
            metadata,
        } = correlation.extension
        else {
            return Err(AppError::Internal(
                "Email renderer returned non-email correlation".into(),
            ));
        };
        if metadata.rfc_message_id.as_str() != provider_key.as_str() {
            return Err(AppError::Internal(
                "Frozen email correlation disagrees with its provider key".into(),
            ));
        }
        Ok(MessageWrite {
            id: message_id,
            ..MessageWrite::internal(
                self.thread_id,
                MessageAuthorWrite::Platform,
                content.subject().to_string(),
                content.body_text().to_string(),
                MessageDirection::Outbound,
                MessageRole::System,
                routes.correlation_id,
            )
        }
        .external_conversation()
        .with_participants(vec![
            MessageParticipantWrite::new(
                MessageParticipantKind::Sender,
                qualified_email_identity(from.as_str())?,
            ),
            MessageParticipantWrite::new(
                MessageParticipantKind::To,
                qualified_email_identity(self.recipient.as_str())?,
            ),
        ])
        .with_correlation(MessageCorrelation::Email(metadata)))
    }

    fn of(target: &RouteTarget, content: &CanonicalContent) -> AppResult<Self> {
        match target {
            RouteTarget::User {
                user,
                principal,
                email,
                sender,
            } => {
                // A user email always opens its own thread in the run channel: it never joins
                // (and so never answers or re-threads) a customer conversation.
                let thread_id = Uuid::new_v4();
                Ok(Self {
                    sender: sender.clone(),
                    recipient: email.clone(),
                    threading: EmailThreading::Standalone,
                    // The scheduled-delivery precedent: the sending channel starts the chain.
                    relay: EmailRelayTrace {
                        source_channel_id: sender.id,
                        hop_count: 0,
                        trace_channels: vec![sender.id],
                    },
                    purpose: DeliveryPurpose::WorkflowSend,
                    thread_id,
                    new_thread: Some(NewThread {
                        id: thread_id,
                        channel_id: sender.id,
                        subject: content.subject().to_string(),
                    }),
                    principal: Some(*principal),
                    check: RouteCheck::Member {
                        user: *user,
                        principal: *principal,
                        email: email.clone(),
                        sender: sender.id,
                    },
                })
            }
            RouteTarget::ReplyEmail {
                source,
                thread,
                sender,
                recipient,
                in_reply_to,
                references,
                relay,
            } => Ok(Self {
                sender: sender.clone(),
                recipient: recipient.clone(),
                threading: EmailThreading::Received {
                    in_reply_to: in_reply_to.clone(),
                    references: references.clone(),
                },
                // A reply continues the trace the source arrived with; with none recorded it
                // starts one, as direct mail does.
                relay: EmailRelayTrace {
                    source_channel_id: sender.id,
                    hop_count: relay.as_ref().map_or(0, |relay| relay.hop_count),
                    trace_channels: relay
                        .as_ref()
                        .map(|relay| relay.trace_channels.clone())
                        .unwrap_or_default(),
                },
                purpose: DeliveryPurpose::WorkflowReply,
                thread_id: *thread,
                new_thread: None,
                principal: None,
                check: RouteCheck::Source {
                    source: *source,
                    thread: *thread,
                    channel: sender.id,
                },
            }),
            RouteTarget::Channel { .. } | RouteTarget::ReplyInternal { .. } => {
                Err(invalid_message())
            }
        }
    }
}

fn plan_channel(
    kind: DestinationKind,
    destination_id: Uuid,
    channel: Uuid,
    content: &CanonicalContent,
    routes: &MessageRoutes,
) -> PlannedDestination {
    let thread_id = Uuid::new_v4();
    let message = MessageWrite::internal(
        thread_id,
        MessageAuthorWrite::Platform,
        content.subject().to_string(),
        content.body_text().to_string(),
        MessageDirection::Outbound,
        MessageRole::System,
        routes.correlation_id,
    )
    // It opens its own thread, so a conversation entry answers no earlier turn.
    .with_entry_kind(ThreadEntryKind::Conversation);
    PlannedDestination {
        kind,
        destination_id,
        new_thread: Some(NewThread {
            id: thread_id,
            channel_id: channel,
            subject: content.subject().to_string(),
        }),
        channel_id: channel,
        thread_id,
        message,
        delivery: None,
        principal_id: None,
        recipient: None,
        check: RouteCheck::Channel { channel },
    }
}

/// An email target of an `in_app_only` run (channel simulation's test mode, p08 D1.7): the same
/// thread and the same route checks as the email plan, the message published in-app, and no
/// delivery. A member's send still opens its own thread, with the member as its participant.
fn plan_in_app(
    kind: DestinationKind,
    destination_id: Uuid,
    target: &RouteTarget,
    content: &CanonicalContent,
    routes: &MessageRoutes,
) -> AppResult<PlannedDestination> {
    let email = EmailPlan::of(target, content)?;
    let message = MessageWrite::internal(
        email.thread_id,
        MessageAuthorWrite::Platform,
        content.subject().to_string(),
        content.body_text().to_string(),
        MessageDirection::Outbound,
        MessageRole::System,
        routes.correlation_id,
    )
    .with_entry_kind(ThreadEntryKind::Conversation);
    Ok(PlannedDestination {
        kind,
        destination_id,
        new_thread: email.new_thread,
        channel_id: email.sender.id,
        thread_id: email.thread_id,
        message,
        delivery: None,
        principal_id: email.principal,
        recipient: None,
        check: email.check,
    })
}

fn plan_internal_reply(
    source: CanonicalMessageId,
    thread: Uuid,
    channel: Uuid,
    content: &CanonicalContent,
    routes: &MessageRoutes,
) -> PlannedDestination {
    // A note: it neither answers a customer turn nor carries email headers.
    let message = MessageWrite::internal(
        thread,
        MessageAuthorWrite::Platform,
        content.subject().to_string(),
        content.body_text().to_string(),
        MessageDirection::Outbound,
        MessageRole::System,
        routes.correlation_id,
    )
    .with_entry_kind(ThreadEntryKind::Note);
    PlannedDestination {
        kind: DestinationKind::Reply,
        destination_id: source.as_uuid(),
        new_thread: None,
        channel_id: channel,
        thread_id: thread,
        message,
        delivery: None,
        principal_id: None,
        recipient: None,
        check: RouteCheck::Source {
            source,
            thread,
            channel,
        },
    }
}
