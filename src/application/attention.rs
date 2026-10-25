//! Application boundary for the human-operational queue and its business report.

use async_trait::async_trait;

use crate::{
    app_error::AppResult,
    entities::attention::{AttentionPage, AttentionQuery, OperationalSummary},
};

#[async_trait]
pub trait AttentionPersistence: Send + Sync {
    async fn list_attention(&self, query: AttentionQuery<'_>) -> AppResult<AttentionPage>;

    /// For owners and admins: company-associated sources are included.
    async fn operational_summary(
        &self,
        company_id: uuid::Uuid,
        visible_channel_ids: &[uuid::Uuid],
    ) -> AppResult<OperationalSummary>;
}
