//! What a thread's workflow runs are doing, for the mailbox (p08 D4.6).
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The activity of a thread's newest run, an unfinished one first.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThreadActivity {
    /// Admitted, and waiting for a worker.
    Queued,
    /// A step is running right now.
    Working,
    /// Parked on a decision a reviewer has to make.
    AwaitingReview,
    /// Parked on a check of whether an uncertain action took effect.
    NeedsAttention,
    /// Parked on anything else: an event, a timer, a child run, an effect.
    Waiting,
    /// The run failed.
    Failed,
}

impl ThreadActivity {
    /// The mapping table: a run that succeeded or was cancelled has nothing to say.
    pub fn from_run(state: &str, waiting_reason: Option<&str>) -> Option<Self> {
        match (state, waiting_reason) {
            ("queued", _) => Some(Self::Queued),
            ("running", _) => Some(Self::Working),
            ("waiting", Some("decision")) => Some(Self::AwaitingReview),
            ("waiting", Some("reconciliation")) => Some(Self::NeedsAttention),
            ("waiting", _) => Some(Self::Waiting),
            ("failed", _) => Some(Self::Failed),
            _ => None,
        }
    }

    /// Whether a run is under way right now, as opposed to parked or finished.
    ///
    /// The column animates only this one: it is the single state that is expected to resolve on
    /// its own, without anybody doing anything.
    pub fn is_running(self) -> bool {
        matches!(self, Self::Working)
    }

    /// Whether somebody has to act before the run can go on.
    pub fn needs_someone(self) -> bool {
        matches!(
            self,
            Self::AwaitingReview | Self::NeedsAttention | Self::Failed
        )
    }

    /// Wording for the mailbox, where the reader is waiting on a conversation.
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Working => "Agent working…",
            Self::AwaitingReview => "Awaiting review",
            Self::NeedsAttention => "Needs attention",
            Self::Waiting => "Waiting",
            Self::Failed => "Run failed",
        }
    }
}

/// One thread's newest run and its activity. Threads with no run are absent from the map a
/// reader returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadWorkSummary {
    pub run_id: Uuid,
    pub activity: Option<ThreadActivity>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mapping_table() {
        for (state, reason, activity) in [
            ("queued", None, Some(ThreadActivity::Queued)),
            ("running", None, Some(ThreadActivity::Working)),
            (
                "waiting",
                Some("decision"),
                Some(ThreadActivity::AwaitingReview),
            ),
            (
                "waiting",
                Some("reconciliation"),
                Some(ThreadActivity::NeedsAttention),
            ),
            ("waiting", Some("event"), Some(ThreadActivity::Waiting)),
            ("waiting", Some("timer"), Some(ThreadActivity::Waiting)),
            ("waiting", Some("child_run"), Some(ThreadActivity::Waiting)),
            ("waiting", Some("effect"), Some(ThreadActivity::Waiting)),
            ("failed", None, Some(ThreadActivity::Failed)),
            ("succeeded", None, None),
            ("cancelled", None, None),
        ] {
            assert_eq!(
                ThreadActivity::from_run(state, reason),
                activity,
                "{state}/{reason:?}"
            );
        }
        assert!(ThreadActivity::Working.is_running());
        assert!(!ThreadActivity::Queued.is_running());
        assert!(ThreadActivity::AwaitingReview.needs_someone());
        assert!(!ThreadActivity::Waiting.needs_someone());
    }
}
