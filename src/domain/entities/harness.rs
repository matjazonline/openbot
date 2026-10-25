//! Which runtime executes a workflow model step.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Which runtime executes an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessKind {
    /// The only runtime since 06C, and the fixed `identity.harness` literal every workflow
    /// model-step checkpoint records (p08 D4.8).
    #[default]
    Rig,
}

impl HarnessKind {
    /// Every harness the application knows about; parsing iterates it, so the wire strings and
    /// the stored-value parsing cannot drift apart.
    pub const ALL: [Self; 1] = [Self::Rig];

    /// The wire value recorded in run identities.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rig => "rig",
        }
    }

    /// The human-facing name, for operator-visible copy.
    pub fn label(self) -> &'static str {
        match self {
            Self::Rig => "Rig (in process)",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }
}

impl fmt::Display for HarnessKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_kind_round_trips_through_as_str_and_parse() {
        for kind in HarnessKind::ALL {
            assert_eq!(HarnessKind::parse(kind.as_str()), Some(kind));
            assert_eq!(kind.to_string(), kind.as_str());
            assert!(!kind.label().trim().is_empty());
        }

        assert_eq!(HarnessKind::parse("opencode_microvm"), None);
        assert_eq!(HarnessKind::parse(""), None);
        assert_eq!(HarnessKind::default(), HarnessKind::Rig);
    }

    #[test]
    fn harness_kind_serializes_as_its_stored_value() {
        let stored = serde_json::to_value(HarnessKind::Rig).expect("a harness kind");
        assert_eq!(stored, serde_json::json!("rig"));
        assert!(serde_json::from_value::<HarnessKind>(serde_json::json!("ai_agents")).is_err());
    }
}
