//! PostgreSQL implementation of the application-owned task queue contract.

use crate::{
    entities::{message::CanonicalMessageId, task::BackgroundTask},
    task_queue::{AgentDispatchCommit, CreateOutreachRequest, DispatchCommit, TaskPersistence},
    transport::RecipientRole,
    use_cases::thread::{MessageWrite, TaskChannelTarget},
};

mod board;
mod operations;
mod outreach;
mod queue;
mod rows;

pub(crate) use board::*;
pub(crate) use operations::record_outreach_reply_on;
pub(crate) use outreach::*;
pub(crate) use queue::*;
pub(crate) use rows::*;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
