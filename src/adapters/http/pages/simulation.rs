//! The channel simulator: compose a message, run it, inspect the execution.

use super::*;
use crate::domain::workflow::{RunState, WaitingReason};
use crate::entities::message_outcome::{
    AdmittedRun, BindingOutcome, MessageBindingOutcome, RUN_EXECUTION_LIMIT,
};

#[derive(Debug, Clone)]
pub struct EmailSimulationResult {
    pub resolved: bool,
    pub sender_authorized: bool,
    pub company_slug: Option<crate::entities::value_objects::CompanySlug>,
    pub channel_slug: Option<crate::entities::value_objects::ChannelSlug>,
    pub company: Option<Company>,
    pub channel: Option<Channel>,
    pub to: String,
    pub from: String,
    pub subject: Option<String>,
    pub text_body: Option<String>,
}

mod compose;
mod live;
mod outcomes;
pub use compose::channel_simulation_page;

pub use live::{
    SimulationThreadView, channel_simulation_loaded_thread_fragment,
    channel_simulation_thread_error_fragment, simulation_live_view,
};
pub use outcomes::{
    ChannelSimulationFailure, LlmInfo, channel_simulation_failure_fragment,
    channel_simulation_result_fragment, resolve_llm_info,
};
