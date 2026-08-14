use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message as LettreMessage, Tokio1Executor,
    message::{
        Mailbox,
        header::{ContentType, Header, HeaderName, HeaderValue, InReplyTo, MessageId, References},
    },
    transport::smtp::authentication::Credentials,
};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
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

#[derive(Debug, Clone)]
pub struct OutboundEmail {
    pub channel_id: Uuid,
    pub channel_name: String,
    pub channel_slug: String,
    pub company_slug: String,
    pub trigger_message_id: String,
    pub thread_references: Vec<String>,
    pub recipient_to: String,
    pub recipients_cc: Vec<String>,
    pub subject: String,
    pub body_text: String,
    pub hop_count: u32,
    pub trace_channels: Vec<Uuid>,
}

#[derive(Debug, Clone)]
pub struct SentEmailResult {
    pub outbound_message_id: String,
    pub in_reply_to: String,
    pub references: Vec<String>,
    pub from_address: String,
    pub recipients_to: Vec<String>,
    pub recipients_cc: Vec<String>,
    pub subject: String,
    pub body_text: String,
}

pub struct OutboundDispatcher;

impl OutboundDispatcher {
    pub async fn send(config: &AppConfig, email: OutboundEmail) -> AppResult<SentEmailResult> {
        let outbound_uuid = Uuid::new_v4();
        let outbound_message_id = format!("<{}@{}>", outbound_uuid, config.app_domain_name);

        let from_email = format!(
            "{}@{}.{}",
            email.channel_slug, email.company_slug, config.app_domain_name
        );
        let from_header_value = format!("\"{}\" <{}>", email.channel_name, from_email);

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

        info!(
            "Constructing outbound RFC 5322 email Message-ID: {}, In-Reply-To: {}, To: {}",
            outbound_message_id, in_reply_to, email.recipient_to
        );

        let from_mailbox = from_header_value
            .parse::<Mailbox>()
            .map_err(|e| AppError::Internal(format!("Invalid From address mailbox: {}", e)))?;

        let to_mailbox = email
            .recipient_to
            .parse::<Mailbox>()
            .map_err(|e| AppError::Internal(format!("Invalid To address mailbox: {}", e)))?;

        let mut builder = LettreMessage::builder()
            .from(from_mailbox)
            .to(to_mailbox)
            .subject(&subject)
            .header(ContentType::TEXT_PLAIN);

        for cc in &cc_list {
            if let Ok(cc_mb) = cc.parse::<Mailbox>() {
                builder = builder.cc(cc_mb);
            }
        }

        // Standard RFC 5322 headers
        builder = builder.header(MessageId::from(outbound_message_id.clone()));
        builder = builder.header(InReplyTo::from(in_reply_to.clone()));
        builder = builder.header(References::from(references.join(" ")));

        // RFC 3834 Auto-Reply and Exchange Loop Prevention headers
        builder = builder.header(CustomHeader {
            name: HeaderName::new_from_ascii_str("Auto-Submitted"),
            value: "auto-replied".to_string(),
        });
        builder = builder.header(CustomHeader {
            name: HeaderName::new_from_ascii_str("X-Auto-Response-Suppress"),
            value: "All".to_string(),
        });

        // Platform Inter-Channel Tracking Headers
        let mut trace = email.trace_channels.clone();
        if !trace.contains(&email.channel_id) {
            trace.push(email.channel_id);
        }
        let trace_str = trace.iter().map(|u| u.to_string()).collect::<Vec<_>>().join(",");
        let next_hop = email.hop_count + 1;

        builder = builder.header(CustomHeader {
            name: HeaderName::new_from_ascii_str("X-MailAgents-Channel-ID"),
            value: email.channel_id.to_string(),
        });
        builder = builder.header(CustomHeader {
            name: HeaderName::new_from_ascii_str("X-MailAgents-Hop-Count"),
            value: next_hop.to_string(),
        });
        builder = builder.header(CustomHeader {
            name: HeaderName::new_from_ascii_str("X-MailAgents-Trace"),
            value: trace_str,
        });

        let lettre_msg = builder
            .body(email.body_text.clone())
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
                outbound_message_id
            );
        }

        Ok(SentEmailResult {
            outbound_message_id,
            in_reply_to,
            references,
            from_address: from_email,
            recipients_to: vec![email.recipient_to],
            recipients_cc: cc_list,
            subject,
            body_text: email.body_text,
        })
    }

    pub async fn send_bounce(
        config: &AppConfig,
        recipient_to: &str,
        subject: &str,
        bounce_body: &str,
    ) -> AppResult<SentEmailResult> {
        let outbound_uuid = Uuid::new_v4();
        let outbound_message_id = format!("<bounce-{}@{}>", outbound_uuid, config.app_domain_name);

        let from_email = format!("mailer-daemon@{}", config.app_domain_name);
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

        let email_msg = builder
            .body(bounce_body.to_string())
            .map_err(|e| AppError::Internal(format!("Failed to build bounce email message: {}", e)))?;

        if !config.smtp_host.is_empty() {
            let mut mailer_builder = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.smtp_host)
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
            in_reply_to: String::new(),
            references: vec![],
            from_address: from_email,
            recipients_to: vec![recipient_to.to_string()],
            recipients_cc: vec![],
            subject: formatted_subject,
            body_text: bounce_body.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_outbound_dispatcher_constructs_valid_headers() {
        let config = AppConfig {
            jwt_secret: "secret".into(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".into(),
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
        };

        let outbound_email = OutboundEmail {
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
        };

        let result = OutboundDispatcher::send(&config, outbound_email).await.unwrap();

        assert!(result.outbound_message_id.contains("@mailagents.com"));
        assert_eq!(result.in_reply_to, "<TRIGGER123@mail.com>");
        assert_eq!(result.references, vec!["<REF1@mail.com>", "<TRIGGER123@mail.com>"]);
        assert_eq!(result.subject, "Re: Help needed");
        assert_eq!(result.from_address, "support@acme.mailagents.com");
        assert_eq!(result.recipients_to, vec!["user@example.com"]);
        assert_eq!(result.recipients_cc, vec!["manager@example.com"]);
    }
}
