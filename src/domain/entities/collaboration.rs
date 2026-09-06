//! Transport-neutral, read-only collaboration status.
//!
//! These values are projections over task, outreach, delivery, approval, message and ownership
//! records. They are deliberately not persisted as another workflow state machine.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{
    correlation::CorrelationId,
    outreach::OutreachStatus,
    task::TaskStatus,
    transport::{DeliveryStatus, FailureClass, PrincipalId, QualifiedIdentity},
};

/// The business destination of one delegated request.
///
/// An internal destination is stable across transport/address changes. An external destination is
/// fully qualified by transport namespace; an unqualified email address never crosses this seam.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CollaborationTarget {
    InternalChannel { channel_id: Uuid },
    RestrictedInternal,
    External { identity: QualifiedIdentity },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutreachBusinessStatus {
    Waiting,
    ReadyToResume,
    NeedsDecision,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetBusinessStatus {
    Preparing,
    Sending,
    Waiting,
    Responded,
    NeedsDecision,
    Failed,
    Cancelled,
    Superseded,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollaborationOwner {
    pub kind: CollaborationOwnerKind,
    pub principal_id: Option<PrincipalId>,
    pub label: String,
    /// False when the principal was removed or can no longer receive work.
    pub available: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollaborationOwnerKind {
    Human,
    Agent,
    Unassigned,
    Restricted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollaborationProgress {
    pub responded: usize,
    pub required: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NextActionActor {
    CurrentOwner,
    InternalTarget,
    ExternalTarget,
    TaskQueue,
    DeliveryQueue,
    CompanyManager,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NextActionKind {
    RunTask,
    DeliverRequest,
    ProvideResponse,
    ReviewTimeout,
    ResolveDeliveryOutcome,
    RepairFailure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollaborationNextAction {
    pub actor: NextActionActor,
    pub action: NextActionKind,
    pub due_at: Option<DateTime<Utc>>,
    /// Present only when the viewer is authorized to follow it.
    pub href: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollaborationTargetSummary {
    pub id: Uuid,
    pub target: CollaborationTarget,
    pub label: String,
    pub status: TargetBusinessStatus,
    pub responded_at: Option<DateTime<Utc>>,
    pub next_action: Option<CollaborationNextAction>,
    /// A same-company request can create another task. It remains a child: its owner never
    /// replaces the owner of the task that made the request.
    pub child: Option<Box<CollaborationSummary>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollaborationSummary {
    pub task_id: Uuid,
    pub correlation_id: CorrelationId,
    pub owner: CollaborationOwner,
    pub status: OutreachBusinessStatus,
    pub progress: Option<CollaborationProgress>,
    pub expires_at: Option<DateTime<Utc>>,
    pub next_action: Option<CollaborationNextAction>,
    pub children: Vec<CollaborationTargetSummary>,
    pub as_of: DateTime<Utc>,
    pub truncated: bool,
    /// A viewer-scoped route to this task's collaboration detail.
    pub detail_href: Option<String>,
}

/// Derive the target's public business status from authoritative source states.
pub fn target_business_status(
    responded_at: Option<DateTime<Utc>>,
    outreach_status: OutreachStatus,
    delivery_status: Option<DeliveryStatus>,
    failure_class: Option<FailureClass>,
    expires_at: DateTime<Utc>,
    as_of: DateTime<Utc>,
) -> TargetBusinessStatus {
    if responded_at.is_some() {
        return TargetBusinessStatus::Responded;
    }
    if outreach_status == OutreachStatus::Cancelled {
        return TargetBusinessStatus::Cancelled;
    }
    if failure_class == Some(FailureClass::Superseded) {
        return TargetBusinessStatus::Superseded;
    }
    if outreach_status == OutreachStatus::TimeoutPendingApproval {
        return TargetBusinessStatus::NeedsDecision;
    }
    match delivery_status {
        None if as_of >= expires_at => TargetBusinessStatus::Expired,
        None => TargetBusinessStatus::Preparing,
        Some(DeliveryStatus::OutcomeUnknown) => TargetBusinessStatus::NeedsDecision,
        Some(DeliveryStatus::DeadLetter) => TargetBusinessStatus::Failed,
        Some(DeliveryStatus::Delivered) if as_of >= expires_at => TargetBusinessStatus::Expired,
        Some(DeliveryStatus::Delivered) => TargetBusinessStatus::Waiting,
        Some(DeliveryStatus::Pending | DeliveryStatus::Retryable | DeliveryStatus::Sending)
            if as_of >= expires_at =>
        {
            TargetBusinessStatus::Expired
        }
        Some(DeliveryStatus::Pending | DeliveryStatus::Retryable | DeliveryStatus::Sending) => {
            TargetBusinessStatus::Sending
        }
    }
}

pub fn outreach_business_status(
    stored: OutreachStatus,
    target_statuses: &[TargetBusinessStatus],
) -> OutreachBusinessStatus {
    match stored {
        OutreachStatus::ThresholdMet | OutreachStatus::ProceedPartial => {
            OutreachBusinessStatus::ReadyToResume
        }
        OutreachStatus::TimeoutPendingApproval => OutreachBusinessStatus::NeedsDecision,
        OutreachStatus::Completed => OutreachBusinessStatus::Completed,
        OutreachStatus::Cancelled => OutreachBusinessStatus::Cancelled,
        OutreachStatus::Waiting
            if target_statuses.contains(&TargetBusinessStatus::NeedsDecision) =>
        {
            OutreachBusinessStatus::NeedsDecision
        }
        OutreachStatus::Waiting
            if !target_statuses.is_empty()
                && target_statuses.iter().all(|status| {
                    matches!(
                        status,
                        TargetBusinessStatus::Failed
                            | TargetBusinessStatus::Cancelled
                            | TargetBusinessStatus::Superseded
                            | TargetBusinessStatus::Expired
                    )
                }) =>
        {
            OutreachBusinessStatus::Failed
        }
        OutreachStatus::Waiting => OutreachBusinessStatus::Waiting,
    }
}

/// Status for a task with no outreach row. This keeps the summary useful before delegation starts
/// and after it has been folded back into the owning task.
pub fn task_business_status(status: TaskStatus) -> OutreachBusinessStatus {
    match status {
        TaskStatus::Pending | TaskStatus::Processing => OutreachBusinessStatus::ReadyToResume,
        TaskStatus::PendingApproval => OutreachBusinessStatus::NeedsDecision,
        TaskStatus::WaitingForThirdPartyReply => OutreachBusinessStatus::Waiting,
        TaskStatus::Completed => OutreachBusinessStatus::Completed,
        TaskStatus::Stopped => OutreachBusinessStatus::Cancelled,
        TaskStatus::Failed | TaskStatus::DeadLetter => OutreachBusinessStatus::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-06T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn response_wins_over_late_and_failed_delivery_history() {
        assert_eq!(
            target_business_status(
                Some(now()),
                OutreachStatus::Waiting,
                Some(DeliveryStatus::DeadLetter),
                Some(FailureClass::ProviderFault),
                now() - chrono::Duration::hours(1),
                now(),
            ),
            TargetBusinessStatus::Responded
        );
    }

    #[test]
    fn ambiguous_provider_outcome_requires_a_decision() {
        assert_eq!(
            target_business_status(
                None,
                OutreachStatus::Waiting,
                Some(DeliveryStatus::OutcomeUnknown),
                Some(FailureClass::Timeout),
                now() + chrono::Duration::hours(1),
                now(),
            ),
            TargetBusinessStatus::NeedsDecision
        );
        assert_eq!(
            outreach_business_status(
                OutreachStatus::Waiting,
                &[TargetBusinessStatus::NeedsDecision]
            ),
            OutreachBusinessStatus::NeedsDecision
        );
    }

    #[test]
    fn superseded_delivery_is_not_reported_as_a_provider_failure() {
        assert_eq!(
            target_business_status(
                None,
                OutreachStatus::Completed,
                Some(DeliveryStatus::DeadLetter),
                Some(FailureClass::Superseded),
                now() + chrono::Duration::hours(1),
                now(),
            ),
            TargetBusinessStatus::Superseded
        );
    }

    #[test]
    fn stored_outreach_transitions_have_stable_business_meanings() {
        let cases = [
            (
                OutreachStatus::Waiting,
                vec![
                    TargetBusinessStatus::Responded,
                    TargetBusinessStatus::Waiting,
                ],
                OutreachBusinessStatus::Waiting,
            ),
            (
                OutreachStatus::ThresholdMet,
                vec![
                    TargetBusinessStatus::Responded,
                    TargetBusinessStatus::Waiting,
                ],
                OutreachBusinessStatus::ReadyToResume,
            ),
            (
                OutreachStatus::ProceedPartial,
                vec![
                    TargetBusinessStatus::Responded,
                    TargetBusinessStatus::Expired,
                ],
                OutreachBusinessStatus::ReadyToResume,
            ),
            (
                OutreachStatus::TimeoutPendingApproval,
                vec![TargetBusinessStatus::Expired],
                OutreachBusinessStatus::NeedsDecision,
            ),
            (
                OutreachStatus::Cancelled,
                vec![TargetBusinessStatus::Cancelled],
                OutreachBusinessStatus::Cancelled,
            ),
            (
                OutreachStatus::Completed,
                vec![TargetBusinessStatus::Responded],
                OutreachBusinessStatus::Completed,
            ),
            (
                OutreachStatus::Waiting,
                vec![TargetBusinessStatus::Failed, TargetBusinessStatus::Expired],
                OutreachBusinessStatus::Failed,
            ),
        ];

        for (stored, targets, expected) in cases {
            assert_eq!(outreach_business_status(stored, &targets), expected);
        }
    }

    #[test]
    fn expired_retry_is_reported_as_expired_not_sending() {
        assert_eq!(
            target_business_status(
                None,
                OutreachStatus::Waiting,
                Some(DeliveryStatus::Retryable),
                Some(FailureClass::ProviderFault),
                now() - chrono::Duration::seconds(1),
                now(),
            ),
            TargetBusinessStatus::Expired
        );
    }
}
