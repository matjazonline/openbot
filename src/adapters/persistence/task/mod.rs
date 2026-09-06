//! PostgreSQL implementation of the application-owned task queue contract.

use crate::{
    entities::{message::CanonicalMessageId, task::BackgroundTask},
    task_queue::{
        AgentDispatchCommit, CreateOutreachRequest, DispatchCommit, OwnedAgentExecution,
        TaskPersistence,
    },
    transport::RecipientRole,
    use_cases::thread::{MessageWrite, TaskChannelTarget},
};

mod board;
mod instructions;
mod operations;
mod outreach;
mod ownership;
mod queue;
mod rows;

pub(crate) use board::*;
pub(crate) use operations::record_outreach_reply_on;
pub(crate) use outreach::*;
pub(crate) use ownership::*;
pub(crate) use queue::*;
pub(crate) use rows::*;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
