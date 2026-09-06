//! Immutable response versions, response-level evidence, and their human review lifecycle.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{
    message::{AttachmentMetadata, CanonicalMessageId},
    transport::{ChannelBindingId, ExternalDestination, PrincipalId, TransportKind, uuid_id},
};

uuid_id!(ResponseDraftId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseDraftStatus {
    PendingReview,
    Rejected,
    Expired,
    Superseded,
    Published,
}

impl ResponseDraftStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PendingReview => "pending_review",
            Self::Rejected => "rejected",
            Self::Expired => "expired",
            Self::Superseded => "superseded",
            Self::Published => "published",
        }
    }
}

impl FromStr for ResponseDraftStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending_review" => Ok(Self::PendingReview),
            "rejected" => Ok(Self::Rejected),
            "expired" => Ok(Self::Expired),
            "superseded" => Ok(Self::Superseded),
            "published" => Ok(Self::Published),
            _ => Err(format!("invalid response draft status '{value}'")),
        }
    }
}

/// Whether externally deliverable responses may publish without a human decision.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalResponseReview {
    #[default]
    Autonomous,
    ReviewAllExternal,
}

impl ExternalResponseReview {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Autonomous => "autonomous",
            Self::ReviewAllExternal => "review_all_external",
        }
    }

    pub const fn requires_review(self) -> bool {
        matches!(self, Self::ReviewAllExternal)
    }
}

impl FromStr for ExternalResponseReview {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "autonomous" => Ok(Self::Autonomous),
            "review_all_external" => Ok(Self::ReviewAllExternal),
            _ => Err(format!("invalid external response review policy '{value}'")),
        }
    }
}

/// Recipients frozen with a draft version rather than re-derived when it is published.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "version")]
pub enum DraftRecipientSnapshot {
    #[serde(rename = "1")]
    V1 {
        to: Vec<ExternalDestination>,
        cc: Vec<ExternalDestination>,
    },
}

impl DraftRecipientSnapshot {
    pub fn email(
        to: crate::entities::value_objects::EmailAddress,
        cc: Vec<crate::entities::value_objects::EmailAddress>,
    ) -> Self {
        Self::V1 {
            to: vec![ExternalDestination::Email(to)],
            cc: cc.into_iter().map(ExternalDestination::Email).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "version")]
pub enum DraftTransportSnapshot {
    #[serde(rename = "1")]
    V1 {
        transport: TransportKind,
        source_binding_id: ChannelBindingId,
        destination_binding_id: ChannelBindingId,
        external_destination: Option<ExternalDestination>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSupport {
    DirectEvidence,
    Inference,
}

impl EvidenceSupport {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DirectEvidence => "direct_evidence",
            Self::Inference => "inference",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceAudience {
    ExternalConversation,
    InternalOnly,
    CompanyRestricted,
}

impl EvidenceAudience {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExternalConversation => "external_conversation",
            Self::InternalOnly => "internal_only",
            Self::CompanyRestricted => "company_restricted",
        }
    }
}

/// A stable evidence reference. Content is deliberately absent: authorization is evaluated when
/// a reviewer opens the source, and evidence can never leak into a delivery by being listed here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EvidenceSource {
    Message {
        message_id: CanonicalMessageId,
        thread_id: Uuid,
    },
    Note {
        note_id: Uuid,
    },
    Attachment {
        message_id: CanonicalMessageId,
        sha256_hash: String,
    },
    DelegatedResult {
        task_id: Uuid,
        execution_generation: Uuid,
    },
    RetainedToolResult {
        task_id: Uuid,
        result_id: Uuid,
    },
    ExternalUrl {
        url: String,
        retrieved_at: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseEvidence {
    pub id: Uuid,
    pub source: EvidenceSource,
    pub source_version: String,
    pub content_digest: String,
    pub audience: EvidenceAudience,
    pub support: EvidenceSupport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseReviewStatus {
    Pending,
    Rejected,
    Expired,
    Superseded,
    Published,
}

impl ResponseReviewStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Rejected => "rejected",
            Self::Expired => "expired",
            Self::Superseded => "superseded",
            Self::Published => "published",
        }
    }
}

impl FromStr for ResponseReviewStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "rejected" => Ok(Self::Rejected),
            "expired" => Ok(Self::Expired),
            "superseded" => Ok(Self::Superseded),
            "published" => Ok(Self::Published),
            _ => Err(format!("invalid response review status '{value}'")),
        }
    }
}

/// One immutable draft version. Only lifecycle metadata may advance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseDraft {
    pub id: ResponseDraftId,
    pub version: u32,
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Uuid,
    pub task_id: Option<Uuid>,
    pub source_handoff_generation: Option<Uuid>,
    pub author_principal_id: PrincipalId,
    pub reviewer_principal_id: PrincipalId,
    pub proposed_message_id: CanonicalMessageId,
    pub subject: String,
    pub body: String,
    pub attachments: Vec<AttachmentMetadata>,
    pub recipients: DraftRecipientSnapshot,
    pub transport: DraftTransportSnapshot,
    pub status: ResponseDraftStatus,
    pub created_by_principal_id: PrincipalId,
    pub updated_by_principal_id: PrincipalId,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseReview {
    pub company_id: Uuid,
    pub draft_id: ResponseDraftId,
    pub draft_version: u32,
    pub reviewer_principal_id: PrincipalId,
    pub status: ResponseReviewStatus,
    pub feedback: Option<String>,
    pub reviewer_rationale: Option<String>,
    pub decided_by_principal_id: Option<PrincipalId>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceAvailability {
    pub evidence: ResponseEvidence,
    pub openable: bool,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseReviewDetail {
    pub draft: ResponseDraft,
    pub review: ResponseReview,
    pub evidence: Vec<EvidenceAvailability>,
    pub visibility_warnings: Vec<String>,
    pub history: Vec<ResponseDraft>,
}
