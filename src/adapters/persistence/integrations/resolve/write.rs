use super::*;
use crate::application::integrations::private::CredentialCacheUpdate;

#[derive(Clone, Copy)]
pub(super) enum CacheWrite {
    Renewed,
    Cooldown,
}
pub(super) async fn cache(
    vault: &PostgresVault,
    mut s: Locked,
    payload: SecretString,
    change: CacheWrite,
    i: &AuthorizedInvocation,
    work: &RenewalWork,
) -> ResolutionResult<ResolvedCredential> {
    let entry = vault.registry.get(&s.grant.app)?;
    entry.adapter.validate_cache_update(CredentialCacheUpdate {
        previous: &s.payload,
        replacement: &payload,
    })?;
    if entry.adapter.validate(&payload)? != s.account.external_identity {
        return Err(IntegrationError::Denied.into());
    }
    entry.adapter.validate_use(&payload, &entry.app)?;
    let transition = match change {
        CacheWrite::Renewed => CredentialChange::Refresh,
        CacheWrite::Cooldown => CredentialChange::RenewalState,
    };
    let next = s.versions.changed(transition)?;
    let provider = provider(vault, &s.grant.app)?;
    let decision = provider.assess(&payload, next.generation, Utc::now())?;
    let expires = match (&change, decision) {
        (CacheWrite::Renewed, RenewalDecision::Cached { expires_at }) => Some(expires_at),
        (CacheWrite::Cooldown, RenewalDecision::Cooldown(retry))
            if retry > Utc::now() && retry <= Utc::now() + chrono::Duration::seconds(60) =>
        {
            None
        }
        _ => return Err(IntegrationError::Credential.into()),
    };
    let envelope = vault
        .persistence
        .credential_cipher()
        .map_err(|_| IntegrationError::Credential)?
        .seal_envelope(
            &CredentialContext::integration_account(&s.account),
            &payload,
        )
        .map_err(|_| IntegrationError::Credential)?;
    locked::live(&mut s.tx, i, work).await?;
    accounts::save_payload(
        &mut s.tx,
        &s.account,
        next,
        Some(&envelope),
        AccountStatus::Ready,
        &s.account.metadata,
    )
    .await?;
    let expected = Observation {
        id: s.account.id,
        owner: s.account.owner.clone(),
        authorization: next.authorization,
        revision: next.revision,
        generation: next.generation,
        status: AccountStatus::Ready,
        envelope: Some(envelope),
        pending: None,
    };
    let result = match change {
        CacheWrite::Renewed => Some(resolved(vault, &s, &payload, next.generation, expires, i)?),
        CacheWrite::Cooldown => None,
    };
    commit::commit(vault, s.tx, &expected, work).await?;
    result.ok_or(ResolutionError::Retry(RetryReason::Cooldown, None))
}
pub(super) async fn reconnect(
    vault: &PostgresVault,
    mut s: Locked,
    work: &RenewalWork,
) -> ResolutionResult<ResolvedCredential> {
    let next = s.versions.changed(CredentialChange::Disconnect)?;
    accounts::save_payload(
        &mut s.tx,
        &s.account,
        next,
        None,
        AccountStatus::ReconnectRequired,
        &s.account.metadata,
    )
    .await?;
    super::super::grants::invalidate_account(&mut s.tx, s.account.id).await?;
    let expected = Observation {
        id: s.account.id,
        owner: s.account.owner.clone(),
        authorization: next.authorization,
        revision: next.revision,
        generation: next.generation,
        status: AccountStatus::ReconnectRequired,
        envelope: None,
        pending: None,
    };
    commit::commit(vault, s.tx, &expected, work).await?;
    Err(ResolutionError::ReconnectRequired)
}
pub(super) async fn request(
    vault: &PostgresVault,
    mut s: Locked,
    i: &AuthorizedInvocation,
    work: &RenewalWork,
) -> ResolutionResult<ResolvedCredential> {
    locked::live(&mut s.tx, i, work).await?;
    let provider = provider(vault, &s.grant.app)?;
    if !matches!(
        provider.assess(&s.payload, s.versions.generation, Utc::now())?,
        RenewalDecision::Request
    ) {
        return Err(IntegrationError::Credential.into());
    }
    let next = s.versions.generation.next()?;
    let deadline = work
        .deadline
        .min(Utc::now() + chrono::Duration::seconds(10))
        .min(
            s.pending
                .as_ref()
                .map(|p| p.deadline)
                .unwrap_or(work.deadline),
        );
    let request = RenewalWork {
        deadline,
        cancellation: work.cancellation.clone(),
    };
    let remaining = (deadline - Utc::now())
        .to_std()
        .map_err(|_| IntegrationError::Denied)?;
    let response = tokio::select! { biased;
        _=work.cancellation.cancelled()=>Err(RenewalFailure::Cancelled),
        _=tokio::time::sleep(remaining)=>Err(RenewalFailure::Temporary),
        result=provider.renew(&s.payload,next,&request)=>result,
    };
    match response {
        Ok(payload) => cache(vault, s, payload, CacheWrite::Renewed, i, work).await,
        Err(RenewalFailure::Reconnect) => reconnect(vault, s, work).await,
        Err(RenewalFailure::Temporary) if s.pending.is_none() => {
            let payload = provider.cooldown(&s.payload, Utc::now())?;
            cache(vault, s, payload, CacheWrite::Cooldown, i, work).await
        }
        Err(RenewalFailure::Temporary) => Err(ResolutionError::Retry(
            RetryReason::Pending,
            s.pending.map(|p| p.deadline),
        )),
        Err(RenewalFailure::Configuration) => Err(ResolutionError::Configuration),
        Err(RenewalFailure::Cancelled) => Err(ResolutionError::Cancelled),
        Err(RenewalFailure::Malformed) => Err(IntegrationError::Credential.into()),
    }
}
