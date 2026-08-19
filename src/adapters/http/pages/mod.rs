//! Server-rendered HTML for the whole app.
//!
//! One module per page area; everything is re-exported here so call sites stay
//! `pages::<name>`. Shared chrome (layout, alerts, markdown) lives in [`layout`].

use chrono::{DateTime, Utc};
use pulldown_cmark::{Options, Parser, html};
use std::collections::HashMap;
use uuid::Uuid;

use crate::entities::{
    agent::Agent,
    approval::{ApprovalStatus, HumanApproval},
    channel::Channel,
    company::Company,
    company_invite::CompanyInvite,
    company_member::CompanyMember,
    message::{Message, MessageDirection, MessageRole},
    outbox::{OutboxEntry, OutboxFilter, OutboxStatus},
    task::{BackgroundTask, TaskFilter, TaskStatus, ThreadActivity},
    thread::Thread,
    value_objects::EmailAddress,
};
use crate::use_cases::channel::InboundEmailResult;
use crate::use_cases::thread::{SimulationExecutionResult, SimulationMode};

mod agent_settings;
mod agents;
mod approvals;
mod auth;
mod channel_settings;
mod channels;
mod companies;
mod company_settings;
mod layout;
mod mailbox;
mod onboarding;
mod outbox;
mod simulation;
mod task_monitor;
mod tasks;
mod team_settings;

pub use agent_settings::*;
pub use agents::*;
pub use approvals::*;
pub use auth::*;
pub use channel_settings::*;
pub use channels::*;
pub use companies::*;
pub use company_settings::*;
pub use layout::*;
pub use mailbox::*;
pub use onboarding::*;
pub use outbox::*;
pub use simulation::*;
pub use task_monitor::*;
pub use tasks::*;
pub use team_settings::*;

/// Every timestamp the app stores is UTC, and a page has no idea what zone its reader is in. These
/// three say so outright rather than rendering a bare wall clock the reader has to guess at, and
/// they exist so a new page reaches for one instead of inventing a seventh format string.
///
/// Pick by how much precision the reader actually needs: [`format_date`] for things that happened
/// on a day (signups, invites), [`format_date_time`] for things you scan by recency (threads,
/// messages), [`format_time`] for the queue views where seconds matter.
pub(crate) fn format_date(at: DateTime<Utc>) -> String {
    at.format("%b %d, %Y UTC").to_string()
}

pub(crate) fn format_date_time(at: DateTime<Utc>) -> String {
    at.format("%b %d, %Y %H:%M UTC").to_string()
}

pub(crate) fn format_time(at: DateTime<Utc>) -> String {
    at.format("%b %d, %H:%M:%S UTC").to_string()
}

#[cfg(test)]
mod tests;
