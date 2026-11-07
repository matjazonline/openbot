//! The final publisher observes and commits one immutable response under its own claim.
use super::{ReplyPublicationClaim, ReplyPublicationFailure, ReplyPublications};
use crate::app_error::AppResult;
use crate::application::agent::ChannelFinalOrigin;
use crate::application::transport::CanonicalContent;
use crate::application::workflow::actions::ActionScope;
use crate::application::workflow::message::{MessagePlan, MessageRoutes};
use crate::domain::workflow::ActionInvocationId;
use crate::entities::message::CanonicalMessageId;
use crate::entities::transport::DeliveryId;
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct ReplyPublicationDraft {
    pub scope: ActionScope,
    pub origin: ChannelFinalOrigin,
    pub invocation: ActionInvocationId,
    pub source: CanonicalMessageId,
    pub content: CanonicalContent,
    pub routes: MessageRoutes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplyPublicationReceipt {
    pub message: CanonicalMessageId,
    pub delivery: Option<DeliveryId>,
}

#[derive(Debug, Clone)]
pub enum ReplyPublicationLoad {
    Ready(Box<ReplyPublicationDraft>),
    Published(ReplyPublicationReceipt),
    /// Terminal publication truth already stored, with no canonical effect.
    Failed(ReplyPublicationFailure),
    /// The current claimed artifact is invalid; the caller must settle its refusal.
    Refused(ReplyPublicationFailure),
    Lost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyPublicationOutcome {
    Published(ReplyPublicationReceipt),
    Refused(ReplyPublicationFailure),
    /// Work was released with backoff; no provider call was made.
    Deferred,
    Lost,
}

/// Loading is observational; publication rechecks immutable content, current authority,
/// policy/release/approval and the exact claim in its atomic canonical-write transaction.
#[async_trait]
pub trait ReplyPublicationStore: ReplyPublications {
    async fn load_reply_publication(
        &self,
        claim: ReplyPublicationClaim,
    ) -> AppResult<ReplyPublicationLoad>;
    async fn commit_reply_publication(
        &self,
        claim: ReplyPublicationClaim,
        plan: &MessagePlan,
    ) -> AppResult<ReplyPublicationOutcome>;
}
