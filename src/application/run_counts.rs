use crate::{app_error::AppResult, entities::run_counts::RunCountSnapshot};
use uuid::Uuid;

/// Read-only, viewer-scoped projection of open workflow runs (p08 D4.4).
#[async_trait::async_trait]
pub trait RunCountsReader: Send + Sync {
    /// Open runs on visible desks, current readable direct inboxes and company scope.
    /// This projection is served only to owners and admins.
    async fn run_counts(
        &self,
        company_id: Uuid,
        visible_channel_ids: &[Uuid],
        viewer_user_id: Uuid,
    ) -> AppResult<RunCountSnapshot>;
}
