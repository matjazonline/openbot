use std::{net::IpAddr, sync::Arc, time::Duration};

use async_trait::async_trait;
use lettre::{
    Address, AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{
        Mailbox,
        header::{ContentType, Header, HeaderName, HeaderValue, InReplyTo, MessageId, References},
    },
    transport::smtp::authentication::Credentials,
};

use crate::{
    adapters::protocols::email::{MailHeader, MailMessage, MailSendOutcome, MailTransport},
    app_error::{AppError, AppResult},
    entities::transport::FailureClass,
    infra::config::{AppConfig, is_loopback_host},
};

#[derive(Clone, Debug)]
struct DynamicHeader(MailHeader);

impl Header for DynamicHeader {
    fn name() -> HeaderName {
        HeaderName::new_from_ascii_str("X-Custom")
    }

    fn parse(_: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Err("Parsing dynamic header is unsupported".into())
    }

    fn display(&self) -> HeaderValue {
        HeaderValue::new(
            HeaderName::new_from_ascii(self.0.name.clone())
                .unwrap_or_else(|_| HeaderName::new_from_ascii_str("X-Invalid-Header")),
            self.0.value.clone(),
        )
    }
}

/// The one process-wide SMTP client. Lettre's transport owns and reuses its async connection pool.
pub struct LettreMailTransport {
    inner: Option<AsyncSmtpTransport<Tokio1Executor>>,
    #[cfg(test)]
    uses_plaintext: bool,
}

impl LettreMailTransport {
    pub async fn from_config(
        config: &AppConfig,
        allow_plaintext_local: bool,
    ) -> AppResult<Arc<Self>> {
        let has_username = !config.smtp_username.trim().is_empty();
        let has_password = !config.smtp_password.trim().is_empty();
        if has_username != has_password {
            return Err(AppError::Internal(
                "SMTP_USERNAME and SMTP_PASSWORD must either both be set or both be absent".into(),
            ));
        }
        if allow_plaintext_local && (has_username || has_password) {
            return Err(AppError::Internal(
                "SMTP plaintext cannot be configured together with credentials".into(),
            ));
        }

        let host = config.smtp_host.trim();
        if host.is_empty() || (is_loopback_host(host) && !allow_plaintext_local) {
            return Ok(Arc::new(Self {
                inner: None,
                #[cfg(test)]
                uses_plaintext: false,
            }));
        }

        let mut builder = if allow_plaintext_local {
            ensure_resolves_only_to_loopback(host, config.smtp_port).await?;
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host)
        } else {
            AsyncSmtpTransport::<Tokio1Executor>::relay(host)
                .map_err(|error| AppError::Internal(format!("Invalid SMTP relay: {error}")))?
        }
        .port(config.smtp_port)
        .timeout(Some(Duration::from_secs(30)));

        if has_username {
            builder = builder.credentials(Credentials::new(
                config.smtp_username.clone(),
                config.smtp_password.clone(),
            ));
        }

        Ok(Arc::new(Self {
            inner: Some(builder.build()),
            #[cfg(test)]
            uses_plaintext: allow_plaintext_local,
        }))
    }
}

async fn ensure_resolves_only_to_loopback(host: &str, port: u16) -> AppResult<()> {
    let unbracketed = host.trim_matches(['[', ']']);
    if let Ok(address) = unbracketed.parse::<IpAddr>() {
        return if address.is_loopback() {
            Ok(())
        } else {
            Err(AppError::Internal(
                "SMTP_ALLOW_PLAINTEXT_LOCAL requires a loopback SMTP host".into(),
            ))
        };
    }

    let addresses = tokio::net::lookup_host((unbracketed, port))
        .await
        .map_err(|error| AppError::Internal(format!("Could not resolve SMTP host: {error}")))?
        .collect::<Vec<_>>();
    if addresses.is_empty() || addresses.iter().any(|address| !address.ip().is_loopback()) {
        return Err(AppError::Internal(
            "SMTP_ALLOW_PLAINTEXT_LOCAL requires a host resolving only to loopback".into(),
        ));
    }
    Ok(())
}

fn mailbox(
    address: &crate::entities::value_objects::EmailAddress,
    name: Option<String>,
) -> AppResult<Mailbox> {
    let address = address
        .parse::<Address>()
        .map_err(|error| AppError::Internal(format!("Invalid outbound email address: {error}")))?;
    Ok(Mailbox::new(name, address))
}

#[async_trait]
impl MailTransport for LettreMailTransport {
    /// SMTP has exactly two outcomes to report.
    ///
    /// A submission this deployment never built is [`MailSendOutcome::Rejected`]: an address
    /// lettre will not parse is not going to parse on the fifth attempt. Everything else -- a
    /// refused connection, a 5xx, an accepted `DATA` whose final acknowledgement was lost -- is
    /// one `Err` from `lettre`, and the third of those may already be in the relay's queue. That
    /// is [`MailSendOutcome::Unknown`], not `Retryable`: SMTP submission carries no idempotency
    /// key, so re-sending is how one answer arrives twice.
    async fn send(&self, mail: MailMessage) -> MailSendOutcome {
        let Some(transport) = self.inner.as_ref() else {
            tracing::info!(
                message_id = mail.message_id.as_ref().map(|id| id.as_str()),
                "SMTP relay is disabled; simulating dispatch"
            );
            return MailSendOutcome::Accepted { provider_key: None };
        };

        let message = match build_message(mail) {
            Ok(message) => message,
            Err(error) => {
                return MailSendOutcome::Rejected {
                    class: FailureClass::InvalidPayload,
                    detail: error.to_string(),
                };
            }
        };
        match transport.send(message).await {
            Ok(_) => MailSendOutcome::Accepted { provider_key: None },
            Err(error) => MailSendOutcome::Unknown {
                class: FailureClass::Network,
                detail: format!("SMTP dispatch failed: {error}"),
            },
        }
    }
}

/// One [`MailMessage`] as RFC 5322 bytes.
///
/// Split out from the send so the fallible composition has one exit rather than being threaded
/// through the outcome enum at six `?` sites.
fn build_message(mail: MailMessage) -> AppResult<Message> {
    let mut builder = Message::builder()
        .from(mailbox(&mail.from, mail.from_name)?)
        .subject(mail.subject)
        .header(ContentType::TEXT_PLAIN);
    for recipient in &mail.recipients_to {
        builder = builder.to(mailbox(recipient, None)?);
    }
    for recipient in &mail.recipients_cc {
        builder = builder.cc(mailbox(recipient, None)?);
    }
    if let Some(message_id) = mail.message_id {
        builder = builder.header(MessageId::from(message_id.to_string()));
    }
    if let Some(in_reply_to) = mail.in_reply_to.filter(|id| !id.is_empty()) {
        builder = builder.header(InReplyTo::from(in_reply_to.to_string()));
    }
    if !mail.references.is_empty() {
        builder = builder.header(References::from(
            mail.references
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>()
                .join(" "),
        ));
    }
    for header in mail.headers {
        builder = builder.header(DynamicHeader(header));
    }
    builder
        .body(mail.body_text)
        .map_err(|error| AppError::Internal(format!("Failed to build outbound email: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn plaintext_accepts_ipv4_and_ipv6_loopback_without_credentials() {
        for host in ["127.0.0.1", "[::1]"] {
            let config = AppConfig {
                smtp_host: host.into(),
                ..AppConfig::for_test()
            };
            let transport = LettreMailTransport::from_config(&config, true)
                .await
                .unwrap();
            assert!(transport.inner.is_some());
        }
    }

    #[tokio::test]
    async fn plaintext_rejects_credentials() {
        let config = AppConfig {
            smtp_host: "127.0.0.1".into(),
            smtp_username: "user".into(),
            smtp_password: "secret".into(),
            ..AppConfig::for_test()
        };
        let error = LettreMailTransport::from_config(&config, true)
            .await
            .err()
            .expect("plaintext credentials must fail");
        assert!(error.to_string().contains("plaintext"));
    }

    #[tokio::test]
    async fn plaintext_rejects_remote_ip() {
        let config = AppConfig {
            smtp_host: "192.0.2.1".into(),
            ..AppConfig::for_test()
        };
        assert!(
            LettreMailTransport::from_config(&config, true)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn remote_credentials_build_only_a_tls_transport() {
        let config = AppConfig {
            smtp_host: "smtp.example.com".into(),
            smtp_username: "user".into(),
            smtp_password: "secret".into(),
            ..AppConfig::for_test()
        };
        let transport = LettreMailTransport::from_config(&config, false)
            .await
            .unwrap();
        assert!(transport.inner.is_some());
        assert!(!transport.uses_plaintext);
    }
}
