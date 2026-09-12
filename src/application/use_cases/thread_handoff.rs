//! Reply-handling policy reads and writes, as the HTTP boundary asks for them.
//!
//! Not to be confused with `manual_handoffs` (see [`crate::entities::attention`]).

use std::{collections::HashMap, sync::Arc};

use uuid::Uuid;

use crate::{
    app_error::AppResult,
    application::thread_handoff::ThreadHandoffPolicyPersistence,
    entities::thread_handoff::{
        ExternalReplyHandling, ExternalReplyHandlingPolicy, ThreadHandoff, ThreadHandoffCommand,
    },
};

#[derive(Clone)]
pub struct ThreadHandoffUseCases {
    persistence: Arc<dyn ThreadHandoffPolicyPersistence>,
}

impl ThreadHandoffUseCases {
    pub fn new(persistence: Arc<dyn ThreadHandoffPolicyPersistence>) -> Self {
        Self { persistence }
    }

    pub async fn reply_handling_policy(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Option<ExternalReplyHandlingPolicy>> {
        self.persistence
            .reply_handling_policy(company_id, channel_id)
            .await
    }

    pub async fn set_company_reply_handling(
        &self,
        company_id: Uuid,
        policy: ExternalReplyHandling,
    ) -> AppResult<()> {
        self.persistence
            .set_company_reply_handling(company_id, policy)
            .await
    }

    /// `None` clears the override back to inheriting the company default.
    pub async fn set_channel_reply_handling_override(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
        policy_override: Option<ExternalReplyHandling>,
    ) -> AppResult<()> {
        self.persistence
            .set_channel_reply_handling_override(company_id, channel_id, policy_override)
            .await
    }

    /// Move one handoff, returning its new version.
    ///
    /// The caller must already have proved company membership and filled
    /// `command.visible_channel_ids` from the channels the actor may read: that list is the
    /// tenant and channel scope of every statement the command runs.
    pub async fn change_thread_handoff(&self, command: ThreadHandoffCommand) -> AppResult<u64> {
        self.persistence.change_thread_handoff(command).await
    }

    /// One handoff, scoped to the channels the caller proved they may read.
    ///
    /// `None` covers "no such handoff", "another company's" and "a channel you cannot view"
    /// alike, so a caller cannot learn which of the three it was.
    pub async fn get_thread_handoff(
        &self,
        company_id: Uuid,
        handoff_id: Uuid,
        visible_channel_ids: &[Uuid],
    ) -> AppResult<Option<ThreadHandoff>> {
        self.persistence
            .get_thread_handoff(company_id, handoff_id, visible_channel_ids)
            .await
    }

    /// The open handoff of each thread that has one, keyed by thread id.
    pub async fn thread_handoffs_for_threads(
        &self,
        company_id: Uuid,
        thread_ids: &[Uuid],
        visible_channel_ids: &[Uuid],
    ) -> AppResult<HashMap<Uuid, ThreadHandoff>> {
        self.persistence
            .thread_handoffs_for_threads(company_id, thread_ids, visible_channel_ids)
            .await
    }
}
