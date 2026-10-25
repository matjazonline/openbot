//! Production composition of the built-in provisioning tools (p08 D2.2a): the shared
//! ActionService over the local SQL dispatch, with the provisioning authority and the
//! provisioning effects as its first production `SqlLocalAction`.
use crate::{
    adapters::persistence::{
        PostgresPersistence,
        workflow::{
            action_dispatch::PostgresActionDispatch,
            provisioning::{ProvisioningActionAuthority, ProvisioningEffects},
        },
    },
    app_error::AppResult,
    application::workflow::{
        agent::AgentToolObservation,
        lease::{LeasePolicy, WorkflowFence},
        provisioning::{ProvisioningActions, ProvisioningToolCall},
    },
};

pub struct ProvisioningRuntime {
    inner: ProvisioningActions<
        PostgresPersistence,
        PostgresActionDispatch<ProvisioningActionAuthority, ProvisioningEffects>,
    >,
}
impl ProvisioningRuntime {
    pub fn new(
        persistence: PostgresPersistence,
        effects: ProvisioningEffects,
        lease: LeasePolicy,
    ) -> Self {
        let dispatch =
            PostgresActionDispatch::new(persistence.clone(), ProvisioningActionAuthority, effects);
        Self {
            inner: ProvisioningActions::new(persistence, dispatch, lease),
        }
    }
    /// Direct tool entry: a committed receipt (the result or a typed refusal) or an error that
    /// left no marker.
    pub async fn call_tool(
        &self,
        fence: WorkflowFence,
        call: ProvisioningToolCall,
    ) -> AppResult<AgentToolObservation> {
        Box::pin(self.inner.call_tool(fence, call)).await
    }
}
