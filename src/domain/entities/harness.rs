//! What an agent is and may do, said once, in terms no agent runtime owns.
//!
//! Today there is exactly one harness -- the in-process `ai-agents` runtime -- and it reads a YAML
//! dialect of its own. [`AgentCapabilitySpec`] is the shape that describes an agent *before* any
//! dialect: a harness adapter compiles it into whatever its runtime accepts, and nothing above the
//! adapter needs to know which runtime answered.

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{
    skill::Skill,
    value_objects::{ModelName, ModelProvider, ToolId},
};

pub const MAX_AGENT_HARNESS_CONFIG_BYTES: usize = 65_536;
pub const MAX_NATIVE_TOOL_POLICY_BYTES: usize = 16_384;
pub const MAX_REASONING_ITERATIONS: u8 = 16;
pub const MAX_REFLECTION_RETRIES: u8 = 5;
pub const MAX_OUTREACH_TARGETS: u16 = 100;
pub const MAX_OUTREACH_TIMEOUT_HOURS: u16 = 720;
pub const MAX_DIRECTORY_RESULTS: u16 = 100;

/// The harness-specific, reviewed portion of an agent configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "harness", content = "config", rename_all = "snake_case")]
pub enum HarnessConfig {
    AiAgents(AiAgentsAdvancedConfigV1),
}

impl HarnessConfig {
    pub fn empty(kind: HarnessKind) -> Self {
        match kind {
            HarnessKind::AiAgents => Self::AiAgents(AiAgentsAdvancedConfigV1::default()),
        }
    }

    /// Decode persisted or submitted JSON through a fail-closed, path-reporting schema.
    pub fn parse(kind: HarnessKind, value: Option<&serde_json::Value>) -> Result<Self, String> {
        let Some(value) = value else {
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
            HarnessKind::AiAgents => {
                parse_json_path::<AiAgentsAdvancedConfigV1>(value).and_then(|config| {
                    config.validate()?;
                    Ok(Self::AiAgents(config))
                })
            }
        }
    }

    pub fn to_json(&self) -> Result<serde_json::Value, String> {
        match self {
            Self::AiAgents(config) => serde_json::to_value(config),
        }
        .map_err(|error| format!("Agent harness config could not be serialized: {error}"))
    }

    pub fn ai_agents(&self) -> Option<&AiAgentsAdvancedConfigV1> {
        match self {
            Self::AiAgents(config) => Some(config),
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

fn config_version() -> u8 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiAgentsAdvancedConfigV1 {
    pub version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<AiAgentsReasoningConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reflection: Option<AiAgentsReflectionConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disambiguation: Option<AiAgentsDisambiguationConfig>,
}

impl Default for AiAgentsAdvancedConfigV1 {
    fn default() -> Self {
        Self {
            version: config_version(),
            reasoning: None,
            reflection: None,
            disambiguation: None,
        }
    }
}

impl AiAgentsAdvancedConfigV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != config_version() {
            return Err(format!(
                "Unsupported ai-agents config version {}; expected 1.",
                self.version
            ));
        }
        if let Some(reasoning) = &self.reasoning
            && !(1..=MAX_REASONING_ITERATIONS).contains(&reasoning.max_iterations)
        {
            return Err(format!(
                "Reasoning max_iterations must be between 1 and {MAX_REASONING_ITERATIONS}."
            ));
        }
        if let Some(reflection) = &self.reflection
            && reflection.max_retries > MAX_REFLECTION_RETRIES
        {
            return Err(format!(
                "Reflection max_retries must be between 0 and {MAX_REFLECTION_RETRIES}."
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiAgentsReasoningMode {
    #[default]
    None,
    ChainOfThought,
    React,
    Auto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiAgentsReasoningConfig {
    #[serde(default)]
    pub mode: AiAgentsReasoningMode,
    #[serde(default = "default_reasoning_iterations")]
    pub max_iterations: u8,
}

fn default_reasoning_iterations() -> u8 {
    5
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiAgentsReflectionMode {
    #[default]
    Disabled,
    Enabled,
    Auto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiAgentsReflectionConfig {
    #[serde(default)]
    pub enabled: AiAgentsReflectionMode,
    #[serde(default = "default_reflection_retries")]
    pub max_retries: u8,
}

fn default_reflection_retries() -> u8 {
    2
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiAgentsDisambiguationConfig {
    #[serde(default)]
    pub enabled: bool,
}

/// Typed policy for the native tools. Approval and execution ceilings remain server-owned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeToolPolicy {
    pub version: u8,
    #[serde(default)]
    pub outreach: OutreachToolPolicy,
    #[serde(default)]
    pub directory: DirectoryToolPolicy,
}

impl Default for NativeToolPolicy {
    fn default() -> Self {
        Self {
            version: config_version(),
            outreach: OutreachToolPolicy::default(),
            directory: DirectoryToolPolicy::default(),
        }
    }
}

impl NativeToolPolicy {
    pub fn parse(value: &serde_json::Value) -> Result<Self, String> {
        let encoded = serde_json::to_vec(value)
            .map_err(|error| format!("Native tool policy is not valid JSON: {error}"))?;
        if encoded.len() > MAX_NATIVE_TOOL_POLICY_BYTES {
            return Err(format!(
                "Native tool policy may be at most {MAX_NATIVE_TOOL_POLICY_BYTES} bytes."
            ));
        }
        let policy = parse_json_path::<Self>(value)?;
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != config_version() {
            return Err(format!(
                "Unsupported native tool policy version {}; expected 1.",
                self.version
            ));
        }
        if !(1..=MAX_OUTREACH_TARGETS).contains(&self.outreach.max_targets) {
            return Err(format!(
                "Outreach max_targets must be between 1 and {MAX_OUTREACH_TARGETS}."
            ));
        }
        if !(1..=MAX_OUTREACH_TIMEOUT_HOURS).contains(&self.outreach.max_timeout_hours) {
            return Err(format!(
                "Outreach max_timeout_hours must be between 1 and {MAX_OUTREACH_TIMEOUT_HOURS}."
            ));
        }
        if self.outreach.default_timeout_hours == 0
            || self.outreach.default_timeout_hours > self.outreach.max_timeout_hours
        {
            return Err(
                "Outreach default_timeout_hours must be between 1 and max_timeout_hours."
                    .to_string(),
            );
        }
        if !(1..=MAX_DIRECTORY_RESULTS).contains(&self.directory.max_results) {
            return Err(format!(
                "Directory max_results must be between 1 and {MAX_DIRECTORY_RESULTS}."
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutreachToolPolicy {
    #[serde(default = "default_outreach_targets")]
    pub max_targets: u16,
    #[serde(default = "default_outreach_timeout")]
    pub default_timeout_hours: u16,
    #[serde(default = "max_outreach_timeout")]
    pub max_timeout_hours: u16,
    #[serde(default)]
    pub allowed_target_scope: OutreachTargetScope,
}

impl Default for OutreachToolPolicy {
    fn default() -> Self {
        Self {
            max_targets: default_outreach_targets(),
            default_timeout_hours: default_outreach_timeout(),
            max_timeout_hours: max_outreach_timeout(),
            allowed_target_scope: OutreachTargetScope::default(),
        }
    }
}

fn default_outreach_targets() -> u16 {
    50
}

fn default_outreach_timeout() -> u16 {
    96
}

fn max_outreach_timeout() -> u16 {
    MAX_OUTREACH_TIMEOUT_HOURS
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutreachTargetScope {
    #[default]
    ExternalOnly,
    SameCompanyChannels,
    Any,
}

impl OutreachTargetScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExternalOnly => "external_only",
            Self::SameCompanyChannels => "same_company_channels",
            Self::Any => "any",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryToolPolicy {
    #[serde(default = "default_directory_results")]
    pub max_results: u16,
}

impl Default for DirectoryToolPolicy {
    fn default() -> Self {
        Self {
            max_results: default_directory_results(),
        }
    }
}

fn default_directory_results() -> u16 {
    50
}

/// Which runtime executes an agent.
///
/// One variant looks silly and is not: it is the enum a second harness becomes a variant of, and
/// the `match` that then fails to compile at every place a decision must be made. A sandboxed
/// runtime -- the thing that would let the host-access built-ins be granted at all -- attaches
/// here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessKind {
    /// The `ai-agents` runtime, in this process. The only harness today.
    #[default]
    AiAgents,
}

impl HarnessKind {
    /// Every harness the application knows about, in the order the settings UI offers them.
    /// Iterating this is what keeps the wire strings, the `<select>` options and the stored-value
    /// parsing from drifting apart as harnesses are added.
    pub const ALL: [Self; 1] = [Self::AiAgents];

    /// The wire and database value. Must stay in sync with the `agents_harness_kind_check`
    /// CHECK in `migrations/20260817000000_init_schema.sql`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AiAgents => "ai_agents",
        }
    }

    /// The human-facing name, for settings copy and operator-visible errors.
    pub fn label(self) -> &'static str {
        match self {
            Self::AiAgents => "ai-agents (in process)",
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
    pub granted_tools: Vec<ToolId>,
    pub sub_agents: SubAgentScope,
    pub harness_config: HarnessConfig,
}

impl AgentCapabilitySpec {
    /// Every tool the harness must put in its grant list: what was granted directly, plus what the
    /// attached skills' steps invoke.
    ///
    /// The union happens here rather than in each adapter because forgetting it is not a
    /// compilation error -- it is a skill that dies mid-run when the runtime denies a step it was
    /// never granted. Order is stable: direct grants first, then each skill's tools in the order
    /// its steps use them.
    pub fn required_tool_ids(&self) -> Vec<ToolId> {
        let mut ids: Vec<ToolId> = Vec::new();
        let from_skills = self.skills.iter().flat_map(Skill::referenced_tool_ids);
        for tool in self.granted_tools.iter().cloned().chain(from_skills) {
            if !ids.contains(&tool) {
                ids.push(tool);
            }
        }
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::Utc;

    use crate::entities::{
        creation::CreationProvenance,
        skill::{Skill, SkillInstruction},
        value_objects::SkillSlug,
    };

    fn skill_using(slug: &str, tools: &[&str]) -> Skill {
        let mut instructions: Vec<SkillInstruction> = tools
            .iter()
            .map(|tool| SkillInstruction::Tool {
                tool: ToolId::from(*tool),
                args: None,
                output_as: None,
            })
            .collect();
        instructions.push(SkillInstruction::Prompt {
            text: "Answer.".to_string(),
        });

        Skill {
            id: Uuid::new_v4(),
            company_id: Some(Uuid::new_v4()),
            slug: SkillSlug::parse(slug).expect("a well-formed slug"),
            name: "A skill".to_string(),
            description: "What it does.".to_string(),
            trigger: "When to use it.".to_string(),
            instructions,
            created_by: CreationProvenance::system(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn spec(granted: &[&str], skills: Vec<Skill>) -> AgentCapabilitySpec {
        AgentCapabilitySpec {
            harness: HarnessKind::AiAgents,
            name: "Support".to_string(),
            system_prompt: "You are a helpful email agent.".to_string(),
            provider: ModelProvider::canonical("anthropic"),
            model: ModelName::from("claude-opus-5"),
            provider_base_url: None,
            skills,
            granted_tools: granted.iter().map(|id| ToolId::from(*id)).collect(),
            sub_agents: SubAgentScope::AllCompanySiblings,
            harness_config: HarnessConfig::empty(HarnessKind::AiAgents),
        }
    }

    #[test]
    fn required_tool_ids_unions_direct_grants_with_every_skill_step() {
        let spec = spec(
            &["calculator", "datetime"],
            vec![
                skill_using("read-the-clock", &["datetime", "json"]),
                skill_using("do-the-sums", &["math", "json"]),
            ],
        );

        assert_eq!(
            spec.required_tool_ids(),
            [
                ToolId::from("calculator"),
                ToolId::from("datetime"),
                ToolId::from("json"),
                ToolId::from("math"),
            ],
            "grants first, then each skill's tools in first-use order, deduplicated"
        );
    }

    #[test]
    fn required_tool_ids_of_a_spec_with_no_skills_is_its_grant_list() {
        let repeated = spec(&["datetime", "datetime"], Vec::new());

        assert_eq!(repeated.required_tool_ids(), [ToolId::from("datetime")]);
        assert!(spec(&[], Vec::new()).required_tool_ids().is_empty());
    }

    #[test]
    fn harness_kind_round_trips_through_as_str_and_parse() {
        for kind in HarnessKind::ALL {
            assert_eq!(HarnessKind::parse(kind.as_str()), Some(kind));
            assert_eq!(kind.to_string(), kind.as_str());
            assert!(!kind.label().trim().is_empty());
        }

        assert_eq!(HarnessKind::parse("opencode_microvm"), None);
        assert_eq!(HarnessKind::parse(""), None);
        assert_eq!(HarnessKind::default(), HarnessKind::AiAgents);
    }

    #[test]
    fn harness_kind_serializes_as_its_stored_value() {
        let stored = serde_json::to_value(HarnessKind::AiAgents).expect("a harness kind");
        assert_eq!(stored, serde_json::json!("ai_agents"));
        assert_eq!(
            serde_json::from_value::<HarnessKind>(stored).expect("round trip"),
            HarnessKind::AiAgents
        );
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
