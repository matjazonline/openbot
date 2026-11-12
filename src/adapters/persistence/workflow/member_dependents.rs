//! Membership mutations refresh both current human assignments and escalation capability.
use super::*;

/// Called after the final membership/account state, under the already-owned company lock.
pub(crate) async fn refresh_member_dependents_on(
    db: &mut PgConnection,
    company: CompanyId,
) -> AppResult<()> {
    Box::pin(follow_reviewers::refresh_on(db, company)).await?;
    Box::pin(crate::adapters::persistence::notification::refresh_person_escalations_on(db, company))
        .await
}
