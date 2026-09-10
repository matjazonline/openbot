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
        agent::MAX_AGENT_SKILLS,
        company::CompanyChannelDefaults,
        creation::CreationProvenance,
        transport::{ChannelSelector, TransportKind},
        value_objects::{ChannelSlug, CompanySlug, SkillSlug, ToolId},
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
    pub lease: crate::entities::task::TaskLeaseRef,
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
    /// Direct tool grants for the child. Skill-required tools are added by the harness.
    #[serde(default)]
    granted_tool_ids: Vec<String>,
    /// Existing company-owned skills to attach, addressed by their stable human-readable slugs.
    #[serde(default)]
    skill_slugs: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ProvisionAgentChannelRequest {
    pub invocation: Option<crate::services::harness::runs::InvocationRef>,
    pub request_hash: String,
    pub company_id: Uuid,
    pub lease: crate::entities::task::TaskLeaseRef,
    pub response: ProvisionResponseContext,
    pub agent: AgentWrite,
    pub skill_slugs: Vec<SkillSlug>,
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

#[derive(Debug, Clone)]
pub struct ProvisionResponseContext {
    pub name: String,
    pub slug: ChannelSlug,
    pub company_slug: CompanySlug,
    pub app_domain_name: String,
}

pub(crate) fn provision_response(
    context: &ProvisionResponseContext,
    result: &ProvisionedAgentChannel,
) -> Value {
    let address = crate::entities::channel::Channel::address_for(
        &context.slug,
        &context.company_slug,
        &context.app_domain_name,
    );
    serde_json::json!({"created":result.created, "agent_id":result.agent_id, "channel_id":result.channel_id,
        "channel":ChannelSelector::CurrentCompany(context.slug.clone()).to_string(), "name":context.name,
        "slug":context.slug, "interfaces":[{"transport":TransportKind::Email.as_str(),"display_address":address.as_str()}],
        "warnings":result.warnings})
}

#[async_trait]
pub trait AgentChannelProvisioning: Send + Sync {
    async fn provision_agent_channel(
        &self,
        request: ProvisionAgentChannelRequest,
    ) -> AppResult<ProvisionedAgentChannel>;
}

pub struct CreateAgentChannelTool {
    default_agent_harness: crate::entities::harness::HarnessKind,
    persistence: Arc<dyn AgentChannelProvisioning>,
    context: AgentChannelToolContext,
}

impl CreateAgentChannelTool {
    pub fn new(
        persistence: Arc<dyn AgentChannelProvisioning>,
        context: AgentChannelToolContext,
    ) -> Self {
        Self {
            default_agent_harness: crate::entities::harness::HarnessKind::default(),
            persistence,
            context,
        }
    }

    pub fn with_default_agent_harness(
        mut self,
        kind: crate::entities::harness::HarnessKind,
    ) -> Self {
        self.default_agent_harness = kind;
        self
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
            self.context.lease.task_id,
        );
        let mut skill_slugs = Vec::new();
        for raw_slug in input.skill_slugs {
            let normalized = raw_slug.trim().to_ascii_lowercase();
            let slug = SkillSlug::parse(&normalized)?;
            if !skill_slugs.contains(&slug) {
                skill_slugs.push(slug);
            }
        }
        if skill_slugs.len() > MAX_AGENT_SKILLS {
            return Err(format!(
                "An agent may carry at most {MAX_AGENT_SKILLS} skills."
            ));
        }

        let mut agent = AgentWrite {
            name: input.name.clone(),
            slug: input.slug.clone(),
            description: Some(description.into()),
            system_prompt: Some(instructions.into()),
            granted_tool_ids: input
                .granted_tool_ids
                .into_iter()
                .map(ToolId::from)
                .collect(),
            created_by: Some(provenance.clone()),
            ..AgentWrite::default()
        };
        agent.resolve_harness(self.default_agent_harness);
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
            "granted_tool_ids": agent.granted_tool_ids,
            "skill_slugs": skill_slugs,
        });
        let request_hash = format!("{:x}", Sha256::digest(canonical.to_string().as_bytes()));
        Ok(ProvisionAgentChannelRequest {
            invocation: None,
            request_hash,
            company_id: self.context.company_id,
            lease: self.context.lease,
            response: ProvisionResponseContext {
                name: agent.name.clone(),
                slug: ChannelSlug::from(decision.channel.slug.clone()),
                company_slug: self.context.company_slug.clone(),
                app_domain_name: self.context.app_domain_name.clone(),
            },
            agent,
            skill_slugs,
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
            description: "Permanently create a specialist agent and a callable channel for it in this company, with selected direct tool grants and existing company skills. Name skills by exact slug. The new agent inherits company model settings. The returned channel can be used with outreach_and_await_quorum when that tool is available.",
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
    pub async fn call(
        &self,
        args: Value,
        invocation: Option<crate::services::harness::runs::InvocationRef>,
    ) -> crate::app_error::AppResult<ToolInvocation> {
        let input = match serde_json::from_value(args) {
            Ok(input) => input,
            Err(error) => return Ok(ToolInvocation::failure(format!("Invalid input: {error}"))),
        };
        let mut request = match self.request(input) {
            Ok(request) => request,
            Err(error) => return Ok(ToolInvocation::failure(error)),
        };
        request.invocation = invocation;
        let response = request.response.clone();
        match self.persistence.provision_agent_channel(request).await {
            Ok(result) => Ok(ToolInvocation::success(provision_response(
                &response, &result,
            ))),
            Err(error) => ToolInvocation::denial_or_error(error),
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
                lease: crate::entities::task::TaskLeaseRef {
                    task_id: Uuid::from_u128(3),
                    worker_id: Uuid::new_v4(),
                    execution_generation: Uuid::new_v4(),
                    claimed_owner: Default::default(),
                    ownership_version: 1,
                },
                app_domain_name: "mailagents.test".into(),
                channel_defaults: CompanyChannelDefaults::default(),
                spam_scanning: SpamScanning::Available,
            },
        )
    }

    #[test]
    fn request_is_normalized_internal_and_attributed_to_parent() {
        for harness in crate::entities::harness::HarnessKind::ALL {
            let request = tool()
                .with_default_agent_harness(harness)
                .request(CreateAgentChannelInput {
                    name: "  Research Helper  ".into(),
                    slug: "Research Helper".into(),
                    description: " Finds sources ".into(),
                    instructions: " Research carefully ".into(),
                    granted_tool_ids: vec!["web_fetch".into()],
                    skill_slugs: vec!["  SOURCE-REVIEW  ".into()],
                })
                .unwrap();

            assert_eq!(request.agent.harness_kind, Some(harness));
            assert_eq!(request.agent.slug, "research-helper");
            assert_eq!(request.agent.provider, None);
            assert_eq!(request.agent.granted_tool_ids, [ToolId::from("web_fetch")]);
            assert_eq!(request.skill_slugs, [SkillSlug::from("source-review")]);
            assert_eq!(request.channel.agent_ids, None);
            assert!(request.channel.enabled);
            assert!(request.channel.add_3rd_party);
            let provenance = request.agent.created_by.unwrap();
            assert_eq!(provenance.actor_id, Some(Uuid::from_u128(1)));
            assert_eq!(provenance.source_channel_id, Some(Uuid::from_u128(2)));
            assert_eq!(provenance.source_task_id, Some(Uuid::from_u128(3)));
        }
    }

    #[test]
    fn normalized_identical_requests_have_the_same_idempotency_hash() {
        let first = tool()
            .request(CreateAgentChannelInput {
                name: "Helper".into(),
                slug: "helper".into(),
                description: "Role".into(),
                instructions: "Do work".into(),
                granted_tool_ids: vec!["datetime".into()],
                skill_slugs: vec!["review".into()],
            })
            .unwrap();
        let second = tool()
            .request(CreateAgentChannelInput {
                name: " Helper ".into(),
                slug: "HELPER".into(),
                description: " Role ".into(),
                instructions: " Do work ".into(),
                granted_tool_ids: vec!["datetime".into()],
                skill_slugs: vec![" REVIEW ".into()],
            })
            .unwrap();
        assert_eq!(first.request_hash, second.request_hash);
    }

    #[test]
    fn declaration_exposes_tools_and_skill_slugs_to_the_model() {
        let schema = CreateAgentChannelTool::declaration().input_schema;
        let properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .expect("the tool input has object properties");

        assert!(properties.contains_key("granted_tool_ids"));
        assert!(properties.contains_key("skill_slugs"));
    }
}
