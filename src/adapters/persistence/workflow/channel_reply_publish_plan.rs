//! Validate the shared planner's effect against the exact frozen final artifact.
use super::channel_reply_publish_read::PublicationArtifact;
use super::*;
use crate::application::ports::thread::{MessageAuthorWrite, MessageCorrelation};
use crate::application::workflow::DeliveryPolicy;
use crate::application::workflow::message::{
    DestinationKind, MessagePlan, RouteCheck, RouteTarget,
};
use crate::entities::message::{
    CanonicalMessageId, MessageAudience, MessageDirection, MessageRole, ThreadEntryKind,
};
use crate::entities::transport::TransportKind;
use crate::entities::value_objects::EmailAddress;

struct FinalRoute<'a> {
    source: CanonicalMessageId,
    thread: Uuid,
    channel: Uuid,
    recipient: Option<&'a EmailAddress>,
}

pub(super) fn matches(artifact: &PublicationArtifact, plan: &MessagePlan) -> bool {
    let Some(route) = route(&artifact.draft.routes.targets) else {
        return false;
    };
    let [destination] = plan.destinations.as_slice() else {
        return false;
    };
    let message = &destination.message;
    if destination.kind != DestinationKind::Reply
        || destination.destination_id != route.source.as_uuid()
        || route.source != artifact.draft.source
        || destination.channel_id != route.channel
        || destination.thread_id != route.thread
        || destination.new_thread.is_some()
        || destination.principal_id.is_some()
        || message.id.as_uuid().is_nil()
        || message.thread_id != route.thread
        || message.subject != artifact.draft.content.subject()
        || message.clean_text_body != artifact.draft.content.body_text()
        || message.correlation_id != artifact.draft.routes.correlation_id
        || message.direction != MessageDirection::Outbound
        || message.role != MessageRole::System
        || !matches!(&message.author, MessageAuthorWrite::Platform)
        || message.structured.is_some()
        || !message.attachments.is_empty()
        || !matches!(&destination.check, RouteCheck::Source { source,thread,channel }
            if *source==route.source && *thread==route.thread && *channel==route.channel)
    {
        return false;
    }
    let entry = if route.recipient.is_some() {
        ThreadEntryKind::Conversation
    } else {
        ThreadEntryKind::Note
    };
    if message.entry_kind != entry {
        return false;
    }
    let external = route.recipient.is_some()
        && artifact.draft.routes.delivery_policy == DeliveryPolicy::Deliver;
    if !external {
        return destination.delivery.is_none()
            && destination.recipient.is_none()
            && message.audience == MessageAudience::InternalOnly
            && matches!(&message.correlation, MessageCorrelation::Internal);
    }
    let Some(delivery) = &destination.delivery else {
        return false;
    };
    destination.recipient.as_ref() == route.recipient
        && delivery
            .external_destination
            .as_ref()
            .map(|recipient| recipient.as_str())
            == route.recipient.map(|recipient| recipient.as_str())
        && !delivery.id.as_uuid().is_nil()
        && delivery.company_id == artifact.draft.scope.company.as_uuid()
        && delivery.message_id == message.id
        && delivery.channel_id == route.channel
        && delivery.correlation_id == message.correlation_id
        && delivery.source_binding_id == delivery.destination_binding_id
        && delivery.depends_on_delivery_id.is_none()
        && message.audience == MessageAudience::ExternalConversation
        && delivery.message_audience == MessageAudience::ExternalConversation
        && delivery.transport == TransportKind::Email
        && delivery.purpose == crate::application::transport::DeliveryPurpose::WorkflowReply
        && matches!(&message.correlation, MessageCorrelation::Email(_))
}

fn route(targets: &[RouteTarget]) -> Option<FinalRoute<'_>> {
    match targets {
        [
            RouteTarget::ReplyEmail {
                source,
                thread,
                sender,
                recipient,
                ..
            },
        ] => Some(FinalRoute {
            source: *source,
            thread: *thread,
            channel: sender.id,
            recipient: Some(recipient),
        }),
        [
            RouteTarget::ReplyInternal {
                source,
                thread,
                channel,
            },
        ] => Some(FinalRoute {
            source: *source,
            thread: *thread,
            channel: *channel,
            recipient: None,
        }),
        _ => None,
    }
}
