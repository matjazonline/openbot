//! Human review of immutable external-response drafts.
//!
//! Tool-action approvals intentionally do not appear here. A response review owns an authored
//! version and its exact frozen publication; `HumanApproval` owns permission for a tool action.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        message::{CanonicalMessageId, MessageAttachments},
        response_draft::{
            DraftRecipientSnapshot, DraftTransportSnapshot, ExternalResponseReview,
            ResponseDraftId, ResponseEvidence, ResponseReviewDetail,
        },
        transport::PrincipalId,
    },
    transport::{DeliveryCreation, NewDelivery},
    use_cases::thread::MessageWrite,
};

pub const MAX_RESPONSE_EVIDENCE: usize = 128;
pub const MAX_REVIEW_FEEDBACK_BYTES: usize = 8 * 1024;
pub const MAX_REVIEW_RATIONALE_BYTES: usize = 2 * 1024;
pub const DEFAULT_REVIEW_EXPIRY_DAYS: i64 = 7;

/// The exact canonical message and already-rendered delivery approved by a reviewer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "version")]
pub enum DraftPublicationSnapshot {
    #[serde(rename = "1")]
    V1 {
        message: MessageWrite,
        delivery: NewDelivery,
        #[serde(default)]
        also_in_threads: Vec<Uuid>,
    },
}

impl DraftPublicationSnapshot {
    pub fn new(message: MessageWrite, delivery: NewDelivery) -> AppResult<Self> {
        if !message.audience.is_externally_deliverable()
            || !delivery.message_audience.is_externally_deliverable()
            || message.id != delivery.message_id
            || delivery.channel_id == Uuid::nil()
            || message.thread_id == Uuid::nil()
        {
            return Err(AppError::BadRequest(
                "A review draft requires one matching external message and delivery.".into(),
            ));
        }
        Ok(Self::V1 {
            message,
            delivery,
            also_in_threads: Vec::new(),
        })
    }

    pub fn with_also_in_threads(mut self, thread_ids: Vec<Uuid>) -> AppResult<Self> {
        let Self::V1 {
            message,
            also_in_threads,
            ..
        } = &mut self;
        if thread_ids.contains(&message.thread_id)
            || thread_ids.iter().any(|id| *id == Uuid::nil())
            || thread_ids.iter().enumerate().any(|(index, id)| {
                thread_ids[index + 1..]
                    .iter()
                    .any(|candidate| candidate == id)
            })
        {
            return Err(AppError::BadRequest(
                "A draft cannot list its primary thread twice.".into(),
            ));
        }
        *also_in_threads = thread_ids;
        Ok(self)
    }

    pub const fn message(&self) -> &MessageWrite {
        match self {
            Self::V1 { message, .. } => message,
        }
    }

    pub const fn delivery(&self) -> &NewDelivery {
        match self {
            Self::V1 { delivery, .. } => delivery,
        }
    }

    pub fn also_in_threads(&self) -> &[Uuid] {
        match self {
            Self::V1 {
                also_in_threads, ..
            } => also_in_threads,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PreparedReviewDraft {
    pub id: ResponseDraftId,
    pub version: u32,
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Uuid,
    pub task_id: Option<Uuid>,
    pub source_handoff_generation: Option<Uuid>,
    pub author_principal_id: PrincipalId,
    pub created_by_principal_id: PrincipalId,
    pub recipients: DraftRecipientSnapshot,
    pub evidence: Vec<ResponseEvidence>,
    pub publication: DraftPublicationSnapshot,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct AgentReviewSubmission {
    pub id: ResponseDraftId,
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Uuid,
    pub agent_id: Uuid,
    pub recipients: DraftRecipientSnapshot,
    pub evidence: Vec<ResponseEvidence>,
    pub publication: DraftPublicationSnapshot,
}

impl PreparedReviewDraft {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: ResponseDraftId,
        version: u32,
        company_id: Uuid,
        channel_id: Uuid,
        thread_id: Uuid,
        task_id: Option<Uuid>,
        author_principal_id: PrincipalId,
        created_by_principal_id: PrincipalId,
        recipients: DraftRecipientSnapshot,
        evidence: Vec<ResponseEvidence>,
        publication: DraftPublicationSnapshot,
    ) -> AppResult<Self> {
        if version == 0 || evidence.len() > MAX_RESPONSE_EVIDENCE {
            return Err(AppError::BadRequest(format!(
                "A draft version must be positive and may list at most {MAX_RESPONSE_EVIDENCE} sources."
            )));
        }
        let message = publication.message();
        let delivery = publication.delivery();
        if message.id != delivery.message_id
            || message.thread_id != thread_id
            || delivery.company_id != company_id
            || delivery.channel_id != channel_id
            || delivery.task_id != task_id
        {
            return Err(AppError::BadRequest(
                "The draft scope, message, and frozen delivery do not match.".into(),
            ));
        }
        let DraftRecipientSnapshot::V1 { to, .. } = &recipients;
        if to.is_empty()
            || delivery
                .external_destination
                .as_ref()
                .is_some_and(|destination| to.first() != Some(destination))
        {
            return Err(AppError::BadRequest(
                "The primary recipient does not match the frozen destination.".into(),
            ));
        }
        Ok(Self {
            id,
            version,
            company_id,
            channel_id,
            thread_id,
            task_id,
            source_handoff_generation: None,
            author_principal_id,
            created_by_principal_id,
            recipients,
            evidence,
            publication,
            expires_at: Utc::now() + chrono::Duration::days(DEFAULT_REVIEW_EXPIRY_DAYS),
        })
    }

    pub fn transport_snapshot(&self) -> DraftTransportSnapshot {
        let delivery = self.publication.delivery();
        DraftTransportSnapshot::V1 {
            transport: delivery.transport,
            source_binding_id: delivery.source_binding_id,
            destination_binding_id: delivery.destination_binding_id,
            external_destination: delivery.external_destination.clone(),
        }
    }

    pub fn attachment_snapshot(&self) -> MessageAttachments {
        MessageAttachments::new(self.publication.message().attachments.clone())
    }
}

#[derive(Debug, Clone)]
pub struct ReviewCommand {
    pub company_id: Uuid,
    pub draft_id: ResponseDraftId,
    pub expected_draft_version: u32,
    pub command_id: Uuid,
    pub actor_principal_id: PrincipalId,
    pub action: ReviewAction,
}

#[derive(Debug, Clone)]
pub enum ReviewAction {
    Approve {
        rationale: Option<String>,
    },
    Edit {
        replacement: Box<PreparedReviewDraft>,
    },
    Reject {
        feedback: String,
    },
    Reassign {
        reviewer_principal_id: PrincipalId,
    },
}

impl ReviewAction {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Approve { .. } => "approve",
            Self::Edit { .. } => "edit",
            Self::Reject { .. } => "reject",
            Self::Reassign { .. } => "reassign",
        }
    }
}

impl ReviewCommand {
    pub fn validate(&self) -> AppResult<()> {
        if self.expected_draft_version == 0 {
            return Err(AppError::BadRequest(
                "Draft version must be positive.".into(),
            ));
        }
        match &self.action {
            ReviewAction::Approve { rationale } => {
                validate_optional_text("Reviewer rationale", rationale, MAX_REVIEW_RATIONALE_BYTES)
            }
            ReviewAction::Reject { feedback } => {
                validate_required_text("Review feedback", feedback, MAX_REVIEW_FEEDBACK_BYTES)
            }
            ReviewAction::Edit { replacement } => {
                if replacement.id != self.draft_id
                    || self.expected_draft_version.checked_add(1) != Some(replacement.version)
                    || replacement.company_id != self.company_id
                    || replacement.created_by_principal_id != self.actor_principal_id
                {
                    return Err(AppError::BadRequest(
                        "An edit must create the next version of this draft by the acting reviewer."
                            .into(),
                    ));
                }
                Ok(())
            }
            ReviewAction::Reassign {
                reviewer_principal_id,
            } => {
                if *reviewer_principal_id == self.actor_principal_id {
                    return Err(AppError::BadRequest(
                        "The review is already assigned to that reviewer.".into(),
                    ));
                }
                Ok(())
            }
        }
    }

    pub fn fingerprint(&self) -> String {
        let action_value = match &self.action {
            ReviewAction::Approve { rationale } => {
                format!(
                    "approve\0{}",
                    rationale.as_deref().unwrap_or_default().trim()
                )
            }
            ReviewAction::Reject { feedback } => format!("reject\0{}", feedback.trim()),
            ReviewAction::Reassign {
                reviewer_principal_id,
            } => format!("reassign\0{reviewer_principal_id}"),
            ReviewAction::Edit { replacement } => {
                let serialized = serde_json::to_vec(&(
                    &replacement.publication,
                    &replacement.recipients,
                    &replacement.evidence,
                    replacement.source_handoff_generation,
                    replacement.expires_at,
                ))
                .unwrap_or_else(|_| b"invalid-publication".to_vec());
                format!(
                    "edit\0{}\0{:x}",
                    replacement.version,
                    Sha256::digest(serialized)
                )
            }
        };
        format!(
            "{:x}",
            Sha256::digest(
                format!(
                    "{}\0{}\0{}\0{}\0{action_value}",
                    self.company_id,
                    self.draft_id,
                    self.expected_draft_version,
                    self.actor_principal_id
                )
                .as_bytes()
            )
        )
    }
}

fn validate_optional_text(label: &str, value: &Option<String>, max: usize) -> AppResult<()> {
    match value {
        Some(value) if !value.trim().is_empty() => validate_required_text(label, value, max),
        Some(_) => Ok(()),
        None => Ok(()),
    }
}

fn validate_required_text(label: &str, value: &str, max: usize) -> AppResult<()> {
    if value.trim().is_empty() || value.len() > max {
        return Err(AppError::BadRequest(format!(
            "{label} must contain between 1 and {max} bytes."
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewCommandResult {
    pub draft_id: ResponseDraftId,
    pub draft_version: u32,
    pub published_message_id: Option<CanonicalMessageId>,
    pub delivery: Option<DeliveryCreation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseReviewPolicy {
    pub company_default: ExternalResponseReview,
    pub channel_override: Option<ExternalResponseReview>,
    pub effective: ExternalResponseReview,
    pub preferred_reviewer_principal_id: Option<PrincipalId>,
}

#[async_trait]
pub trait ResponseReviewPersistence: Send + Sync {
    /// Create version one only when the effective policy requires it. The policy read and draft
    /// insert share one transaction so a direct, task-less response cannot slip between them.
    async fn submit_agent_review_if_required(
        &self,
        submission: AgentReviewSubmission,
    ) -> AppResult<bool>;

    async fn review_policy(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Option<ResponseReviewPolicy>>;

    async fn set_company_review_policy(
        &self,
        company_id: Uuid,
        policy: ExternalResponseReview,
    ) -> AppResult<()>;

    async fn set_channel_review_policy(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
        policy_override: Option<ExternalResponseReview>,
        preferred_reviewer_principal_id: Option<PrincipalId>,
    ) -> AppResult<()>;

    async fn get_for_reviewer(
        &self,
        company_id: Uuid,
        draft_id: ResponseDraftId,
        reviewer_principal_id: PrincipalId,
    ) -> AppResult<Option<ResponseReviewDetail>>;

    async fn list_pending_for_reviewer(
        &self,
        company_id: Uuid,
        reviewer_principal_id: PrincipalId,
        limit: usize,
    ) -> AppResult<Vec<ResponseReviewDetail>>;

    async fn publication_for_reviewer(
        &self,
        company_id: Uuid,
        draft_id: ResponseDraftId,
        draft_version: u32,
        reviewer_principal_id: PrincipalId,
    ) -> AppResult<Option<DraftPublicationSnapshot>>;

    async fn execute_review_command(
        &self,
        command: ReviewCommand,
    ) -> AppResult<ReviewCommandResult>;
}

#[derive(Clone)]
pub struct ResponseReviewUseCases {
    persistence: Arc<dyn ResponseReviewPersistence>,
}

impl ResponseReviewUseCases {
    pub fn new(persistence: Arc<dyn ResponseReviewPersistence>) -> Self {
        Self { persistence }
    }

    pub async fn get(
        &self,
        company_id: Uuid,
        draft_id: ResponseDraftId,
        reviewer_principal_id: PrincipalId,
    ) -> AppResult<Option<ResponseReviewDetail>> {
        self.persistence
            .get_for_reviewer(company_id, draft_id, reviewer_principal_id)
            .await
    }

    pub async fn submit_agent_if_required(
        &self,
        submission: AgentReviewSubmission,
    ) -> AppResult<bool> {
        self.persistence
            .submit_agent_review_if_required(submission)
            .await
    }

    pub async fn policy(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Option<ResponseReviewPolicy>> {
        self.persistence.review_policy(company_id, channel_id).await
    }

    pub async fn set_company_policy(
        &self,
        company_id: Uuid,
        policy: ExternalResponseReview,
    ) -> AppResult<()> {
        self.persistence
            .set_company_review_policy(company_id, policy)
            .await
    }

    pub async fn set_channel_policy(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
        policy_override: Option<ExternalResponseReview>,
        preferred_reviewer_principal_id: Option<PrincipalId>,
    ) -> AppResult<()> {
        self.persistence
            .set_channel_review_policy(
                company_id,
                channel_id,
                policy_override,
                preferred_reviewer_principal_id,
            )
            .await
    }

    pub async fn list_pending(
        &self,
        company_id: Uuid,
        reviewer_principal_id: PrincipalId,
    ) -> AppResult<Vec<ResponseReviewDetail>> {
        self.persistence
            .list_pending_for_reviewer(company_id, reviewer_principal_id, 100)
            .await
    }

    pub async fn publication(
        &self,
        company_id: Uuid,
        draft_id: ResponseDraftId,
        draft_version: u32,
        reviewer_principal_id: PrincipalId,
    ) -> AppResult<Option<DraftPublicationSnapshot>> {
        self.persistence
            .publication_for_reviewer(company_id, draft_id, draft_version, reviewer_principal_id)
            .await
    }

    pub async fn execute(&self, command: ReviewCommand) -> AppResult<ReviewCommandResult> {
        command.validate()?;
        self.persistence.execute_review_command(command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_fingerprints_bind_actor_version_and_decision() {
        let base = ReviewCommand {
            company_id: Uuid::new_v4(),
            draft_id: ResponseDraftId::random(),
            expected_draft_version: 1,
            command_id: Uuid::new_v4(),
            actor_principal_id: PrincipalId::random(),
            action: ReviewAction::Approve { rationale: None },
        };
        let mut changed = base.clone();
        changed.action = ReviewAction::Reject {
            feedback: "needs a citation".into(),
        };
        assert_ne!(base.fingerprint(), changed.fingerprint());
    }
}
