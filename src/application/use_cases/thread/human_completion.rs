//! Human-owned task completion: prepare the canonical reply and freeze its delivery before the
//! persistence adapter commits both behind the ownership fence.

use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        channel::Channel,
        company::Company,
        email_message::EmailMessageMetadata,
        message::{CanonicalMessageId, MessageDirection, MessageRole},
        task::BackgroundTask,
        thread::Thread,
        transport::{DeliveryPurpose, PrincipalId, TransportKind},
        value_objects::MessageId,
    },
    task_queue::{HumanTaskCompletion, HumanTaskCompletionResult},
    transport::{
        CanonicalContent, DeliveryContext, DeliveryRequest, EmailDeliveryContext, EmailThreading,
    },
};

use super::{MessageAuthorWrite, MessageCorrelation, MessageWrite, ThreadUseCases};

pub struct HumanCompletionDraft<'a> {
    pub task: &'a BackgroundTask,
    pub company: &'a Company,
    pub channel: &'a Channel,
    pub thread: &'a Thread,
    pub owner_principal_id: PrincipalId,
    pub expected_ownership_version: u64,
    pub command_id: Uuid,
    pub text_body: &'a str,
}

impl ThreadUseCases {
    /// Publish a human owner's final response and complete its task atomically.
    pub async fn complete_human_owned_task(
        &self,
        draft: HumanCompletionDraft<'_>,
    ) -> AppResult<HumanTaskCompletionResult> {
        let body = draft.text_body.trim();
        if body.is_empty() {
            return Err(AppError::BadRequest("A final response is required.".into()));
        }
        if draft.task.thread_id != Some(draft.thread.id)
            || draft.task.channel_id != draft.channel.id
            || draft.task.company_id != draft.channel.company_id
            || draft.task.company_id != draft.company.id
        {
            return Err(AppError::BadRequest(
                "The task, channel, and thread do not describe the same work.".into(),
            ));
        }

        let context = self
            .latest_replyable_email_context(draft.thread.id)
            .await?
            .ok_or_else(|| AppError::Conflict("This thread has no message to answer.".into()))?;
        let recipient = context.author_email.ok_or_else(|| {
            AppError::Conflict("The latest message has no email address to answer.".into())
        })?;
        let from = Channel::address_for(
            &draft.channel.slug,
            &draft.company.slug,
            &self.config.app_domain_name,
        );
        let recipients_cc = context
            .cc
            .into_iter()
            .filter(|address| address != &recipient && address != &from)
            .collect();
        let threading = EmailThreading::received(context.rfc_message_id, context.references);
        let subject = draft.thread.reply_subject();
        let content = CanonicalContent::parse(subject.clone(), body.to_string())?;
        let message_id = CanonicalMessageId::random();
        let composed = self
            .compose_delivery(DeliveryRequest {
                company_id: draft.task.company_id,
                channel_id: draft.channel.id,
                message_id,
                task_id: Some(draft.task.id),
                correlation_id: draft.task.correlation_id,
                purpose: DeliveryPurpose::Reply,
                source_key: format!("task:{}:human-completion", draft.task.id),
                content: &content,
                context: DeliveryContext::Email(EmailDeliveryContext {
                    from,
                    from_name: Some(draft.channel.name.clone()),
                    recipient_to: recipient,
                    recipients_cc,
                    threading: threading.clone(),
                    relay: None,
                }),
            })
            .await?;

        let correlation = outbound_email_correlation(&composed, &threading, body.to_string());
        let message = MessageWrite {
            id: message_id,
            thread_id: draft.thread.id,
            author: MessageAuthorWrite::Principal(draft.owner_principal_id),
            subject,
            clean_text_body: body.to_string(),
            attachments: Vec::new(),
            direction: MessageDirection::Outbound,
            role: MessageRole::Human,
            correlation_id: draft.task.correlation_id,
            participants: Vec::new(),
            correlation,
            created_at: chrono::Utc::now(),
        };

        let fingerprint = completion_fingerprint(
            draft.task.id,
            draft.owner_principal_id,
            draft.expected_ownership_version,
            body,
        );
        self.task_persistence
            .complete_human_task(HumanTaskCompletion {
                task_id: draft.task.id,
                company_id: draft.task.company_id,
                owner_principal_id: draft.owner_principal_id,
                expected_ownership_version: draft.expected_ownership_version,
                command_id: draft.command_id,
                command_fingerprint: fingerprint,
                message: &message,
                deliveries: vec![composed.delivery],
            })
            .await
    }
}

fn completion_fingerprint(task_id: Uuid, owner: PrincipalId, version: u64, body: &str) -> String {
    let value = format!("{task_id}\0{}\0{version}\0{body}", owner.as_uuid());
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn outbound_email_correlation(
    composed: &crate::transport::ComposedDelivery,
    threading: &EmailThreading,
    body: String,
) -> MessageCorrelation {
    if composed.delivery.transport != TransportKind::Email {
        return MessageCorrelation::Internal;
    }
    let Some(provider_key) = composed.provider_key.as_ref() else {
        return MessageCorrelation::Internal;
    };
    let (in_reply_to, references) = match threading {
        EmailThreading::Received {
            in_reply_to,
            references,
        } => (Some(in_reply_to.clone()), references.clone()),
        EmailThreading::Standalone | EmailThreading::Anchored(_) => (None, Vec::new()),
    };
    MessageCorrelation::Email(
        EmailMessageMetadata::new(MessageId::from(provider_key.as_str().to_string()))
            .in_reply_to(in_reply_to)
            .references(references)
            .raw_bodies(Some(body), None),
    )
}
