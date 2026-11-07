//! Read projections of independent durable facts; these values confer no execution/send authority.
use crate::application::workflow::reads::RunStateName;
use crate::domain::workflow::{RunId, WaitId};
use crate::entities::message::CanonicalMessageId;
use crate::entities::transport::{DeliveryId, DeliveryStatus};
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub const MAX_RESPONSE_OUTCOMES: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalPublicationState {
    Suppressed,
    Pending,
    Claimed,
    Published,
    Failed,
}
impl FinalPublicationState {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "suppressed" => Some(Self::Suppressed),
            "pending" => Some(Self::Pending),
            "claimed" => Some(Self::Claimed),
            "published" => Some(Self::Published),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Suppressed => "suppressed",
            Self::Pending => "pending",
            Self::Claimed => "claimed",
            Self::Published => "published",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseDelivery {
    pub id: DeliveryId,
    pub state: DeliveryStatus,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalResponseOutcome {
    pub publication: FinalPublicationState,
    pub message: Option<CanonicalMessageId>,
    pub delivery: Option<ResponseDelivery>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanPromptOutcome {
    pub run: RunId,
    pub wait: WaitId,
    pub message: CanonicalMessageId,
    pub deadline: DateTime<Utc>,
    pub waiting: bool,
    pub awaiting_viewer: bool,
    pub delivery: Option<ResponseDelivery>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelResponseOutcome {
    pub run: RunId,
    pub source: CanonicalMessageId,
    pub channel: Uuid,
    pub thread: Uuid,
    pub execution: RunStateName,
    pub waiting_for_review: bool,
    pub final_response: Option<FinalResponseOutcome>,
    pub prompt: Option<HumanPromptOutcome>,
}

impl ChannelResponseOutcome {
    /// Derived on read. Delivery acceptance never changes whether human input is pending.
    pub fn label(&self) -> &'static str {
        match self.execution {
            RunStateName::Failed => return "Failed; earlier messages remain in history",
            RunStateName::Cancelled => return "Cancelled; earlier messages remain in history",
            RunStateName::Succeeded => return self.completed_label(),
            RunStateName::Queued | RunStateName::Running | RunStateName::Waiting => {}
        }
        if let Some(prompt) = self.prompt.as_ref().filter(|prompt| prompt.waiting) {
            if prompt
                .delivery
                .as_ref()
                .is_some_and(|delivery| delivery.state.needs_attention())
            {
                return "Waiting for a response; request needs attention";
            }
            return if prompt.awaiting_viewer {
                "Waiting for your response"
            } else {
                "Waiting for a response"
            };
        }
        if self.waiting_for_review {
            "Waiting for reviewer"
        } else {
            "Processing"
        }
    }

    fn completed_label(&self) -> &'static str {
        let Some(response) = &self.final_response else {
            return "Work completed; reply pending";
        };
        match response.publication {
            FinalPublicationState::Suppressed => "Completed without automatic reply",
            FinalPublicationState::Pending | FinalPublicationState::Claimed => {
                "Work completed; reply pending"
            }
            FinalPublicationState::Failed => "Work completed; reply needs attention",
            FinalPublicationState::Published => {
                match response.delivery.as_ref().map(|delivery| delivery.state) {
                    None => "Completed; reply posted",
                    Some(DeliveryStatus::Delivered) => "Completed; provider accepted reply",
                    Some(DeliveryStatus::OutcomeUnknown | DeliveryStatus::DeadLetter) => {
                        "Work completed; reply needs attention"
                    }
                    Some(
                        DeliveryStatus::Pending
                        | DeliveryStatus::Sending
                        | DeliveryStatus::Retryable,
                    ) => "Work completed; reply pending",
                }
            }
        }
    }
}
