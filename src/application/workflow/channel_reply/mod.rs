//! Channel response contracts. A frozen ingress snapshot is eligibility data;
//! only its admitted root run owns an automatic final response.
mod approval;
mod contract;
mod finalization;
mod publication;
mod publication_io;
mod publisher;
pub use finalization::*;
pub use publication::*;
pub use publication_io::*;
pub use publisher::ChannelReplyPublisher;
#[cfg(test)]
mod origin_tests;
pub use approval::{ReplyApprovalPolicy, ReplyReviewTimeout};
pub use contract::{ChannelReplyContract, ReplyReference, ReplyText};

use super::{RelatedChannelId, WorkflowActor};
use crate::app_error::AppResult;
use crate::domain::workflow::{
    BindingRevision, BindingStateRevision, CompanyId, WorkflowBindingId,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct ReplySettingsRevision(u64);

impl TryFrom<u64> for ReplySettingsRevision {
    type Error = &'static str;
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        (value > 0 && value <= i64::MAX as u64)
            .then_some(Self(value))
            .ok_or("Invalid channel response revision")
    }
}
impl From<ReplySettingsRevision> for u64 {
    fn from(value: ReplySettingsRevision) -> Self {
        value.0
    }
}
impl ReplySettingsRevision {
    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedResponder {
    pub binding: WorkflowBindingId,
    pub configuration_revision: BindingRevision,
    pub contract: ChannelReplyContract,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelReplySettings {
    pub revision: ReplySettingsRevision,
    pub responder: Option<SelectedResponder>,
    pub approval_policy: Option<ReplyApprovalPolicy>,
}

pub struct ConfigureResponder {
    pub binding: WorkflowBindingId,
    pub expected_binding_revision: BindingStateRevision,
    pub contract: ChannelReplyContract,
}

pub struct ConfigureChannelReply {
    pub company: CompanyId,
    pub channel: RelatedChannelId,
    pub actor: WorkflowActor,
    pub expected: Option<ReplySettingsRevision>,
    pub responder: Option<ConfigureResponder>,
    pub approval_policy: Option<ReplyApprovalPolicy>,
}

/// Implementations authorize reads and writes at their transaction boundary.
/// Saving rechecks activity, exact revisions, schema and resource authority;
/// a discovery read never grants sending or admission authority.
#[async_trait]
pub trait ChannelReplies: Send + Sync {
    async fn channel_reply_settings(
        &self,
        company: CompanyId,
        channel: RelatedChannelId,
        actor: WorkflowActor,
    ) -> AppResult<Option<ChannelReplySettings>>;
    async fn configure_channel_reply(
        &self,
        request: ConfigureChannelReply,
    ) -> AppResult<ChannelReplySettings>;
}
