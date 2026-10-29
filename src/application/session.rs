//! Durable browser-session contracts. Tokens and cookies remain in the HTTP adapter.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::app_error::AppResult;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(pub Uuid);

#[derive(Clone)]
pub struct SessionRecord {
    pub id: SessionId,
    pub user_id: Uuid,
    pub auth_revision: i64,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Copy)]
pub struct VerifiedSession {
    pub user_id: Uuid,
    pub session_id: SessionId,
}

pub struct SessionCleanup {
    pub removed: u64,
    /// Saturates at 1001: a value above 1000 means the backlog is truncated.
    pub backlog: i64,
}

#[async_trait]
pub trait SessionPersistence: Send + Sync {
    /// Lock the account, reject a stale authentication revision, then persist before returning.
    async fn create_session(&self, session: &SessionRecord) -> AppResult<()>;
    async fn session_is_live(&self, session: &SessionRecord) -> AppResult<bool>;
    async fn revoke_session(&self, session: VerifiedSession) -> AppResult<()>;
    /// Delete at most 200 rows; inspect at most 1001 backlog candidates per index.
    async fn cleanup_sessions(&self) -> AppResult<SessionCleanup>;
}
