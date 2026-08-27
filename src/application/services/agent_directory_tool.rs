use crate::{
    entities::{
        channel::Channel,
        value_objects::{ChannelSlug, CompanySlug},
    },
    use_cases::{
        agent::AgentPersistence, channel::ChannelPersistence, channel::check_internal_target,
    },
};
use ai_agents::{
    Tool, ToolResult,
    tools::{
        ToolExecutionContext, ToolOperationKind, ToolSafetyMetadata, ToolSideEffectLevel,
        generate_schema,
    },
};
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

pub const AGENT_DIRECTORY_TOOL_ID: &str = "list_company_agents";

/// Who is asking, established by the server from the task execution context.
///
/// None of this is ever accepted as a tool argument: an agent must not be able to enumerate
/// another company's channels by naming it.
#[derive(Debug, Clone)]
pub struct AgentDirectoryContext {
    pub company_id: Uuid,
    pub company_slug: CompanySlug,
    pub source_channel_id: Uuid,
    pub app_domain_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DirectoryInput {}

#[derive(Debug, Serialize)]
struct DirectoryEntry {
    /// The address to pass to the outreach tool's `target_emails`.
    address: String,
    channel_name: String,
    agent_name: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Serialize)]
struct DirectoryOutput {
    agents: Vec<DirectoryEntry>,
    count: usize,
}

/// Lists the sibling agent channels this agent may address.
///
/// Without it, callable addresses have to be hardcoded into a system prompt, where they go stale
/// the moment a channel is renamed or disabled.
pub struct ListCompanyAgentsTool {
    channel_persistence: Arc<dyn ChannelPersistence>,
    agent_persistence: Arc<dyn AgentPersistence>,
    context: AgentDirectoryContext,
}

impl ListCompanyAgentsTool {
    pub fn new(
        channel_persistence: Arc<dyn ChannelPersistence>,
        agent_persistence: Arc<dyn AgentPersistence>,
        context: AgentDirectoryContext,
    ) -> Self {
        Self {
            channel_persistence,
            agent_persistence,
            context,
        }
    }

    /// The channel's responding agent — the same one dispatch picks, so the description shown
    /// here is the description of whoever actually answers.
    async fn responder_name_and_description(
        &self,
        channel: &Channel,
    ) -> (Option<String>, Option<String>) {
        let Some(agent_id) = channel
            .agent_ids
            .as_ref()
            .and_then(|ids| ids.first().copied())
        else {
            return (None, None);
        };
        match self.agent_persistence.get_by_id(agent_id).await {
            Ok(Some(agent)) => (Some(agent.name), agent.description),
            // A directory entry without a name is still useful; the address is the callable part.
            Ok(None) => (None, None),
            Err(error) => {
                tracing::warn!(
                    "Could not load agent {} for the directory listing: {}",
                    agent_id,
                    error
                );
                (None, None)
            }
        }
    }

    fn address_of(&self, slug: &ChannelSlug) -> String {
        Channel::address_for(
            slug,
            &self.context.company_slug,
            &self.context.app_domain_name,
        )
        .to_string()
    }
}

#[async_trait]
impl Tool for ListCompanyAgentsTool {
    fn id(&self) -> &str {
        AGENT_DIRECTORY_TOOL_ID
    }

    fn name(&self) -> &str {
        "List Company Agents"
    }

    fn description(&self) -> &str {
        "List the other agents in this company that you can contact, with their addresses and what \
         each one does. Pass an address from this list to the outreach tool to delegate work to \
         that agent. Your own channel is never listed."
    }

    fn input_schema(&self) -> Value {
        generate_schema::<DirectoryInput>()
    }

    fn safety_metadata(&self) -> ToolSafetyMetadata {
        ToolSafetyMetadata {
            read_only: true,
            concurrency_safe: true,
            operation: ToolOperationKind::Read,
            side_effect_level: ToolSideEffectLevel::None,
            requires_network: false,
            destructive: false,
            open_world: false,
            host_dependent: true,
            requires_user_interaction: false,
            supports_cancellation: false,
            default_requires_approval: false,
            should_defer_schema: false,
            max_output_chars: Some(4_000),
            max_result_size_chars: Some(8_000),
        }
    }

    async fn execute(&self, _args: Value, ctx: ToolExecutionContext) -> ToolResult {
        let max_results = ctx
            .custom_config
            .get("max_results")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(50);

        let channels = match self
            .channel_persistence
            .list_by_company_id(self.context.company_id)
            .await
        {
            Ok(channels) => channels,
            Err(error) => {
                return ToolResult::error(format!("Failed to list company channels: {error}"));
            }
        };

        let mut agents = Vec::new();
        for channel in channels {
            // The same predicate the send path uses, so nothing is listed that cannot be called.
            if check_internal_target(
                &channel,
                self.context.company_id,
                self.context.source_channel_id,
            )
            .is_err()
            {
                continue;
            }
            let (agent_name, description) = self.responder_name_and_description(&channel).await;
            agents.push(DirectoryEntry {
                address: self.address_of(&channel.slug),
                channel_name: channel.name.clone(),
                agent_name,
                description,
            });
            if agents.len() >= max_results {
                break;
            }
        }

        agents.sort_by(|a, b| a.address.cmp(&b.address));
        let output = DirectoryOutput {
            count: agents.len(),
            agents,
        };
        match serde_json::to_string(&output) {
            Ok(json) => ToolResult::ok(json),
            Err(error) => ToolResult::error(format!("Failed to serialize tool output: {error}")),
        }
    }
}
