//! What each workflow binding made of an inbound message, for the simulation page (p08 D4.6).
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::domain::workflow::RunState;

/// One binding's answer to one inbound message.
#[derive(Debug, Clone, PartialEq)]
pub struct MessageBindingOutcome {
    /// `messages.id`, the canonical message the intent was raised for.
    pub message_id: Uuid,
    pub binding_id: Uuid,
    /// The title of the workflow version the binding ran at the time.
    pub workflow_title: String,
    pub outcome: BindingOutcome,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BindingOutcome {
    /// The binding started a run for the message.
    Admitted(AdmittedRun),
    /// The binding declined the message, for the stored reason.
    Refused { reason: String },
    /// Admission failed, for the stored reason.
    Failed { reason: String },
}

/// The run an admission started, as it stands now.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedRun {
    pub run_id: Uuid,
    pub state: RunState,
    /// The run's newest executions, oldest first, at most [`RUN_EXECUTION_LIMIT`].
    pub executions: Vec<RunExecutionSummary>,
    /// Model calls grouped by provider and model.
    pub model_usage: Vec<RunModelUsage>,
}

/// How many of a run's executions the simulation shows: a run may take up to 100 000 steps.
pub const RUN_EXECUTION_LIMIT: usize = 20;

#[derive(Debug, Clone, PartialEq)]
pub struct RunExecutionSummary {
    pub step_id: String,
    pub activation: i64,
    /// The route the step committed, once it completed.
    pub route: Option<String>,
    pub created_at: DateTime<Utc>,
    pub activated_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// A run's model calls to one provider and model. Tokens count settled calls only; an unsettled
/// call reports none yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunModelUsage {
    pub provider: String,
    pub model: String,
    pub calls: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
}
