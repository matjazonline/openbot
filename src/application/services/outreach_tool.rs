use crate::{
    adapters::persistence::task::TaskPersistence,
    entities::outreach::{CreateOutreachRequest, OutreachTargetRequest},
    services::outbound_dispatcher::OutboundEmail,
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
    pub channel_slug: String,
    pub company_slug: String,
    pub trigger_message_id: String,
    pub thread_references: Vec<String>,
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
    context: OutreachToolContext,
    suspended: Arc<AtomicBool>,
}

impl OutreachAndAwaitQuorumTool {
    pub fn new(
        persistence: Arc<dyn TaskPersistence>,
        context: OutreachToolContext,
        suspended: Arc<AtomicBool>,
    ) -> Self {
        Self {
            persistence,
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
        "Send an email to each external recipient and pause this task until the configured percentage of distinct recipients replies or the timeout requires a human decision. Use one recipient with a 100% threshold for single-party delegation."
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
            input.target_emails,
            max_targets,
            &self.context.app_domain_name,
        ) {
            Ok(targets) => targets,
            Err(error) => return ToolResult::error(error),
        };
        if !input.completion_threshold_percent.is_finite()
            || input.completion_threshold_percent <= 0.0
            || input.completion_threshold_percent > 100.0
        {
            return ToolResult::error(
                "completion_threshold_percent must be greater than 0 and at most 100",
            );
        }
        if input.timeout_hours == 0 || input.timeout_hours > max_timeout_hours {
            return ToolResult::error(format!(
                "timeout_hours must be between 1 and {max_timeout_hours}"
            ));
        }
        let subject = input.subject.trim();
        let body = input.body.trim();
        if subject.is_empty() || subject.chars().count() > 300 {
            return ToolResult::error("subject must contain between 1 and 300 characters");
        }
        if body.is_empty() || body.chars().count() > 20_000 {
            return ToolResult::error("body must contain between 1 and 20000 characters");
        }

        let canonical = serde_json::json!({
            "task_id": self.context.task_id,
            "target_emails": target_emails,
            "completion_threshold_percent": input.completion_threshold_percent,
            "timeout_hours": input.timeout_hours,
            "subject": subject,
            "body": body,
        });
        let outreach_key = format!("{:x}", Sha256::digest(canonical.to_string().as_bytes()));
        let outreach_id = Uuid::new_v4();
        let expires_at = Utc::now().naive_utc() + Duration::hours(input.timeout_hours as i64);
        let targets = target_emails
            .iter()
            .map(|email| {
                let outbox_id = Uuid::new_v4();
                let payload = serde_json::to_value(OutboundEmail {
                    channel_id: self.context.channel_id,
                    channel_name: self.context.channel_name.clone(),
                    channel_slug: self.context.channel_slug.clone(),
                    company_slug: self.context.company_slug.clone(),
                    trigger_message_id: self.context.trigger_message_id.clone(),
                    thread_references: self.context.thread_references.clone(),
                    recipient_to: email.clone(),
                    recipients_cc: Vec::new(),
                    subject: subject.to_string(),
                    body_text: body.to_string(),
                    hop_count: self.context.hop_count,
                    trace_channels: self.context.trace_channels.clone(),
                })
                .map_err(|error| format!("Failed to serialize outreach email: {error}"))?;
                Ok(OutreachTargetRequest {
                    email: email.clone(),
                    outbox_id,
                    outbox_payload: payload,
                })
            })
            .collect::<Result<Vec<_>, String>>();
        let targets = match targets {
            Ok(targets) => targets,
            Err(error) => return ToolResult::error(error),
        };

        let progress = match self
            .persistence
            .create_outreach_and_pause(CreateOutreachRequest {
                id: outreach_id,
                task_id: self.context.task_id,
                company_id: self.context.company_id,
                worker_id: self.context.worker_id,
                outreach_key,
                required_threshold_percent: input.completion_threshold_percent,
                expires_at,
                subject: subject.to_string(),
                body: body.to_string(),
                targets,
            })
            .await
        {
            Ok(progress) => progress,
            Err(error) => return ToolResult::error(format!("Failed to create outreach: {error}")),
        };
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
            expires_at: progress.expires_at.and_utc().to_rfc3339(),
        };
        match serde_json::to_string(&output) {
            Ok(output) => ToolResult::ok(output),
            Err(error) => ToolResult::error(format!("Failed to serialize tool output: {error}")),
        }
    }
}

fn normalize_targets(
    values: Vec<String>,
    max_targets: usize,
    app_domain_name: &str,
) -> Result<Vec<String>, String> {
    if values.is_empty() || values.len() > max_targets {
        return Err(format!(
            "target_emails must contain between 1 and {max_targets} addresses"
        ));
    }
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
        if domain.eq_ignore_ascii_case(app_domain_name)
            || domain.ends_with(&format!(".{app_domain_name}"))
        {
            return Err(format!(
                "Platform address cannot be an outreach target: {email}"
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

    #[test]
    fn normalizes_and_deduplicates_targets() {
        let targets = normalize_targets(
            vec!["User@Example.com".into(), "user@example.com".into()],
            10,
            "mailagents.test",
        )
        .unwrap();
        assert_eq!(targets, vec!["user@example.com"]);
    }

    #[test]
    fn rejects_platform_targets() {
        let result = normalize_targets(
            vec!["support@acme.mailagents.test".into()],
            10,
            "mailagents.test",
        );
        assert!(result.is_err());
    }

    #[test]
    fn canonical_target_order_is_stable() {
        let first = normalize_targets(
            vec!["b@example.com".into(), "a@example.com".into()],
            10,
            "mailagents.test",
        )
        .unwrap();
        let second = normalize_targets(
            vec!["a@example.com".into(), "b@example.com".into()],
            10,
            "mailagents.test",
        )
        .unwrap();
        assert_eq!(first, second);
    }
}
