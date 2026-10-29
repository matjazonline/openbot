//! Signed browser cookies backed by a revocable, revision-fenced server record.

use axum::http::HeaderMap;
use axum_extra::extract::{
    CookieJar,
    cookie::{Cookie, SameSite},
};
use chrono::{DateTime, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use std::{future::Future, sync::Arc};
use time::Duration;
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    application::session::{SessionId, SessionPersistence, SessionRecord, VerifiedSession},
    domain::monitoring::MonitoringService,
    entities::user::User,
    infra::config::AppConfig,
};

pub const SESSION_COOKIE: &str = "session";
pub const LEGACY_USER_ID_COOKIE: &str = "user_id";
pub const MIN_SECRET_BYTES: usize = 32;
const MAX_SESSION_TOKEN_BYTES: usize = 2048;
const STORE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionClaims {
    sub: Uuid,
    jti: SessionId,
    rev: i64,
    iat: i64,
    exp: i64,
}

#[derive(Serialize, Deserialize)]
pub struct OAuthState {
    pub transaction: String,
    pub provider: String,
    pub flow: String,
    pub user_id: Option<Uuid>,
    pub nonce: String,
    iat: i64,
    exp: i64,
}

pub struct SessionAuthority {
    encoding: EncodingKey,
    decoding: DecodingKey,
    validation: Validation,
    oauth_validation: Validation,
    lifetime: Duration,
    secure_cookies: bool,
    store: Arc<dyn SessionPersistence>,
    monitoring: Option<Arc<dyn MonitoringService>>,
}

impl SessionAuthority {
    pub fn new(config: &AppConfig, store: Arc<dyn SessionPersistence>) -> Self {
        assert!(
            (1..=365).contains(&config.refresh_token_ttl.whole_days()),
            "REFRESH_TOKEN_TTL_DAYS must be between 1 and 365"
        );
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_required_spec_claims(&["sub", "jti", "iat", "exp"]);
        validation.leeway = 0;
        Self {
            encoding: EncodingKey::from_secret(config.jwt_secret.as_bytes()),
            decoding: DecodingKey::from_secret(config.jwt_secret.as_bytes()),
            validation,
            oauth_validation: Validation::new(Algorithm::HS256),
            lifetime: config.refresh_token_ttl,
            secure_cookies: config.secure_cookies,
            store,
            monitoring: None,
        }
    }

    pub fn with_monitoring(mut self, monitoring: Arc<dyn MonitoringService>) -> Self {
        self.monitoring = Some(monitoring);
        self
    }

    fn validation_failure(&self, reason: &'static str) {
        if let Some(monitoring) = &self.monitoring {
            monitoring.increment_counter("session_validation_failures", 1, &[("reason", reason)]);
        }
    }

    /// Authentication's account revision is checked under the account lock before committing.
    pub async fn cookie(&self, user: &User) -> AppResult<Cookie<'static>> {
        let iat = Utc::now().timestamp();
        let claims = SessionClaims {
            sub: user.id,
            jti: SessionId(Uuid::new_v4()),
            rev: user.auth_revision,
            iat,
            exp: iat + self.lifetime.whole_seconds(),
        };
        let token = encode(&Header::new(Algorithm::HS256), &claims, &self.encoding)
            .map_err(|_| AppError::Internal("Session signing failed".into()))?;
        let record = record_from_claims(claims)
            .ok_or_else(|| AppError::Internal("Invalid session lifetime".into()))?;
        session_store(self.store.create_session(&record)).await?;
        Ok(Cookie::build((SESSION_COOKIE, token))
            .path("/")
            .http_only(true)
            .same_site(SameSite::Lax)
            .secure(self.secure_cookies)
            .max_age(self.lifetime)
            .build())
    }

    pub fn cleared_cookies(&self) -> [Cookie<'static>; 2] {
        [
            Cookie::build((SESSION_COOKIE, "")).path("/").build(),
            Cookie::build((LEGACY_USER_ID_COOKIE, "")).path("/").build(),
        ]
    }

    pub async fn verify(&self, token: &str) -> AppResult<Option<VerifiedSession>> {
        let Some(record) = self.decode_session(token) else {
            self.validation_failure("invalid_token");
            return Ok(None);
        };
        match session_store(self.store.session_is_live(&record)).await {
            Ok(true) => {}
            Ok(false) => {
                self.validation_failure("inactive_session");
                return Ok(None);
            }
            Err(error) => {
                self.validation_failure("store_unavailable");
                return Err(error);
            }
        }
        Ok(Some(VerifiedSession {
            user_id: record.user_id,
            session_id: record.id,
        }))
    }

    fn decode_session(&self, token: &str) -> Option<SessionRecord> {
        if token.len() > MAX_SESSION_TOKEN_BYTES {
            return None;
        }
        let claims = decode::<SessionClaims>(token, &self.decoding, &self.validation)
            .ok()?
            .claims;
        let now = Utc::now().timestamp();
        let duration = claims.exp.checked_sub(claims.iat)?;
        if claims.sub.is_nil()
            || claims.jti.0.get_version_num() != 4
            || claims.rev < 0
            || claims.iat > now
            || claims.exp <= now
            || duration <= 0
            || duration > self.lifetime.whole_seconds()
        {
            return None;
        }
        record_from_claims(claims)
    }

    pub async fn user_from_headers(
        &self,
        headers: &HeaderMap,
    ) -> AppResult<Option<VerifiedSession>> {
        let jar = CookieJar::from_headers(headers);
        let Some(cookie) = jar.get(SESSION_COOKIE) else {
            return Ok(None);
        };
        self.verify(cookie.value()).await
    }

    pub async fn revoke(&self, session: VerifiedSession) -> AppResult<()> {
        session_store(self.store.revoke_session(session)).await
    }

    pub fn issue_oauth_state(
        &self,
        provider: &str,
        flow: &str,
        user_id: Option<Uuid>,
        nonce: String,
    ) -> OAuthState {
        let issued_at = time::OffsetDateTime::now_utc();
        OAuthState {
            transaction: Uuid::new_v4().simple().to_string(),
            provider: provider.to_string(),
            flow: flow.to_string(),
            user_id,
            nonce,
            iat: issued_at.unix_timestamp(),
            exp: (issued_at + Duration::minutes(10)).unix_timestamp(),
        }
    }

    pub fn encode_oauth_state(&self, state: &OAuthState) -> String {
        encode(&Header::new(Algorithm::HS256), state, &self.encoding).unwrap_or_default()
    }

    pub fn verify_oauth_state(&self, token: &str) -> Option<OAuthState> {
        decode::<OAuthState>(token, &self.decoding, &self.oauth_validation)
            .ok()
            .map(|token| token.claims)
    }
}

fn record_from_claims(claims: SessionClaims) -> Option<SessionRecord> {
    Some(SessionRecord {
        id: claims.jti,
        user_id: claims.sub,
        auth_revision: claims.rev,
        issued_at: DateTime::from_timestamp(claims.iat, 0)?,
        expires_at: DateTime::from_timestamp(claims.exp, 0)?,
    })
}

async fn session_store<T>(operation: impl Future<Output = AppResult<T>>) -> AppResult<T> {
    match tokio::time::timeout(STORE_TIMEOUT, operation).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(AppError::InvalidCredentials)) => Err(AppError::InvalidCredentials),
        _ => Err(AppError::ServiceUnavailable(
            "Session store unavailable".into(),
        )),
    }
}

#[cfg(test)]
#[path = "session_concurrency_tests.rs"]
mod concurrency_tests;
#[cfg(test)]
#[path = "session_test_support.rs"]
pub(crate) mod test_support;
#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;

#[path = "session_cleanup.rs"]
mod cleanup;
#[cfg(test)]
#[path = "session_security_change_tests.rs"]
mod security_change_tests;
