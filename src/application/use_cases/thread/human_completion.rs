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
        response_draft::{DraftRecipientSnapshot, ResponseDraftId, ResponseEvidence},
        task::BackgroundTask,
        thread::Thread,
        transport::{DeliveryPurpose, PrincipalId, TransportKind},
        value_objects::MessageId,
    },
    task_queue::{HumanTaskCompletion, HumanTaskCompletionResult},
    transport::{
        CanonicalContent, DeliveryContext, DeliveryRequest, EmailDeliveryContext, EmailThreading,
    },
    use_cases::response_review::{DraftPublicationSnapshot, PreparedReviewDraft},
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

pub struct ResponseReviewEditDraft<'a> {
    pub draft_id: ResponseDraftId,
    pub current_version: u32,
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Uuid,
    pub task_id: Option<Uuid>,
    pub source_handoff_generation: Option<Uuid>,
    pub actor_principal_id: PrincipalId,
    pub subject: &'a str,
    pub text_body: &'a str,
    pub recipient_to: crate::entities::value_objects::EmailAddress,
    pub recipients_cc: Vec<crate::entities::value_objects::EmailAddress>,
    pub evidence: Vec<ResponseEvidence>,
    pub current_publication: &'a DraftPublicationSnapshot,
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
        let recipients_cc: Vec<crate::entities::value_objects::EmailAddress> = context
            .cc
            .into_iter()
            .filter(|address| address != &recipient && address != &from)
            .collect();
        let recipient_snapshot =
            DraftRecipientSnapshot::email(recipient.clone(), recipients_cc.clone());
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
            structured: None,
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
            audience: crate::entities::message::MessageAudience::ExternalConversation,
            entry_kind: crate::entities::message::ThreadEntryKind::Conversation,
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
                draft_id: ResponseDraftId::new(draft.command_id),
                draft_version: 1,
                recipient_snapshot,
                evidence: Vec::new(),
                message: &message,
                deliveries: vec![composed.delivery],
            })
            .await
    }

    /// Freeze a reviewer's edit as the next immutable version. Nothing is published here.
    pub async fn prepare_response_review_edit(
        &self,
        edit: ResponseReviewEditDraft<'_>,
    ) -> AppResult<PreparedReviewDraft> {
        let subject = edit.subject.trim();
        let body = edit.text_body.trim();
        if subject.is_empty() || body.is_empty() || edit.recipient_to.is_empty() {
            return Err(AppError::BadRequest(
                "Subject, response, and primary recipient are required.".into(),
            ));
        }
        let structured = match &edit.current_publication.message().structured {
            Some(original) => {
                let validator = self
                    .response_validator
                    .as_ref()
                    .ok_or_else(|| AppError::Internal("Response validator unavailable".into()))?;
                Some(
                    crate::services::response_contract::StructuredResponse::validate(
                        original.contract(),
                        body,
                        None,
                        validator.as_ref(),
                    )?
                    .map_err(|_| {
                        AppError::BadRequest(
                            "response: content does not satisfy the saved response contract".into(),
                        )
                    })?,
                )
            }
            None => None,
        };
        let canonical_body = structured
            .as_ref()
            .map_or(body, |response| response.body())
            .to_string();
        let body = canonical_body.as_str();
        let company = self
            .company_persistence
            .get_by_id(edit.company_id)
            .await?
            .ok_or_else(|| AppError::NotFound("Company not found.".into()))?;
        let channel = self
            .channel_persistence
            .get_by_id(edit.channel_id)
            .await?
            .filter(|channel| channel.company_id == edit.company_id)
            .ok_or_else(|| AppError::NotFound("Channel not found.".into()))?;
        let reply_to = self
            .latest_replyable_email_context(edit.thread_id)
            .await?
            .ok_or_else(|| AppError::Conflict("This thread has no message to answer.".into()))?;
        let threading = EmailThreading::received(reply_to.rfc_message_id, reply_to.references);
        let message_id = CanonicalMessageId::random();
        let content = CanonicalContent::parse(subject.to_string(), body.to_string())?;
        let next_version = edit
            .current_version
            .checked_add(1)
            .ok_or_else(|| AppError::Conflict("Draft version is exhausted.".into()))?;
        let composed = self
            .compose_delivery(DeliveryRequest {
                company_id: edit.company_id,
                channel_id: edit.channel_id,
                message_id,
                task_id: edit.task_id,
                correlation_id: edit.current_publication.message().correlation_id,
                purpose: edit.current_publication.delivery().purpose,
                source_key: format!("review:{}:v{}", edit.draft_id, next_version),
                content: &content,
                context: DeliveryContext::Email(EmailDeliveryContext {
                    from: Channel::address_for(
                        &channel.slug,
                        &company.slug,
                        &self.config.app_domain_name,
                    ),
                    from_name: Some(channel.name.clone()),
                    recipient_to: edit.recipient_to.clone(),
                    recipients_cc: edit.recipients_cc.clone(),
                    threading: threading.clone(),
                    relay: None,
                }),
            })
            .await?;
        let current_message = edit.current_publication.message();
        let message = MessageWrite {
            structured,
            id: message_id,
            thread_id: edit.thread_id,
            author: MessageAuthorWrite::Principal(edit.actor_principal_id),
            subject: subject.to_string(),
            clean_text_body: body.to_string(),
            attachments: current_message.attachments.clone(),
            direction: MessageDirection::Outbound,
            role: MessageRole::Human,
            correlation_id: current_message.correlation_id,
            participants: Vec::new(),
            correlation: outbound_email_correlation(&composed, &threading, body.to_string()),
            created_at: chrono::Utc::now(),
            audience: crate::entities::message::MessageAudience::ExternalConversation,
            entry_kind: current_message.entry_kind,
        };
        let publication = DraftPublicationSnapshot::new(message, composed.delivery)?
            .with_also_in_threads(edit.current_publication.also_in_threads().to_vec())?;
        let recipients = DraftRecipientSnapshot::email(edit.recipient_to, edit.recipients_cc);
        let mut prepared = PreparedReviewDraft::new(
            edit.draft_id,
            next_version,
            edit.company_id,
            edit.channel_id,
            edit.thread_id,
            edit.task_id,
            edit.actor_principal_id,
            edit.actor_principal_id,
            recipients,
            edit.evidence,
            publication,
        )?;
        prepared.source_handoff_generation = edit.source_handoff_generation;
        Ok(prepared)
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
