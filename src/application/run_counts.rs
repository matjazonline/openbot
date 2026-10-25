use crate::{app_error::AppResult, entities::run_counts::RunCountSnapshot};
use uuid::Uuid;

/// Read-only, viewer-scoped projection of open workflow runs (p08 D4.4).
#[async_trait::async_trait]
pub trait RunCountsReader: Send + Sync {
    /// Open runs in `visible_channel_ids`, plus the company's runs without a channel: the
    /// counts are an owner's and admin's view.
    async fn run_counts(
        &self,
        company_id: Uuid,
        visible_channel_ids: &[Uuid],
    ) -> AppResult<RunCountSnapshot>;
}
