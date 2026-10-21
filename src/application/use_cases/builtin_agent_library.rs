//! Product-supplied agent-library definitions installed from typed Rust values at startup.

use async_trait::async_trait;
use tracing::info;
use uuid::Uuid;

use crate::{
    app_error::AppResult, entities::creation::CreationProvenance, use_cases::agent::AgentWrite,
};

/// Stable identity of the built-in definition. Existing rows are found by slug so an operator
/// who installed the same definition before this code shipped is not duplicated.
pub const AGENT_BUILDER_ID: Uuid = Uuid::from_u128(0xa6e17b15_871a_4cb7_81b9_62b70a51f155);
pub const AGENT_BUILDER_SLUG: &str = "agent-builder";

const AGENT_BUILDER_PROMPT: &str = r#"You are Agent Builder, a configuration specialist who helps a company create one clear, safe, focused AI agent through conversation.

Do not create an agent immediately. First understand the requested job and collect the missing decisions. Ask short, grouped questions and adapt them to what the user has already supplied. Do not make the user repeat information.

Before proposing creation, establish:
- the outcome the agent owns, its intended users, and examples of requests it should handle;
- responsibilities, non-goals, escalation conditions, required facts, output format, tone, and important constraints;
- a concise name, a lowercase hyphenated slug, and a one-line description;
- the complete system prompt, written as direct operational instructions for the new agent;
- the minimum workflow tools it needs and why;
- any existing company skills to attach, identified by their exact skill slugs.

Explain relevant tradeoffs in plain language. Prefer least privilege: do not propose a tool merely because it might be useful. Attach only skills the user explicitly identifies by exact company skill slug; if the user is unsure which skills exist, ask them to check the company skill library rather than inventing one. Ask the user whether an existing agent already covers the job; do not design a duplicate when extending or editing an existing agent is the better answer.

When the design is complete, show a compact final specification containing name, slug, description, responsibilities, non-goals, tools, skills, and the proposed system prompt. Ask for explicit confirmation. After confirmation, present the agreed specification once more as the final answer so the user can create the agent in the company agent settings. The new agent inherits the company's model and channel defaults."#;

#[derive(Debug, Clone)]
pub struct BuiltinAgentDefinition {
    pub id: Uuid,
    pub write: AgentWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinAgentInstallOutcome {
    Created,
    AlreadyPresent,
}

#[async_trait]
pub trait BuiltinAgentLibraryPersistence: Send + Sync {
    async fn ensure_library_agent(
        &self,
        definition: BuiltinAgentDefinition,
    ) -> AppResult<BuiltinAgentInstallOutcome>;
}

pub fn agent_builder_definition() -> BuiltinAgentDefinition {
    BuiltinAgentDefinition {
        id: AGENT_BUILDER_ID,
        write: AgentWrite {
            name: "Agent Builder".into(),
            slug: AGENT_BUILDER_SLUG.into(),
            description: Some(
                "Designs and creates focused company agents from a guided conversation.".into(),
            ),
            system_prompt: Some(AGENT_BUILDER_PROMPT.into()),
            created_by: Some(CreationProvenance::system()),
            ..AgentWrite::default()
        },
    }
}

pub async fn install_builtin_agent_library(
    persistence: &dyn BuiltinAgentLibraryPersistence,
) -> AppResult<()> {
    let outcome = persistence
        .ensure_library_agent(agent_builder_definition())
        .await?;
    info!(
        slug = AGENT_BUILDER_SLUG,
        ?outcome,
        "Built-in agent library definition is ready"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_builder_definition_is_valid_and_asks_for_confirmation() {
        let mut definition = agent_builder_definition();
        definition.write.normalize().unwrap();

        assert_eq!(definition.id, AGENT_BUILDER_ID);
        assert_eq!(definition.write.slug, AGENT_BUILDER_SLUG);
        assert!(
            definition
                .write
                .system_prompt
                .as_deref()
                .is_some_and(|prompt| {
                    prompt.contains("explicit confirmation") && prompt.contains("exact skill slugs")
                })
        );
    }
}
