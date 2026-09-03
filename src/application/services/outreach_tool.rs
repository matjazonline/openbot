use crate::{
    adapters::persistence::task::{CreateOutreachRequest, OutreachTargetRequest, TaskPersistence},
    adapters::protocols::email::{EmailChannelSelectorParser, EmailRecipientDestination},
    app_error::AppResult,
    entities::{
        channel::Channel,
        correlation::CorrelationId,
        email_message::EmailMessageMetadata,
        message::CanonicalMessageId,
        message::{MessageDirection, MessageParticipantKind, MessageRole},
        transport::{ChannelSelector, ExternalDestination},
        value_objects::{ChannelSlug, CompanySlug, EmailAddress, MessageId},
    },
    transport::{
        CanonicalContent, DeliveryComposer, DeliveryContext, DeliveryPurpose, DeliveryRequest,
        EmailDeliveryContext, EmailRelayTrace,
    },
    use_cases::{
        channel::{ChannelPersistence, InternalTargetOutcome, resolve_internal_target},
        thread::{
            MessageAuthorWrite, MessageCorrelation, MessageParticipantWrite, MessageWrite,
            qualified_email_identity,
        },
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
use chrono::{Duration, Utc};
use lettre::message::Mailbox;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use uuid::Uuid;

pub const OUTREACH_TOOL_ID: &str = "outreach_and_await_quorum";

#[derive(Debug, Clone)]
pub struct OutreachToolContext {
    pub task_id: Uuid,
    /// The chain the running task belongs to, so every email this tool sends and every reply it
    /// waits for stays on the trail of the run that asked for them.
    pub correlation_id: CorrelationId,
    pub worker_id: Uuid,
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub channel_name: String,
    pub channel_slug: ChannelSlug,
    pub company_slug: CompanySlug,
    /// The conversation the outreach belongs to. Every question is recorded here as a message
    /// before it is sent, which is what tells the reply guard that an outbound message was the
    /// agent asking rather than the agent answering.
    pub thread_id: Uuid,
    pub trigger_message_id: MessageId,
    pub thread_references: Vec<MessageId>,
    pub hop_count: u32,
    pub trace_channels: Vec<Uuid>,
    pub app_domain_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct OutreachInput {
    /// Same-company agent channels to delegate to, as the `channel` value from
    /// `list_company_agents` (`channel` or `company/channel`). Not an address: how the request
    /// reaches that channel is decided from its own interfaces.
    #[serde(default)]
    target_channels: Vec<String>,
    /// People outside the platform to contact, by email address. Each recipient receives a
    /// separate email. A platform channel address is refused here -- use `target_channels`.
    #[serde(default)]
    target_emails: Vec<String>,
    /// Percentage of distinct recipients that must reply before the task resumes. Omit for 100,
    /// which is what a single delegated request wants.
    #[serde(default)]
    completion_threshold_percent: Option<f64>,
    /// Maximum hours to wait before requesting a human timeout decision. Omit for the configured
    /// default.
    #[serde(default)]
    timeout_hours: Option<u32>,
    /// Subject for the outreach emails.
    subject: String,
    /// Plain-text body for the outreach emails.
    body: String,
}

#[derive(Debug, Serialize)]
struct OutreachOutput {
    accepted: bool,
    status: String,
    outreach_id: Uuid,
    target_count: usize,
    response_count: usize,
    required_threshold_percent: f64,
    required_response_count: usize,
    queued_message_count: usize,
    expires_at: String,
}

pub struct OutreachAndAwaitQuorumTool {
    persistence: Arc<dyn TaskPersistence>,
    channel_persistence: Arc<dyn ChannelPersistence>,
    /// Freezes each question's mail so the outreach, its target rows, the questions themselves and
    /// their deliveries all land in one transaction.
    deliveries: DeliveryComposer,
    context: OutreachToolContext,
    suspended: Arc<AtomicBool>,
}

impl OutreachAndAwaitQuorumTool {
    pub fn new(
        persistence: Arc<dyn TaskPersistence>,
        channel_persistence: Arc<dyn ChannelPersistence>,
        deliveries: DeliveryComposer,
        context: OutreachToolContext,
        suspended: Arc<AtomicBool>,
    ) -> Self {
        Self {
            persistence,
            channel_persistence,
            deliveries,
            context,
            suspended,
        }
    }
}

#[async_trait]
impl Tool for OutreachAndAwaitQuorumTool {
    fn id(&self) -> &str {
        OUTREACH_TOOL_ID
    }

    fn name(&self) -> &str {
        "Outreach and Await Quorum"
    }

    fn description(&self) -> &str {
        "Contact one or more recipients and pause this task until enough of them reply, or until the timeout requires a human decision. \
         Recipients may be third parties, or — when tool policy permits — other agents in this company, addressed by their channel address. \
         To delegate one request to one agent or person, pass a single address and omit completion_threshold_percent and timeout_hours. \
         Use list_company_agents to discover which agents in this company you can address."
    }

    fn input_schema(&self) -> Value {
        generate_schema::<OutreachInput>()
    }

    fn safety_metadata(&self) -> ToolSafetyMetadata {
        ToolSafetyMetadata {
            read_only: false,
            concurrency_safe: false,
            operation: ToolOperationKind::Write,
            side_effect_level: ToolSideEffectLevel::ExternalWrite,
            requires_network: true,
            destructive: false,
            open_world: true,
            host_dependent: true,
            requires_user_interaction: false,
            supports_cancellation: false,
            default_requires_approval: true,
            should_defer_schema: false,
            max_output_chars: Some(4_000),
            max_result_size_chars: Some(8_000),
        }
    }

    async fn execute(&self, args: Value, ctx: ToolExecutionContext) -> ToolResult {
        let input: OutreachInput = match serde_json::from_value(args) {
            Ok(input) => input,
            Err(error) => return ToolResult::error(format!("Invalid input: {error}")),
        };

        let limits = OutreachLimits::from_config(&ctx.custom_config);
        let resolved = match resolve_targets(
            &input,
            TargetPolicy {
                max_targets: config_usize(&ctx.custom_config, "max_targets", 50),
                scope: match configured_target_scope(&ctx.custom_config) {
                    Ok(scope) => scope,
                    Err(error) => return ToolResult::error(error),
                },
            },
            &self.context,
            self.channel_persistence.as_ref(),
        )
        .await
        {
            Ok(targets) => targets,
            Err(error) => return ToolResult::error(error),
        };
        // The idempotency key is hashed over the *resolved* destinations, so the short and long
        // forms of one request re-attach instead of mailing everybody twice.
        let canonical_targets: Vec<String> = resolved.iter().map(|target| target.key()).collect();
        let request = match ValidatedOutreach::from_input(&input, limits) {
            Ok(request) => request,
            Err(error) => return ToolResult::error(error),
        };

        let targets = match self.build_target_requests(&resolved, &request).await {
            Ok(targets) => targets,
            Err(error) => return ToolResult::error(error),
        };

        let progress = match self
            .persistence
            .create_outreach_and_pause(CreateOutreachRequest {
                id: Uuid::new_v4(),
                task_id: self.context.task_id,
                company_id: self.context.company_id,
                channel_id: self.context.channel_id,
                correlation_id: self.context.correlation_id,
                worker_id: self.context.worker_id,
                outreach_key: request.idempotency_key(self.context.task_id, &canonical_targets),
                required_threshold_percent: request.threshold_percent,
                expires_at: Utc::now() + Duration::hours(request.timeout_hours as i64),
                subject: request.subject.to_string(),
                body: request.body.to_string(),
                targets,
            })
            .await
        {
            Ok(progress) => progress,
            Err(error) => return ToolResult::error(format!("Failed to create outreach: {error}")),
        };

        // Suspending parks the whole agent run until the replies arrive or the outreach times out.
        if progress.suspended {
            self.suspended.store(true, Ordering::SeqCst);
        }

        let output = OutreachOutput {
            accepted: true,
            status: progress.status.as_str().to_string(),
            outreach_id: progress.id,
            target_count: progress.target_count,
            response_count: progress.response_count,
            required_threshold_percent: progress.required_threshold_percent,
            required_response_count: progress.required_response_count,
            queued_message_count: if progress.suspended {
                progress.target_count
            } else {
                0
            },
            expires_at: progress.expires_at.to_rfc3339(),
        };
        match serde_json::to_string(&output) {
            Ok(output) => ToolResult::ok(output),
            Err(error) => ToolResult::error(format!("Failed to serialize tool output: {error}")),
        }
    }
}

/// An outreach request whose numbers and text have passed validation.
struct ValidatedOutreach<'a> {
    threshold_percent: f64,
    timeout_hours: u32,
    subject: &'a str,
    body: &'a str,
}

/// The two hour bounds from tool policy, named so they cannot be swapped at the call site.
#[derive(Debug, Clone, Copy)]
struct OutreachLimits {
    /// Applied when the model omits `timeout_hours`.
    default_timeout_hours: u32,
    max_timeout_hours: u32,
}

impl OutreachLimits {
    fn from_config(config: &Value) -> Self {
        Self {
            default_timeout_hours: config_u32(config, "default_timeout_hours", 96),
            max_timeout_hours: config_u32(config, "max_timeout_hours", 720),
        }
    }
}

impl<'a> ValidatedOutreach<'a> {
    /// Resolve omitted numbers *here*, before [`ValidatedOutreach::idempotency_key`] runs, so the
    /// short and long forms of the same request hash alike and a retry re-attaches instead of
    /// mailing everyone a second time.
    fn from_input(input: &'a OutreachInput, limits: OutreachLimits) -> Result<Self, String> {
        let max_timeout_hours = limits.max_timeout_hours;
        let threshold = input.completion_threshold_percent.unwrap_or(100.0);
        let timeout_hours = input.timeout_hours.unwrap_or(limits.default_timeout_hours);

        if !threshold.is_finite() || threshold <= 0.0 || threshold > 100.0 {
            return Err(
                "completion_threshold_percent must be greater than 0 and at most 100".into(),
            );
        }
        if timeout_hours == 0 || timeout_hours > max_timeout_hours {
            return Err(format!(
                "timeout_hours must be between 1 and {max_timeout_hours}"
            ));
        }
        let subject = input.subject.trim();
        let body = input.body.trim();
        if subject.is_empty() || subject.chars().count() > 300 {
            return Err("subject must contain between 1 and 300 characters".into());
        }
        if body.is_empty() || body.chars().count() > 20_000 {
            return Err("body must contain between 1 and 20000 characters".into());
        }

        Ok(Self {
            threshold_percent: threshold,
            timeout_hours,
            subject,
            body,
        })
    }

    /// Hash of everything that defines this outreach, so an agent retrying the same tool call
    /// re-attaches to the existing outreach instead of mailing everyone twice.
    fn idempotency_key(&self, task_id: Uuid, targets: &[String]) -> String {
        let canonical = serde_json::json!({
            "task_id": task_id,
            "targets": targets,
            "completion_threshold_percent": self.threshold_percent,
            "timeout_hours": self.timeout_hours,
            "subject": self.subject,
            "body": self.body,
        });
        format!("{:x}", Sha256::digest(canonical.to_string().as_bytes()))
    }
}

impl OutreachAndAwaitQuorumTool {
    /// One question and one queued mail per target, each carrying the originating channel's
    /// identity so replies land back on the same thread.
    async fn build_target_requests(
        &self,
        targets: &[NormalizedOutreachTarget],
        request: &ValidatedOutreach<'_>,
    ) -> Result<Vec<OutreachTargetRequest>, String> {
        let mut built = Vec::with_capacity(targets.len());
        for (position, target) in targets.iter().enumerate() {
            built.push(
                self.target_request(target, request, position)
                    .await
                    .map_err(|error| format!("Failed to prepare an outreach target: {error}"))?,
            );
        }
        Ok(built)
    }

    async fn target_request(
        &self,
        target: &NormalizedOutreachTarget,
        request: &ValidatedOutreach<'_>,
        position: usize,
    ) -> AppResult<OutreachTargetRequest> {
        let email = target.delivery_address().clone();
        let from = Channel::address_for(
            &self.context.channel_slug,
            &self.context.company_slug,
            &self.context.app_domain_name,
        );

        let context = EmailDeliveryContext {
            from: from.clone(),
            from_name: Some(self.context.channel_name.clone()),
            recipient_to: email.clone(),
            recipients_cc: Vec::new(),
            in_reply_to: Some(self.context.trigger_message_id.clone()),
            references: self.context.thread_references.clone(),
            // An outreach continues the run's chain into someone else's mailbox, so it carries the
            // hop budget: a question delegated to another channel must not be able to loop.
            relay: Some(EmailRelayTrace {
                source_channel_id: self.context.channel_id,
                hop_count: self.context.hop_count,
                trace_channels: self.context.trace_channels.clone(),
            }),
        };
        let content = CanonicalContent::parse(request.subject, request.body)?;

        // The message id is minted here so the delivery can name it, and the *provider* key comes
        // back from the composer so the question can be recorded under the key it will go out
        // under. That is what lets the answer -- which quotes nothing else -- find this thread.
        let message_id = CanonicalMessageId::random();
        let composed = self
            .deliveries
            .compose(DeliveryRequest {
                company_id: self.context.company_id,
                channel_id: self.context.channel_id,
                message_id,
                task_id: Some(self.context.task_id),
                correlation_id: self.context.correlation_id,
                purpose: DeliveryPurpose::Outreach,
                // The task and the target's position, so an agent retrying the same tool call
                // re-derives the keys the first call used and mails nobody twice. The recipient is
                // already part of the key the composer builds, so two targets never collide.
                source_key: format!("task:{}:outreach:{position}", self.context.task_id),
                content: &content,
                context: DeliveryContext::Email(context),
            })
            .await?;

        // The question, recorded in the thread as the agent asking. Its participants are the real
        // sender and recipient, so the conversation shows who was asked without anyone having to
        // read a delivery payload -- and its provider key is what a reply quoting it resolves
        // through.
        let mut message = MessageWrite {
            id: message_id,
            ..MessageWrite::internal(
                self.context.thread_id,
                MessageAuthorWrite::Platform,
                request.subject.to_string(),
                request.body.to_string(),
                MessageDirection::Outbound,
                MessageRole::Agent,
                self.context.correlation_id,
            )
        }
        .with_participants(vec![
            MessageParticipantWrite::new(
                MessageParticipantKind::Sender,
                qualified_email_identity(from.as_str())?,
            ),
            MessageParticipantWrite::new(
                MessageParticipantKind::To,
                qualified_email_identity(email.as_str())?,
            ),
        ]);
        if let Some(provider_key) = composed.provider_key.as_ref() {
            message = message.with_correlation(MessageCorrelation::Email(
                EmailMessageMetadata::new(MessageId::from(provider_key.as_str().to_string()))
                    .in_reply_to(Some(self.context.trigger_message_id.clone()))
                    .references(self.context.thread_references.clone())
                    .raw_bodies(Some(request.body.to_string()), None),
            ));
        }

        Ok(OutreachTargetRequest {
            email,
            request: message,
            delivery: composed.delivery,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AllowedTargetScope {
    ExternalOnly,
    SameCompanyChannels,
    Any,
}

/// The two bounds a request is checked against before anything is resolved.
///
/// Named because both come from tool policy rather than from the model, and because a count and
/// an enum next to each other in a signature are the classic transposition.
#[derive(Clone, Copy)]
struct TargetPolicy {
    max_targets: usize,
    scope: AllowedTargetScope,
}

/// Where one outreach request goes.
///
/// The two arms are the split this type exists for. A business channel is named by its own
/// selector and reached through whichever interfaces it has; somebody outside the platform is
/// named by an address on one transport. Collapsing them -- classifying a model-supplied string
/// and discovering an address was "really" a channel -- is what made a mailbox the internal
/// identity of a channel.
#[derive(Debug, Clone, PartialEq, Eq)]
enum OutreachDestination {
    Channel {
        selector: ChannelSelector,
        channel_id: Uuid,
        /// The channel's own inbound address.
        ///
        /// Still needed because delivery and reply correlation are still email-keyed until the
        /// generic delivery model lands: `task_outreach_targets` is keyed by address, and a reply
        /// is matched by its sender. Carried as the *delivery* address of a destination that is
        /// already identified by `channel_id`, never as the thing that identifies it.
        delivery_address: EmailAddress,
    },
    External(ExternalDestination),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedOutreachTarget {
    destination: OutreachDestination,
}

impl NormalizedOutreachTarget {
    /// The address this request is delivered to today.
    fn delivery_address(&self) -> &EmailAddress {
        match &self.destination {
            OutreachDestination::Channel {
                delivery_address, ..
            } => delivery_address,
            OutreachDestination::External(ExternalDestination::Email(address)) => address,
        }
    }

    /// What this target is in the idempotency key: the canonical destination, so renaming a
    /// channel's mailbox does not look like a different request.
    fn key(&self) -> String {
        match &self.destination {
            OutreachDestination::Channel { channel_id, .. } => format!("channel:{channel_id}"),
            OutreachDestination::External(ExternalDestination::Email(address)) => {
                format!("email:{address}")
            }
        }
    }
}

fn configured_target_scope(config: &Value) -> Result<AllowedTargetScope, String> {
    match config
        .get("allowed_target_scope")
        .and_then(Value::as_str)
        .unwrap_or("external_only")
    {
        "external_only" => Ok(AllowedTargetScope::ExternalOnly),
        "same_company_channels" => Ok(AllowedTargetScope::SameCompanyChannels),
        "any" => Ok(AllowedTargetScope::Any),
        value => Err(format!(
            "Unsupported allowed_target_scope '{value}'; expected external_only, same_company_channels, or any"
        )),
    }
}

/// Turn what the model asked for into destinations this tool policy permits.
///
/// Every allowlist decision here is taken against something the *server* resolved -- a channel row
/// it loaded, an address it parsed -- and never against the model's text. The two lists are checked
/// separately because they are separately permitted: a policy may allow delegating to colleagues
/// and forbid mailing strangers, or the reverse.
///
/// A free function, not a method: nothing about resolving a target touches task state, and a test
/// of the policy should not have to stand up a task store to make one.
async fn resolve_targets(
    input: &OutreachInput,
    policy: TargetPolicy,
    context: &OutreachToolContext,
    channels: &dyn ChannelPersistence,
) -> Result<Vec<NormalizedOutreachTarget>, String> {
    let requested = input.target_channels.len() + input.target_emails.len();
    if requested == 0 || requested > policy.max_targets {
        return Err(format!(
            "an outreach must name between 1 and {} targets across target_channels and \
             target_emails",
            policy.max_targets
        ));
    }

    let mut targets: Vec<NormalizedOutreachTarget> = Vec::with_capacity(requested);
    for value in &input.target_channels {
        let target = resolve_channel_target(value, policy.scope, context, channels).await?;
        push_unique(&mut targets, target);
    }
    for value in &input.target_emails {
        push_unique(
            &mut targets,
            resolve_email_target(value, policy.scope, context)?,
        );
    }
    targets.sort_by_key(NormalizedOutreachTarget::key);
    Ok(targets)
}

/// One same-company channel, resolved from its selector.
async fn resolve_channel_target(
    value: &str,
    scope: AllowedTargetScope,
    context: &OutreachToolContext,
    channels: &dyn ChannelPersistence,
) -> Result<NormalizedOutreachTarget, String> {
    if scope == AllowedTargetScope::ExternalOnly {
        return Err(format!(
            "This tool policy permits no platform channels as targets: {value}"
        ));
    }
    let selector = ChannelSelector::parse(value).map_err(|error| error.to_string())?;
    let outcome =
        resolve_internal_target(&selector, context.company_id, context.channel_id, channels)
            .await
            .map_err(|error| format!("Failed to resolve platform channel {selector}: {error}"))?;
    match outcome {
        InternalTargetOutcome::Callable(channel) => Ok(NormalizedOutreachTarget {
            destination: OutreachDestination::Channel {
                // The channel decides its own address; the selector does not spell it.
                delivery_address: channel
                    .inbound_address(&context.company_slug, &context.app_domain_name),
                selector,
                channel_id: channel.id,
            },
        }),
        InternalTargetOutcome::Rejected(reason) => Err(reason),
    }
}

/// One recipient outside the platform.
///
/// A platform address is refused rather than quietly routed internally: the two are different
/// requests under different policy, and accepting one here would let the model reach a colleague
/// through the list meant for strangers.
fn resolve_email_target(
    value: &str,
    scope: AllowedTargetScope,
    context: &OutreachToolContext,
) -> Result<NormalizedOutreachTarget, String> {
    if scope == AllowedTargetScope::SameCompanyChannels {
        return Err(format!(
            "Only same-company platform channels are permitted by this tool policy: {value}"
        ));
    }
    let mailbox: Mailbox = value
        .trim()
        .parse()
        .map_err(|_| format!("Invalid target email address: {value}"))?;
    let email = EmailAddress::from(mailbox.email.to_string().to_lowercase());
    match EmailChannelSelectorParser::new(&context.app_domain_name).classify(email.clone()) {
        EmailRecipientDestination::External(destination) => Ok(NormalizedOutreachTarget {
            destination: OutreachDestination::External(destination),
        }),
        EmailRecipientDestination::Channel(_) => Err(format!(
            "{email} is a platform channel address; name it in target_channels instead"
        )),
        EmailRecipientDestination::InvalidPlatformAddress => {
            Err(format!("Invalid platform channel address: {email}"))
        }
    }
}

/// Keep the first mention of a destination and drop a repeat, so naming somebody twice mails them
/// once.
fn push_unique(targets: &mut Vec<NormalizedOutreachTarget>, target: NormalizedOutreachTarget) {
    if !targets.iter().any(|seen| seen.key() == target.key()) {
        targets.push(target);
    }
}

fn config_usize(config: &Value, key: &str, default: usize) -> usize {
    config
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(default)
}

fn config_u32(config: &Value, key: &str, default: u32) -> u32 {
    config
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app_error::AppResult,
        entities::channel::{Channel, ChannelAccessMode},
        use_cases::channel::{ChannelPersistence, ChannelWrite},
    };

    struct MockChannelPersistence {
        channel: Option<Channel>,
    }

    #[async_trait]
    impl ChannelPersistence for MockChannelPersistence {
        async fn create(&self, _company_id: Uuid, _write: ChannelWrite) -> AppResult<Channel> {
            unimplemented!()
        }

        async fn get_by_id(&self, _id: Uuid) -> AppResult<Option<Channel>> {
            Ok(self.channel.clone())
        }

        async fn get_by_company_slug_and_channel_slug(
            &self,
            _company_slug: &crate::entities::value_objects::CompanySlug,
            channel_slug: &crate::entities::value_objects::ChannelSlug,
        ) -> AppResult<Option<Channel>> {
            Ok(self
                .channel
                .clone()
                .filter(|channel| &channel.slug == channel_slug))
        }

        async fn list_by_company_id(&self, _company_id: Uuid) -> AppResult<Vec<Channel>> {
            Ok(self.channel.clone().into_iter().collect())
        }

        async fn update(&self, _id: Uuid, _write: ChannelWrite) -> AppResult<Channel> {
            unimplemented!()
        }

        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }
    }

    fn channel(id: Uuid, company_id: Uuid, slug: &str) -> Channel {
        Channel {
            owner_agent_id: None,
            enabled: true,
            add_3rd_party: true,
            id,
            company_id,
            name: slug.to_string(),
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
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
        }
    }

    /// Target resolution needs a caller and a channel directory, and nothing else -- which is why
    /// it is a free function rather than a method on the tool.
    async fn resolve(
        company_id: Uuid,
        source_channel_id: Uuid,
        channel: Option<Channel>,
        input: &OutreachInput,
        scope: AllowedTargetScope,
    ) -> Result<Vec<NormalizedOutreachTarget>, String> {
        let context = OutreachToolContext {
            task_id: Uuid::new_v4(),
            correlation_id: CorrelationId::new(),
            worker_id: Uuid::new_v4(),
            company_id,
            channel_id: source_channel_id,
            thread_id: Uuid::new_v4(),
            channel_name: "Source".into(),
            channel_slug: "source".into(),
            company_slug: "acme".into(),
            trigger_message_id: MessageId::new("<trigger@acme.mailagents.test>"),
            thread_references: Vec::new(),
            hop_count: 0,
            trace_channels: Vec::new(),
            app_domain_name: "mailagents.test".into(),
        };
        resolve_targets(
            input,
            TargetPolicy {
                max_targets: 10,
                scope,
            },
            &context,
            &MockChannelPersistence { channel },
        )
        .await
    }

    fn request(channels: &[&str], emails: &[&str]) -> OutreachInput {
        OutreachInput {
            target_channels: channels.iter().map(|value| value.to_string()).collect(),
            target_emails: emails.iter().map(|value| value.to_string()).collect(),
            completion_threshold_percent: None,
            timeout_hours: None,
            subject: "Subject".into(),
            body: "Body".into(),
        }
    }

    #[tokio::test]
    async fn external_addresses_are_normalized_and_deduplicated() {
        let targets = resolve(
            Uuid::new_v4(),
            Uuid::new_v4(),
            None,
            &request(&[], &["User@Example.com", "user@example.com"]),
            AllowedTargetScope::ExternalOnly,
        )
        .await
        .unwrap();

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].delivery_address(), "user@example.com");
        assert!(matches!(
            targets[0].destination,
            OutreachDestination::External(ExternalDestination::Email(_))
        ));
    }

    /// The split, stated: a channel is named by its selector and reached at whatever address the
    /// channel itself decides. Nothing here parses an address to discover a channel.
    #[tokio::test]
    async fn a_channel_target_is_named_by_its_selector_and_carries_its_own_address() {
        let company_id = Uuid::new_v4();
        let target = channel(Uuid::new_v4(), company_id, "support");
        let targets = resolve(
            company_id,
            Uuid::new_v4(),
            Some(target.clone()),
            &request(&["support"], &[]),
            AllowedTargetScope::SameCompanyChannels,
        )
        .await
        .unwrap();

        assert_eq!(targets.len(), 1);
        assert_eq!(
            targets[0].destination,
            OutreachDestination::Channel {
                selector: ChannelSelector::CurrentCompany("support".into()),
                channel_id: target.id,
                delivery_address: EmailAddress::from("support@acme.mailagents.test"),
            }
        );
    }

    /// A colleague reached through the strangers list would be governed by the wrong policy, so
    /// the address is refused rather than quietly routed internally.
    #[tokio::test]
    async fn a_platform_address_in_the_email_list_is_refused_not_rerouted() {
        let company_id = Uuid::new_v4();
        let target = channel(Uuid::new_v4(), company_id, "support");
        let error = resolve(
            company_id,
            Uuid::new_v4(),
            Some(target),
            &request(&[], &["support@acme.mailagents.test"]),
            AllowedTargetScope::Any,
        )
        .await
        .unwrap_err();

        assert!(error.contains("target_channels"), "{error}");
    }

    #[tokio::test]
    async fn each_list_is_permitted_separately_by_tool_policy() {
        let company_id = Uuid::new_v4();
        let target = channel(Uuid::new_v4(), company_id, "support");

        assert!(
            resolve(
                company_id,
                Uuid::new_v4(),
                Some(target.clone()),
                &request(&["support"], &[]),
                AllowedTargetScope::ExternalOnly,
            )
            .await
            .is_err(),
            "no channels under external_only"
        );
        assert!(
            resolve(
                company_id,
                Uuid::new_v4(),
                Some(target),
                &request(&[], &["stranger@example.com"]),
                AllowedTargetScope::SameCompanyChannels,
            )
            .await
            .is_err(),
            "no strangers under same_company_channels"
        );
    }

    /// The idempotency key is hashed over this order, so a retry that lists the same targets
    /// differently must re-attach rather than mail everybody again.
    #[tokio::test]
    async fn resolved_target_order_does_not_depend_on_how_they_were_listed() {
        let first = resolve(
            Uuid::new_v4(),
            Uuid::new_v4(),
            None,
            &request(&[], &["b@example.com", "a@example.com"]),
            AllowedTargetScope::ExternalOnly,
        )
        .await
        .unwrap();
        let second = resolve(
            Uuid::new_v4(),
            Uuid::new_v4(),
            None,
            &request(&[], &["a@example.com", "b@example.com"]),
            AllowedTargetScope::ExternalOnly,
        )
        .await
        .unwrap();
        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn an_uncallable_channel_is_refused_with_its_reason() {
        let company_id = Uuid::new_v4();
        let source_channel_id = Uuid::new_v4();

        let cases: Vec<(Channel, &str)> = vec![
            (
                channel(source_channel_id, company_id, "support"),
                "cannot call itself",
            ),
            (
                channel(Uuid::new_v4(), Uuid::new_v4(), "support"),
                "Cross-company channel calls are not allowed",
            ),
            (
                Channel {
                    agent_ids: None,
                    ..channel(Uuid::new_v4(), company_id, "support")
                },
                "has no configured agent",
            ),
        ];

        for (target, expected) in cases {
            let error = resolve(
                company_id,
                source_channel_id,
                Some(target),
                &request(&["support"], &[]),
                AllowedTargetScope::SameCompanyChannels,
            )
            .await
            .unwrap_err();
            assert!(error.contains(expected), "{error}");
        }
    }

    /// A selector is `channel` or `company/channel`. An address is not a selector -- accepting one
    /// here is exactly how a transport's addressing became the platform's routing key.
    #[tokio::test]
    async fn an_address_or_a_malformed_selector_is_not_a_channel() {
        let company_id = Uuid::new_v4();
        let target = channel(Uuid::new_v4(), company_id, "support");
        for value in ["support@acme.mailagents.test", "a/b/c", "  "] {
            let error = resolve(
                company_id,
                Uuid::new_v4(),
                Some(target.clone()),
                &request(&[value], &[]),
                AllowedTargetScope::SameCompanyChannels,
            )
            .await
            .unwrap_err();
            assert!(!error.is_empty(), "expected {value:?} to be refused");
        }
    }

    #[tokio::test]
    async fn an_outreach_must_name_at_least_one_and_at_most_the_configured_number_of_targets() {
        assert!(
            resolve(
                Uuid::new_v4(),
                Uuid::new_v4(),
                None,
                &request(&[], &[]),
                AllowedTargetScope::Any,
            )
            .await
            .is_err()
        );

        let too_many: Vec<String> = (0..11).map(|n| format!("p{n}@example.com")).collect();
        assert!(
            resolve(
                Uuid::new_v4(),
                Uuid::new_v4(),
                None,
                &OutreachInput {
                    target_emails: too_many,
                    ..request(&[], &[])
                },
                AllowedTargetScope::Any,
            )
            .await
            .is_err()
        );
    }

    fn input(threshold: Option<f64>, timeout: Option<u32>) -> OutreachInput {
        OutreachInput {
            target_channels: Vec::new(),
            target_emails: vec!["b@acme.example".into()],
            completion_threshold_percent: threshold,
            timeout_hours: timeout,
            subject: "Capacity".into(),
            body: "Please confirm available capacity.".into(),
        }
    }

    fn limits() -> OutreachLimits {
        OutreachLimits {
            default_timeout_hours: 96,
            max_timeout_hours: 720,
        }
    }

    /// The short delegation form and the fully-spelled-out form are the same request, so a retry
    /// that switches between them must re-attach to the existing outreach rather than send twice.
    #[test]
    fn omitted_numbers_hash_like_their_explicit_defaults() {
        let task_id = Uuid::new_v4();
        let targets = vec!["b@acme.example".to_string()];

        let short = input(None, None);
        let long = input(Some(100.0), Some(96));

        let short_key = ValidatedOutreach::from_input(&short, limits())
            .expect("short form is valid")
            .idempotency_key(task_id, &targets);
        let long_key = ValidatedOutreach::from_input(&long, limits())
            .expect("long form is valid")
            .idempotency_key(task_id, &targets);

        assert_eq!(short_key, long_key);
    }

    #[test]
    fn omitted_timeout_takes_the_configured_default() {
        let short = input(None, None);
        let resolved = ValidatedOutreach::from_input(&short, limits()).expect("defaults are valid");
        assert_eq!(resolved.timeout_hours, 96);
        assert_eq!(resolved.threshold_percent, 100.0);
    }

    #[test]
    fn an_explicit_timeout_over_the_maximum_is_still_rejected() {
        let over = input(None, Some(1_000));
        let Err(message) = ValidatedOutreach::from_input(&over, limits()) else {
            panic!("a timeout above the maximum must be rejected");
        };
        assert!(message.contains("between 1 and 720"));
    }

    /// A default larger than the maximum is a misconfiguration, not a licence to exceed the cap.
    #[test]
    fn a_default_over_the_maximum_is_rejected_rather_than_silently_clamped() {
        let bad = OutreachLimits {
            default_timeout_hours: 900,
            max_timeout_hours: 720,
        };
        let short = input(None, None);
        assert!(ValidatedOutreach::from_input(&short, bad).is_err());
    }
}
