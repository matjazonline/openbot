use super::crypto::{Cipher, verifier};
use crate::application::identity::*;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use std::sync::Arc;
use uuid::Uuid;
pub struct IdentityPostgres {
    pub pool: PgPool,
    pub cipher: Arc<Cipher>,
}
pub(super) fn database(_: sqlx::Error) -> Failure {
    Failure::Unavailable
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Secrets {
    grant: String,
    csrf: String,
}
#[async_trait]
impl SessionStore for IdentityPostgres {
    async fn begin_login(&self, transaction: &LoginTransaction) -> Result<()> {
        let encrypted = self
            .cipher
            .seal("login", transaction.id, transaction.pkce.0.as_bytes())?;
        sqlx::query("INSERT INTO attribution_login_transactions (id, state_verifier, browser_verifier, encrypted_pkce, return_path) VALUES ($1, $2, $3, $4, $5)")
            .bind(transaction.id).bind(verifier(&transaction.state.0)).bind(verifier(&transaction.browser.0)).bind(encrypted).bind(&transaction.return_path)
            .execute(&self.pool).await.map_err(database)?;
        Ok(())
    }
    async fn consume_login(
        &self,
        state: &LoginState,
        browser: &BrowserBinding,
    ) -> Result<LoginCompletion> {
        let row = sqlx::query("DELETE FROM attribution_login_transactions WHERE state_verifier = $1 AND browser_verifier = $2 AND expires_at > clock_timestamp() RETURNING id, encrypted_pkce, return_path")
            .bind(verifier(&state.0)).bind(verifier(&browser.0)).fetch_optional(&self.pool).await.map_err(database)?.ok_or(Failure::Unauthorized)?;
        let id: Uuid = row.try_get("id").map_err(database)?;
        let encrypted: Vec<u8> = row.try_get("encrypted_pkce").map_err(database)?;
        let pkce = String::from_utf8(self.cipher.open("login", id, &encrypted)?)
            .map_err(|_| Failure::Unavailable)?;
        Ok(LoginCompletion {
            pkce: PkceVerifier(pkce),
            return_path: row.try_get("return_path").map_err(database)?,
        })
    }
    async fn create_session(&self, session: &NewSession, old: Option<&SessionToken>) -> Result<()> {
        let payload = serde_json::to_vec(&Secrets {
            grant: session.grant.0.clone(),
            csrf: session.csrf.0.clone(),
        })
        .map_err(|_| Failure::Unavailable)?;
        let encrypted = self.cipher.seal("session", session.id.0, &payload)?;
        let mut tx = self.pool.begin().await.map_err(database)?;
        if let Some(old) = old {
            sqlx::query("UPDATE attribution_sessions SET revoked_at = clock_timestamp() WHERE verifier = $1 AND revoked_at IS NULL").bind(verifier(&old.0)).execute(&mut *tx).await.map_err(database)?;
        }
        sqlx::query("INSERT INTO attribution_sessions (id, verifier, user_id, encrypted_secrets, absolute_expires_at, idle_expires_at) VALUES ($1, $2, $3, $4, LEAST($5, clock_timestamp() + interval '8 hours'), LEAST($5, clock_timestamp() + interval '30 minutes'))")
            .bind(session.id.0).bind(verifier(&session.token.0)).bind(session.user_id.0).bind(encrypted).bind(session.expires_at)
            .execute(&mut *tx).await.map_err(database)?;
        tx.commit().await.map_err(database)
    }
    async fn session(&self, token: &SessionToken) -> Result<Session> {
        let row = sqlx::query("UPDATE attribution_sessions SET idle_expires_at = LEAST(absolute_expires_at, clock_timestamp() + interval '30 minutes') WHERE verifier = $1 AND revoked_at IS NULL AND absolute_expires_at > clock_timestamp() AND idle_expires_at > clock_timestamp() RETURNING id, user_id, encrypted_secrets, absolute_expires_at")
            .bind(verifier(&token.0)).fetch_optional(&self.pool).await.map_err(database)?.ok_or(Failure::Unauthorized)?;
        decode_session(&self.cipher, row)
    }
    async fn logout_session(&self, token: &SessionToken) -> Result<Session> {
        let row = sqlx::query("SELECT id, user_id, encrypted_secrets, absolute_expires_at FROM attribution_sessions WHERE verifier = $1")
            .bind(verifier(&token.0)).fetch_optional(&self.pool).await.map_err(database)?.ok_or(Failure::Unauthorized)?;
        decode_session(&self.cipher, row)
    }
    async fn revoke_session(&self, session: SessionId) -> Result<()> {
        sqlx::query("UPDATE attribution_sessions SET revoked_at = clock_timestamp() WHERE id = $1 AND revoked_at IS NULL").bind(session.0).execute(&self.pool).await.map_err(database)?;
        Ok(())
    }
}

fn decode_session(cipher: &Cipher, row: sqlx::postgres::PgRow) -> Result<Session> {
    let id: Uuid = row.try_get("id").map_err(database)?;
    let encrypted: Vec<u8> = row.try_get("encrypted_secrets").map_err(database)?;
    let secrets: Secrets = serde_json::from_slice(&cipher.open("session", id, &encrypted)?)
        .map_err(|_| Failure::Unavailable)?;
    let expires_at: DateTime<Utc> = row.try_get("absolute_expires_at").map_err(database)?;
    Ok(Session {
        id: SessionId(id),
        user_id: UserId(row.try_get("user_id").map_err(database)?),
        grant: IdentityGrant(secrets.grant),
        csrf: CsrfToken(secrets.csrf),
        expires_at,
    })
}
