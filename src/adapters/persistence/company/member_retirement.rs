//! Company removal shared by team management, root management and account deletion.
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    domain::workflow::CompanyId,
};

/// Caller holds the account and company locks. Capture recipients and revoke external authority
/// before demotion; the principal trigger authenticates held-role audit facts before clearing rows.
pub(crate) async fn retire_member_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    user: Uuid,
) -> AppResult<()> {
    let owner: Option<Uuid> = sqlx::query_scalar("SELECT user_id FROM companies WHERE id = $1")
        .bind(company.as_uuid())
        .fetch_optional(&mut **tx)
        .await?;
    if owner == Some(user) {
        return Err(AppError::BadRequest(
            "Cannot remove company owner from the team.".into(),
        ));
    }
    let principals: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM principals WHERE company_id = $1 AND user_id = $2 ORDER BY id FOR UPDATE",
    )
    .bind(company.as_uuid())
    .bind(user)
    .fetch_all(&mut **tx)
    .await?;
    sqlx::query(
        "SELECT id FROM department_members WHERE company_id = $1 AND root_user_id = $2 FOR UPDATE",
    )
    .bind(company.as_uuid())
    .bind(user)
    .fetch_all(&mut **tx)
    .await?;
    if super::root_member_on(tx, company, user).await?.is_none() {
        return Ok(());
    }
    let recipient = super::super::delivery::cancellation::member_notice_recipient_on(
        tx,
        company.as_uuid(),
        user,
    )
    .await?;
    super::super::delivery::cancellation::cancel_member_notices_on(
        tx,
        company.as_uuid(),
        recipient,
    )
    .await?;
    for principal in principals {
        super::super::memory::retire_namespaces_on(
            tx,
            company.as_uuid(),
            crate::application::memory::NamespaceOwner::Principal(
                crate::entities::transport::PrincipalId::new(principal),
            ),
        )
        .await?;
    }
    super::super::integrations::lifecycle::revoke_membership_on(
        tx,
        &crate::adapters::integrations::owner::owner_for_user(user),
        &crate::adapters::integrations::owner::company_binding(company.as_uuid()),
    )
    .await
    .map_err(AppError::from)?;
    // Keep roles until this update: its trigger records authenticated retirement snapshots first.
    sqlx::query(
        "UPDATE principals SET kind = 'external', user_id = NULL, updated_at = clock_timestamp() \
                 WHERE company_id = $1 AND user_id = $2",
    )
    .bind(company.as_uuid())
    .bind(user)
    .execute(&mut **tx)
    .await?;
    Box::pin(super::super::workflow::refresh_member_dependents_on(
        tx, company,
    ))
    .await
}
