//! Fixed vocabulary only: no account/provider labels, remote errors or payloads.
use crate::application::integrations::renewal::{RenewalFailure, ResolutionError, RetryReason};

pub(super) enum Event {
    CacheHit,
    RefreshAttempt,
    RefreshResponse(Result<(), RenewalFailure>),
    RefreshCommitted,
    CooldownCommitted,
    LockTimeout,
    GrantRevoked,
    ReconnectRequired,
    ResolutionFailed(ResolutionError),
}
impl Event {
    fn labels(self) -> (&'static str, &'static str) {
        match self {
            Self::CacheHit => ("cache_hit", "usable"),
            Self::RefreshAttempt => ("refresh_attempt", "started"),
            Self::RefreshResponse(Ok(())) => ("refresh_outcome", "response_received"),
            Self::RefreshResponse(Err(error)) => (
                "refresh_outcome",
                match error {
                    RenewalFailure::Reconnect => "reconnect",
                    RenewalFailure::Temporary => "temporary",
                    RenewalFailure::Configuration => "configuration",
                    RenewalFailure::Cancelled => "cancelled",
                    RenewalFailure::Malformed => "invalid_response",
                },
            ),
            Self::RefreshCommitted => ("refresh_outcome", "committed"),
            Self::CooldownCommitted => ("refresh_outcome", "cooldown_committed"),
            Self::LockTimeout => ("lock_timeout", "retry"),
            Self::GrantRevoked => ("grant_revoked", "committed"),
            Self::ReconnectRequired => ("reconnect_required", "committed"),
            Self::ResolutionFailed(error) => (
                "resolution_failed",
                match error {
                    ResolutionError::Cancelled => "cancelled",
                    ResolutionError::Configuration => "configuration",
                    ResolutionError::ReconnectRequired => "reconnect",
                    ResolutionError::Integration(_) => "denied_or_unavailable",
                    ResolutionError::Retry(reason, _) => match reason {
                        RetryReason::LockBusy => "lock_busy",
                        RetryReason::Cooldown => "cooldown",
                        RetryReason::Pending => "pending",
                        RetryReason::AmbiguousCommit => "ambiguous_commit",
                        RetryReason::Unavailable => "unavailable",
                        RetryReason::TokenMargin => "token_margin",
                    },
                },
            ),
        }
    }
    pub(super) fn emit(self) {
        let (event, outcome) = self.labels();
        tracing::info!(target: "open_bots::integrations", event, outcome, "Integration lifecycle");
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex};
    #[derive(Clone, Default)]
    pub(crate) struct Capture(Arc<Mutex<Vec<u8>>>);
    impl Write for Capture {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl Capture {
        pub(crate) fn subscriber(&self) -> impl tracing::Subscriber + Send + Sync + 'static {
            let writer = self.clone();
            tracing_subscriber::fmt()
                .without_time()
                .with_ansi(false)
                .with_writer(move || writer.clone())
                .finish()
        }
        pub(crate) fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }
    #[test]
    fn integration_service_contract_lifecycle_observations_have_only_fixed_labels() {
        let capture = Capture::default();
        tracing::subscriber::with_default(capture.subscriber(), || {
            for event in [
                Event::CacheHit,
                Event::RefreshAttempt,
                Event::RefreshCommitted,
                Event::CooldownCommitted,
                Event::LockTimeout,
                Event::GrantRevoked,
                Event::ReconnectRequired,
                Event::RefreshResponse(Ok(())),
                Event::RefreshResponse(Err(RenewalFailure::Malformed)),
            ] {
                event.emit();
            }
        });
        let text = capture.text();
        assert_eq!(text.lines().count(), 9);
        for event in [
            "cache_hit",
            "refresh_attempt",
            "refresh_outcome",
            "lock_timeout",
            "grant_revoked",
            "reconnect_required",
        ] {
            assert!(text.contains(event));
        }
        assert!(!text.contains("SecretString"));
    }
}
