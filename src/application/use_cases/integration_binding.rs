//! Host consent commands contain host IDs; the reusable integrations core does not.
use crate::{
    app_error::AppResult,
    application::integrations::VerifiedOwner,
    domain::integrations::{authority::*, *},
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BindingKind {
    Http,
    Mcp,
}
impl BindingKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Mcp => "mcp",
        }
    }
    pub fn purpose(self) -> InvocationKind {
        match self {
            Self::Http => InvocationKind::DeterministicHttp,
            Self::Mcp => InvocationKind::DeterministicMcp,
        }
    }
}
#[derive(Clone, Serialize)]
pub struct BindingProposal {
    pub id: Uuid,
    pub company: Uuid,
    pub company_name: String,
    pub connection: Uuid,
    pub kind: BindingKind,
    pub destination: ApprovedDestination,
    pub app: AppId,
    pub app_policy: AppPolicyDigest,
    pub connection_revision: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposeBinding {
    pub company: Uuid,
    pub connection: Uuid,
    pub kind: BindingKind,
    pub app: AppId,
    pub expected_revision: i64,
}
#[async_trait]
pub trait IntegrationBindings: Send + Sync {
    async fn propose(&self, actor: Uuid, command: ProposeBinding) -> AppResult<BindingProposal>;
    async fn proposal(&self, owner: &VerifiedOwner, id: Uuid) -> AppResult<BindingProposal>;
    async fn confirm(
        &self,
        owner: VerifiedOwner,
        proposal: Uuid,
        account: AccountId,
        authorization: AuthorizationRevision,
    ) -> AppResult<UseGrant>;
}
