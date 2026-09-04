//! Provider-facing mail values and account-confirmation delivery.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use tracing::info;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        transport::{ExternalMessageKey, FailureClass},
        value_objects::{EmailAddress, MessageId},
    },
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

/// What a mail provider said about one submission.
///
/// A value rather than an `AppResult<()>` because the difference between the arms is the whole
/// reason a delivery queue exists. `Err` collapses "this address is malformed and always will be",
/// "come back in thirty seconds" and "the connection dropped after I sent the body" into one
/// thing, and only the last of those forbids sending again. SMTP genuinely cannot tell the second
/// from the third and answers [`Self::Unknown`]; an HTTP API can, and an adapter that knows should
/// not have to throw the knowledge away at this seam.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailSendOutcome {
    /// The provider took it.
    Accepted {
        /// The provider's own key, when it minted one instead of honouring the `Message-ID` this
        /// deployment chose. `None` means the rendered `Message-ID` is still the key, which is the
        /// SMTP case and the case a compliant API leaves alone.
        provider_key: Option<ExternalMessageKey>,
    },
    /// Refused, and the identical submission will be refused again.
    Rejected { class: FailureClass, detail: String },
    /// Refused with an explicit wait the provider asked for.
    RateLimited {
        retry_after: Option<Duration>,
        detail: String,
    },
    /// Not accepted, and worth another attempt. Only for a transport that can promise re-sending
    /// cannot duplicate -- an idempotency key the provider honours, or a request that provably
    /// never reached it.
    Retryable { class: FailureClass, detail: String },
    /// It may or may not have been accepted. Reconcile or dead-letter; never blind-retry.
    Unknown { class: FailureClass, detail: String },
}

impl MailSendOutcome {
    /// This outcome as an `AppResult`, for the callers that only need "did it go".
    ///
    /// Used by [`SmtpConfirmationSender`], which sends a confirmation code inside a registration
    /// and has no queue to record a nuance in.
    pub fn into_result(self) -> AppResult<()> {
        match self {
            Self::Accepted { .. } => Ok(()),
            Self::Rejected { detail, .. }
            | Self::RateLimited { detail, .. }
            | Self::Retryable { detail, .. }
            | Self::Unknown { detail, .. } => Err(AppError::Internal(detail)),
        }
    }
}

#[async_trait]
pub trait MailTransport: Send + Sync {
    async fn send(&self, message: MailMessage) -> MailSendOutcome;
}

/// A transport that logs and drops, used when no relay is configured and by focused tests.
pub struct DisabledMailTransport;

#[async_trait]
impl MailTransport for DisabledMailTransport {
    async fn send(&self, message: MailMessage) -> MailSendOutcome {
        info!(
            message_id = message.message_id.as_ref().map(MessageId::as_str),
            "Mail delivery is disabled; simulating dispatch"
        );
        MailSendOutcome::Accepted { provider_key: None }
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
            .into_result()
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
