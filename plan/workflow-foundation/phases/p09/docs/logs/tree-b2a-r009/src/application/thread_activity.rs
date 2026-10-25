use std::collections::HashMap;

use uuid::Uuid;

use crate::{app_error::AppResult, entities::thread_activity::ThreadWorkSummary};

/// The mailbox's activity read (p08 D4.6): each thread's newest run, an unfinished one first.
/// The callers have proved the threads visible; the company scopes the index.
#[async_trait::async_trait]
pub trait ThreadActivityReader: Send + Sync {
    async fn thread_activity(
        &self,
        company_id: Uuid,
        thread_ids: &[Uuid],
    ) -> AppResult<HashMap<Uuid, ThreadWorkSummary>>;
}
