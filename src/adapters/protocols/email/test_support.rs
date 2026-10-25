//! Mail transports the email tests share.
//!
//! Hoisted here rather than re-declared per file: `egress_tests.rs` proves what the renderer
//! freezes, and the round-trip tests in `use_cases::thread` need the same double to read the mail
//! a real send actually posted.

use std::sync::Mutex;

use async_trait::async_trait;

use super::{MailMessage, MailSendOutcome, MailTransport};

/// Accepts every mail and keeps it, so a test can assert on the envelope that went out.
#[derive(Default)]
pub struct RecordingTransport {
    sent: Mutex<Vec<MailMessage>>,
}

impl RecordingTransport {
    /// Everything posted so far, in order.
    pub fn sent(&self) -> Vec<MailMessage> {
        self.sent.lock().unwrap().clone()
    }
}

#[async_trait]
impl MailTransport for RecordingTransport {
    async fn send(&self, message: MailMessage) -> MailSendOutcome {
        self.sent.lock().unwrap().push(message);
        MailSendOutcome::Accepted { provider_key: None }
    }
}

/// A transport whose relay always fails without a verdict, for the ambiguity rules.
pub struct RefusingTransport;

#[async_trait]
impl MailTransport for RefusingTransport {
    async fn send(&self, _message: MailMessage) -> MailSendOutcome {
        MailSendOutcome::Unknown {
            class: crate::entities::transport::FailureClass::Network,
            detail: "the relay closed the connection".to_string(),
        }
    }
}
