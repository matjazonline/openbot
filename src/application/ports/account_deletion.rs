//! Host user-deletion port. Integrations itself knows only opaque client owners.
use crate::app_error::AppResult;
use async_trait::async_trait;
use uuid::Uuid;
#[async_trait]
pub trait HostAccountDeletion: Send + Sync {
    /// Privileged host lifecycle operation; callers must establish user/operator authority.
    /// Repeating deletion must retain or complete durable module cleanup.
    async fn delete_user_account(&self, user: Uuid) -> AppResult<()>;
}
