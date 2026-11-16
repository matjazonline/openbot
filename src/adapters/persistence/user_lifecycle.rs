use super::{PostgresPersistence, integrations::lifecycle};
use crate::{
    adapters::integrations::owner::{client_namespace, company_binding, owner_for_user},
    app_error::{AppError, AppResult},
    application::ports::account_deletion::HostAccountDeletion,
};
use async_trait::async_trait;
use uuid::Uuid;
#[async_trait]
impl HostAccountDeletion for PostgresPersistence {
    async fn delete_user_account(&self, user: Uuid) -> AppResult<()> {
        let mut tx = self.pool().begin().await.map_err(AppError::from)?;
        let locks = lock_account_on(&mut tx, user).await?;
        finish_account_deletion_on(&mut tx, user, locks).await?;
        tx.commit().await.map_err(AppError::from)?;
        Ok(())
    }
}

/// The rows an account deletion holds before it writes anything.
pub(in crate::adapters::persistence) struct AccountLocks {
    owned: Vec<Uuid>,
    memberships: Vec<Uuid>,
}

/// The account first (serializing against membership and ownership writes), then every company it
/// touches in one id-ordered pass: owned ones `FOR UPDATE` (they are deleted), member companies
/// `FOR NO KEY UPDATE`. Only then the membership rows. Taking the member companies first orders this
/// against a concurrent company delete (`FOR UPDATE`, then the memory rows, then the cascade to
/// members), which otherwise deadlocks with the memory hook below (review p7b r1 M4).
pub(in crate::adapters::persistence) async fn lock_account_on(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user: Uuid,
) -> AppResult<AccountLocks> {
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(user)
        .fetch_optional(&mut **tx)
        .await
        .map_err(AppError::from)?;
    let touched: Vec<(Uuid, bool)> = sqlx::query_as(
        "SELECT company.id, company.user_id = $1 FROM companies AS company \
         WHERE company.user_id = $1 OR EXISTS (SELECT 1 FROM company_members AS member \
           WHERE member.company_id = company.id AND member.user_id = $1 AND member.role <> 'owner') \
         ORDER BY company.id",
    )
    .bind(user)
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)?;
    let mut owned = Vec::new();
    for (company, is_owner) in touched {
        let statement = if is_owner {
            "SELECT id FROM companies WHERE id=$1 AND user_id=$2 FOR UPDATE"
        } else {
            "SELECT id FROM companies WHERE id=$1 AND $2::uuid IS NOT NULL FOR NO KEY UPDATE"
        };
        let locked: Option<Uuid> = sqlx::query_scalar(statement)
            .bind(company)
            .bind(user)
            .fetch_optional(&mut **tx)
            .await
            .map_err(AppError::from)?;
        if is_owner && locked.is_some() {
            owned.push(company);
        }
    }
    let memberships = sqlx::query_scalar::<_,Uuid>("SELECT company_id FROM company_members WHERE user_id=$1 AND role<>'owner' ORDER BY company_id FOR UPDATE")
        .bind(user).fetch_all(&mut **tx).await.map_err(AppError::from)?;
    Ok(AccountLocks { owned, memberships })
}

/// Everything after [`lock_account_on`]: membership teardown, then the owned companies, then the
/// account.
pub(in crate::adapters::persistence) async fn finish_account_deletion_on(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user: Uuid,
    locks: AccountLocks,
) -> AppResult<()> {
    let AccountLocks {
        owned: companies,
        memberships,
    } = locks;
    for company in &memberships {
        let company = *company;
        lifecycle::revoke_membership_on(&mut *tx, &owner_for_user(user), &company_binding(company))
            .await
            .map_err(AppError::from)?;
        let notice_recipient =
            super::delivery::cancellation::member_notice_recipient_on(tx, company, user).await?;
        super::delivery::cancellation::cancel_member_notices_on(tx, company, notice_recipient)
            .await?;
        super::memory::retire_namespaces_on(
            &mut *tx,
            company,
            crate::application::memory::NamespaceOwner::User(user),
        )
        .await?;
    }
    sqlx::query("UPDATE principals SET kind='external',user_id=NULL,updated_at=clock_timestamp() WHERE user_id=$1")
        .bind(user).execute(&mut **tx).await.map_err(AppError::from)?;
    lifecycle::retire_owner_on(tx, &owner_for_user(user))
        .await
        .map_err(AppError::from)?;
    for company in companies {
        lifecycle::revoke_company_on(tx, &client_namespace(), &company_binding(company))
            .await
            .map_err(AppError::from)?;
        super::memory::retire_company_on(&mut *tx, company).await?;
        sqlx::query("DELETE FROM companies WHERE id=$1 AND user_id=$2")
            .bind(company)
            .bind(user)
            .execute(&mut **tx)
            .await
            .map_err(AppError::from)?;
    }
    sqlx::query("DELETE FROM users WHERE id=$1")
        .bind(user)
        .execute(&mut **tx)
        .await
        .map_err(AppError::from)?;
    for company in memberships {
        Box::pin(super::workflow::refresh_member_dependents_on(
            tx,
            crate::domain::workflow::CompanyId::new(company),
        ))
        .await?;
    }
    Ok(())
}
