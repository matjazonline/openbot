//! Identities shared by workflow escalation commands and durable notification projections.
use crate::entities::department::DepartmentRole;
use crate::entities::transport::uuid_id;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

uuid_id!(EscalationId);
uuid_id!(EscalationDecisionId);
uuid_id!(EscalationNoticeId);
uuid_id!(AdmissionRefusalId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum EscalationControlPhase {
    Start,
    Advance,
    Finalize,
}

impl TryFrom<u8> for EscalationControlPhase {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Start),
            2 => Ok(Self::Advance),
            3 => Ok(Self::Finalize),
            _ => Err("Escalation control phase must be 1, 2 or 3"),
        }
    }
}

impl From<EscalationControlPhase> for u8 {
    fn from(value: EscalationControlPhase) -> Self {
        match value {
            EscalationControlPhase::Start => 1,
            EscalationControlPhase::Advance => 2,
            EscalationControlPhase::Finalize => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EscalationPolicy {
    pub notify: DepartmentRole,
    pub hops: EscalationHops,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct EscalationHops(u8);

impl TryFrom<u8> for EscalationHops {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 | 2 => Ok(Self(value)),
            _ => Err("Escalation hops must be 1 or 2"),
        }
    }
}

impl From<EscalationHops> for u8 {
    fn from(value: EscalationHops) -> Self {
        value.0
    }
}

impl EscalationHops {
    pub const fn get(self) -> u8 {
        self.0
    }
}
