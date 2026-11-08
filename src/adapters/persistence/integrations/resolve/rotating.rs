use super::*;
#[derive(sqlx::FromRow)]
struct Reserved {
    renewal_attempt_id: Uuid,
    renewal_generation: i64,
    renewal_started_at: DateTime<Utc>,
    renewal_deadline: DateTime<Utc>,
}
pub(super) async fn reserve(
    vault: &PostgresVault,
    mut s: Locked,
    work: &RenewalWork,
) -> ResolutionResult<PendingRenewal> {
    let row:Reserved=sqlx::query_as("WITH clock AS MATERIALIZED(SELECT clock_timestamp() AS now) UPDATE integration_accounts SET renewal_attempt_id=$2,renewal_generation=credential_generation,renewal_started_at=clock.now,renewal_deadline=LEAST(clock.now+interval '10 seconds',$3) FROM clock WHERE id=$1 AND renewal_attempt_id IS NULL AND clock.now<$3 RETURNING renewal_attempt_id,renewal_generation,renewal_started_at,renewal_deadline")
        .bind(s.account.id.as_uuid()).bind(Uuid::new_v4()).bind(work.deadline).fetch_optional(&mut *s.tx).await.map_err(db_error)?.ok_or(IntegrationError::Denied)?;
    let pending = PendingRenewal {
        id: row.renewal_attempt_id,
        generation: CredentialGeneration::new(row.renewal_generation)?,
        started: row.renewal_started_at,
        deadline: row.renewal_deadline,
    };
    let mut expected = Observation::current(&s);
    expected.pending = Some(pending.clone());
    commit::commit(vault, s.tx, &expected, work).await?;
    Ok(pending)
}
pub(super) async fn execute(
    vault: &PostgresVault,
    pending: &PendingRenewal,
    i: &AuthorizedInvocation,
    context: &ExecutionContext,
    work: &RenewalWork,
) -> ResolutionResult<ResolvedCredential> {
    vault.final_dispatch_guard(i, context).await?;
    let mut s = locked::lock(vault, i, work).await?;
    if s.pending.as_ref() != Some(pending) || s.versions.generation != pending.generation {
        return Err(IntegrationError::Denied.into());
    }
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *s.tx)
        .await
        .map_err(db_error)?;
    if now >= pending.deadline {
        return write::reconnect(vault, s, work).await;
    }
    write::request(vault, s, i, work).await
}
