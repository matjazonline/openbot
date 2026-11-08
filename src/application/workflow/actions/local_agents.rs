use super::{ActionRequest, ActionService, ActionTarget, FrozenAction, freeze};
use crate::app_error::AppResult;
use crate::application::agent::ToolCallOrigin;
use crate::application::local_agents::workflow::{
    self, FrozenLocalRun, LocalAgentWorkflow, LocalRunInputs, LocalRunProgress,
};
use crate::application::workflow::lease::{ClaimedWorkflow, LeasePolicy};
use std::future::Future;

impl<P: LocalAgentWorkflow> ActionService<P> {
    pub fn run_local_agent<'a>(
        &'a self,
        claim: &'a ClaimedWorkflow,
        inputs: &'a LocalRunInputs,
        policy: LeasePolicy,
    ) -> impl Future<Output = AppResult<LocalRunProgress>> + Send + 'a {
        self.persistence.run_local_agent(claim, inputs, policy)
    }
}
impl FrozenAction {
    pub(crate) fn local_agent(request: ActionRequest) -> AppResult<Self> {
        let args: FrozenLocalRun =
            serde_json::from_value(request.arguments.clone()).map_err(|_| workflow::invalid())?;
        if crate::domain::local_agents::canonical_deadline(args.deadline)
            .map_err(|_| workflow::invalid())?
            != args.deadline
        {
            return Err(workflow::invalid());
        }
        if request.operation_key.as_str() != workflow::KIND
            || request.context.origin != ToolCallOrigin::Workflow
            || !workflow::is_trusted_snapshot(&request.contract)?
            || !matches!(&request.target, ActionTarget::Local {resource,resource_kind}
                if *resource==args.profile_id.as_uuid() && resource_kind.as_str()==workflow::RESOURCE_KIND)
        {
            return Err(workflow::invalid());
        }
        freeze::freeze(request)
    }
}
