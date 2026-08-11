use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
    Expired,
}

impl ApprovalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ApprovalStatus::Pending => "pending",
            ApprovalStatus::Approved => "approved",
            ApprovalStatus::Rejected => "rejected",
            ApprovalStatus::Expired => "expired",
        }
    }
}

impl FromStr for ApprovalStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pending" => Ok(ApprovalStatus::Pending),
            "approved" => Ok(ApprovalStatus::Approved),
            "rejected" => Ok(ApprovalStatus::Rejected),
            "expired" => Ok(ApprovalStatus::Expired),
            _ => Err(format!("Unknown approval status: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HumanApproval {
    pub id: Uuid,
    pub company_id: Uuid,
    pub workflow_id: Uuid,
    pub thread_id: Option<Uuid>,
    pub task_id: Option<Uuid>,
    pub step_key: String,
    pub approver_email: String,
    pub action_type: String,
    pub action_title: String,
    pub action_summary: String,
    pub payload: serde_json::Value,
    pub token: String,
    pub status: ApprovalStatus,
    pub expires_at: chrono::NaiveDateTime,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}
