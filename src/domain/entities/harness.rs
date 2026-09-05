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
    pub skills: Vec<Skill>,
    pub granted_tools: Vec<ToolId>,
    pub sub_agents: SubAgentScope,
    /// The residual `agents.config_json` -- everything the typed fields above do not own. Merged
    /// over the harness's own defaults by the adapter, exactly as the runner's `merge_json` does
    /// today.
    pub extra_config: serde_json::Value,
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
        }
    }

    fn spec(granted: &[&str], skills: Vec<Skill>) -> AgentCapabilitySpec {
        AgentCapabilitySpec {
            harness: HarnessKind::AiAgents,
            name: "Support".to_string(),
            system_prompt: "You are a helpful email agent.".to_string(),
            provider: ModelProvider::canonical("anthropic"),
            model: ModelName::from("claude-opus-5"),
            skills,
            granted_tools: granted.iter().map(|id| ToolId::from(*id)).collect(),
            sub_agents: SubAgentScope::AllCompanySiblings,
            extra_config: serde_json::json!({}),
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
    fn a_sub_agent_scope_with_no_entries_allows_every_sibling() {
        let sibling = Uuid::new_v4();

        assert!(SubAgentScope::AllCompanySiblings.allows(sibling));
        assert!(SubAgentScope::Restricted(Vec::new()).allows(sibling));
        assert!(SubAgentScope::default().allows(sibling));
        assert!(SubAgentScope::AllCompanySiblings.allowed_ids().is_empty());
    }

    #[test]
    fn a_restricted_sub_agent_scope_allows_only_the_named_siblings() {
        let allowed = Uuid::new_v4();
        let excluded = Uuid::new_v4();
        let scope = SubAgentScope::Restricted(vec![allowed]);

        assert!(scope.allows(allowed));
        assert!(!scope.allows(excluded));
        assert_eq!(scope.allowed_ids(), [allowed]);
    }
}
