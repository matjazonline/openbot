//! Canonical publication has its own lease, independent of workflow execution and delivery.
use crate::app_error::AppResult;
use crate::domain::workflow::{CompanyId, ReplyIntegrity, ReplyProposalId, RunId};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplyPublicationPage(u8);
impl TryFrom<u8> for ReplyPublicationPage {
    type Error = &'static str;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        (1..=64)
            .contains(&value)
            .then_some(Self(value))
            .ok_or("Invalid reply publication page")
    }
}
impl ReplyPublicationPage {
    pub fn get(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplyPublicationWorker(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplyPublicationClaim {
    pub company: CompanyId,
    pub run: RunId,
    pub proposal: ReplyProposalId,
    pub integrity: ReplyIntegrity,
    pub worker: ReplyPublicationWorker,
    pub generation: Uuid,
    pub expires_at: DateTime<Utc>,
    pub attempt: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplyPublicationFailure {
    AttemptsExhausted,
    AuthorityRevoked,
    PolicyChanged,
    SourceUnavailable,
    #[serde(rename = "knowledge.audience_below_content")]
    ReleaseDenied,
    InvalidProposal,
    PublicationFailed,
}
impl ReplyPublicationFailure {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AttemptsExhausted => "attempts_exhausted",
            Self::AuthorityRevoked => "authority_revoked",
            Self::PolicyChanged => "policy_changed",
            Self::SourceUnavailable => "source_unavailable",
            Self::ReleaseDenied => crate::domain::workflow::KNOWLEDGE_AUDIENCE_BELOW_CONTENT,
            Self::InvalidProposal => "invalid_proposal",
            Self::PublicationFailed => "publication_failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyPublicationRetry {
    Retry,
    Terminal(ReplyPublicationFailure),
}

/// Claiming grants only the immutable final decision's canonical publication work.
#[async_trait]
pub trait ReplyPublications: Send + Sync {
    async fn claim_replies(
        &self,
        worker: ReplyPublicationWorker,
        page: ReplyPublicationPage,
    ) -> AppResult<Vec<ReplyPublicationClaim>>;
    async fn release_reply(
        &self,
        claim: ReplyPublicationClaim,
        retry: ReplyPublicationRetry,
    ) -> AppResult<bool>;
}
