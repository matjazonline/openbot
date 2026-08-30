use std::sync::Arc;

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
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    app_error::AppResult,
    entities::{
        creation::CreationProvenance,
        value_objects::{ChannelSlug, CompanySlug},
    },
    use_cases::{agent::AgentWrite, channel::ChannelWrite},
};

pub const CREATE_AGENT_CHANNEL_TOOL_ID: &str = "create_agent_channel";

#[derive(Debug, Clone)]
pub struct AgentChannelToolContext {
    pub company_id: Uuid,
    pub company_slug: CompanySlug,
    pub source_agent_id: Uuid,
    pub source_agent_name: String,
    pub source_channel_id: Uuid,
    pub task_id: Uuid,
    pub app_domain_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CreateAgentChannelInput {
    name: String,
    slug: String,
    description: String,
    instructions: String,
}

#[derive(Debug, Clone)]
pub struct ProvisionAgentChannelRequest {
    pub request_hash: String,
    pub company_id: Uuid,
    pub source_task_id: Uuid,
    pub agent: AgentWrite,
    pub channel: ChannelWrite,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProvisionedAgentChannel {
    pub created: bool,
    pub agent_id: Uuid,
    pub channel_id: Uuid,
}

#[async_trait]
pub trait AgentChannelProvisioning: Send + Sync {
    async fn provision_agent_channel(
        &self,
        request: ProvisionAgentChannelRequest,
    ) -> AppResult<ProvisionedAgentChannel>;
}

pub struct CreateAgentChannelTool {
    persistence: Arc<dyn AgentChannelProvisioning>,
    context: AgentChannelToolContext,
}

impl CreateAgentChannelTool {
    pub fn new(
        persistence: Arc<dyn AgentChannelProvisioning>,
        context: AgentChannelToolContext,
    ) -> Self {
        Self {
            persistence,
            context,
        }
    }

    fn request(
        &self,
        input: CreateAgentChannelInput,
    ) -> Result<ProvisionAgentChannelRequest, String> {
        let instructions = input.instructions.trim();
        let description = input.description.trim();
        if instructions.is_empty() || description.is_empty() {
            return Err("description and instructions cannot be empty".into());
        }
        let provenance = CreationProvenance::agent(
            self.context.source_agent_id,
            self.context.source_agent_name.clone(),
            self.context.source_channel_id,
            self.context.task_id,
        );
        let mut agent = AgentWrite {
            name: input.name.clone(),
            slug: input.slug.clone(),
            description: Some(description.into()),
            system_prompt: Some(instructions.into()),
            created_by: Some(provenance.clone()),
            ..AgentWrite::default()
        };
        let mut channel = ChannelWrite {
            name: input.name,
            slug: input.slug,
            // The agent id is allocated inside the provisioning transaction. Normalize while
            // disabled, then enable before the deferred database invariant is checked at commit.
            enabled: false,
            add_3rd_party: false,
            retrieve_company_memory: false,
            retrieve_agent_memory: false,
            retrieve_user_memory: false,
            persist_company_memory: false,
            persist_agent_memory: false,
            persist_user_memory: false,
            created_by: Some(provenance),
            ..ChannelWrite::default()
        };
        agent.normalize().map_err(|e| e.to_string())?;
        channel.normalize().map_err(|e| e.to_string())?;
        channel.enabled = true;
        let canonical = serde_json::json!({
            "name": agent.name,
            "slug": agent.slug,
            "description": agent.description,
            "instructions": agent.system_prompt,
        });
        let request_hash = format!("{:x}", Sha256::digest(canonical.to_string().as_bytes()));
        Ok(ProvisionAgentChannelRequest {
            request_hash,
            company_id: self.context.company_id,
            source_task_id: self.context.task_id,
            agent,
            channel,
        })
    }
}

#[async_trait]
impl Tool for CreateAgentChannelTool {
    fn id(&self) -> &str {
        CREATE_AGENT_CHANNEL_TOOL_ID
    }
    fn name(&self) -> &str {
        "Create Agent Channel"
    }
    fn description(&self) -> &str {
        "Permanently create a specialist agent and a callable channel for it in this company. The new agent inherits company model settings. After creation, delegate to the returned address with outreach_and_await_quorum."
    }
    fn input_schema(&self) -> Value {
        generate_schema::<CreateAgentChannelInput>()
    }
    fn safety_metadata(&self) -> ToolSafetyMetadata {
        ToolSafetyMetadata {
            read_only: false,
            concurrency_safe: false,
            operation: ToolOperationKind::Write,
            side_effect_level: ToolSideEffectLevel::ExternalWrite,
            requires_network: false,
            destructive: false,
            open_world: false,
            host_dependent: true,
            requires_user_interaction: false,
            supports_cancellation: false,
            default_requires_approval: true,
            should_defer_schema: false,
            max_output_chars: Some(2_000),
            max_result_size_chars: Some(4_000),
        }
    }
    async fn execute(&self, args: Value, _ctx: ToolExecutionContext) -> ToolResult {
        let input = match serde_json::from_value(args) {
            Ok(input) => input,
            Err(error) => return ToolResult::error(format!("Invalid input: {error}")),
        };
        let request = match self.request(input) {
            Ok(request) => request,
            Err(error) => return ToolResult::error(error),
        };
        let name = request.agent.name.clone();
        let slug = ChannelSlug::from(request.channel.slug.clone());
        match self.persistence.provision_agent_channel(request).await {
            Ok(result) => {
                let address = crate::entities::channel::Channel::address_for(
                    &slug,
                    &self.context.company_slug,
                    &self.context.app_domain_name,
                );
                ToolResult::ok(
                    serde_json::json!({
                        "created": result.created, "agent_id": result.agent_id,
                        "channel_id": result.channel_id, "name": name, "slug": slug.as_str(),
                        "address": address.as_str(),
                    })
                    .to_string(),
                )
            }
            Err(error) => ToolResult::error(format!("Failed to create agent channel: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct UnusedPersistence;

    #[async_trait]
    impl AgentChannelProvisioning for UnusedPersistence {
        async fn provision_agent_channel(
            &self,
            _request: ProvisionAgentChannelRequest,
        ) -> AppResult<ProvisionedAgentChannel> {
            unreachable!()
        }
    }

    fn tool() -> CreateAgentChannelTool {
        CreateAgentChannelTool::new(
            Arc::new(UnusedPersistence),
            AgentChannelToolContext {
                company_id: Uuid::nil(),
                company_slug: "acme".into(),
                source_agent_id: Uuid::from_u128(1),
                source_agent_name: "Coordinator".into(),
                source_channel_id: Uuid::from_u128(2),
                task_id: Uuid::from_u128(3),
                app_domain_name: "mailagents.test".into(),
            },
        )
    }

    #[test]
    fn request_is_normalized_internal_and_attributed_to_parent() {
        let request = tool()
            .request(CreateAgentChannelInput {
                name: "  Research Helper  ".into(),
                slug: "Research Helper".into(),
                description: " Finds sources ".into(),
                instructions: " Research carefully ".into(),
            })
            .unwrap();

        assert_eq!(request.agent.slug, "research-helper");
        assert_eq!(request.agent.provider, None);
        assert_eq!(request.channel.agent_ids, None);
        assert!(request.channel.enabled);
        assert!(!request.channel.add_3rd_party);
        let provenance = request.agent.created_by.unwrap();
        assert_eq!(provenance.actor_id, Some(Uuid::from_u128(1)));
        assert_eq!(provenance.source_channel_id, Some(Uuid::from_u128(2)));
        assert_eq!(provenance.source_task_id, Some(Uuid::from_u128(3)));
    }

    #[test]
    fn normalized_identical_requests_have_the_same_idempotency_hash() {
        let first = tool()
            .request(CreateAgentChannelInput {
                name: "Helper".into(),
                slug: "helper".into(),
                description: "Role".into(),
                instructions: "Do work".into(),
            })
            .unwrap();
        let second = tool()
            .request(CreateAgentChannelInput {
                name: " Helper ".into(),
                slug: "HELPER".into(),
                description: " Role ".into(),
                instructions: " Do work ".into(),
            })
            .unwrap();
        assert_eq!(first.request_hash, second.request_hash);
    }
}
