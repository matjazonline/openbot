use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

use crate::entities::value_objects::EmailAddress;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutreachStatus {
    Waiting,
    ThresholdMet,
    TimeoutPendingApproval,
    ProceedPartial,
    Cancelled,
    Completed,
}

impl OutreachStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::ThresholdMet => "threshold_met",
            Self::TimeoutPendingApproval => "timeout_pending_approval",
            Self::ProceedPartial => "proceed_partial",
            Self::Cancelled => "cancelled",
            Self::Completed => "completed",
        }
    }
}

impl FromStr for OutreachStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "waiting" => Ok(Self::Waiting),
            "threshold_met" => Ok(Self::ThresholdMet),
            "timeout_pending_approval" => Ok(Self::TimeoutPendingApproval),
            "proceed_partial" => Ok(Self::ProceedPartial),
            "cancelled" => Ok(Self::Cancelled),
            "completed" => Ok(Self::Completed),
            _ => Err(format!("Unknown outreach status: {value}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OutreachProgress {
    pub id: Uuid,
    pub task_id: Uuid,
    pub status: OutreachStatus,
    pub required_threshold_percent: f64,
    pub target_count: usize,
    pub response_count: usize,
    pub required_response_count: usize,
    pub expires_at: DateTime<Utc>,
    pub suspended: bool,
}

impl OutreachProgress {
    pub fn response_percent(&self) -> f64 {
        if self.target_count == 0 {
            0.0
        } else {
            self.response_count as f64 * 100.0 / self.target_count as f64
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutreachReplyMatch {
    pub outreach_id: Uuid,
    pub task_id: Uuid,
    pub target_email: EmailAddress,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DueOutreach {
    pub outreach_id: Uuid,
    pub task_id: Uuid,
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Option<Uuid>,
    pub required_threshold_percent: f64,
    pub target_count: usize,
    pub response_count: usize,
    pub expires_at: DateTime<Utc>,
}
