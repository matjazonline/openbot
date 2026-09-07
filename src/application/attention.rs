//! Application boundary for the human-operational queue and its business report.

use async_trait::async_trait;

use crate::{
    app_error::{AppError, AppResult},
    entities::attention::{
        AttentionPage, AttentionQuery, AttentionSourceCommand, NewManualHandoff,
        OperationalSummary, ResolveHandoffCommand,
    },
};

#[async_trait]
pub trait AttentionPersistence: Send + Sync {
    async fn list_attention(&self, query: AttentionQuery<'_>) -> AppResult<AttentionPage>;

    async fn create_handoff(&self, handoff: NewManualHandoff) -> AppResult<u64>;

    async fn change_source_attributes(&self, command: AttentionSourceCommand) -> AppResult<u64>;

    async fn resolve_handoff(&self, command: ResolveHandoffCommand) -> AppResult<u64>;

    async fn operational_summary(
        &self,
        company_id: uuid::Uuid,
        visible_channel_ids: &[uuid::Uuid],
    ) -> AppResult<OperationalSummary>;
}

pub fn validate_handoff(handoff: &NewManualHandoff) -> AppResult<()> {
    validate_text("Handoff title", &handoff.title, 512)?;
    validate_text("Handoff next action", &handoff.next_action, 2_048)
}

fn validate_text(label: &str, value: &str, max_bytes: usize) -> AppResult<()> {
    if value.trim().is_empty() || value.len() > max_bytes {
        return Err(AppError::BadRequest(format!(
            "{label} must be non-empty and at most {max_bytes} bytes."
        )));
    }
    Ok(())
}
