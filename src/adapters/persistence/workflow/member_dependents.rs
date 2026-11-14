//! Membership mutations refresh both current human assignments and escalation capability.
use super::*;

/// The caller owns the account FOR UPDATE. Company locks precede identity/principal writes,
/// matching account retirement without taking its deletion-only membership/owner locks.
pub(crate) async fn prelock_account_companies_on(
    db: &mut PgConnection,
    user: Uuid,
) -> AppResult<Vec<CompanyId>> {
    let companies: Vec<Uuid> = sqlx::query_scalar(
        "SELECT company.id FROM companies AS company WHERE company.user_id=$1 \
         OR EXISTS (SELECT 1 FROM company_members AS member WHERE member.company_id=company.id AND member.user_id=$1) \
         OR EXISTS (SELECT 1 FROM principals AS principal WHERE principal.company_id=company.id AND principal.user_id=$1 AND principal.kind='person') \
         ORDER BY company.id FOR NO KEY UPDATE",
    )
    .bind(user)
    .fetch_all(db)
    .await?;
    Ok(companies.into_iter().map(CompanyId::new).collect())
}

/// Refresh final proof/grants under the same account/company ownership; no recipient accounts.
pub(crate) async fn refresh_account_dependents_on(
    db: &mut PgConnection,
    companies: &[CompanyId],
) -> AppResult<()> {
    for company in companies {
        Box::pin(refresh_member_dependents_on(db, *company)).await?;
    }
    Ok(())
}

/// Called after the final membership/account state, under the already-owned company lock.
pub(crate) async fn refresh_member_dependents_on(
    db: &mut PgConnection,
    company: CompanyId,
) -> AppResult<()> {
    Box::pin(follow_reviewers::refresh_on(db, company)).await?;
    Box::pin(crate::adapters::persistence::notification::refresh_person_escalations_on(db, company))
        .await
}
