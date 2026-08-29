use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

use crate::entities::{
    correlation::CorrelationId,
    task::TaskSuspension,
    value_objects::{ChannelSlug, CompanySlug, EmailAddress},
};

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
    pub channel_id: Uuid,
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
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Who is being asked to approve something, and on whose behalf.
///
/// This is the half of an approval request that a single agent run settles once and then reuses:
/// the run knows its company, its channel, the thread it is answering, the lease it would park,
/// its chain, and who to mail. Only [`ApprovalAction`] changes between one request and the next.
///
/// Held as a value because the alternative was fourteen positional parameters, nine of which were
/// this. Five of those nine are `Uuid`, `Option<Uuid>` or `String`, so a transposed pair compiled
/// and mailed the approval to the wrong place.
#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalSubject {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub channel_name: String,
    pub channel_slug: ChannelSlug,
    pub company_slug: CompanySlug,
    pub thread_id: Option<Uuid>,
    /// Which task this parks if the approval is raised, and on what authority.
    ///
    /// Carries the task id, so nothing is lost against the bare `Option<Uuid>` this replaced --
    /// but parking a task is a write against a leased row, and only the lease can tell the run
    /// that owns it from one that has already been superseded.
    pub suspension: Option<TaskSuspension>,
    /// The chain of the run asking for approval, carried onto the notification so the human's
    /// answer and the resumed run stay on the same trail.
    pub correlation_id: CorrelationId,
    pub approver_email: EmailAddress,
}

/// What is being approved.
///
/// The half that differs per request. `step_key` is the identity -- approvals are deduplicated on
/// it -- while the rest is what the human reads in the mail.
#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalAction {
    /// Idempotency key for this decision. Asking twice about the same step returns the standing
    /// approval rather than mailing a second link.
    pub step_key: String,
    /// Which kind of decision this is. `quorum_timeout` offers extend/proceed options; anything
    /// else is a plain confirm/reject.
    pub action_type: String,
    pub title: String,
    pub summary: String,
    pub payload: serde_json::Value,
}

impl ApprovalAction {
    /// Whether this decision offers the outreach timeout options rather than confirm/reject.
    ///
    /// A named predicate rather than `action_type == "quorum_timeout"` spelled out at each site:
    /// the string is a stored value, and comparing it by hand is how the mail body and the
    /// handler drift apart.
    pub fn is_quorum_timeout(&self) -> bool {
        self.action_type == QUORUM_TIMEOUT_ACTION
    }
}

/// The `action_type` that asks a human what to do about an outreach that never reached quorum.
pub const QUORUM_TIMEOUT_ACTION: &str = "quorum_timeout";
