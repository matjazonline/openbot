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
        ThreadHandoffDismiss, ThreadHandoffDraft,
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

    /// Give up on one handoff generation, returning its new version.
    ///
    /// Same scoping contract as [`Self::change_thread_handoff`]: the caller proves membership and
    /// fills `visible_channel_ids` before this is called.
    pub async fn dismiss_thread_handoff(&self, command: ThreadHandoffDismiss) -> AppResult<u64> {
        self.persistence.dismiss_thread_handoff(command).await
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

    /// The draft this handoff generation's run produced, scoped to the caller's channels.
    pub async fn thread_handoff_draft(
        &self,
        company_id: Uuid,
        handoff_id: Uuid,
        generation: Uuid,
        visible_channel_ids: &[Uuid],
    ) -> AppResult<Option<ThreadHandoffDraft>> {
        self.persistence
            .thread_handoff_draft(company_id, handoff_id, generation, visible_channel_ids)
            .await
    }

    /// End a drafting run whose draft expired, and hand the reply back to the team.
    pub async fn expire_thread_handoff_draft(
        &self,
        company_id: Uuid,
        task_id: Uuid,
        reason: &str,
    ) -> AppResult<()> {
        self.persistence
            .expire_thread_handoff_draft(company_id, task_id, reason)
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
