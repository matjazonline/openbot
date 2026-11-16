//! The attribution-owned session and authority boundary. Transport and SQL live in adapters.
use super::freshness::{Freshness, Observation, Ticket};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::time::Instant;
use uuid::Uuid;

macro_rules! identity {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);
    };
}
identity!(CompanyId);
identity!(UserId);
identity!(SessionId);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Failure {
    TooLarge,
    Invalid,
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    Gone,
    Unavailable,
    RateLimited,
}
pub type Result<T> = std::result::Result<T, Failure>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    Active,
    Suspended,
    Deleted,
}
impl LifecycleState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Suspended => "suspended",
            Self::Deleted => "deleted",
        }
    }
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "active" => Ok(Self::Active),
            "suspended" => Ok(Self::Suspended),
            "deleted" => Ok(Self::Deleted),
            _ => Err(Failure::Unavailable),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Permission {
    #[serde(rename = "read")]
    Read,
    #[serde(rename = "links:manage")]
    ManageLinks,
    #[serde(rename = "integrations:manage")]
    ManageIntegrations,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyStatus {
    pub company_id: CompanyId,
    pub version: i64,
    pub state: LifecycleState,
    pub changed_at: DateTime<Utc>,
}
impl CompanyStatus {
    pub fn validate(&self, id: CompanyId) -> Result<()> {
        if self.company_id != id || id.0.is_nil() || self.version < 1 {
            return Err(Failure::Unavailable);
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyAccess {
    pub company_id: CompanyId,
    pub name: String,
    pub permissions: Vec<Permission>,
    pub lifecycle: CompanyStatus,
}
impl CompanyAccess {
    pub fn validate(&self, id: CompanyId) -> Result<()> {
        self.lifecycle.validate(id)?;
        if self.company_id != id
            || self.name.is_empty()
            || self.name.chars().count() > 120
            || self.permissions.len() > 3
        {
            return Err(Failure::Unavailable);
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyPage {
    pub items: Vec<CompanyAccess>,
    pub next_cursor: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub user_id: UserId,
    pub display_name: String,
    pub expires_at: DateTime<Utc>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u32,
}

// Secret-bearing types intentionally have no Debug implementation.
pub struct SessionToken(pub String);
pub struct IdentityGrant(pub String);
pub struct LoginState(pub String);
pub struct BrowserBinding(pub String);
pub struct Code(pub String);
pub struct PkceVerifier(pub String);
pub struct CsrfToken(pub String);
pub struct LoginTransaction {
    pub id: Uuid,
    pub state: LoginState,
    pub browser: BrowserBinding,
    pub pkce: PkceVerifier,
    pub return_path: String,
}
pub struct LoginCompletion {
    pub pkce: PkceVerifier,
    pub return_path: String,
}
pub struct NewSession {
    pub id: SessionId,
    pub token: SessionToken,
    pub user_id: UserId,
    pub grant: IdentityGrant,
    pub csrf: CsrfToken,
    pub expires_at: DateTime<Utc>,
}
pub struct Session {
    pub id: SessionId,
    pub user_id: UserId,
    pub grant: IdentityGrant,
    pub csrf: CsrfToken,
    pub expires_at: DateTime<Utc>,
}
pub struct Principal {
    pub session: Session,
    pub identity: Identity,
}
pub struct TenantPrincipal {
    pub principal: Principal,
    pub access: CompanyAccess,
    pub ticket: Ticket,
}
pub struct ReconcileClaim {
    pub company_id: CompanyId,
    pub execution: Uuid,
}

#[async_trait]
pub trait Authority: Send + Sync {
    async fn exchange(&self, code: &Code, verifier: &PkceVerifier) -> Result<IdentityGrant>;
    async fn identity(&self, grant: &IdentityGrant) -> Result<Identity>;
    async fn companies(
        &self,
        grant: &IdentityGrant,
        limit: u16,
        cursor: Option<&str>,
    ) -> Result<CompanyPage>;
    async fn access(&self, grant: &IdentityGrant, company: CompanyId) -> Result<CompanyAccess>;
    async fn status(&self, company: CompanyId) -> Result<CompanyStatus>;
    async fn revoke(&self, grant: &IdentityGrant) -> Result<()>;
}
#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn begin_login(&self, transaction: &LoginTransaction) -> Result<()>;
    async fn consume_login(
        &self,
        state: &LoginState,
        browser: &BrowserBinding,
    ) -> Result<LoginCompletion>;
    async fn create_session(&self, session: &NewSession, old: Option<&SessionToken>) -> Result<()>;
    async fn session(&self, token: &SessionToken) -> Result<Session>;
    /// Retained target, including revoked sessions; only for CSRF-validated logout retries.
    async fn logout_session(&self, token: &SessionToken) -> Result<Session>;
    async fn revoke_session(&self, session: SessionId) -> Result<()>;
}
#[async_trait]
pub trait TenantStore: Send + Sync {
    async fn health(&self) -> Result<CompanyStatusHealth>;
    async fn observation(&self) -> Result<Observation>;
    async fn observe(
        &self,
        status: &CompanyStatus,
        name: Option<&str>,
        observation: Option<&Observation>,
        claim: Option<&ReconcileClaim>,
    ) -> Result<()>;
    async fn check_tenant(&self, company: CompanyId, ticket: &Ticket) -> Result<()>;
    async fn claim_reconciliation(&self) -> Result<Vec<ReconcileClaim>>;
    async fn finish_reconciliation(&self, claim: &ReconcileClaim, success: bool) -> Result<()>;
    async fn cleanup_sessions(&self) -> Result<()>;
}

#[derive(Debug, PartialEq, Eq)]
pub struct CompanyStatusHealth {
    pub expired: bool,
    pub reconciliation_failed: bool,
}

pub enum RefreshedTenant {
    Active(Ticket),
    Inactive,
}

pub struct IdentityService {
    pub authority: Arc<dyn Authority>,
    pub sessions: Arc<dyn SessionStore>,
    pub tenants: Arc<dyn TenantStore>,
    pub freshness: Freshness,
}
impl IdentityService {
    pub fn new(
        authority: Arc<dyn Authority>,
        sessions: Arc<dyn SessionStore>,
        tenants: Arc<dyn TenantStore>,
    ) -> Self {
        Self {
            authority,
            sessions,
            tenants,
            freshness: Freshness::default(),
        }
    }
    pub async fn authenticate(&self, token: &SessionToken) -> Result<Principal> {
        let session = self.sessions.session(token).await?;
        let identity = self.authority.identity(&session.grant).await?;
        if identity.user_id != session.user_id
            || identity.expires_at <= Utc::now()
            || identity.user_id.0.is_nil()
        {
            return Err(Failure::Unauthorized);
        }
        Ok(Principal { session, identity })
    }
    pub async fn company(
        &self,
        token: &SessionToken,
        company: CompanyId,
        permission: Permission,
    ) -> Result<TenantPrincipal> {
        let principal = self.authenticate(token).await?;
        let started = Instant::now();
        let observation = self.tenants.observation().await?;
        let access = self
            .authority
            .access(&principal.session.grant, company)
            .await?;
        access.validate(company)?;
        self.tenants
            .observe(
                &access.lifecycle,
                Some(&access.name),
                Some(&observation),
                None,
            )
            .await?;
        let ticket = self.freshness.record(&access.lifecycle, started)?;
        self.tenants.check_tenant(company, &ticket).await?;
        if !access.permissions.contains(&permission) {
            return Err(Failure::Forbidden);
        }
        Ok(TenantPrincipal {
            principal,
            access,
            ticket,
        })
    }
    pub async fn tenant(&self, company: CompanyId) -> Result<Ticket> {
        if let Some(ticket) = self.freshness.ticket(company) {
            self.tenants.check_tenant(company, &ticket).await?;
            return Ok(ticket);
        }
        match self.refresh(company, None).await? {
            RefreshedTenant::Active(ticket) => Ok(ticket),
            RefreshedTenant::Inactive => Err(Failure::Forbidden),
        }
    }
    pub async fn refresh(
        &self,
        company: CompanyId,
        claim: Option<&ReconcileClaim>,
    ) -> Result<RefreshedTenant> {
        let started = Instant::now();
        let observation = self.tenants.observation().await?;
        let status = self.authority.status(company).await?;
        status.validate(company)?;
        self.tenants
            .observe(&status, None, Some(&observation), claim)
            .await?;
        if status.state != LifecycleState::Active {
            self.freshness.invalidate(company);
            return Ok(RefreshedTenant::Inactive);
        }
        let ticket = self.freshness.record(&status, started)?;
        self.tenants.check_tenant(company, &ticket).await?;
        Ok(RefreshedTenant::Active(ticket))
    }
    pub async fn logout(&self, token: &SessionToken, csrf: &str) -> Result<()> {
        let session = self.sessions.logout_session(token).await?;
        super::identity::verify_csrf(&session.csrf, csrf)?;
        self.sessions.revoke_session(session.id).await?;
        // Local revocation is authoritative even if upstream is unavailable.
        let _ = self.authority.revoke(&session.grant).await;
        Ok(())
    }
}
pub fn verify_csrf(expected: &CsrfToken, supplied: &str) -> Result<()> {
    use subtle::ConstantTimeEq;
    if supplied.len() > 128 || !bool::from(expected.0.as_bytes().ct_eq(supplied.as_bytes())) {
        return Err(Failure::Forbidden);
    }
    Ok(())
}
