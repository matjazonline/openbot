//! Durable child facts, read by the parent owner before settling its exact wait.
//! Reading never advances a parent; phase07 owns mapping and atomic consumption.
use crate::app_error::AppResult;
use crate::domain::workflow::{ExecutionId, ExecutionRef, RunId};
use async_trait::async_trait;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildTerminalState {
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug)]
pub struct ParentWakeupQuery {
    pub parent: ExecutionRef,
    pub child: RunId,
    /// Cursor is scoped to this exact child, never shared across sibling runs.
    pub after_sequence: u64,
    pub limit: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParentWakeup {
    pub child: RunId,
    pub sequence: u64,
    pub state: ChildTerminalState,
    /// Output stays on the child's completed execution; failure may have none.
    pub terminal_execution: Option<ExecutionId>,
}

#[async_trait]
pub trait WorkflowParentWakeups: Send + Sync {
    async fn parent_wakeups(&self, query: ParentWakeupQuery) -> AppResult<Vec<ParentWakeup>>;
}
