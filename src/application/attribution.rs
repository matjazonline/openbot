//! Dedicated attribution identity and lifecycle ports. Openbot remains the account authority.
use super::session::VerifiedSession;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityError {
    TooLarge,
    Invalid,
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    Unavailable,
}
pub type IdentityResult<T> = Result<T, IdentityError>;
#[derive(Clone)]
pub struct SecretHash(pub Vec<u8>);
#[derive(Clone)]
pub struct Callback(pub String);
#[derive(Clone)]
pub struct Challenge(pub String);
#[derive(Clone)]
pub struct RequestState(pub String);
#[derive(Clone)]
pub struct AuthorizationBinding {
    pub callback: Callback,
    pub challenge: Challenge,
    pub state: RequestState,
}
pub enum AuthorizationSource {
    Direct(AuthorizationBinding),
    Continuation(SecretHash),
}
pub struct IssueCode {
    pub actor: VerifiedSession,
    pub source: AuthorizationSource,
    pub code: SecretHash,
}
pub struct RedeemCode {
    pub code: SecretHash,
    pub callback: Callback,
    pub challenge: Challenge,
    pub grant: SecretHash,
}
#[derive(Serialize)]
pub struct Identity {
    pub user_id: Uuid,
    pub display_name: String,
    pub expires_at: DateTime<Utc>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyState {
    Active,
    Suspended,
    Deleted,
}
impl CompanyState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Suspended => "suspended",
            Self::Deleted => "deleted",
        }
    }
    pub fn parse(value: &str) -> IdentityResult<Self> {
        match value {
            "active" => Ok(Self::Active),
            "suspended" => Ok(Self::Suspended),
            "deleted" => Ok(Self::Deleted),
            _ => Err(IdentityError::Unavailable),
        }
    }
}
#[derive(Clone, Serialize)]
pub struct CompanyStatus {
    pub company_id: Uuid,
    pub version: i64,
    pub state: CompanyState,
    pub changed_at: DateTime<Utc>,
}
#[derive(Serialize)]
pub struct CompanyAccess {
    pub company_id: Uuid,
    pub name: String,
    pub permissions: Vec<&'static str>,
    pub lifecycle: CompanyStatus,
}
pub struct CompanyList {
    pub items: Vec<CompanyAccess>,
    pub more: bool,
}
pub struct CompanyPageRequest {
    pub grant: SecretHash,
    pub after: Option<Uuid>,
    pub limit: u16,
}
pub struct StatusChange {
    pub actor: VerifiedSession,
    pub company_id: Uuid,
    pub expected_version: i64,
    pub state: CompanyState,
}
pub struct LifecycleClaim {
    pub id: Uuid,
    pub execution: Uuid,
    pub payload: CompanyStatus,
}
#[derive(Clone, Copy)]
pub enum DeliveryResult {
    Delivered,
    Unavailable,
    Rejected,
}

#[derive(Debug, PartialEq, Eq)]
pub struct LifecycleHealth {
    pub undelivered: bool,
    pub failed: bool,
}

#[async_trait]
pub trait AttributionIdentity: Send + Sync {
    async fn begin_continuation(
        &self,
        verifier: &SecretHash,
        binding: &AuthorizationBinding,
    ) -> IdentityResult<()>;
    async fn issue_code(&self, issue: IssueCode) -> IdentityResult<AuthorizationBinding>;
    async fn redeem(&self, redemption: RedeemCode) -> IdentityResult<DateTime<Utc>>;
    async fn identity(&self, grant: &SecretHash) -> IdentityResult<Identity>;
    async fn companies(&self, page: CompanyPageRequest) -> IdentityResult<CompanyList>;
    async fn access(&self, grant: &SecretHash, company: Uuid) -> IdentityResult<CompanyAccess>;
    async fn status(&self, company: Uuid) -> IdentityResult<CompanyStatus>;
    async fn set_status(&self, change: StatusChange) -> IdentityResult<CompanyStatus>;
    async fn revoke(&self, grant: &SecretHash) -> IdentityResult<()>;
}
#[async_trait]
pub trait AttributionOutbox: Send + Sync {
    async fn health(&self) -> IdentityResult<LifecycleHealth>;
    async fn claim(&self) -> IdentityResult<Vec<LifecycleClaim>>;
    async fn complete(&self, claim: &LifecycleClaim, result: DeliveryResult) -> IdentityResult<()>;
    async fn cleanup(&self) -> IdentityResult<()>;
}
