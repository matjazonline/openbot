use super::*;
use crate::adapters::persistence::workflow::tests::action_outcome_support::CompletedOutcome;
use crate::application::workflow::{actions::ModelToolCallId, lease::WorkflowFence};
use crate::application::{
    agent::ToolCallOrigin,
    workflow::agent::{AgentToolDispatch, PreparedTool},
};
use crate::services::test_support::{LlmTurn, register_scripted_agent_base_url, scripted_llm};

impl ToolSetup {
    /// Save a real model invocation before testing the direct child dispatch protocol.
    /// Replays read the same immutable call; this factory never manufactures an origin.
    pub(in super::super) async fn call(
        &self,
        pin: WorkflowToolPin,
        arguments: Value,
        claim: &ClaimedWorkflow,
    ) -> WorkflowToolCall {
        if let Some(call) = pending(self, claim, &pin, &arguments).await {
            return call;
        }
        let llm = scripted_llm(vec![LlmTurn::tool_call(DECLARED, arguments.clone())]).await;
        register_scripted_agent_base_url(self.agent, &llm.base_url);
        let runtime = WorkflowAgentRuntime::new(
            ProviderRegistry::standard().unwrap(),
            self.persistence().clone(),
            SaveOnly,
            None,
            lease(),
        );
        // The real provider/checkpoint seam stays boxed at the same stock stack budget.
        let outcome = Box::pin(runtime.execute(&kind(), claim)).await;
        assert!(
            outcome.completed_result().is_err(),
            "the saved call must remain pending"
        );
        assert_eq!(llm.finish().await.unwrap(), 1);
        pending(self, claim, &pin, &arguments)
            .await
            .expect("the real driver saved this call")
    }
}

impl ToolSetup {
    /// The pending `channel.call` the real driver saved for `claim`, saving one with
    /// `arguments` first when the attempt has none. A claim holds one pending call, so a test
    /// that needs another call takes another claim.
    pub(in super::super) async fn channel_call(
        &self,
        arguments: &Value,
        claim: &ClaimedWorkflow,
    ) -> crate::application::workflow::channel_call::ChannelCallToolCall {
        if let Some(call) = pending_channel_call(self, claim).await {
            return call;
        }
        let llm = scripted_llm(vec![LlmTurn::tool_call(
            CHANNEL_CALL_DECLARED,
            arguments.clone(),
        )])
        .await;
        register_scripted_agent_base_url(self.agent, &llm.base_url);
        let runtime = WorkflowAgentRuntime::new(
            ProviderRegistry::standard().unwrap(),
            self.persistence().clone(),
            SaveOnly,
            None,
            lease(),
        );
        let outcome = Box::pin(runtime.execute(&kind(), claim)).await;
        assert!(
            outcome.completed_result().is_err(),
            "the saved call must remain pending"
        );
        assert_eq!(llm.finish().await.unwrap(), 1);
        pending_channel_call(self, claim)
            .await
            .expect("the real driver saved this call")
    }
}

/// The model-visible name of the built-in `channel.call`.
pub(in super::super) const CHANNEL_CALL_DECLARED: &str = "channel__call";

async fn pending_channel_call(
    setup: &ToolSetup,
    claim: &ClaimedWorkflow,
) -> Option<crate::application::workflow::channel_call::ChannelCallToolCall> {
    let scope = claim.fence.scope;
    let saved: Option<Saved> = sqlx::query_as("SELECT id,checkpoint,workflow_prepared FROM task_harness_runs WHERE company_id=$1 AND workflow_run_id=$2 AND workflow_execution_id=$3 AND task_id=$4")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(scope.job.0)
        .fetch_optional(setup.persistence().pool()).await.unwrap();
    let saved = saved?;
    let checkpoint =
        crate::adapters::persistence::harness_checkpoint::decode(saved.id, saved.checkpoint)
            .unwrap();
    let pending = checkpoint.pending_call()?;
    let tools: Vec<PreparedTool> =
        serde_json::from_value(saved.workflow_prepared["tools"].clone()).unwrap();
    let tool = tools
        .iter()
        .find(|tool| tool.declared.as_str() == pending.tool_id.as_str())
        .unwrap();
    Some(
        crate::application::workflow::channel_call::ChannelCallToolCall {
            origin: tool.origin,
            call: ToolCallId::new(pending.invocation_id.0),
            arguments: pending.arguments.clone(),
        },
    )
}

#[derive(sqlx::FromRow)]
struct Saved {
    id: Uuid,
    checkpoint: Value,
    workflow_prepared: Value,
}

async fn pending(
    setup: &ToolSetup,
    claim: &ClaimedWorkflow,
    pin: &WorkflowToolPin,
    arguments: &Value,
) -> Option<WorkflowToolCall> {
    let scope = claim.fence.scope;
    let saved: Option<Saved> = sqlx::query_as("SELECT id,checkpoint,workflow_prepared FROM task_harness_runs WHERE company_id=$1 AND workflow_run_id=$2 AND workflow_execution_id=$3 AND task_id=$4")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(scope.job.0)
        .fetch_optional(setup.persistence().pool()).await.unwrap();
    let saved = saved?;
    let checkpoint =
        crate::adapters::persistence::harness_checkpoint::decode(saved.id, saved.checkpoint)
            .unwrap();
    let pending = checkpoint.pending_call()?;
    assert_eq!(&pending.arguments, arguments);
    let tools: Vec<PreparedTool> =
        serde_json::from_value(saved.workflow_prepared["tools"].clone()).unwrap();
    let tool = tools
        .iter()
        .find(|tool| tool.declared.as_str() == pending.tool_id.as_str())
        .unwrap();
    Some(WorkflowToolCall {
        origin: ToolCallOrigin::Agent(tool.origin),
        name: tool.name.clone(),
        pin: pin.clone(),
        call: ToolCallId::new(pending.invocation_id.0),
        arguments: pending.arguments.clone(),
    })
}

struct SaveOnly;
#[async_trait]
impl AgentToolDispatch for SaveOnly {
    async fn dispatch(
        &self,
        _: WorkflowFence,
        _: &PreparedTool,
        _: ModelToolCallId,
        _: Value,
    ) -> AppResult<AgentToolObservation> {
        Err(AppError::ServiceUnavailable("Saved test invocation".into()))
    }
}
