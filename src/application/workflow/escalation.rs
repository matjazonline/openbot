//! Administrative escalation decisions, independent of a run's execution/wait state.
use super::binding::EscalationPolicy;
use super::decisions::DecisionCommandKey;
use super::{RunRevision, WorkflowActor};
use crate::app_error::{AppError, AppResult};
use crate::domain::workflow::{CompanyId, RunId, StepId, WorkflowBindingId};
use crate::entities::department::DepartmentRole;
use crate::entities::transport::PrincipalId;
pub use crate::entities::workflow_escalation::{
    EscalationDecisionId, EscalationId, EscalationNoticeId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[path = "escalation_recipients.rs"]
mod recipients;
pub(crate) use recipients::{
    NotificationRecipients, NotificationRoleRefusal, resolve_notification_recipients,
};

pub const MAX_ESCALATION_RECIPIENTS: usize = 16;
pub const MAX_MANUAL_ASSIGNMENTS: usize = 32;
pub const MAX_PARKED_FOLLOW_DECISIONS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct EscalationHop(u8);

impl EscalationHop {
    pub const FIRST: Self = Self(1);
    pub const SECOND: Self = Self(2);

    pub const fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for EscalationHop {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 | 2 => Ok(Self(value)),
            _ => Err("Escalation hop must be 1 or 2"),
        }
    }
}

impl From<EscalationHop> for u8 {
    fn from(value: EscalationHop) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipientAvailability {
    /// Includes a pending durable notice: only proven failure removes a route.
    Capable,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LadderState {
    Pending,
    Waiting {
        hop: EscalationHop,
        deadline: DateTime<Utc>,
        recipients: RecipientAvailability,
    },
    OwnerWaiting,
    Settled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LadderAction {
    Start,
    Advance,
    WaitUntil(DateTime<Utc>),
    OwnerAttention,
    Stop,
}

/// The queue owns leases and uniqueness; this pure rule never manufactures another case.
pub fn ladder_action(
    policy: EscalationPolicy,
    state: LadderState,
    now: DateTime<Utc>,
) -> LadderAction {
    match state {
        LadderState::Pending => LadderAction::Start,
        LadderState::OwnerWaiting | LadderState::Settled => LadderAction::Stop,
        LadderState::Waiting {
            deadline,
            recipients: RecipientAvailability::Capable,
            ..
        } if deadline > now => LadderAction::WaitUntil(deadline),
        LadderState::Waiting { hop, .. }
            if hop == EscalationHop::FIRST && policy.hops.get() == 2 =>
        {
            LadderAction::Advance
        }
        LadderState::Waiting { .. } => LadderAction::OwnerAttention,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EscalationChoice {
    Acknowledge,
    Reassign { principal: PrincipalId },
    Cancel { expected_run_revision: RunRevision },
}

impl EscalationChoice {
    pub fn value(&self) -> serde_json::Value {
        match self {
            Self::Acknowledge => serde_json::json!({"kind":"acknowledge"}),
            Self::Reassign { principal } => {
                serde_json::json!({"kind":"reassign","principal":principal})
            }
            Self::Cancel {
                expected_run_revision,
            } => {
                serde_json::json!({"kind":"cancel","expected_run_revision":expected_run_revision.0})
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct SubmitEscalation {
    pub company: CompanyId,
    pub actor: WorkflowActor,
    pub decision: EscalationDecisionId,
    pub command: DecisionCommandKey,
    pub choice: EscalationChoice,
}

impl SubmitEscalation {
    pub fn fingerprint(&self) -> AppResult<String> {
        super::decisions::hex_digest(&serde_json::json!({
            "version":1,
            "company":self.company,
            "actor":self.actor.user_id(),
            "decision":self.decision,
            "choice":self.choice.value(),
        }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscalationSubmission {
    Applied,
    Replayed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EscalationReason {
    /// A bounded typed failure code, never text parsed from an exception.
    pub code: crate::domain::workflow::FailureCode,
    pub step: Option<StepId>,
    pub role: Option<DepartmentRole>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EscalationSourceView {
    Run(RunId),
    Admission { binding: WorkflowBindingId },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EscalationRecipientView {
    pub principal: PrincipalId,
    pub label: String,
    pub delivery: EscalationRecipientState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscalationRecipientState {
    Pending,
    Delivered,
    Failed,
    Withdrawn,
}

#[derive(Debug, Clone)]
pub struct EscalationView {
    pub company: CompanyId,
    pub escalation: EscalationId,
    pub decision: EscalationDecisionId,
    pub source: EscalationSourceView,
    pub reason: EscalationReason,
    pub hop: EscalationHop,
    pub created_at: DateTime<Utc>,
    pub deadline: DateTime<Utc>,
    pub recipients: Vec<EscalationRecipientView>,
    pub settled_at: Option<DateTime<Utc>>,
    pub can_acknowledge: bool,
    pub can_reassign: bool,
    pub cancel_revision: Option<RunRevision>,
}

#[async_trait]
pub trait WorkflowEscalations: Send + Sync {
    async fn read_escalation(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        decision: EscalationDecisionId,
    ) -> AppResult<EscalationView>;

    async fn submit_escalation(&self, command: SubmitEscalation)
    -> AppResult<EscalationSubmission>;
}

pub(crate) fn invalid() -> AppError {
    AppError::BadRequest("Escalation is unavailable or its state has changed".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::workflow::binding::EscalationHops;
    use chrono::Duration;

    #[test]
    fn workflow_escalation_ladder_stops_after_its_configured_hops() {
        let now = Utc::now();
        let future = now + Duration::seconds(1);
        let past = now - Duration::seconds(1);
        let waiting = |hop, deadline, recipients| LadderState::Waiting {
            hop,
            deadline,
            recipients,
        };
        for (count, first_exhausted) in [
            (1, LadderAction::OwnerAttention),
            (2, LadderAction::Advance),
        ] {
            let policy = EscalationPolicy {
                notify: DepartmentRole::Reviewer,
                hops: EscalationHops::try_from(count).unwrap(),
            };
            // Explicit state table: expected outcomes are data, independent of the rule's branches.
            for (state, expected) in [
                (LadderState::Pending, LadderAction::Start),
                (LadderState::OwnerWaiting, LadderAction::Stop),
                (LadderState::Settled, LadderAction::Stop),
                (
                    waiting(EscalationHop::FIRST, future, RecipientAvailability::Capable),
                    LadderAction::WaitUntil(future),
                ),
                (
                    waiting(EscalationHop::FIRST, now, RecipientAvailability::Capable),
                    first_exhausted,
                ),
                (
                    waiting(EscalationHop::FIRST, past, RecipientAvailability::Capable),
                    first_exhausted,
                ),
                (
                    waiting(
                        EscalationHop::FIRST,
                        future,
                        RecipientAvailability::Unavailable,
                    ),
                    first_exhausted,
                ),
                (
                    waiting(
                        EscalationHop::FIRST,
                        past,
                        RecipientAvailability::Unavailable,
                    ),
                    first_exhausted,
                ),
                (
                    waiting(
                        EscalationHop::SECOND,
                        future,
                        RecipientAvailability::Capable,
                    ),
                    LadderAction::WaitUntil(future),
                ),
                (
                    waiting(EscalationHop::SECOND, now, RecipientAvailability::Capable),
                    LadderAction::OwnerAttention,
                ),
                (
                    waiting(EscalationHop::SECOND, past, RecipientAvailability::Capable),
                    LadderAction::OwnerAttention,
                ),
                (
                    waiting(
                        EscalationHop::SECOND,
                        future,
                        RecipientAvailability::Unavailable,
                    ),
                    LadderAction::OwnerAttention,
                ),
                (
                    waiting(
                        EscalationHop::SECOND,
                        past,
                        RecipientAvailability::Unavailable,
                    ),
                    LadderAction::OwnerAttention,
                ),
            ] {
                assert_eq!(
                    ladder_action(policy, state, now),
                    expected,
                    "{count} hops: {state:?}"
                );
            }
        }
    }
}
