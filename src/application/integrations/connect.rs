//! Shared connect lifecycle; provider credentials never leave these private ports.
use super::{registry::CodeOwnedRegistry, *};
use crate::domain::integrations::*;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub(crate) struct ConnectBrowser {
    pub session: ConnectSessionReference,
    pub binding: BrowserBinding,
}
impl std::fmt::Debug for ConnectBrowser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ConnectBrowser([redacted])")
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttemptFacts {
    pub id: ConnectAttemptId,
    pub owner: AccountOwner,
    pub provider: ProviderId,
    pub config: AuthConfigId,
    pub app: AppId,
    pub policy: AppPolicyDigest,
    pub capabilities: Vec<Capability>,
    pub browser: BrowserBinding,
    pub nonce: ConnectNonce,
    pub return_target: SafeReturnTarget,
    pub reconnect: Option<ReconnectTarget>,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}
impl AttemptFacts {
    pub fn same(&self, other: &Self) -> IntegrationResult<bool> {
        Ok(
            serde_json::to_value(self).map_err(|_| IntegrationError::Invalid)?
                == serde_json::to_value(other).map_err(|_| IntegrationError::Invalid)?,
        )
    }
    pub fn purpose(&self) -> &'static str {
        if self.reconnect.is_some() {
            "reconnect"
        } else {
            "new"
        }
    }
}
pub(crate) struct ConsumedAttempt {
    pub facts: AttemptFacts,
    pub verifier: SecretString,
}
pub(crate) struct AuthorizedAccount {
    pub identity: ExternalIdentity,
    pub payload: SecretString,
    pub metadata: AccountMetadata,
}
pub(crate) struct AuthorizationRequest<'a> {
    pub app: &'a AppDefinition,
    pub state: &'a str,
    pub challenge: &'a str,
}
pub(crate) struct ProviderExchange<'a> {
    pub app: &'a AppDefinition,
    pub code: &'a SecretString,
    pub verifier: &'a SecretString,
    pub deadline: DateTime<Utc>,
    pub cancellation: &'a CancellationToken,
}
pub(crate) struct FinalizeAuthorization<'a> {
    pub app: &'a AppDefinition,
    pub candidate: &'a SecretString,
    pub previous: Option<&'a super::private::CredentialRecord>,
    pub generation: CredentialGeneration,
}
#[async_trait]
pub(crate) trait AuthorizationProvider: Send + Sync {
    fn callback_error(&self, error: &str) -> ConnectStatus;
    fn authorization_url(
        &self,
        request: AuthorizationRequest<'_>,
    ) -> IntegrationResult<SanitizedConnectUrl>;
    async fn exchange(&self, request: ProviderExchange<'_>) -> Result<SecretString, ConnectStatus>;
    fn finalize(
        &self,
        request: FinalizeAuthorization<'_>,
    ) -> Result<AuthorizedAccount, ConnectStatus>;
}
pub(crate) trait ConnectStateCodec: Send + Sync {
    fn sign(&self, facts: &AttemptFacts) -> IntegrationResult<String>;
    fn verify(&self, state: &str) -> IntegrationResult<AttemptFacts>;
}
#[async_trait]
pub(crate) trait ConnectStore: Send + Sync {
    async fn start(
        &self,
        facts: &AttemptFacts,
        verifier: SecretString,
    ) -> IntegrationResult<ConnectAttempt>;
    async fn consume(&self, facts: &AttemptFacts) -> IntegrationResult<ConsumedAttempt>;
    async fn status(
        &self,
        owner: &AccountOwner,
        id: ConnectAttemptId,
    ) -> IntegrationResult<ConnectAttempt>;
    async fn terminal(
        &self,
        owner: &AccountOwner,
        id: ConnectAttemptId,
        status: ConnectStatus,
    ) -> IntegrationResult<ConnectAttempt>;
    async fn finalize(
        &self,
        facts: &AttemptFacts,
        candidate: &SecretString,
        valid_until: DateTime<Utc>,
    ) -> IntegrationResult<ConnectAttempt>;
}
#[async_trait]
pub(crate) trait ConnectFinalizer: Send + Sync {
    async fn finish(&self, session_valid_until: DateTime<Utc>)
    -> IntegrationResult<ConnectAttempt>;
}
#[async_trait]
pub(crate) trait ConnectAuthority: Send + Sync {
    async fn complete(
        &self,
        owner: &VerifiedOwner,
        finalizer: &dyn ConnectFinalizer,
    ) -> IntegrationResult<()>;
}
pub(crate) struct ConnectService {
    pub registry: Arc<CodeOwnedRegistry>,
    pub store: Arc<dyn ConnectStore>,
    pub codec: Arc<dyn ConnectStateCodec>,
    pub authority: Arc<dyn ConnectAuthority>,
}
fn browser(owner: &VerifiedOwner) -> IntegrationResult<&ConnectBrowser> {
    owner.browser.as_ref().ok_or(IntegrationError::Denied)
}
impl ConnectService {
    fn current(&self, facts: &AttemptFacts) -> IntegrationResult<&registry::Registration> {
        let entry = self.registry.get(&facts.app)?;
        if entry.provider.id != facts.provider
            || entry.config.id != facts.config
            || self.registry.policy(&facts.app)? != facts.policy
            || entry.authorization.is_none()
        {
            return Err(IntegrationError::Denied);
        }
        Ok(entry)
    }
    async fn exchange_and_finish(
        &self,
        owner: &VerifiedOwner,
        c: ConnectCallback,
        w: ConnectWork,
        facts: AttemptFacts,
    ) -> IntegrationResult<ConnectAttempt> {
        let consumed = self.store.consume(&facts).await?;
        if !consumed.facts.same(&facts)? {
            return Err(IntegrationError::Denied);
        }
        let provider = self
            .current(&facts)?
            .authorization
            .as_ref()
            .ok_or(IntegrationError::Unavailable)?;
        if let Some(error) = c.error {
            let status = provider.callback_error(&error);
            return self.store.terminal(owner.owner(), facts.id, status).await;
        }
        let code = c.code.ok_or(IntegrationError::Invalid)?;
        let deadline = w
            .deadline
            .min(Utc::now() + chrono::Duration::seconds(10))
            .min(facts.expires_at);
        let request = ProviderExchange {
            app: &self.current(&facts)?.app,
            code: &code,
            verifier: &consumed.verifier,
            deadline,
            cancellation: &w.cancellation,
        };
        let remaining = (deadline - Utc::now())
            .to_std()
            .map_err(|_| IntegrationError::Denied)?;
        let candidate = tokio::select! {biased;
            _=w.cancellation.cancelled()=>Err(ConnectStatus::Cancelled),
            _=tokio::time::sleep(remaining)=>Err(ConnectStatus::ProviderError),
            result=provider.exchange(request)=>result,
        };
        let candidate = match candidate {
            Ok(value) => value,
            Err(status) => return self.store.terminal(owner.owner(), facts.id, status).await,
        };
        let finalizer = Completion {
            service: self,
            facts: &facts,
            candidate: &candidate,
            deadline: w.deadline,
            called: AtomicBool::new(false),
        };
        let remaining = (w.deadline - Utc::now())
            .to_std()
            .map_err(|_| IntegrationError::Denied)?;
        tokio::select! {biased;
            _=w.cancellation.cancelled()=>return self.store.terminal(owner.owner(),facts.id,ConnectStatus::Cancelled).await,
            _=tokio::time::sleep(remaining)=>return Err(IntegrationError::Denied),
            result=self.authority.complete(owner,&finalizer)=>result?,
        }
        if !finalizer.called.load(Ordering::Acquire) {
            return Err(IntegrationError::Denied);
        }
        self.store.status(owner.owner(), facts.id).await
    }
}
struct Completion<'a> {
    service: &'a ConnectService,
    facts: &'a AttemptFacts,
    candidate: &'a SecretString,
    deadline: DateTime<Utc>,
    called: AtomicBool,
}
#[async_trait]
impl ConnectFinalizer for Completion<'_> {
    async fn finish(
        &self,
        session_valid_until: DateTime<Utc>,
    ) -> IntegrationResult<ConnectAttempt> {
        let result = self
            .service
            .store
            .finalize(
                self.facts,
                self.candidate,
                session_valid_until.min(self.deadline),
            )
            .await?;
        self.called.store(true, Ordering::Release);
        Ok(result)
    }
}
#[async_trait]
impl ConnectGateway for ConnectService {
    async fn connectable_apps(&self) -> IntegrationResult<Vec<ConnectApp>> {
        Ok(self.registry.connectable())
    }
    async fn begin_connect(
        &self,
        owner: &VerifiedOwner,
        command: BeginConnect,
    ) -> IntegrationResult<ConnectStart> {
        use base64::Engine;
        use sha2::{Digest, Sha256};
        let entry = self.registry.get(&command.app)?;
        let provider = entry
            .authorization
            .as_ref()
            .ok_or(IntegrationError::Unavailable)?;
        let requested: Vec<Capability> = command.requested_capabilities.into();
        if command.config != entry.config.id
            || requested.iter().collect::<std::collections::BTreeSet<_>>()
                != entry.app.capabilities.iter().collect()
        {
            return Err(IntegrationError::Denied);
        }
        let now = Utc::now();
        let facts = AttemptFacts {
            id: ConnectAttemptId::new(uuid::Uuid::new_v4()),
            owner: owner.owner().clone(),
            provider: entry.provider.id.clone(),
            config: entry.config.id.clone(),
            app: entry.app.id.clone(),
            policy: self.registry.policy(&entry.app.id)?,
            capabilities: requested,
            browser: browser(owner)?.binding.clone(),
            nonce: ConnectNonce::parse(uuid::Uuid::new_v4().to_string())?,
            return_target: command.return_target,
            reconnect: command.reconnect,
            issued_at: now,
            expires_at: now + chrono::Duration::minutes(10),
        };
        if serde_json::to_vec(&facts)
            .map_err(|_| IntegrationError::Invalid)?
            .len()
            > MAX_METADATA_BYTES
        {
            return Err(IntegrationError::Limit);
        }
        let verifier = SecretString::from(format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        ));
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(Sha256::digest(verifier.expose_secret().as_bytes()));
        let state = self.codec.sign(&facts)?;
        let url = provider.authorization_url(AuthorizationRequest {
            app: &entry.app,
            state: &state,
            challenge: &challenge,
        })?;
        let attempt = self.store.start(&facts, verifier).await?;
        Ok(ConnectStart {
            attempt,
            authorization_url: url,
        })
    }
    async fn finish_connect(
        &self,
        owner: &VerifiedOwner,
        c: ConnectCallback,
        w: ConnectWork,
    ) -> IntegrationResult<ConnectAttempt> {
        if c.state.len() > 4096
            || c.code
                .as_ref()
                .is_some_and(|v| v.expose_secret().is_empty() || v.expose_secret().len() > 4096)
            || c.error.as_ref().is_some_and(|v| v.len() > 256)
            || (c.code.is_some() == c.error.is_some())
        {
            return Err(IntegrationError::Invalid);
        }
        let facts = self.codec.verify(&c.state)?;
        if &facts.owner != owner.owner()
            || facts.browser != browser(owner)?.binding
            || facts.provider != c.provider
            || facts.expires_at <= Utc::now()
        {
            return Err(IntegrationError::Denied);
        }
        self.current(&facts)?;
        self.exchange_and_finish(owner, c, w, facts).await
    }
    async fn get_connect_attempt(
        &self,
        owner: &VerifiedOwner,
        id: ConnectAttemptId,
    ) -> IntegrationResult<ConnectAttempt> {
        self.store.status(owner.owner(), id).await
    }
    async fn cancel_connect(
        &self,
        owner: &VerifiedOwner,
        id: ConnectAttemptId,
    ) -> IntegrationResult<ConnectAttempt> {
        self.store
            .terminal(owner.owner(), id, ConnectStatus::Cancelled)
            .await
    }
}
