//! Mail transports the email tests share.
//!
//! Hoisted here rather than re-declared per file: `egress_tests.rs` proves what the renderer
//! freezes, and the round-trip tests in `use_cases::thread` need the same double to read the mail
//! a real send actually posted.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use super::{MailMessage, MailTransport};
use crate::app_error::AppResult;

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

    /// The only mail posted so far, or a panic naming how many there actually were.
    pub fn only_mail(&self) -> MailMessage {
        let sent = self.sent();
        assert_eq!(
            sent.len(),
            1,
            "expected exactly one mail, sent {}",
            sent.len()
        );
        sent.into_iter().next().expect("a mail was recorded")
    }
}

#[async_trait]
impl MailTransport for RecordingTransport {
    async fn send(&self, message: MailMessage) -> AppResult<()> {
        self.sent.lock().unwrap().push(message);
        Ok(())
    }
}

/// A transport whose relay always refuses, for the ambiguity rules.
pub struct RefusingTransport;

#[async_trait]
impl MailTransport for RefusingTransport {
    async fn send(&self, _message: MailMessage) -> AppResult<()> {
        Err(crate::app_error::AppError::Internal(
            "the relay closed the connection".into(),
        ))
    }
}

/// A [`RecordingTransport`] behind the `Arc` every `EmailSender` wants.
pub fn recording_transport() -> Arc<RecordingTransport> {
    Arc::new(RecordingTransport::default())
}
