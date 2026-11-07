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
        // Serialize against membership/ownership writes before recording their teardown.
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
            .bind(user)
            .fetch_optional(&mut *tx)
            .await
            .map_err(AppError::from)?;
        let companies = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM companies WHERE user_id=$1 ORDER BY id FOR UPDATE",
        )
        .bind(user)
        .fetch_all(&mut *tx)
        .await
        .map_err(AppError::from)?;
        let memberships = sqlx::query_scalar::<_,Uuid>("SELECT company_id FROM company_members WHERE user_id=$1 AND role<>'owner' ORDER BY company_id FOR UPDATE")
            .bind(user).fetch_all(&mut *tx).await.map_err(AppError::from)?;
        for company in memberships {
            lifecycle::revoke_membership_on(
                &mut tx,
                &owner_for_user(user),
                &company_binding(company),
            )
            .await
            .map_err(AppError::from)?;
            super::delivery::cancellation::cancel_member_notices_on(&mut tx, company, user).await?;
        }
        sqlx::query("UPDATE principals SET kind='external',user_id=NULL,updated_at=clock_timestamp() WHERE user_id=$1")
            .bind(user).execute(&mut *tx).await.map_err(AppError::from)?;
        lifecycle::retire_owner_on(&mut tx, &owner_for_user(user))
            .await
            .map_err(AppError::from)?;
        for company in companies {
            lifecycle::revoke_company_on(&mut tx, &client_namespace(), &company_binding(company))
                .await
                .map_err(AppError::from)?;
            sqlx::query("DELETE FROM companies WHERE id=$1 AND user_id=$2")
                .bind(company)
                .bind(user)
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;
        }
        sqlx::query("DELETE FROM users WHERE id=$1")
            .bind(user)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
        tx.commit().await.map_err(AppError::from)?;
        Ok(())
    }
}
