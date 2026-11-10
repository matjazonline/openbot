//! The provider authentication breaker (p06 gate A4-1, F3).
//!
//! A revoked or wrong key, a missing permission or a billing problem fails every call the same
//! way. Without a breaker, each queued operation would fail `failed(authentication_error)` one by
//! one. Instead, the first such rejection opens the breaker:
//! - the operation goes back to `accepted` with its dispatch not counted;
//! - no worker of this process claims while the breaker is open, for 5 minutes, doubling up to
//!   1 hour;
//! - once the window has passed, exactly one worker holds the probe permit (half-open);
//! - the probe's call decides: a provider answer that is not one of these rejections closes the
//!   breaker, and another such rejection re-opens it with the window doubled.
//!
//! The rejection is definite, so no inference ran. Request deadlines still apply: the reaper
//! expires accepted operations past `expires_at`, so nothing waits forever. The breaker is per
//! process; one is shared by all of a process's workers.
use std::sync::Mutex;
use std::time::Duration;
use tokio::time::Instant;

/// The provider error types that open the breaker: they reject every call until an operator
/// fixes the key, its permissions or the account's billing.
pub const AUTH_KINDS: [&str; 3] = ["authentication_error", "permission_error", "billing_error"];

pub const BREAKER_BASE: Duration = Duration::from_secs(300);
pub const BREAKER_MAX: Duration = Duration::from_secs(3_600);

/// What a worker may do before claiming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permit {
    /// Closed: claim as usual.
    Closed,
    /// Half-open: this worker alone may make one call.
    Probe,
    /// Open: claim nothing.
    Open,
}

#[derive(Debug)]
struct State {
    open_until: Option<Instant>,
    /// The current window; zero while closed.
    window: Duration,
    probing: bool,
}

#[derive(Debug)]
pub struct ProviderBreaker {
    base: Duration,
    max: Duration,
    state: Mutex<State>,
}

impl ProviderBreaker {
    pub fn new(base: Duration, max: Duration) -> Self {
        Self {
            base,
            max,
            state: Mutex::new(State {
                open_until: None,
                window: Duration::ZERO,
                probing: false,
            }),
        }
    }

    pub fn permit(&self) -> Permit {
        let Ok(mut state) = self.state.lock() else {
            return Permit::Closed;
        };
        match state.open_until {
            None => Permit::Closed,
            Some(until) if Instant::now() < until || state.probing => Permit::Open,
            Some(_) => {
                state.probing = true;
                Permit::Probe
            }
        }
    }

    /// One of [`AUTH_KINDS`] answered a call made under `permit`. Returns how long the
    /// operation waits before it may be claimed again.
    ///
    /// A closed breaker opens for the base window, and a probe's rejection doubles the window
    /// (bounded by the maximum). A call that was already in flight when the breaker opened
    /// changes nothing, so each window learns from at most one call.
    pub fn tripped(&self, permit: Permit, kind: &str) -> Duration {
        let Ok(mut state) = self.state.lock() else {
            return self.base;
        };
        let now = Instant::now();
        let window = match (permit, state.open_until) {
            (Permit::Probe, _) => (state.window * 2).clamp(self.base, self.max),
            (_, None) => self.base,
            (_, Some(until)) => return until.saturating_duration_since(now),
        };
        state.window = window;
        state.open_until = Some(now + window);
        state.probing = false;
        tracing::error!(
            kind,
            window_seconds = window.as_secs(),
            "the inference provider refused the service's credentials; generation is paused"
        );
        window
    }

    /// The provider answered without one of [`AUTH_KINDS`]: the credentials work, so close.
    pub fn answered(&self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.open_until.is_some() {
            tracing::info!("the inference provider accepted the service's credentials again");
        }
        state.open_until = None;
        state.window = Duration::ZERO;
        state.probing = false;
    }

    /// A probe that ended without a provider answer (nothing due, a stale claim, a call that was
    /// not sent or whose outcome is unknown) returns its permit, so another worker may probe.
    pub fn released(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.probing = false;
        }
    }

    pub fn is_open(&self) -> bool {
        self.state
            .lock()
            .map(|state| state.open_until.is_some())
            .unwrap_or(false)
    }
}

impl Default for ProviderBreaker {
    fn default() -> Self {
        Self::new(BREAKER_BASE, BREAKER_MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: Duration = Duration::from_millis(40);

    async fn wait(window: Duration) {
        tokio::time::sleep(window + Duration::from_millis(5)).await;
    }

    #[tokio::test]
    async fn memory_operations_breaker_opens_probes_once_doubles_and_closes() {
        let breaker = ProviderBreaker::new(BASE, Duration::from_millis(100));
        assert_eq!(breaker.permit(), Permit::Closed);
        assert_eq!(
            breaker.tripped(Permit::Closed, "authentication_error"),
            BASE
        );
        // A second call already in flight changes nothing.
        assert!(breaker.tripped(Permit::Closed, "authentication_error") <= BASE);
        assert_eq!(breaker.permit(), Permit::Open);
        wait(BASE).await;
        assert_eq!(breaker.permit(), Permit::Probe);
        assert_eq!(breaker.permit(), Permit::Open, "one probe at a time");
        assert_eq!(breaker.tripped(Permit::Probe, "billing_error"), BASE * 2);
        assert_eq!(breaker.permit(), Permit::Open);
        wait(BASE * 2).await;
        assert_eq!(breaker.permit(), Permit::Probe);
        assert_eq!(
            breaker.tripped(Permit::Probe, "permission_error"),
            Duration::from_millis(100),
            "bounded by the maximum"
        );
        wait(Duration::from_millis(100)).await;
        assert_eq!(breaker.permit(), Permit::Probe);
        breaker.released();
        assert_eq!(
            breaker.permit(),
            Permit::Probe,
            "a released probe can be retaken"
        );
        breaker.answered();
        assert!(!breaker.is_open());
        assert_eq!(breaker.permit(), Permit::Closed);
        assert_eq!(
            breaker.tripped(Permit::Closed, "authentication_error"),
            BASE,
            "a new opening starts from the base window"
        );
    }
}
