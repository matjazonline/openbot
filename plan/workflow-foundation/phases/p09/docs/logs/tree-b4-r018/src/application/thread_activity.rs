use std::collections::HashMap;

use uuid::Uuid;

use crate::{app_error::AppResult, entities::thread_activity::ThreadWorkSummary};

/// Mailbox activity: compact newest-run row marks and independent runs in the open thread.
/// The callers have proved the threads visible; the company scopes the index.
#[async_trait::async_trait]
pub trait ThreadActivityReader: Send + Sync {
    /// Inspector eligibility for the actual displayed batch, including a reconnect backlog.
    async fn inspectable_runs(
        &self,
        company: Uuid,
        viewer: Uuid,
        runs: &[Uuid],
    ) -> AppResult<std::collections::HashSet<Uuid>>;

    async fn thread_runs(
        &self,
        company_id: Uuid,
        thread_id: Uuid,
        viewer: Uuid,
    ) -> AppResult<ThreadRuns>;

    async fn thread_activity(
        &self,
        company_id: Uuid,
        thread_ids: &[Uuid],
    ) -> AppResult<HashMap<Uuid, ThreadWorkSummary>>;
}

/// The open thread's independently actionable runs. Callers must authorize the thread first.
#[derive(Debug, Clone)]
pub struct ThreadRunActivity {
    pub run_id: Uuid,
    pub title: String,
    pub activity: crate::entities::thread_activity::ThreadActivity,
}

#[derive(Debug, Clone, Default)]
pub struct ThreadRuns {
    /// Inspector links permitted for this viewer, including replies produced on another channel.
    pub inspectable: std::collections::HashSet<Uuid>,
    pub items: Vec<ThreadRunActivity>,
    pub has_more: bool,
    pub message_runs: Vec<MessageRuns>,
}

pub const THREAD_RUN_LIMIT: usize = 50;

/// The attribution of a message already authorized through its visible thread.
#[derive(Debug, Clone)]
pub struct MessageRuns {
    pub direction: crate::entities::message::MessageDirection,
    pub canonical_id: Uuid,
    pub run_ids: Vec<Uuid>,
}

/// At most one producer plus the admission binding cap for each retained message, and the
/// independently displayed run strip. A caller cannot turn link eligibility into a catalogue.
pub const MAX_THREAD_RUN_LINKS: usize = crate::entities::message_view::THREAD_HISTORY_LIMIT
    * (crate::domain::workflow::MAX_MESSAGE_BINDINGS_PER_CHANNEL as usize + 1)
    + THREAD_RUN_LIMIT;
