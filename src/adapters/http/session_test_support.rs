//! Explicit memory persistence for transport unit tests; security acceptance uses PostgreSQL.
use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use super::*;
use crate::application::session::SessionCleanup;

#[derive(Default)]
pub(crate) struct MemorySessions(Mutex<HashMap<SessionId, (SessionRecord, bool)>>);

#[async_trait]
impl SessionPersistence for MemorySessions {
    async fn create_session(&self, session: &SessionRecord) -> AppResult<()> {
        self.0
            .lock()
            .unwrap()
            .insert(session.id, (session.clone(), false));
        Ok(())
    }
    async fn session_is_live(&self, session: &SessionRecord) -> AppResult<bool> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(&session.id)
            .is_some_and(|(row, revoked)| {
                !revoked
                    && row.user_id == session.user_id
                    && row.auth_revision == session.auth_revision
                    && row.issued_at == session.issued_at
                    && row.expires_at == session.expires_at
                    && row.expires_at > Utc::now()
            }))
    }
    async fn revoke_session(&self, session: VerifiedSession) -> AppResult<()> {
        if let Some((row, revoked)) = self.0.lock().unwrap().get_mut(&session.session_id)
            && row.user_id == session.user_id
        {
            *revoked = true;
        }
        Ok(())
    }
    async fn cleanup_sessions(&self) -> AppResult<SessionCleanup> {
        let mut rows = self.0.lock().unwrap();
        let ids: Vec<_> = rows
            .iter()
            .filter(|(_, (r, revoked))| *revoked || r.expires_at <= Utc::now())
            .map(|(id, _)| *id)
            .take(200)
            .collect();
        for id in &ids {
            rows.remove(id);
        }
        let backlog = rows
            .values()
            .filter(|(r, revoked)| *revoked || r.expires_at <= Utc::now())
            .take(1001)
            .count() as i64;
        Ok(SessionCleanup {
            removed: ids.len() as u64,
            backlog,
        })
    }
}

impl SessionAuthority {
    pub(crate) fn for_test(config: &AppConfig) -> Self {
        Self::new(config, Arc::new(MemorySessions::default()))
    }
    pub(crate) async fn cookie_for_test(&self, id: Uuid) -> Cookie<'static> {
        self.cookie(&account(id)).await.unwrap()
    }
}

pub(crate) fn account(id: Uuid) -> User {
    User {
        id,
        auth_revision: 0,
        username: "unit-user".into(),
        email: "unit@example.test".into(),
        email_verified_at: None,
        password_hash: "unit-hash".into(),
        avatar_url: None,
        created_at: Utc::now(),
    }
}

pub(crate) struct TestHasher;
impl crate::use_cases::user::UserCredentialsHasher for TestHasher {
    fn hash_password(&self, password: &str) -> AppResult<String> {
        Ok(format!("test:{password}"))
    }
    fn verify_password(&self, password: &str, hash: &str) -> AppResult<bool> {
        Ok(hash == format!("test:{password}"))
    }
}
