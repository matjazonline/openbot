use std::sync::Arc;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    app_error::AppResult,
    entities::{
        company::CompanyChannelDefaults,
        creation::CreationProvenance,
        transport::{ChannelSelector, TransportKind},
        value_objects::{ChannelSlug, CompanySlug, ToolId},
    },
    services::harness::{NativeToolDeclaration, NativeToolSafety, ToolInvocation},
    use_cases::{
        agent::{AgentWrite, ProvisioningWarning, SpamScanning, personal_channel_write},
        channel::ChannelWrite,
    },
};

/// Re-exported from the tool catalogue, which owns the id: the entry a picker offers and the
/// tool that answers to it must be the same string.
pub use crate::entities::tool_catalogue::CREATE_AGENT_CHANNEL_TOOL_ID;

#[derive(Debug, Clone)]
pub struct AgentChannelToolContext {
    pub company_id: Uuid,
    pub company_slug: CompanySlug,
    pub source_agent_id: Uuid,
    pub source_agent_name: String,
    pub source_channel_id: Uuid,
    pub task_id: Uuid,
    pub app_domain_name: String,
    pub channel_defaults: CompanyChannelDefaults,
    pub spam_scanning: SpamScanning,
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
    pub warnings: Vec<ProvisioningWarning>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProvisionedAgentChannel {
    pub created: bool,
    pub agent_id: Uuid,
    pub channel_id: Uuid,
    pub warnings: Vec<ProvisioningWarning>,
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
        agent.normalize().map_err(|e| e.to_string())?;
        let mut decision = personal_channel_write(
            &agent,
            &self.context.channel_defaults,
            self.context.spam_scanning,
        );
        decision.channel.created_by = Some(provenance);
        decision
            .channel
            .normalize_with(crate::use_cases::channel::ActiveAgent::SuppliedByCaller)
            .map_err(|e| e.to_string())?;
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
            channel: decision.channel,
            warnings: decision.warnings,
        })
    }
}

impl CreateAgentChannelTool {
    /// How this tool is offered to whichever harness is running.
    ///
    /// An associated function rather than a method: a picker and a boot-time check want to know
    /// what the tool accepts without a company, a task or a persistence handle to construct one
    /// with.
    pub fn declaration() -> NativeToolDeclaration {
        NativeToolDeclaration {
            id: ToolId::from(CREATE_AGENT_CHANNEL_TOOL_ID),
            name: "Create Agent Channel",
            description: "Permanently create a specialist agent and a callable channel for it in this company. The new agent inherits company model settings. After creation, delegate to the returned `channel` selector with outreach_and_await_quorum.",
            input_schema: serde_json::to_value(schemars::schema_for!(CreateAgentChannelInput))
                .unwrap_or_else(|_| serde_json::json!({})),
            safety: NativeToolSafety {
                read_only: false,
                concurrency_safe: false,
                has_external_effect: true,
                requires_network: false,
                destructive: false,
                open_world: false,
                requires_approval_by_default: true,
                max_output_chars: 2_000,
                max_result_chars: 4_000,
            },
        }
    }

    /// Create the agent and its channel, or say why not.
    ///
    /// Returns `ToolInvocation` rather than `AppResult` because most of what can go wrong here is
    /// something the *model* should read and retry differently -- a slug that is taken, a blank
    /// description. Those are `success: false` with the reason, exactly as they were when this was
    /// an `ai_agents::Tool`, and not errors that end the run.
    pub async fn call(&self, args: Value) -> ToolInvocation {
        let input = match serde_json::from_value(args) {
            Ok(input) => input,
            Err(error) => return ToolInvocation::failure(format!("Invalid input: {error}")),
        };
        let request = match self.request(input) {
            Ok(request) => request,
            Err(error) => return ToolInvocation::failure(error),
        };
        let name = request.agent.name.clone();
        let slug = ChannelSlug::from(request.channel.slug.clone());
        match self.persistence.provision_agent_channel(request).await {
            Ok(result) => {
                // What the caller delegates by is the selector and the id, not the address. The
                // address is reported because a person reading the transcript wants it, and is
                // labelled as display data so it is not copied back as a routing key.
                let address = crate::entities::channel::Channel::address_for(
                    &slug,
                    &self.context.company_slug,
                    &self.context.app_domain_name,
                );
                ToolInvocation::success(serde_json::json!({
                    "created": result.created,
                    "agent_id": result.agent_id,
                    "channel_id": result.channel_id,
                    "channel": ChannelSelector::CurrentCompany(slug.clone()).to_string(),
                    "name": name,
                    "slug": slug.as_str(),
                    "interfaces": [{ "transport": TransportKind::Email.as_str(),
                                     "display_address": address.as_str() }],
                    "warnings": result.warnings,
                }))
            }
            Err(error) => {
                ToolInvocation::failure(format!("Failed to create agent channel: {error}"))
            }
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
                channel_defaults: CompanyChannelDefaults::default(),
                spam_scanning: SpamScanning::Available,
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
        assert!(request.channel.add_3rd_party);
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
