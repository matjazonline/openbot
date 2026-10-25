//! The status a queued job row carries in `background_tasks`, as the dashboard counts it.

use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::Processing => "processing",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
        }
    }
}

impl FromStr for TaskStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(TaskStatus::Pending),
            "processing" => Ok(TaskStatus::Processing),
            "completed" => Ok(TaskStatus::Completed),
            "failed" => Ok(TaskStatus::Failed),
            _ => Err(format!("Unknown task status: {}", s)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_status_accepts_only_canonical_stored_values() {
        for status in [
            TaskStatus::Pending,
            TaskStatus::Processing,
            TaskStatus::Completed,
            TaskStatus::Failed,
        ] {
            assert_eq!(status.as_str().parse::<TaskStatus>().unwrap(), status);
        }

        // The legacy runtime's parked and dead-letter states are gone with it (p08 D4.8).
        for noncanonical in ["PENDING", "pending_approval", "dead_letter", "stopped"] {
            assert!(
                noncanonical.parse::<TaskStatus>().is_err(),
                "{noncanonical}"
            );
        }
    }
}
