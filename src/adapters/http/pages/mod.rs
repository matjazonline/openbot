//! Server-rendered HTML for the whole app.
//!
//! One module per page area; everything is re-exported here so call sites stay
//! `pages::<name>`. Shared chrome (layout, alerts, markdown) lives in [`layout`].

use pulldown_cmark::{Options, Parser, html};
use uuid::Uuid;

use crate::entities::{
    agent::Agent,
    approval::{ApprovalStatus, HumanApproval},
    channel::Channel,
    company::Company,
    company_invite::CompanyInvite,
    company_member::CompanyMember,
    message::{Message, MessageDirection, MessageRole},
    task::{BackgroundTask, TaskStatus},
    thread::Thread,
    value_objects::EmailAddress,
};
use crate::use_cases::channel::InboundEmailResult;
use crate::use_cases::thread::{SimulationExecutionResult, SimulationMode};

mod agents;
mod approvals;
mod auth;
mod channels;
mod companies;
mod layout;
mod mailbox;
mod onboarding;
mod simulation;
mod tasks;

pub use agents::*;
pub use approvals::*;
pub use auth::*;
pub use channels::*;
pub use companies::*;
pub use layout::*;
pub use mailbox::*;
pub use onboarding::*;
pub use simulation::*;
pub use tasks::*;

#[cfg(test)]
mod tests;
