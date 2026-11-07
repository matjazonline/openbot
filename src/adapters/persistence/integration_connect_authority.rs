//! Host-only session authority. Integration persistence never reads session/user tables.
use super::PostgresPersistence;
use crate::{
    adapters::integrations::owner::client_namespace,
    application::integrations::{
        VerifiedOwner,
        connect::{ConnectAuthority, ConnectFinalizer},
    },
    domain::integrations::*,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;
pub(crate) struct PostgresConnectAuthority {
    pub persistence: PostgresPersistence,
}
#[async_trait]
impl ConnectAuthority for PostgresConnectAuthority {
    async fn complete(
        &self,
        owner: &VerifiedOwner,
        finalizer: &dyn ConnectFinalizer,
    ) -> IntegrationResult<()> {
        let browser = owner.browser.as_ref().ok_or(IntegrationError::Denied)?;
        if owner.owner().namespace != client_namespace() {
            return Err(IntegrationError::Denied);
        }
        let user = Uuid::parse_str(owner.owner().subject.as_str())
            .map_err(|_| IntegrationError::Denied)?;
        let session =
            Uuid::parse_str(browser.session.as_str()).map_err(|_| IntegrationError::Denied)?;
        let mut tx = self
            .persistence
            .pool()
            .begin()
            .await
            .map_err(|_| IntegrationError::Storage)?;
        sqlx::query("SET LOCAL lock_timeout='5s'")
            .execute(&mut *tx)
            .await
            .map_err(|_| IntegrationError::Storage)?;
        let revision: Option<i64> =
            sqlx::query_scalar("SELECT auth_revision FROM users WHERE id=$1 FOR SHARE")
                .bind(user)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|_| IntegrationError::Storage)?;
        let revision = revision.ok_or(IntegrationError::Denied)?;
        let expires:Option<DateTime<Utc>>=sqlx::query_scalar("SELECT expires_at FROM user_sessions WHERE id=$1 AND user_id=$2 AND auth_revision=$3 AND revoked_at IS NULL AND expires_at>clock_timestamp() FOR SHARE")
            .bind(session).bind(user).bind(revision).fetch_optional(&mut *tx).await.map_err(|_|IntegrationError::Storage)?;
        finalizer
            .finish(expires.ok_or(IntegrationError::Denied)?)
            .await?;
        tx.rollback().await.map_err(|_| IntegrationError::Storage)?;
        Ok(())
    }
}
