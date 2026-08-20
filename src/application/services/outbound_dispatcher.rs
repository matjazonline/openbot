use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message as LettreMessage, Tokio1Executor,
    message::{
        Mailbox,
        header::{
            ContentType, Header, HeaderName, HeaderValue, InReplyTo, MessageId as LettreMessageId,
            References,
        },
    },
    transport::smtp::authentication::Credentials,
};
use sha2::{Digest, Sha256};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::value_objects::{ChannelSlug, CompanySlug, EmailAddress, MessageId},
    infra::config::AppConfig,
};

#[derive(Clone, Debug)]
struct CustomHeader {
    name: HeaderName,
    value: String,
}

impl Header for CustomHeader {
    fn name() -> HeaderName {
        HeaderName::new_from_ascii_str("X-Custom")
    }

    fn parse(_: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Err("Parsing custom header not supported".into())
    }

    fn display(&self) -> HeaderValue {
        HeaderValue::new(self.name.clone(), self.value.clone())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OutboundEmail {
    pub channel_id: Uuid,
    pub channel_name: String,
    pub channel_slug: ChannelSlug,
    pub company_slug: CompanySlug,
    pub trigger_message_id: MessageId,
    pub thread_references: Vec<MessageId>,
    pub recipient_to: EmailAddress,
    pub recipients_cc: Vec<EmailAddress>,
    pub subject: String,
    pub body_text: String,
    pub hop_count: u32,
    pub trace_channels: Vec<Uuid>,
}

#[derive(Debug, Clone)]
pub struct SentEmailResult {
    pub outbound_message_id: MessageId,
    pub in_reply_to: MessageId,
    pub references: Vec<MessageId>,
    pub from_address: EmailAddress,
    pub from_name: Option<String>,
    pub recipients_to: Vec<EmailAddress>,
    pub recipients_cc: Vec<EmailAddress>,
    pub subject: String,
    pub body_text: String,
    pub source_channel_id: Option<Uuid>,
    pub hop_count: u32,
    pub trace_channels: Vec<Uuid>,
}

pub struct OutboundDispatcher;

impl OutboundDispatcher {
    pub fn prepare(config: &AppConfig, email: OutboundEmail) -> AppResult<SentEmailResult> {
        Self::prepare_with_message_id(config, email, None)
    }

    pub fn prepare_idempotent(
        config: &AppConfig,
        email: OutboundEmail,
        idempotency_key: &str,
    ) -> AppResult<SentEmailResult> {
        let digest = Sha256::digest(idempotency_key.as_bytes());
        let local_part = digest
            .iter()
            .take(16)
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let message_id = format!("<task-{local_part}@{}>", config.app_domain_name);
        Self::prepare_with_message_id(config, email, Some(message_id))
    }

    pub async fn send(config: &AppConfig, email: OutboundEmail) -> AppResult<SentEmailResult> {
        let prepared = Self::prepare(config, email)?;
        Self::send_prepared(config, prepared).await
    }

    pub async fn send_idempotent(
        config: &AppConfig,
        email: OutboundEmail,
        idempotency_key: &str,
    ) -> AppResult<SentEmailResult> {
        let prepared = Self::prepare_idempotent(config, email, idempotency_key)?;
        Self::send_prepared(config, prepared).await
    }

    fn prepare_with_message_id(
        config: &AppConfig,
        email: OutboundEmail,
        outbound_message_id: Option<String>,
    ) -> AppResult<SentEmailResult> {
        let outbound_message_id: MessageId = outbound_message_id
            .unwrap_or_else(|| format!("<{}@{}>", Uuid::new_v4(), config.app_domain_name))
            .into();

        let from_email: EmailAddress = format!(
            "{}@{}.{}",
            email.channel_slug, email.company_slug, config.app_domain_name
        )
        .into();
        let in_reply_to = email.trigger_message_id.clone();

        let mut references = email.thread_references.clone();
        if !references.contains(&in_reply_to) {
            references.push(in_reply_to.clone());
        }

        let subject = if email.subject.to_lowercase().starts_with("re:") {
            email.subject.clone()
        } else {
            format!("Re: {}", email.subject)
        };

        let mut cc_list = email.recipients_cc.clone();
        let domain_suffix = format!(".{}", config.app_domain_name);
        cc_list.retain(|c| {
            let lower = c.trim().to_lowercase();
            !lower.eq_ignore_ascii_case(&email.recipient_to)
                && !lower.eq_ignore_ascii_case(&from_email)
                && !lower.ends_with(&domain_suffix)
                && lower != config.app_domain_name
        });
        cc_list.dedup();

        let mut trace = email.trace_channels.clone();
        if !trace.contains(&email.channel_id) {
            trace.push(email.channel_id);
        }
        let next_hop = email.hop_count + 1;

        Ok(SentEmailResult {
            outbound_message_id,
            in_reply_to,
            references,
            from_address: from_email,
            from_name: Some(email.channel_name),
            recipients_to: vec![email.recipient_to],
            recipients_cc: cc_list,
            subject,
            body_text: email.body_text,
            source_channel_id: Some(email.channel_id),
            hop_count: next_hop,
            trace_channels: trace,
        })
    }

    async fn send_prepared(
        config: &AppConfig,
        prepared: SentEmailResult,
    ) -> AppResult<SentEmailResult> {
        info!(
            "Constructing outbound RFC 5322 email Message-ID: {}, In-Reply-To: {}, To: {}",
            prepared.outbound_message_id,
            prepared.in_reply_to,
            prepared
                .recipients_to
                .first()
                .map(|e| e.as_str())
                .unwrap_or_default()
        );

        let from_header = prepared
            .from_name
            .as_ref()
            .map(|name| format!("\"{name}\" <{}>", prepared.from_address))
            .unwrap_or_else(|| prepared.from_address.to_string());
        let from_mailbox = from_header
            .parse::<Mailbox>()
            .map_err(|e| AppError::Internal(format!("Invalid From address mailbox: {}", e)))?;
        let to_mailbox = prepared
            .recipients_to
            .first()
            .ok_or_else(|| AppError::Internal("Outbound message has no primary recipient".into()))?
            .parse::<Mailbox>()
            .map_err(|e| AppError::Internal(format!("Invalid To address mailbox: {}", e)))?;

        let mut builder = LettreMessage::builder()
            .from(from_mailbox)
            .to(to_mailbox)
            .subject(&prepared.subject)
            .header(ContentType::TEXT_PLAIN);
        for cc in &prepared.recipients_cc {
            if let Ok(cc_mb) = cc.parse::<Mailbox>() {
                builder = builder.cc(cc_mb);
            }
        }
        builder = builder.header(LettreMessageId::from(
            prepared.outbound_message_id.to_string(),
        ));
        builder = builder.header(InReplyTo::from(prepared.in_reply_to.to_string()));
        builder = builder.header(References::from(
            prepared
                .references
                .iter()
                .map(MessageId::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        ));
        builder = builder.header(CustomHeader {
            name: HeaderName::new_from_ascii_str("Auto-Submitted"),
            value: "auto-replied".to_string(),
        });
        builder = builder.header(CustomHeader {
            name: HeaderName::new_from_ascii_str("X-Auto-Response-Suppress"),
            value: "All".to_string(),
        });
        if let Some(channel_id) = prepared.source_channel_id {
            builder = builder.header(CustomHeader {
                name: HeaderName::new_from_ascii_str("X-MailAgents-Channel-ID"),
                value: channel_id.to_string(),
            });
            builder = builder.header(CustomHeader {
                name: HeaderName::new_from_ascii_str("X-MailAgents-Hop-Count"),
                value: prepared.hop_count.to_string(),
            });
            builder = builder.header(CustomHeader {
                name: HeaderName::new_from_ascii_str("X-MailAgents-Trace"),
                value: prepared
                    .trace_channels
                    .iter()
                    .map(Uuid::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            });
        }

        let lettre_msg = builder
            .body(prepared.body_text.clone())
            .map_err(|e| AppError::Internal(format!("Failed to build MIME message: {}", e)))?;

        // If SMTP credentials/host configured, dispatch via SMTP; otherwise log
        if !config.smtp_host.is_empty() && config.smtp_host != "localhost" {
            let mut transport_builder =
                AsyncSmtpTransport::<Tokio1Executor>::relay(&config.smtp_host)
                    .map_err(|e| AppError::Internal(format!("Invalid SMTP host: {}", e)))?
                    .port(config.smtp_port);

            if !config.smtp_username.is_empty() {
                transport_builder = transport_builder.credentials(Credentials::new(
                    config.smtp_username.clone(),
                    config.smtp_password.clone(),
                ));
            }

            let transport = transport_builder.build();
            match transport.send(lettre_msg).await {
                Ok(_) => {
                    info!("Successfully dispatched outbound SMTP email for thread");
                }
                Err(err) => {
                    warn!("Failed to dispatch email via SMTP: {}", err);
                    return Err(AppError::Internal(format!("SMTP dispatch failed: {}", err)));
                }
            }
        } else {
            info!(
                "SMTP host set to localhost/empty. Simulating SMTP dispatch for Message-ID: {}",
                prepared.outbound_message_id
            );
        }

        Ok(prepared)
    }

    pub async fn send_bounce(
        config: &AppConfig,
        recipient_to: &EmailAddress,
        subject: &str,
        bounce_body: &str,
    ) -> AppResult<SentEmailResult> {
        let outbound_uuid = Uuid::new_v4();
        let outbound_message_id: MessageId =
            format!("<bounce-{}@{}>", outbound_uuid, config.app_domain_name).into();

        let from_email: EmailAddress = format!("mailer-daemon@{}", config.app_domain_name).into();
        let from_header_value = format!("\"Mail Agents Server\" <{}>", from_email);

        let formatted_subject = if subject.to_lowercase().starts_with("[undeliverable]") {
            subject.to_string()
        } else {
            format!("[Undeliverable] {}", subject)
        };

        let from_mailbox = from_header_value
            .parse::<Mailbox>()
            .map_err(|e| AppError::Internal(format!("Invalid From address mailbox: {}", e)))?;

        let to_mailbox = recipient_to
            .parse::<Mailbox>()
            .map_err(|e| AppError::Internal(format!("Invalid To address mailbox: {}", e)))?;

        let builder = LettreMessage::builder()
            .from(from_mailbox)
            .to(to_mailbox)
            .subject(&formatted_subject)
            .header(ContentType::TEXT_PLAIN)
            .header(CustomHeader {
                name: HeaderName::new_from_ascii_str("Auto-Submitted"),
                value: "auto-replied".to_string(),
            });

        let email_msg = builder.body(bounce_body.to_string()).map_err(|e| {
            AppError::Internal(format!("Failed to build bounce email message: {}", e))
        })?;

        if !config.smtp_host.is_empty() {
            let mut mailer_builder =
                AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.smtp_host)
                    .port(config.smtp_port);

            if !config.smtp_username.is_empty() {
                mailer_builder = mailer_builder.credentials(Credentials::new(
                    config.smtp_username.clone(),
                    config.smtp_password.clone(),
                ));
            }

            let mailer = mailer_builder.build();
            if let Err(err) = mailer.send(email_msg).await {
                warn!("Failed to dispatch bounce email via SMTP: {err}");
            }
        }

        Ok(SentEmailResult {
            outbound_message_id,
            in_reply_to: MessageId::default(),
            references: vec![],
            from_address: from_email,
            from_name: Some("Mail Agents Server".to_string()),
            recipients_to: vec![recipient_to.clone()],
            recipients_cc: vec![],
            subject: formatted_subject,
            body_text: bounce_body.to_string(),
            source_channel_id: None,
            hop_count: 0,
            trace_channels: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> AppConfig {
        AppConfig {
            jwt_secret: "secret".into(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".into(),
            cors_allowed_origins: vec![],
            smtp_host: "localhost".into(),
            smtp_port: 1025,
            smtp_username: "".into(),
            smtp_password: "".into(),
            smtp_from_address: "noreply@mailagents.com".into(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "0.0.0.0".into(),
            incoming_smtp_port: 2525,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
            operator_emails: Vec::new(),
        }
    }

    fn test_email() -> OutboundEmail {
        OutboundEmail {
            channel_id: Uuid::new_v4(),
            channel_name: "Support Bot".into(),
            channel_slug: "support".into(),
            company_slug: "acme".into(),
            trigger_message_id: "<TRIGGER123@mail.com>".into(),
            thread_references: vec!["<REF1@mail.com>".into()],
            recipient_to: "user@example.com".into(),
            recipients_cc: vec!["manager@example.com".into()],
            subject: "Help needed".into(),
            body_text: "Hello! I am your AI assistant.".into(),
            hop_count: 0,
            trace_channels: Vec::new(),
        }
    }

    #[tokio::test]
    async fn test_outbound_dispatcher_constructs_valid_headers() {
        let config = test_config();
        let result = OutboundDispatcher::send(&config, test_email())
            .await
            .unwrap();

        assert!(result.outbound_message_id.contains("@mailagents.com"));
        assert_eq!(result.in_reply_to, "<TRIGGER123@mail.com>");
        assert_eq!(
            result.references,
            vec!["<REF1@mail.com>", "<TRIGGER123@mail.com>"]
        );
        assert_eq!(result.subject, "Re: Help needed");
        assert_eq!(result.from_address, "support@acme.mailagents.com");
        assert_eq!(result.recipients_to, vec!["user@example.com"]);
        assert_eq!(result.recipients_cc, vec!["manager@example.com"]);
    }

    /// The queue-then-deliver split rests on this: whoever queues an email derives its Message-ID
    /// from the idempotency key and persists it *before* the poller sends, so both sides must
    /// arrive at the same Message-ID from the same key. If this drifts, the message recorded in the
    /// thread and the mail actually delivered stop matching, and replies stop threading.
    #[test]
    fn prepared_message_id_is_a_pure_function_of_the_idempotency_key() {
        let config = test_config();
        let key = "task:1b7f0f9e-0000-4000-8000-000000000000:agent-reply";

        let queued = OutboundDispatcher::prepare_idempotent(&config, test_email(), key).unwrap();
        let delivered = OutboundDispatcher::prepare_idempotent(&config, test_email(), key).unwrap();

        assert_eq!(queued.outbound_message_id, delivered.outbound_message_id);

        let other =
            OutboundDispatcher::prepare_idempotent(&config, test_email(), "task:other:agent-reply")
                .unwrap();
        assert_ne!(
            queued.outbound_message_id, other.outbound_message_id,
            "a different send must not collide onto the same Message-ID"
        );
    }
}
