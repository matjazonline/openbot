use crate::{
    entities::{channel::Channel, transport::ChannelSelector, value_objects::ToolId},
    services::harness::{NativeToolDeclaration, NativeToolSafety, ToolInvocation},
    use_cases::{
        agent::AgentPersistence, channel::ChannelPersistence, channel::check_internal_target,
        integration::ChannelBindingPersistence,
    },
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

/// Re-exported from the tool catalogue, which owns the id: the entry a picker offers and the
/// tool that answers to it must be the same string.
pub use crate::entities::tool_catalogue::AGENT_DIRECTORY_TOOL_ID;

/// Who is asking, established by the server from the task execution context.
///
/// None of this is ever accepted as a tool argument: an agent must not be able to enumerate
/// another company's channels by naming it.
#[derive(Debug, Clone)]
pub struct AgentDirectoryContext {
    pub company_id: Uuid,
    pub source_channel_id: Uuid,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DirectoryInput {}

#[derive(Debug, Serialize)]
struct DirectoryEntry {
    /// What to pass to the outreach tool's `target_channels`.
    ///
    /// The channel's own selector, not an address. An address is one transport's way of reaching
    /// this channel; handing the model one and taking it back as the routing key is what made a
    /// mailbox the internal identity of a business channel.
    channel: String,
    /// The channel's stable id, for a caller that has one to correlate against.
    channel_id: Uuid,
    channel_name: String,
    agent_name: Option<String>,
    description: Option<String>,
    /// Which interfaces this channel is currently reachable on, as display data.
    ///
    /// Listed so an agent can say "I'll ask Legal, who is on Slack" without any of it being a
    /// routing decision: delivery is planned from the channel's bindings, not from this text.
    interfaces: Vec<DirectoryInterface>,
}

/// One interface a listed channel currently carries traffic on.
#[derive(Debug, Serialize)]
struct DirectoryInterface {
    transport: &'static str,
    /// The binding's own label -- a mailbox for email, a conversation name for Slack. Display
    /// only.
    label: String,
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
    binding_persistence: Arc<dyn ChannelBindingPersistence>,
    context: AgentDirectoryContext,
    /// This tool's slice of the agent's tool policy, when the agent configured one.
    ///
    /// Supplied by the caller rather than read from the running harness, so the bound applies the
    /// same way whichever runtime is invoking the tool.
    policy_config: Option<Value>,
}

impl ListCompanyAgentsTool {
    pub fn new(
        channel_persistence: Arc<dyn ChannelPersistence>,
        agent_persistence: Arc<dyn AgentPersistence>,
        binding_persistence: Arc<dyn ChannelBindingPersistence>,
        context: AgentDirectoryContext,
    ) -> Self {
        Self {
            channel_persistence,
            agent_persistence,
            binding_persistence,
            context,
            policy_config: None,
        }
    }

    pub fn with_policy_config(mut self, config: Value) -> Self {
        self.policy_config = Some(config);
        self
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

    /// The interfaces a listed channel is reachable on.
    ///
    /// A channel with no active interface is still listed: it can be delegated to and answered in
    /// the app, and hiding it would make the directory disagree with what the send path accepts.
    /// A failed read degrades to an empty list rather than propagating, because this is display
    /// enrichment -- the callable decision above is what must not degrade.
    async fn interfaces_of(&self, channel: &Channel) -> Vec<DirectoryInterface> {
        match self
            .binding_persistence
            .active_bindings_for_channel(self.context.company_id, channel.id)
            .await
        {
            Ok(bindings) => bindings
                .into_iter()
                .map(|binding| DirectoryInterface {
                    transport: binding.transport.as_str(),
                    label: binding.display_label,
                })
                .collect(),
            Err(error) => {
                tracing::warn!(
                    channel_id = %channel.id,
                    %error,
                    "Could not list a channel's interfaces for the directory listing"
                );
                Vec::new()
            }
        }
    }
}

/// The most entries one listing returns when policy names no other bound.
///
/// `src/AGENTS.md` -- bound work at every external boundary. A company with a thousand channels
/// must not put a thousand of them in a prompt.
pub const DEFAULT_DIRECTORY_MAX_RESULTS: usize = 50;

impl ListCompanyAgentsTool {
    /// How this tool is offered to whichever harness is running.
    pub fn declaration() -> NativeToolDeclaration {
        NativeToolDeclaration {
            id: ToolId::from(AGENT_DIRECTORY_TOOL_ID),
            name: "List Company Agents",
            description: "List the other agents in this company that you can contact, with what \
                          each one does and which interfaces it is reachable on. Pass a `channel` \
                          value from this list to the outreach tool's target_channels to delegate \
                          work to that agent. Your own channel is never listed.",
            input_schema: serde_json::to_value(schemars::schema_for!(DirectoryInput))
                .unwrap_or_else(|_| serde_json::json!({})),
            safety: NativeToolSafety {
                read_only: true,
                concurrency_safe: true,
                has_external_effect: false,
                requires_network: false,
                destructive: false,
                open_world: false,
                requires_approval_by_default: false,
                max_output_chars: 4_000,
                max_result_chars: 8_000,
            },
        }
    }

    /// The callable siblings of this run's channel.
    pub async fn call(&self, _args: Value) -> ToolInvocation {
        let policy = self.policy_config.clone().unwrap_or(Value::Null);
        let max_results = policy
            .get("max_results")
            .or_else(|| policy.get("config").and_then(|c| c.get("max_results")))
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(DEFAULT_DIRECTORY_MAX_RESULTS);

        let channels = match self
            .channel_persistence
            .list_by_company_id(self.context.company_id)
            .await
        {
            Ok(channels) => channels,
            Err(error) => {
                return ToolInvocation::failure(format!(
                    "Failed to list company channels: {error}"
                ));
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
                channel: ChannelSelector::CurrentCompany(channel.slug.clone()).to_string(),
                channel_id: channel.id,
                channel_name: channel.name.clone(),
                agent_name,
                description,
                interfaces: self.interfaces_of(&channel).await,
            });
            if agents.len() >= max_results {
                break;
            }
        }

        agents.sort_by(|a, b| a.channel.cmp(&b.channel));
        let output = DirectoryOutput {
            count: agents.len(),
            agents,
        };
        match serde_json::to_value(&output) {
            Ok(json) => ToolInvocation::success(json),
            Err(error) => {
                ToolInvocation::failure(format!("Failed to serialize tool output: {error}"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app_error::AppResult,
        entities::{
            agent::Agent,
            channel::ChannelAccessMode,
            creation::CreationProvenance,
            transport::{
                BindingAccessPolicy, BindingAccessSnapshot, BindingDeliveryPolicy, BindingStatus,
                ChannelBinding, ChannelBindingId, EndpointNamespace, ExternalEndpointKey,
                TransportKind,
            },
            value_objects::CompanySlug,
        },
        use_cases::{channel::ChannelWrite, integration::BindingWrite},
    };
    use async_trait::async_trait;
    use serde_json::Value as Json;

    struct Directory {
        channels: Vec<Channel>,
        agent: Option<Agent>,
        bindings: Vec<ChannelBinding>,
    }

    #[async_trait]
    impl ChannelPersistence for Directory {
        async fn create(&self, _company_id: Uuid, _write: ChannelWrite) -> AppResult<Channel> {
            unreachable!("the directory only reads")
        }
        async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Channel>> {
            Ok(self.channels.iter().find(|c| c.id == id).cloned())
        }
        async fn get_by_company_slug_and_channel_slug(
            &self,
            _company: &CompanySlug,
            _channel: &crate::entities::value_objects::ChannelSlug,
        ) -> AppResult<Option<Channel>> {
            Ok(None)
        }
        async fn list_by_company_id(&self, _company_id: Uuid) -> AppResult<Vec<Channel>> {
            Ok(self.channels.clone())
        }
        async fn update(&self, _id: Uuid, _write: ChannelWrite) -> AppResult<Channel> {
            unreachable!("the directory only reads")
        }
        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unreachable!("the directory only reads")
        }
    }

    #[async_trait]
    impl AgentPersistence for Directory {
        async fn get_by_id(&self, _id: Uuid) -> AppResult<Option<Agent>> {
            Ok(self.agent.clone())
        }
        async fn create(
            &self,
            _company_id: Uuid,
            _write: crate::use_cases::agent::AgentWrite,
        ) -> AppResult<Agent> {
            unreachable!("the directory only reads")
        }
        async fn get_by_company_slug_and_agent_slug(
            &self,
            _company_slug: &str,
            _agent_slug: &str,
        ) -> AppResult<Option<Agent>> {
            Ok(None)
        }
        async fn list_by_company_id(&self, _company_id: Uuid) -> AppResult<Vec<Agent>> {
            Ok(self.agent.clone().into_iter().collect())
        }
        async fn update(
            &self,
            _id: Uuid,
            _write: crate::use_cases::agent::AgentWrite,
        ) -> AppResult<Agent> {
            unreachable!("the directory only reads")
        }
        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unreachable!("the directory only reads")
        }
    }

    #[async_trait]
    impl ChannelBindingPersistence for Directory {
        async fn create_binding(&self, _write: BindingWrite) -> AppResult<ChannelBinding> {
            unreachable!("the directory only reads")
        }
        async fn active_bindings_for_channel(
            &self,
            _company_id: Uuid,
            channel_id: Uuid,
        ) -> AppResult<Vec<ChannelBinding>> {
            Ok(self
                .bindings
                .iter()
                .filter(|binding| binding.channel_id == channel_id)
                .cloned()
                .collect())
        }
        async fn find_active_binding_by_endpoint(
            &self,
            _endpoint: &crate::use_cases::integration::InboundEndpoint,
        ) -> AppResult<Option<ChannelBinding>> {
            Ok(None)
        }
        async fn list_bindings_for_company(
            &self,
            _company_id: Uuid,
        ) -> AppResult<Vec<ChannelBinding>> {
            Ok(self.bindings.clone())
        }
        async fn get_binding(
            &self,
            _company_id: Uuid,
            _binding_id: ChannelBindingId,
        ) -> AppResult<Option<ChannelBinding>> {
            Ok(None)
        }
        async fn set_binding_status(
            &self,
            _change: crate::use_cases::integration::BindingStatusChange,
        ) -> AppResult<ChannelBinding> {
            unreachable!("the directory only reads")
        }
        async fn list_binding_audit_events(
            &self,
            _company_id: Uuid,
            _binding_id: ChannelBindingId,
            _limit: i64,
        ) -> AppResult<Vec<crate::entities::transport::BindingAuditEvent>> {
            Ok(Vec::new())
        }
    }

    fn channel(company_id: Uuid, slug: &str) -> Channel {
        Channel {
            owner_agent_id: None,
            enabled: true,
            add_3rd_party: true,
            id: Uuid::new_v4(),
            company_id,
            name: format!("{slug} channel"),
            description: None,
            slug: slug.into(),
            alias_slugs: Vec::new(),
            participant_emails: None,
            access_mode: ChannelAccessMode::Team,
            principal_grants: Vec::new(),
            agent_ids: Some(vec![Uuid::new_v4()]),
            retrieve_company_memory: false,
            retrieve_agent_memory: false,
            retrieve_user_memory: false,
            persist_company_memory: false,
            persist_agent_memory: false,
            persist_user_memory: false,
            created_by: CreationProvenance::system(),
            created_at: chrono::Utc::now(),
        }
    }

    fn binding(channel: &Channel, transport: TransportKind, label: &str) -> ChannelBinding {
        ChannelBinding {
            id: ChannelBindingId::random(),
            company_id: channel.company_id,
            channel_id: channel.id,
            installation_id: None,
            transport,
            namespace: EndpointNamespace::parse("deployment").unwrap(),
            external_endpoint_key: ExternalEndpointKey::parse(label).unwrap(),
            display_label: label.to_string(),
            access_policy: BindingAccessPolicy::ChannelAcl,
            delivery_policy: BindingDeliveryPolicy::ReplyOnly,
            status: BindingStatus::Active,
            disabled_reason: None,
            created_by: CreationProvenance::system(),
            access_snapshot: BindingAccessSnapshot::deployment_endpoint(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    async fn listing(directory: Directory, source_channel_id: Uuid) -> Json {
        let company_id = directory.channels[0].company_id;
        let directory = Arc::new(directory);
        let tool = ListCompanyAgentsTool::new(
            directory.clone(),
            directory.clone(),
            directory,
            AgentDirectoryContext {
                company_id,
                source_channel_id,
            },
        );
        let result = tool.call(Json::Null).await;
        assert!(result.success, "{:?}", result.output);
        result.output
    }

    /// The listing is what an agent delegates from, so it names each channel by its selector and
    /// its id. It used to hand out an address and take that address back as the routing key, which
    /// made a mailbox the internal identity of a business channel.
    #[tokio::test]
    async fn the_listing_names_each_channel_by_its_selector_and_id() {
        let company_id = Uuid::new_v4();
        let source = channel(company_id, "source");
        let target = channel(company_id, "legal");

        let output = listing(
            Directory {
                channels: vec![source.clone(), target.clone()],
                agent: None,
                bindings: Vec::new(),
            },
            source.id,
        )
        .await;

        let entries = output["agents"].as_array().unwrap();
        assert_eq!(entries.len(), 1, "the caller's own channel is never listed");
        assert_eq!(entries[0]["channel"], "legal");
        assert_eq!(entries[0]["channel_id"], target.id.to_string());
        assert_eq!(entries[0]["channel_name"], "legal channel");
        assert!(
            entries[0].get("address").is_none(),
            "no address is offered as a routing key: {output}"
        );
    }

    /// Interfaces are reported so an agent can say *how* a colleague is reachable, and are labelled
    /// per transport rather than flattened into one address.
    #[tokio::test]
    async fn the_listing_reports_every_interface_a_channel_is_reachable_on() {
        let company_id = Uuid::new_v4();
        let source = channel(company_id, "source");
        let target = channel(company_id, "legal");

        let output = listing(
            Directory {
                bindings: vec![
                    binding(&target, TransportKind::Email, "legal@acme.example"),
                    binding(&target, TransportKind::Slack, "C0LEGAL"),
                ],
                channels: vec![source.clone(), target],
                agent: None,
            },
            source.id,
        )
        .await;

        let interfaces = output["agents"][0]["interfaces"].as_array().unwrap();
        assert_eq!(interfaces.len(), 2);
        assert_eq!(interfaces[0]["transport"], "email");
        assert_eq!(interfaces[0]["label"], "legal@acme.example");
        assert_eq!(interfaces[1]["transport"], "slack");
        assert_eq!(interfaces[1]["label"], "C0LEGAL");
    }

    /// A channel with no active interface is still delegable -- the answer is read in the app -- so
    /// hiding it would make the directory disagree with what the send path accepts.
    #[tokio::test]
    async fn a_channel_with_no_active_interface_is_still_listed() {
        let company_id = Uuid::new_v4();
        let source = channel(company_id, "source");
        let target = channel(company_id, "legal");

        let output = listing(
            Directory {
                channels: vec![source.clone(), target],
                agent: None,
                bindings: Vec::new(),
            },
            source.id,
        )
        .await;

        assert_eq!(output["count"], 1);
        assert!(
            output["agents"][0]["interfaces"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}
