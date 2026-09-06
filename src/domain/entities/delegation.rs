use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{task::TaskStatus, transport::PrincipalId};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DelegationAuthority {
    HumanOwner,
    CompanyManager,
    OwningAgent,
}

impl DelegationAuthority {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HumanOwner => "human_owner",
            Self::CompanyManager => "company_manager",
            Self::OwningAgent => "owning_agent",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DelegationReason {
    DeadlineChanged,
    NoLongerNeeded,
    TargetUnavailable,
    IncorrectTarget,
    PartialResultsAccepted,
    TaskStopped,
    Other,
}

impl DelegationReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DeadlineChanged => "deadline_changed",
            Self::NoLongerNeeded => "no_longer_needed",
            Self::TargetUnavailable => "target_unavailable",
            Self::IncorrectTarget => "incorrect_target",
            Self::PartialResultsAccepted => "partial_results_accepted",
            Self::TaskStopped => "task_stopped",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct DelegationActor {
    pub principal_id: PrincipalId,
    pub authority: DelegationAuthority,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum DelegationOperation {
    ExtendOutreach {
        outreach_id: Uuid,
        expires_at: DateTime<Utc>,
    },
    CancelTarget {
        outreach_id: Uuid,
        target_id: Uuid,
    },
    CancelOutreach {
        outreach_id: Uuid,
    },
    ReassignInternalTarget {
        outreach_id: Uuid,
        target_id: Uuid,
        new_channel_id: Uuid,
    },
    ProceedWithPartial {
        outreach_id: Uuid,
    },
    StopTask {
        outreach_id: Uuid,
    },
}

impl DelegationOperation {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ExtendOutreach { .. } => "extend_outreach",
            Self::CancelTarget { .. } => "cancel_target",
            Self::CancelOutreach { .. } => "cancel_outreach",
            Self::ReassignInternalTarget { .. } => "reassign_internal_target",
            Self::ProceedWithPartial { .. } => "proceed_with_partial",
            Self::StopTask { .. } => "stop_task",
        }
    }

    pub const fn outreach_id(&self) -> Uuid {
        match *self {
            Self::ExtendOutreach { outreach_id, .. }
            | Self::CancelTarget { outreach_id, .. }
            | Self::CancelOutreach { outreach_id }
            | Self::ReassignInternalTarget { outreach_id, .. }
            | Self::ProceedWithPartial { outreach_id }
            | Self::StopTask { outreach_id } => outreach_id,
        }
    }

    pub const fn target_id(&self) -> Option<Uuid> {
        match *self {
            Self::CancelTarget { target_id, .. }
            | Self::ReassignInternalTarget { target_id, .. } => Some(target_id),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DelegationCommand {
    pub company_id: Uuid,
    pub task_id: Uuid,
    pub command_id: Uuid,
    pub expected_version: u64,
    pub actor: DelegationActor,
    pub reason: DelegationReason,
    pub reason_detail: Option<String>,
    pub operation: DelegationOperation,
}

impl DelegationCommand {
    pub const MAX_REASON_DETAIL_BYTES: usize = 512;

    pub fn validate(&self) -> Result<(), String> {
        if self.expected_version == 0 {
            return Err("Expected delegation version must be positive.".into());
        }
        if self
            .reason_detail
            .as_ref()
            .is_some_and(|detail| detail.len() > Self::MAX_REASON_DETAIL_BYTES)
        {
            return Err("Delegation reason detail exceeds 512 bytes.".into());
        }
        if let DelegationOperation::ExtendOutreach { expires_at, .. } = self.operation
            && expires_at <= Utc::now()
        {
            return Err("The new outreach deadline must be in the future.".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryCancellation {
    None,
    UnsentCancelled,
    MayHaveBeenReceived,
    Mixed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DelegationCommandResult {
    pub version: u8,
    pub command_id: Uuid,
    pub task_id: Uuid,
    pub outreach_id: Uuid,
    pub operation: String,
    pub outreach_version: u64,
    pub task_status: TaskStatus,
    pub target_id: Option<Uuid>,
    pub replacement_target_id: Option<Uuid>,
    pub response_association_ids: Vec<Uuid>,
    pub delivery_cancellation: DeliveryCancellation,
}
