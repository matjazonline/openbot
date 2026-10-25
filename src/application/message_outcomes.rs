use uuid::Uuid;

use crate::{app_error::AppResult, entities::message_outcome::MessageBindingOutcome};

/// The simulation's read (p08 D4.6): every binding outcome of the given messages in a thread, in
/// message then binding id order. The caller has proved the thread is the reader's to inspect;
/// the company scopes every index.
#[async_trait::async_trait]
pub trait MessageOutcomeReader: Send + Sync {
    async fn message_outcomes(
        &self,
        company_id: Uuid,
        thread_id: Uuid,
        message_ids: &[Uuid],
    ) -> AppResult<Vec<MessageBindingOutcome>>;
}
