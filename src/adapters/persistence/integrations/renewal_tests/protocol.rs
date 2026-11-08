//! Test-only access to actual private reserve/execute stages; no fault flags or SQLx escape.
use super::*;
#[derive(Clone)]
pub(crate) struct Claim(PendingRenewal);
fn work(i: &AuthorizedInvocation, c: &ExecutionContext) -> RenewalWork {
    RenewalWork {
        deadline: i.deadline.min(Utc::now() + chrono::Duration::seconds(30)),
        cancellation: c.cancellation.clone(),
    }
}
pub(crate) async fn reserve(
    vault: &PostgresVault,
    i: &AuthorizedInvocation,
    c: &ExecutionContext,
) -> ResolutionResult<Claim> {
    vault.final_dispatch_guard(i, c).await?;
    let work = work(i, c);
    let s = locked::lock(vault, i, &work).await?;
    if s.pending.is_some()
        || !matches!(
            vault.registry.get(&s.grant.app)?.renewal,
            RenewalStrategy::StrictRotating(_)
        )
        || !matches!(
            provider(vault, &s.grant.app)?.assess(&s.payload, s.versions.generation, Utc::now())?,
            RenewalDecision::Request
        )
    {
        return Err(IntegrationError::Denied.into());
    }
    rotating::reserve(vault, s, &work).await.map(Claim)
}
pub(crate) async fn complete(
    vault: &PostgresVault,
    claim: &Claim,
    i: &AuthorizedInvocation,
    c: &ExecutionContext,
) -> ResolutionResult<ResolvedCredential> {
    let work = work(i, c);
    let resolved = rotating::execute(vault, &claim.0, i, c, &work).await?;
    vault.final_dispatch_guard(i, c).await?;
    Ok(resolved)
}
pub(crate) async fn expire(vault: &PostgresVault, account: &ConnectedAccount, claim: &Claim) {
    let result=sqlx::query("WITH clock AS MATERIALIZED(SELECT clock_timestamp() AS now) UPDATE integration_accounts SET renewal_started_at=clock.now-interval '11 seconds',renewal_deadline=clock.now-interval '1 second' FROM clock WHERE id=$1 AND renewal_attempt_id=$2")
        .bind(account.id.as_uuid()).bind(claim.0.id).execute(vault.persistence.pool()).await.unwrap();
    assert_eq!(result.rows_affected(), 1);
}
pub(crate) async fn pending(vault: &PostgresVault, account: &ConnectedAccount) -> bool {
    sqlx::query_scalar(
        "SELECT renewal_attempt_id IS NOT NULL FROM integration_accounts WHERE id=$1",
    )
    .bind(account.id.as_uuid())
    .fetch_one(vault.persistence.pool())
    .await
    .unwrap()
}

#[derive(sqlx::FromRow)]
pub(crate) struct Settings {
    pub lock_timeout: String,
    pub statement_timeout: String,
    pub idle_timeout: String,
    pub pid: i32,
}
pub(crate) async fn outside_settings(vault: &PostgresVault) -> Settings {
    sqlx::query_as("SELECT current_setting('lock_timeout') AS lock_timeout,current_setting('statement_timeout') AS statement_timeout,current_setting('idle_in_transaction_session_timeout') AS idle_timeout,pg_backend_pid() AS pid")
        .fetch_one(vault.persistence.pool()).await.unwrap()
}
pub(crate) async fn resolver_settings(vault: &PostgresVault) -> Settings {
    let work = RenewalWork {
        deadline: Utc::now() + chrono::Duration::seconds(20),
        cancellation: tokio_util::sync::CancellationToken::new(),
    };
    let mut tx = locked::begin(vault, &work).await.unwrap();
    let value=sqlx::query_as("SELECT current_setting('lock_timeout') AS lock_timeout,current_setting('statement_timeout') AS statement_timeout,current_setting('idle_in_transaction_session_timeout') AS idle_timeout,pg_backend_pid() AS pid")
        .fetch_one(&mut *tx).await.unwrap();
    tx.rollback().await.unwrap();
    value
}
