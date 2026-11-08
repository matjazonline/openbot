use super::*;
#[derive(PartialEq, Eq)]
pub(super) struct Observation {
    pub id: AccountId,
    pub owner: AccountOwner,
    pub authorization: AuthorizationRevision,
    pub revision: AccountRevision,
    pub generation: CredentialGeneration,
    pub status: AccountStatus,
    pub envelope: Option<String>,
    pub pending: Option<PendingRenewal>,
}
impl Observation {
    pub fn current(s: &Locked) -> Self {
        Self {
            id: s.account.id,
            owner: s.account.owner.clone(),
            authorization: s.versions.authorization,
            revision: s.versions.revision,
            generation: s.versions.generation,
            status: s.account.status,
            envelope: Some(s.envelope.clone()),
            pending: s.pending.clone(),
        }
    }
}
pub(super) async fn commit(
    vault: &PostgresVault,
    tx: Tx,
    expected: &Observation,
    work: &RenewalWork,
) -> ResolutionResult<()> {
    if tx.commit().await.is_ok() {
        return Ok(());
    }
    // A fresh lock waits for the original transaction to settle before observing.
    match locked::observe(vault, expected, work).await {
        Ok(actual) if actual == *expected => Ok(()),
        Ok(actual) if actual.authorization != expected.authorization => {
            Err(IntegrationError::Denied.into())
        }
        _ => Err(ResolutionError::Retry(RetryReason::AmbiguousCommit, None)),
    }
}
