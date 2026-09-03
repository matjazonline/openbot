use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tracing::{info, warn};
use uuid::Uuid;

use std::sync::Arc;

use crate::{
    app_error::AppResult,
    entities::{
        correlation::{CORRELATION_HEADER, CorrelationId},
        value_objects::{ChannelSlug, CompanySlug, EmailAddress, MessageId},
    },
    infra::config::AppConfig,
    use_cases::user::ConfirmationCodeSender,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MailHeader {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug)]
pub struct MailMessage {
    pub from: EmailAddress,
    pub from_name: Option<String>,
    pub recipients_to: Vec<EmailAddress>,
    pub recipients_cc: Vec<EmailAddress>,
    pub subject: String,
    pub body_text: String,
    pub message_id: Option<MessageId>,
    pub in_reply_to: Option<MessageId>,
    pub references: Vec<MessageId>,
    pub headers: Vec<MailHeader>,
}

#[async_trait]
pub trait MailTransport: Send + Sync {
    async fn send(&self, message: MailMessage) -> AppResult<()>;
}

/// A transport that logs and drops. What a deployment with SMTP switched off sends through, and
/// what a test that must never reach a relay uses.
pub struct DisabledMailTransport;

#[async_trait]
impl MailTransport for DisabledMailTransport {
    async fn send(&self, message: MailMessage) -> AppResult<()> {
        info!(
            message_id = message.message_id.as_ref().map(MessageId::as_str),
            "SMTP delivery is disabled; simulating dispatch"
        );
        Ok(())
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
    /// The chain that produced this email, stamped onto the wire so an inter-channel recipient
    /// stays on it -- see [`CorrelationId`].
    pub correlation_id: CorrelationId,
}

/// One server-generated reply from a reserved `_` address.
///
/// A struct rather than positional arguments because `from_name`, `subject` and `body` are three
/// adjacent strings, and a transposed pair would compile and mail the wrong thing.
struct SystemMail<'a> {
    message_id: MessageId,
    from: EmailAddress,
    from_name: &'a str,
    to: &'a EmailAddress,
    subject: String,
    body: &'a str,
    in_reply_to: Option<MessageId>,
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
    pub correlation_id: CorrelationId,
}

/// Wrap an agent's answer in the shared plain-text email template.
///
/// Keep this at the delivery boundary so the thread and API retain the agent's original answer.
pub fn agent_response_email_body(response: &str) -> String {
    format!("{response}\n\nDone by busybots.net")
}

/// Mails confirmation codes over this deployment's SMTP relay.
///
/// It exists so [`crate::use_cases::user::UserUseCases`] depends on the *act* of sending a code
/// rather than on SMTP: a test can then hand it a sender that keeps the code, which is the only
/// way to exercise a confirmation flow end to end without a mailbox.
pub struct SmtpConfirmationSender {
    dispatcher: Arc<OutboundDispatcher>,
}

impl SmtpConfirmationSender {
    pub fn new(dispatcher: Arc<OutboundDispatcher>) -> Self {
        Self { dispatcher }
    }
}

#[async_trait::async_trait]
impl ConfirmationCodeSender for SmtpConfirmationSender {
    async fn send_code(
        &self,
        recipient: &EmailAddress,
        code: &str,
        purpose: ConfirmationPurpose,
    ) -> AppResult<()> {
        self.dispatcher
            .send_confirmation_code(recipient, code, purpose)
            .await
    }
}

/// What a mailed confirmation code proves, and so what its mail should say.
///
/// It changes the wording and nothing else. The distinction that matters is *where* each code is
/// sent, and that is the caller's -- see `EmailConfirmation::request`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationPurpose {
    Registration,
    EmailChange,
    PasswordChange,
}

impl ConfirmationPurpose {
    fn subject(self) -> &'static str {
        match self {
            ConfirmationPurpose::Registration => "Confirm your BusyBots email",
            ConfirmationPurpose::EmailChange => "Confirm your new BusyBots email",
            ConfirmationPurpose::PasswordChange => "Confirm your BusyBots password change",
        }
    }

    fn lead(self) -> &'static str {
        match self {
            ConfirmationPurpose::Registration => "Your BusyBots confirmation code is",
            ConfirmationPurpose::EmailChange => {
                "To use this address for your BusyBots account, enter the code"
            }
            ConfirmationPurpose::PasswordChange => {
                "To finish changing your BusyBots password, enter the code"
            }
        }
    }

    /// What to do if the code was not asked for. A registration code reaching a stranger is a
    /// typo; a code about an account that already exists may be somebody else at its controls, so
    /// those two say to change the password rather than to ignore it.
    fn footer(self) -> &'static str {
        match self {
            ConfirmationPurpose::Registration => {
                "If you did not sign up for BusyBots, you can ignore this email."
            }
            ConfirmationPurpose::EmailChange | ConfirmationPurpose::PasswordChange => {
                "If you did not ask for this, change your BusyBots password -- somebody else may be signed in to your account."
            }
        }
    }
}

pub struct OutboundDispatcher {
    config: Arc<AppConfig>,
    transport: Arc<dyn MailTransport>,
}

impl OutboundDispatcher {
    pub fn new(config: Arc<AppConfig>, transport: Arc<dyn MailTransport>) -> Self {
        Self { config, transport }
    }

    pub fn disabled(config: Arc<AppConfig>) -> Self {
        Self::new(config, Arc::new(DisabledMailTransport))
    }

    pub fn app_domain_name(&self) -> &str {
        &self.config.app_domain_name
    }

    /// Mails one confirmation code.
    ///
    /// Every code this app sends goes out through here, so the subject and the sentence above the
    /// code are the only thing [`ConfirmationPurpose`] changes -- what a code *proves* differs,
    /// how it is delivered does not.
    async fn send_confirmation_code(
        &self,
        recipient: &EmailAddress,
        code: &str,
        purpose: ConfirmationPurpose,
    ) -> AppResult<()> {
        self.transport
            .send(MailMessage {
                from: self.config.smtp_from_address.clone().into(),
                from_name: None,
                recipients_to: vec![recipient.clone()],
                recipients_cc: Vec::new(),
                subject: purpose.subject().to_string(),
                body_text: format!(
                    "{lead} {code}.\n\nThis code expires in 15 minutes.\n\n{footer}",
                    lead = purpose.lead(),
                    footer = purpose.footer(),
                ),
                message_id: None,
                in_reply_to: None,
                references: Vec::new(),
                headers: Vec::new(),
            })
            .await?;
        Ok(())
    }

    pub fn prepare(&self, email: OutboundEmail) -> AppResult<SentEmailResult> {
        Self::prepare_with_message_id(&self.config, email, None)
    }

    pub fn prepare_idempotent(
        &self,
        email: OutboundEmail,
        idempotency_key: &str,
    ) -> AppResult<SentEmailResult> {
        let digest = Sha256::digest(idempotency_key.as_bytes());
        let local_part = digest
            .iter()
            .take(16)
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let message_id = format!("<task-{local_part}@{}>", self.config.app_domain_name);
        Self::prepare_with_message_id(&self.config, email, Some(message_id))
    }

    pub async fn send(&self, email: OutboundEmail) -> AppResult<SentEmailResult> {
        let prepared = self.prepare(email)?;
        self.send_prepared(prepared).await
    }

    pub async fn send_idempotent(
        &self,
        email: OutboundEmail,
        idempotency_key: &str,
    ) -> AppResult<SentEmailResult> {
        let prepared = self.prepare_idempotent(email, idempotency_key)?;
        self.send_prepared(prepared).await
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
            // Carried, never re-minted: the reply belongs to the chain of the message it answers.
            correlation_id: email.correlation_id,
        })
    }

    async fn send_prepared(&self, prepared: SentEmailResult) -> AppResult<SentEmailResult> {
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

        let mut headers = vec![
            MailHeader {
                name: "Auto-Submitted".into(),
                value: "auto-replied".into(),
            },
            MailHeader {
                name: "X-Auto-Response-Suppress".into(),
                value: "All".into(),
            },
        ];
        if let Some(channel_id) = prepared.source_channel_id {
            headers.push(MailHeader {
                name: "X-MailAgents-Channel-ID".into(),
                value: channel_id.to_string(),
            });
            headers.push(MailHeader {
                name: "X-MailAgents-Hop-Count".into(),
                value: prepared.hop_count.to_string(),
            });
            headers.push(MailHeader {
                name: CORRELATION_HEADER.into(),
                value: prepared.correlation_id.to_string(),
            });
            headers.push(MailHeader {
                name: "X-MailAgents-Trace".into(),
                value: prepared
                    .trace_channels
                    .iter()
                    .map(Uuid::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            });
        }
        self.transport
            .send(MailMessage {
                from: prepared.from_address.clone(),
                from_name: prepared.from_name.clone(),
                recipients_to: prepared.recipients_to.clone(),
                recipients_cc: prepared.recipients_cc.clone(),
                subject: prepared.subject.clone(),
                body_text: prepared.body_text.clone(),
                message_id: Some(prepared.outbound_message_id.clone()),
                in_reply_to: Some(prepared.in_reply_to.clone()),
                references: prepared.references.clone(),
                headers,
            })
            .await
            .map_err(|error| {
                warn!(%error, "Failed to dispatch email via mail transport");
                error
            })?;

        Ok(prepared)
    }

    /// The answer a reserved `_`-prefixed address sends back.
    ///
    /// Unlike a channel reply this carries no `X-MailAgents-*` headers and never joins the
    /// inter-channel trace: it comes from the server, not from an agent.
    pub async fn send_system_reply(
        &self,
        company_slug: &CompanySlug,
        system_local_part: &str,
        recipient_to: &EmailAddress,
        subject: &str,
        in_reply_to: Option<MessageId>,
        body: &str,
    ) -> AppResult<SentEmailResult> {
        let trimmed = subject.trim();
        let formatted_subject = if trimmed.is_empty() {
            "Mail Agents Help".to_string()
        } else if trimmed.to_lowercase().starts_with("re:") {
            trimmed.to_string()
        } else {
            format!("Re: {trimmed}")
        };

        let from: EmailAddress = format!(
            "{system_local_part}@{company_slug}.{}",
            self.config.app_domain_name
        )
        .into();

        self.send_system_mail(SystemMail {
            message_id: format!(
                "<system-{}@{}>",
                Uuid::new_v4(),
                self.config.app_domain_name
            )
            .into(),
            from,
            from_name: "Mail Agents",
            to: recipient_to,
            subject: formatted_subject,
            body,
            in_reply_to,
        })
        .await
    }

    /// Build and post one server-generated message.
    ///
    /// The headers that make an automatic system reply safe -- `Auto-Submitted: auto-replied`,
    /// which `check_inbound_guards` refuses on the way back in -- are set here rather than at each
    /// caller.
    async fn send_system_mail(&self, mail: SystemMail<'_>) -> AppResult<SentEmailResult> {
        self.transport
            .send(MailMessage {
                from: mail.from.clone(),
                from_name: Some(mail.from_name.to_string()),
                recipients_to: vec![mail.to.clone()],
                recipients_cc: Vec::new(),
                subject: mail.subject.clone(),
                body_text: mail.body.to_string(),
                message_id: Some(mail.message_id.clone()),
                in_reply_to: mail.in_reply_to.clone(),
                references: Vec::new(),
                headers: vec![MailHeader {
                    name: "Auto-Submitted".into(),
                    value: "auto-replied".into(),
                }],
            })
            .await?;

        Ok(SentEmailResult {
            outbound_message_id: mail.message_id,
            in_reply_to: mail.in_reply_to.unwrap_or_default(),
            references: vec![],
            from_address: mail.from,
            from_name: Some(mail.from_name.to_string()),
            recipients_to: vec![mail.to.clone()],
            recipients_cc: vec![],
            subject: mail.subject,
            body_text: mail.body.to_string(),
            source_channel_id: None,
            hop_count: 0,
            trace_channels: Vec::new(),
            // A `_` address reply answers a message we refused to process, so there is no task
            // chain to join: this notice is its own event.
            correlation_id: CorrelationId::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct RecordingTransport(AtomicUsize);

    #[async_trait]
    impl MailTransport for RecordingTransport {
        async fn send(&self, _message: MailMessage) -> AppResult<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn test_config() -> AppConfig {
        AppConfig {
            jwt_secret: "secret".into(),
            sendgrid_inbound: None,
            hydradb: None,
            hindsight: None,
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
            secure_cookies: false,
            gcs: None,
            operator_emails: Vec::new(),
        }
    }

    fn test_email() -> OutboundEmail {
        OutboundEmail {
            correlation_id: CorrelationId::new(),
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

    #[test]
    fn agent_response_email_template_adds_busy_bots_footer() {
        assert_eq!(
            agent_response_email_body("The report is ready."),
            "The report is ready.\n\nDone by busybots.net"
        );
    }

    #[tokio::test]
    async fn test_outbound_dispatcher_constructs_valid_headers() {
        let dispatcher = OutboundDispatcher::disabled(Arc::new(test_config()));
        let result = dispatcher.send(test_email()).await.unwrap();

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

    #[tokio::test]
    async fn one_transport_instance_is_reused_across_sends() {
        let transport = Arc::new(RecordingTransport::default());
        let dispatcher = OutboundDispatcher::new(Arc::new(test_config()), transport.clone());

        dispatcher.send(test_email()).await.unwrap();
        dispatcher.send(test_email()).await.unwrap();

        assert_eq!(transport.0.load(Ordering::SeqCst), 2);
    }

    /// The queue-then-deliver split rests on this: whoever queues an email derives its Message-ID
    /// from the idempotency key and persists it *before* the poller sends, so both sides must
    /// arrive at the same Message-ID from the same key. If this drifts, the message recorded in the
    /// thread and the mail actually delivered stop matching, and replies stop threading.
    #[test]
    fn prepared_message_id_is_a_pure_function_of_the_idempotency_key() {
        let config = test_config();
        let dispatcher = OutboundDispatcher::disabled(Arc::new(config));
        let key = "task:1b7f0f9e-0000-4000-8000-000000000000:agent-reply";

        let queued = dispatcher.prepare_idempotent(test_email(), key).unwrap();
        let delivered = dispatcher.prepare_idempotent(test_email(), key).unwrap();

        assert_eq!(queued.outbound_message_id, delivered.outbound_message_id);

        let other = dispatcher
            .prepare_idempotent(test_email(), "task:other:agent-reply")
            .unwrap();
        assert_ne!(
            queued.outbound_message_id, other.outbound_message_id,
            "a different send must not collide onto the same Message-ID"
        );
    }
}
