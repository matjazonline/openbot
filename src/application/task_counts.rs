use crate::{app_error::AppResult, entities::task_counts::TaskCountSnapshot};
use uuid::Uuid;

/// Read-only, viewer-scoped projection; independent of the worker's queue protocol.
#[async_trait::async_trait]
pub trait TaskCountsReader: Send + Sync {
    async fn task_counts(
        &self,
        company_id: Uuid,
        visible_channel_ids: &[Uuid],
    ) -> AppResult<TaskCountSnapshot>;
}
