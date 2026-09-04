//! SMTP-facing mail values and account-confirmation delivery.

use std::sync::Arc;

use async_trait::async_trait;
use tracing::info;

use crate::{
    app_error::AppResult,
    entities::value_objects::{EmailAddress, MessageId},
    infra::config::AppConfig,
    use_cases::user::{ConfirmationCodeSender, ConfirmationPurpose},
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

/// A transport that logs and drops, used when SMTP is disabled and by focused tests.
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

pub struct SmtpConfirmationSender {
    config: Arc<AppConfig>,
    transport: Arc<dyn MailTransport>,
}

impl SmtpConfirmationSender {
    pub fn new(config: Arc<AppConfig>, transport: Arc<dyn MailTransport>) -> Self {
        Self { config, transport }
    }
}

#[async_trait]
impl ConfirmationCodeSender for SmtpConfirmationSender {
    async fn send_code(
        &self,
        recipient: &EmailAddress,
        code: &str,
        purpose: ConfirmationPurpose,
    ) -> AppResult<()> {
        let (subject, lead, footer) = confirmation_copy(purpose);
        self.transport
            .send(MailMessage {
                from: self.config.smtp_from_address.clone().into(),
                from_name: None,
                recipients_to: vec![recipient.clone()],
                recipients_cc: Vec::new(),
                subject: subject.to_string(),
                body_text: format!(
                    "{lead} {code}.\n\nThis code expires in 15 minutes.\n\n{footer}"
                ),
                message_id: None,
                in_reply_to: None,
                references: Vec::new(),
                headers: Vec::new(),
            })
            .await
    }
}

fn confirmation_copy(purpose: ConfirmationPurpose) -> (&'static str, &'static str, &'static str) {
    match purpose {
        ConfirmationPurpose::Registration => (
            "Confirm your BusyBots email",
            "Your BusyBots confirmation code is",
            "If you did not sign up for BusyBots, you can ignore this email.",
        ),
        ConfirmationPurpose::EmailChange => (
            "Confirm your new BusyBots email",
            "To use this address for your BusyBots account, enter the code",
            "If you did not ask for this, change your BusyBots password -- somebody else may be signed in to your account.",
        ),
        ConfirmationPurpose::PasswordChange => (
            "Confirm your BusyBots password change",
            "To finish changing your BusyBots password, enter the code",
            "If you did not ask for this, change your BusyBots password -- somebody else may be signed in to your account.",
        ),
    }
}
