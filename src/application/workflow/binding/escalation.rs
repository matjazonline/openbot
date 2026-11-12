//! Validated routing and acknowledgement bounds for an opted-in escalation ladder.
use crate::entities::department::DepartmentRole;
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EscalationAckHours(u16);

impl Default for EscalationAckHours {
    fn default() -> Self {
        Self(24)
    }
}

impl EscalationAckHours {
    pub fn parse(value: Option<&str>) -> Result<Self, &'static str> {
        let Some(value) = value else {
            return Ok(Self::default());
        };
        value
            .parse::<u16>()
            .ok()
            .filter(|hours| (1..=168).contains(hours))
            .map(Self)
            .ok_or("ESCALATION_ACK_HOURS must be an integer between 1 and 168")
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn workflow_escalation_policy_rejects_unbounded_or_unknown_configuration() {
        for hops in [1, 2] {
            let source = json!({"notify":"reviewer","hops":hops});
            let policy: EscalationPolicy = serde_json::from_value(source.clone()).unwrap();
            assert_eq!(serde_json::to_value(policy).unwrap(), source);
        }
        for source in [
            json!({"notify":"lead","hops":0}),
            json!({"notify":"lead","hops":3}),
            json!({"notify":"lead","hops":1.5}),
            json!({"notify":"unknown","hops":1}),
            json!({"notify":"lead","hops":1,"fallback":"owner"}),
        ] {
            assert!(serde_json::from_value::<EscalationPolicy>(source).is_err());
        }
    }

    #[test]
    fn workflow_escalation_ack_hours_has_a_real_default_and_closed_bounds() {
        assert_eq!(EscalationAckHours::parse(None).unwrap().get(), 24);
        for hours in [1, 24, 168] {
            assert_eq!(
                EscalationAckHours::parse(Some(&hours.to_string()))
                    .unwrap()
                    .get(),
                hours
            );
        }
        for value in ["", "0", "169", "65536", "-1", "1.5", "tomorrow"] {
            assert!(EscalationAckHours::parse(Some(value)).is_err(), "{value}");
        }
    }
}
