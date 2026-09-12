//! Application boundary for reply-handling policy, and for the thread handoffs it will govern.
//!
//! Not to be confused with `manual_handoffs` (see [`crate::application::attention`]), which is a
//! generic, human-created work item with no source message and no generation.

use std::collections::HashMap;

use async_trait::async_trait;
use uuid::Uuid;

use crate::{
    app_error::AppResult,
    entities::thread_handoff::{
        ExternalReplyHandling, ExternalReplyHandlingPolicy, ThreadHandoff, ThreadHandoffCommand,
    },
};

#[async_trait]
pub trait ThreadHandoffPolicyPersistence: Send + Sync {
    /// The company default, the channel's override, and the effective value, in one round trip.
    /// `None` when the channel does not exist in that company.
    async fn reply_handling_policy(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Option<ExternalReplyHandlingPolicy>>;

    async fn set_company_reply_handling(
        &self,
        company_id: Uuid,
        policy: ExternalReplyHandling,
    ) -> AppResult<()>;

    /// `None` clears the override back to inheriting the company default.
    async fn set_channel_reply_handling_override(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
        policy_override: Option<ExternalReplyHandling>,
    ) -> AppResult<()>;

    /// Claim, release, reassign, or re-prioritise one handoff, returning its new version.
    ///
    /// Fenced on both the generation and the version, idempotent under `command.command_id`, and
    /// audited by exactly one `thread_handoff_events` row. Every unauthorized, invisible or
    /// non-existent case answers `NotFound` with one message, so an id cannot be probed.
    async fn change_thread_handoff(&self, command: ThreadHandoffCommand) -> AppResult<u64>;

    /// One handoff in any state, or `None` when this company and channel list cannot see it.
    async fn get_thread_handoff(
        &self,
        company_id: Uuid,
        handoff_id: Uuid,
        visible_channel_ids: &[Uuid],
    ) -> AppResult<Option<ThreadHandoff>>;

    /// The open handoff of each of `thread_ids` that has one, keyed by thread.
    async fn thread_handoffs_for_threads(
        &self,
        company_id: Uuid,
        thread_ids: &[Uuid],
        visible_channel_ids: &[Uuid],
    ) -> AppResult<HashMap<Uuid, ThreadHandoff>>;
}
