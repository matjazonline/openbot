use crate::{
    adapters::persistence::task::TaskPersistence,
    entities::{
        outreach::{CreateOutreachRequest, OutreachTargetRequest},
        value_objects::{ChannelSlug, CompanySlug, MessageId},
    },
    services::outbound_dispatcher::OutboundEmail,
    use_cases::channel::{ChannelPersistence, parse_recipient_address_pipeline},
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
    pub worker_id: Uuid,
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub channel_name: String,
    pub channel_slug: ChannelSlug,
    pub company_slug: CompanySlug,
    pub trigger_message_id: MessageId,
    pub thread_references: Vec<MessageId>,
    pub hop_count: u32,
    pub trace_channels: Vec<Uuid>,
    pub app_domain_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct OutreachInput {
    /// External email addresses to contact. Each recipient receives a separate email.
    target_emails: Vec<String>,
    /// Percentage of distinct recipients that must reply before the task resumes.
    completion_threshold_percent: f64,
    /// Maximum hours to wait before requesting a human timeout decision.
    timeout_hours: u32,
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
    context: OutreachToolContext,
    suspended: Arc<AtomicBool>,
}

impl OutreachAndAwaitQuorumTool {
    pub fn new(
        persistence: Arc<dyn TaskPersistence>,
        channel_persistence: Arc<dyn ChannelPersistence>,
        context: OutreachToolContext,
        suspended: Arc<AtomicBool>,
    ) -> Self {
        Self {
            persistence,
            channel_persistence,
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
        "Send a message to each permitted recipient and pause this task until the configured percentage of distinct recipients replies or the timeout requires a human decision. Same-company agent channels can be permitted by tool policy. Use one recipient with a 100% threshold for single-party delegation."
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

        let max_targets = config_usize(&ctx.custom_config, "max_targets", 50);
        let max_timeout_hours = config_u32(&ctx.custom_config, "max_timeout_hours", 720);
        let target_emails = match normalize_targets(
            input.target_emails.clone(),
            max_targets,
            &self.context.app_domain_name,
            self.context.company_id,
            self.context.channel_id,
            &ctx.custom_config,
            self.channel_persistence.as_ref(),
        )
        .await
        {
            Ok(targets) => targets,
            Err(error) => return ToolResult::error(error),
        };
        let request = match ValidatedOutreach::from_input(&input, max_timeout_hours) {
            Ok(request) => request,
            Err(error) => return ToolResult::error(error),
        };

        let targets = match self.build_target_requests(&target_emails, &request) {
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
                worker_id: self.context.worker_id,
                outreach_key: request.idempotency_key(self.context.task_id, &target_emails),
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

impl<'a> ValidatedOutreach<'a> {
    fn from_input(input: &'a OutreachInput, max_timeout_hours: u32) -> Result<Self, String> {
        if !input.completion_threshold_percent.is_finite()
            || input.completion_threshold_percent <= 0.0
            || input.completion_threshold_percent > 100.0
        {
            return Err(
                "completion_threshold_percent must be greater than 0 and at most 100".into(),
            );
        }
        if input.timeout_hours == 0 || input.timeout_hours > max_timeout_hours {
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
            threshold_percent: input.completion_threshold_percent,
            timeout_hours: input.timeout_hours,
            subject,
            body,
        })
    }

    /// Hash of everything that defines this outreach, so an agent retrying the same tool call
    /// re-attaches to the existing outreach instead of mailing everyone twice.
    fn idempotency_key(&self, task_id: Uuid, target_emails: &[String]) -> String {
        let canonical = serde_json::json!({
            "task_id": task_id,
            "target_emails": target_emails,
            "completion_threshold_percent": self.threshold_percent,
            "timeout_hours": self.timeout_hours,
            "subject": self.subject,
            "body": self.body,
        });
        format!("{:x}", Sha256::digest(canonical.to_string().as_bytes()))
    }
}

impl OutreachAndAwaitQuorumTool {
    /// One queued email per target, each carrying the originating channel's identity so replies
    /// land back on the same thread.
    fn build_target_requests(
        &self,
        target_emails: &[String],
        request: &ValidatedOutreach<'_>,
    ) -> Result<Vec<OutreachTargetRequest>, String> {
        target_emails
            .iter()
            .map(|email| {
                let payload = serde_json::to_value(OutboundEmail {
                    channel_id: self.context.channel_id,
                    channel_name: self.context.channel_name.clone(),
                    channel_slug: self.context.channel_slug.clone(),
                    company_slug: self.context.company_slug.clone(),
                    trigger_message_id: self.context.trigger_message_id.clone(),
                    thread_references: self.context.thread_references.clone(),
                    recipient_to: email.clone().into(),
                    recipients_cc: Vec::new(),
                    subject: request.subject.to_string(),
                    body_text: request.body.to_string(),
                    hop_count: self.context.hop_count,
                    trace_channels: self.context.trace_channels.clone(),
                })
                .map_err(|error| format!("Failed to serialize outreach email: {error}"))?;
                Ok(OutreachTargetRequest {
                    email: email.clone().into(),
                    outbox_id: Uuid::new_v4(),
                    outbox_payload: payload,
                })
            })
            .collect()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AllowedTargetScope {
    ExternalOnly,
    SameCompanyChannels,
    Any,
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

async fn normalize_targets(
    values: Vec<String>,
    max_targets: usize,
    app_domain_name: &str,
    company_id: Uuid,
    source_channel_id: Uuid,
    config: &Value,
    channel_persistence: &dyn ChannelPersistence,
) -> Result<Vec<String>, String> {
    if values.is_empty() || values.len() > max_targets {
        return Err(format!(
            "target_emails must contain between 1 and {max_targets} addresses"
        ));
    }
    let scope = configured_target_scope(config)?;
    let app_domain_lower = app_domain_name.trim().to_lowercase();
    let mut targets = Vec::with_capacity(values.len());
    for value in values {
        let mailbox: Mailbox = value
            .trim()
            .parse()
            .map_err(|_| format!("Invalid target email address: {value}"))?;
        let email = mailbox.email.to_string().to_lowercase();
        let domain = email
            .rsplit_once('@')
            .map(|(_, domain)| domain)
            .unwrap_or("");
        let is_platform_address = domain.eq_ignore_ascii_case(&app_domain_lower)
            || domain.ends_with(&format!(".{app_domain_lower}"));

        if is_platform_address {
            if scope == AllowedTargetScope::ExternalOnly {
                return Err(format!(
                    "Platform address cannot be an external outreach target: {email}"
                ));
            }
            let (company_slug, channel_slugs, is_context_only) =
                parse_recipient_address_pipeline(&email, app_domain_name)
                    .ok_or_else(|| format!("Invalid platform channel address: {email}"))?;
            if is_context_only || channel_slugs.len() != 1 {
                return Err(format!(
                    "Internal outreach requires one direct channel address without pipeline or context-only suffix: {email}"
                ));
            }
            let channel = channel_persistence
                .get_by_company_slug_and_channel_slug(&company_slug, &channel_slugs[0])
                .await
                .map_err(|error| format!("Failed to resolve platform channel {email}: {error}"))?
                .ok_or_else(|| format!("Platform channel does not exist: {email}"))?;
            if channel.company_id != company_id {
                return Err(format!(
                    "Cross-company channel calls are not allowed: {email}"
                ));
            }
            if channel.id == source_channel_id {
                return Err(format!("A channel cannot call itself: {email}"));
            }
            if !channel.enabled {
                return Err(format!("Target channel is disabled: {email}"));
            }
            if channel.agent_ids.as_ref().is_none_or(Vec::is_empty) {
                return Err(format!("Target channel has no configured agent: {email}"));
            }
        } else if scope == AllowedTargetScope::SameCompanyChannels {
            return Err(format!(
                "Only same-company platform channels are permitted by this tool policy: {email}"
            ));
        }
        if !targets.contains(&email) {
            targets.push(email);
        }
    }
    targets.sort();
    Ok(targets)
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
        entities::channel::Channel,
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
            enabled: true,
            id,
            company_id,
            name: slug.to_string(),
            slug: slug.into(),
            alias_slugs: Vec::new(),
            api_key: None,
            provider: None,
            model: None,
            participant_emails: None,
            agent_ids: Some(vec![Uuid::new_v4()]),
            channel_config: None,
            created_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn normalizes_and_deduplicates_targets() {
        let company_id = Uuid::new_v4();
        let targets = normalize_targets(
            vec!["User@Example.com".into(), "user@example.com".into()],
            10,
            "mailagents.test",
            company_id,
            Uuid::new_v4(),
            &serde_json::json!({}),
            &MockChannelPersistence { channel: None },
        )
        .await
        .unwrap();
        assert_eq!(targets, vec!["user@example.com"]);
    }

    #[tokio::test]
    async fn rejects_platform_targets_for_external_scope() {
        let company_id = Uuid::new_v4();
        let result = normalize_targets(
            vec!["support@acme.mailagents.test".into()],
            10,
            "mailagents.test",
            company_id,
            Uuid::new_v4(),
            &serde_json::json!({}),
            &MockChannelPersistence {
                channel: Some(channel(Uuid::new_v4(), company_id, "support")),
            },
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn accepts_same_company_agent_channel_for_internal_scope() {
        let company_id = Uuid::new_v4();
        let target = channel(Uuid::new_v4(), company_id, "support");
        let result = normalize_targets(
            vec!["support@acme.mailagents.test".into()],
            10,
            "mailagents.test",
            company_id,
            Uuid::new_v4(),
            &serde_json::json!({"allowed_target_scope": "same_company_channels"}),
            &MockChannelPersistence {
                channel: Some(target),
            },
        )
        .await
        .unwrap();
        assert_eq!(result, vec!["support@acme.mailagents.test"]);
    }

    #[tokio::test]
    async fn canonical_target_order_is_stable() {
        let company_id = Uuid::new_v4();
        let persistence = MockChannelPersistence { channel: None };
        let first = normalize_targets(
            vec!["b@example.com".into(), "a@example.com".into()],
            10,
            "mailagents.test",
            company_id,
            Uuid::new_v4(),
            &serde_json::json!({}),
            &persistence,
        )
        .await
        .unwrap();
        let second = normalize_targets(
            vec!["a@example.com".into(), "b@example.com".into()],
            10,
            "mailagents.test",
            company_id,
            Uuid::new_v4(),
            &serde_json::json!({}),
            &persistence,
        )
        .await
        .unwrap();
        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn rejects_self_calling_channel() {
        let company_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let self_target = channel(channel_id, company_id, "support");
        let result = normalize_targets(
            vec!["support@acme.mailagents.test".into()],
            10,
            "mailagents.test",
            company_id,
            channel_id,
            &serde_json::json!({"allowed_target_scope": "same_company_channels"}),
            &MockChannelPersistence {
                channel: Some(self_target),
            },
        )
        .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot call itself"));
    }

    #[tokio::test]
    async fn rejects_cross_company_channel_call() {
        let source_company_id = Uuid::new_v4();
        let other_company_id = Uuid::new_v4();
        let cross_target = channel(Uuid::new_v4(), other_company_id, "support");
        let result = normalize_targets(
            vec!["support@acme.mailagents.test".into()],
            10,
            "mailagents.test",
            source_company_id,
            Uuid::new_v4(),
            &serde_json::json!({"allowed_target_scope": "same_company_channels"}),
            &MockChannelPersistence {
                channel: Some(cross_target),
            },
        )
        .await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .contains("Cross-company channel calls are not allowed")
        );
    }

    #[tokio::test]
    async fn rejects_channel_without_configured_agent() {
        let company_id = Uuid::new_v4();
        let mut target = channel(Uuid::new_v4(), company_id, "support");
        target.agent_ids = None;
        let result = normalize_targets(
            vec!["support@acme.mailagents.test".into()],
            10,
            "mailagents.test",
            company_id,
            Uuid::new_v4(),
            &serde_json::json!({"allowed_target_scope": "same_company_channels"}),
            &MockChannelPersistence {
                channel: Some(target),
            },
        )
        .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("has no configured agent"));
    }

    #[tokio::test]
    async fn rejects_pipeline_or_context_only_suffix_for_internal_outreach() {
        let company_id = Uuid::new_v4();
        let target = channel(Uuid::new_v4(), company_id, "support");
        let result = normalize_targets(
            vec!["support+context@acme.mailagents.test".into()],
            10,
            "mailagents.test",
            company_id,
            Uuid::new_v4(),
            &serde_json::json!({"allowed_target_scope": "same_company_channels"}),
            &MockChannelPersistence {
                channel: Some(target),
            },
        )
        .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("one direct channel address"));
    }
}
