//! Entry-scoped credential transactions. SQLx and plaintext never leave this adapter.
mod commit;
mod locked;
mod rotating;
mod write;
use super::{PostgresVault, accounts};
use crate::{
    adapters::persistence::credentials::envelope::CredentialContext,
    application::integrations::{AuthorizedInvocation, ExecutionContext, renewal::*},
    domain::integrations::*,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use commit::Observation;
use locked::{Locked, PendingRenewal};
use secrecy::SecretString;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;
type Tx = Transaction<'static, Postgres>;
fn db_error(error: sqlx::Error) -> ResolutionError {
    match error.as_database_error().and_then(|e| e.code()).as_deref() {
        Some("55P03" | "57014") => ResolutionError::Retry(RetryReason::LockBusy, None),
        _ => super::storage_error(error).into(),
    }
}
fn provider(
    vault: &PostgresVault,
    app: &AppId,
) -> ResolutionResult<std::sync::Arc<dyn RenewalProvider>> {
    match &vault.registry.get(app)?.renewal {
        RenewalStrategy::ReusableRefresh(p) | RenewalStrategy::StrictRotating(p) => Ok(p.clone()),
        RenewalStrategy::NoRenewal => Err(IntegrationError::Unavailable.into()),
    }
}
fn resolved(
    vault: &PostgresVault,
    s: &Locked,
    payload: &SecretString,
    generation: CredentialGeneration,
    expires_at: Option<DateTime<Utc>>,
    i: &AuthorizedInvocation,
) -> ResolutionResult<ResolvedCredential> {
    use crate::application::integrations::private::InjectionMethod;
    use crate::domain::integrations::authority::InvocationKind;
    use secrecy::ExposeSecret;
    let entry = vault.registry.get(&s.grant.app)?;
    let injection = entry.adapter.injection(payload)?;
    let max = match s.grant.purpose {
        InvocationKind::DeterministicHttp => crate::application::workflow::http::MAX_HTTP_SECRET,
        InvocationKind::DeterministicMcp if matches!(injection.method, InjectionMethod::Bearer) => {
            crate::domain::entities::mcp::MAX_MCP_SECRET_BYTES
        }
        _ => return Err(IntegrationError::Denied.into()),
    };
    if injection.value.expose_secret().len() > max {
        return Err(IntegrationError::Credential.into());
    }
    let session_policy = entry.renewal.session_policy();
    Ok(ResolvedCredential {
        injection,
        authorization: i.authorization.clone(),
        generation,
        expires_at,
        session_policy,
    })
}
async fn under_lock(
    vault: &PostgresVault,
    i: &AuthorizedInvocation,
    context: &ExecutionContext,
    work: &RenewalWork,
) -> ResolutionResult<ResolvedCredential> {
    let mut s = locked::lock(vault, i, work).await?;
    let entry = vault.registry.get(&s.grant.app)?;
    if let Some(pending) = &s.pending {
        if !matches!(entry.renewal, RenewalStrategy::StrictRotating(_)) {
            return Err(IntegrationError::Credential.into());
        }
        let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(&mut *s.tx)
            .await
            .map_err(db_error)?;
        if now >= pending.deadline {
            return write::reconnect(vault, s, work).await;
        }
        return Err(ResolutionError::Retry(
            RetryReason::Pending,
            Some(pending.deadline),
        ));
    }
    let provider = match &entry.renewal {
        RenewalStrategy::NoRenewal => {
            let result = resolved(vault, &s, &s.payload, s.versions.generation, None, i)?;
            s.tx.rollback().await.map_err(db_error)?;
            return Ok(result);
        }
        RenewalStrategy::ReusableRefresh(p) | RenewalStrategy::StrictRotating(p) => p.clone(),
    };
    match provider.assess(&s.payload, s.versions.generation, Utc::now())? {
        RenewalDecision::Cached { expires_at } => {
            let result = resolved(
                vault,
                &s,
                &s.payload,
                s.versions.generation,
                Some(expires_at),
                i,
            )?;
            s.tx.rollback().await.map_err(db_error)?;
            Ok(result)
        }
        RenewalDecision::Cooldown(at) => {
            Err(ResolutionError::Retry(RetryReason::Cooldown, Some(at)))
        }
        RenewalDecision::Request => match &entry.renewal {
            RenewalStrategy::StrictRotating(_) => {
                let pending = rotating::reserve(vault, s, work).await?;
                rotating::execute(vault, &pending, i, context, work).await
            }
            _ => write::request(vault, s, i, work).await,
        },
    }
}
#[async_trait]
impl CredentialResolver for PostgresVault {
    async fn resolve(
        &self,
        i: &AuthorizedInvocation,
        context: &ExecutionContext,
    ) -> ResolutionResult<ResolvedCredential> {
        let deadline = i.deadline.min(Utc::now() + chrono::Duration::seconds(30));
        let remaining = (deadline - Utc::now())
            .to_std()
            .map_err(|_| IntegrationError::Denied)?;
        let work = RenewalWork {
            deadline,
            cancellation: context.cancellation.clone(),
        };
        tokio::select! { biased;
            _=context.cancellation.cancelled()=>Err(ResolutionError::Cancelled),
            _=tokio::time::sleep(remaining)=>Err(ResolutionError::Retry(RetryReason::Unavailable,None)),
            result=async {
                self.final_dispatch_guard(i,context).await?;
                let resolved=Box::pin(under_lock(self,i,context,&work)).await?;
                self.final_dispatch_guard(i,context).await?;
                if resolved.expires_at.is_some_and(|e|e<Utc::now()+chrono::Duration::seconds(60)) {
                    return Err(ResolutionError::Retry(RetryReason::TokenMargin,None));
                }
                Ok(resolved)
            }=>result,
        }
    }
}

#[cfg(test)]
#[path = "../renewal_tests/protocol.rs"]
pub(crate) mod testsupport;
