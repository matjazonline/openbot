//! Agent escalation commands run through the ordinary fenced local action service.
use crate::{
    adapters::persistence::{
        PostgresPersistence,
        workflow::{
            action_dispatch::PostgresActionDispatch,
            escalation_tools::{EscalationActionAuthority, EscalationEffects},
        },
    },
    app_error::AppResult,
    application::workflow::{
        agent::AgentToolObservation,
        escalation::tools::{EscalationActions, EscalationToolCall},
        lease::{LeasePolicy, WorkflowFence},
    },
};

pub struct EscalationRuntime {
    inner: EscalationActions<
        PostgresPersistence,
        PostgresActionDispatch<EscalationActionAuthority, EscalationEffects>,
    >,
}
impl EscalationRuntime {
    pub fn new(persistence: PostgresPersistence, lease: LeasePolicy) -> Self {
        let dispatch = PostgresActionDispatch::new(
            persistence.clone(),
            EscalationActionAuthority,
            EscalationEffects,
        );
        Self {
            inner: EscalationActions::new(persistence, dispatch, lease),
        }
    }
    pub async fn call_tool(
        &self,
        fence: WorkflowFence,
        call: EscalationToolCall,
    ) -> AppResult<AgentToolObservation> {
        // Box the substantive action preparation/dispatch seam for stock native stacks.
        Box::pin(self.inner.call_tool(fence, call)).await
    }
}
