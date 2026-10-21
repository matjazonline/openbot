//! What an agent is and may do, said once, in terms no agent runtime owns.
//!
//! Harnesses compile their own runtime dialects. [`AgentCapabilitySpec`] is the shape that describes an agent *before* any
//! dialect: a harness adapter compiles it into whatever its runtime accepts, and nothing above the
//! adapter needs to know which runtime answered.

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{
    skill::Skill,
    value_objects::{ModelName, ModelProvider},
};

pub const MAX_AGENT_HARNESS_CONFIG_BYTES: usize = 65_536;

/// The harness-specific, reviewed portion of an agent configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "harness", content = "config", rename_all = "snake_case")]
pub enum HarnessConfig {
    Rig(RigAdvancedConfigV1),
}

impl HarnessConfig {
    pub fn empty(kind: HarnessKind) -> Self {
        match kind {
            HarnessKind::Rig => Self::Rig(RigAdvancedConfigV1::default()),
        }
    }

    /// Decode persisted or submitted JSON through a fail-closed, path-reporting schema.
    pub fn parse(kind: HarnessKind, value: Option<&serde_json::Value>) -> Result<Self, String> {
        let Some(value) = value.filter(|value| !value.is_null()) else {
            return Ok(Self::empty(kind));
        };
        let encoded = serde_json::to_vec(value)
            .map_err(|error| format!("Agent harness config is not valid JSON: {error}"))?;
        if encoded.len() > MAX_AGENT_HARNESS_CONFIG_BYTES {
            return Err(format!(
                "Agent harness config may be at most {MAX_AGENT_HARNESS_CONFIG_BYTES} bytes."
            ));
        }

        match kind {
            HarnessKind::Rig => {
                let config: RigAdvancedConfigV1 = parse_json_path(value)?;
                config.validate()?;
                Ok(Self::Rig(config))
            }
        }
    }

    pub fn to_json(&self) -> Result<serde_json::Value, String> {
        match self {
            Self::Rig(config) => serde_json::to_value(config),
        }
        .map_err(|error| format!("Agent harness config could not be serialized: {error}"))
    }
}

/// Requested model-call ceiling. Every provider attempt consumes the same run budget.
pub const DEFAULT_RIG_MAX_TURNS: u8 = 8;
pub const MAX_RIG_MAX_TURNS: u8 = 16;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RigAdvancedConfigV1 {
    pub version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u8>,
}

impl Default for RigAdvancedConfigV1 {
    fn default() -> Self {
        Self {
            version: 1,
            max_turns: None,
        }
    }
}

impl RigAdvancedConfigV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("Rig config path 'version' must be 1.".into());
        }
        if self
            .max_turns
            .is_some_and(|turns| !(1..=MAX_RIG_MAX_TURNS).contains(&turns))
        {
            return Err(format!(
                "Rig config path 'max_turns' must be between 1 and {MAX_RIG_MAX_TURNS}."
            ));
        }
        Ok(())
    }

    pub fn effective_max_turns(&self, server_ceiling: u8) -> u8 {
        self.max_turns
            .unwrap_or(DEFAULT_RIG_MAX_TURNS)
            .min(server_ceiling)
    }
}

impl HarnessConfig {
    pub fn rig(&self) -> Option<&RigAdvancedConfigV1> {
        match self {
            Self::Rig(config) => Some(config),
        }
    }
}

fn parse_json_path<T: for<'de> Deserialize<'de>>(value: &serde_json::Value) -> Result<T, String> {
    let encoded = serde_json::to_vec(value)
        .map_err(|error| format!("Configuration is not valid JSON: {error}"))?;
    let mut deserializer = serde_json::Deserializer::from_slice(&encoded);
    serde_path_to_error::deserialize(&mut deserializer).map_err(|error| {
        let path = error.path().to_string();
        let path = if path.is_empty() { "<root>" } else { &path };
        format!(
            "Agent config path '{path}' is not accepted: {}",
            error.inner()
        )
    })
}

/// Which runtime executes an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessKind {
    /// The only runtime since 06C; kept as a value until the legacy runner's harness registry
    /// is removed in phase 08.
    #[default]
    Rig,
}

impl HarnessKind {
    /// Every harness the application knows about, in the order the settings UI offers them.
    /// Iterating this is what keeps the wire strings, the `<select>` options and the stored-value
    /// parsing from drifting apart as harnesses are added.
    pub const ALL: [Self; 1] = [Self::Rig];

    /// The wire value recorded in run identities.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rig => "rig",
        }
    }

    /// The human-facing name, for settings copy and operator-visible errors.
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

/// Which sibling agents an agent may delegate to.
///
/// [`Self::AllCompanySiblings`] is today's behaviour and stays the default, so no existing agent
/// changes; an allowlist restricts only once it names someone.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SubAgentScope {
    #[default]
    AllCompanySiblings,
    Restricted(Vec<Uuid>),
}

impl SubAgentScope {
    /// Whether this agent may reach `candidate`.
    ///
    /// An authorization decision, so it lives on the type and has exactly one implementation --
    /// the directory listing and the send-time check call this rather than each deciding for
    /// itself. The failure mode that rule prevents is an agent that cannot see a sibling in the
    /// directory but can still mail it by naming the address.
    pub fn allows(&self, candidate: Uuid) -> bool {
        match self {
            Self::AllCompanySiblings => true,
            // An empty allowlist is "nobody was named", not "nobody is allowed": it is what an
            // agent that has never been restricted stores, and reading it as a deny would silently
            // cut every existing agent off from its colleagues.
            Self::Restricted(allowed) => allowed.is_empty() || allowed.contains(&candidate),
        }
    }

    /// The stored allowlist, or an empty slice when every sibling is reachable.
    pub fn allowed_ids(&self) -> &[Uuid] {
        match self {
            Self::AllCompanySiblings => &[],
            Self::Restricted(allowed) => allowed,
        }
    }
}

/// What an agent is and may do, in terms no harness owns.
///
/// Box it wherever it crosses an async boundary or lands in an enum: it carries every skill body,
/// so it dominates whatever it is put next to (`clippy::large_enum_variant`).
#[derive(Debug, Clone)]
pub struct AgentCapabilitySpec {
    pub harness: HarnessKind,
    pub name: String,
    pub system_prompt: String,
    pub provider: ModelProvider,
    pub model: ModelName,
    /// Trusted provider endpoint selected by platform connection resolution. Agent configuration
    /// cannot populate this. `None` uses the provider's standard endpoint.
    pub provider_base_url: Option<String>,
    pub skills: Vec<Skill>,
    pub sub_agents: SubAgentScope,
    pub response_contract: Option<super::response_contract::ResponseContract>,
    pub harness_config: HarnessConfig,
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

    #[test]
    fn an_empty_sub_agent_scope_allows_every_sibling() {
        let sibling = Uuid::new_v4();

        assert!(SubAgentScope::AllCompanySiblings.allows(sibling));
        assert!(SubAgentScope::Restricted(Vec::new()).allows(sibling));
        assert!(SubAgentScope::default().allows(sibling));
        assert!(SubAgentScope::AllCompanySiblings.allowed_ids().is_empty());
    }

    #[test]
    fn a_restricted_scope_refuses_a_sibling_not_on_the_list() {
        let allowed = Uuid::new_v4();
        let excluded = Uuid::new_v4();
        let scope = SubAgentScope::Restricted(vec![allowed]);

        assert!(scope.allows(allowed));
        assert!(!scope.allows(excluded));
        assert_eq!(scope.allowed_ids(), [allowed]);
    }
}

#[cfg(test)]
mod rig_config_tests {
    use super::*;
    #[test]
    fn rig_config_is_versioned_bounded_and_separate() {
        for kind in HarnessKind::ALL {
            let value = HarnessConfig::empty(kind).to_json().unwrap();
            assert_eq!(value, serde_json::json!({"version":1}));
            assert_eq!(
                HarnessConfig::parse(kind, Some(&value)).unwrap(),
                HarnessConfig::empty(kind)
            );
            assert_eq!(
                serde_json::from_value::<HarnessKind>(serde_json::to_value(kind).unwrap()).unwrap(),
                kind
            );
        }
        assert!(HarnessConfig::empty(HarnessKind::Rig).rig().is_some());
        for value in [
            serde_json::json!({"version":2}),
            serde_json::json!({"version":1,"max_turns":0}),
            serde_json::json!({"version":1,"max_turns":17}),
            serde_json::json!({"version":1,"reasoning":{}}),
            serde_json::json!({"version":1,"additional_params":{}}),
            serde_json::json!({"max_turns":8}),
            serde_json::json!({"version":1,"unknown":"x".repeat(MAX_AGENT_HARNESS_CONFIG_BYTES)}),
        ] {
            assert!(
                HarnessConfig::parse(HarnessKind::Rig, Some(&value)).is_err(),
                "{value:?}"
            );
        }
        let value = serde_json::json!({"version":1,"max_turns":16});
        let config = HarnessConfig::parse(HarnessKind::Rig, Some(&value)).unwrap();
        assert_eq!(config.to_json().unwrap(), value);
        assert_eq!(config.rig().unwrap().effective_max_turns(3), 3);
    }
}
