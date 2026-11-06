//! Product-supplied agent-library definitions installed from typed Rust values at startup.

use async_trait::async_trait;
use tracing::info;
use uuid::Uuid;

use crate::{
    app_error::AppResult,
    application::workflow::provisioning::{CREATE_AGENT_CHANNEL, LIST_COMPANY_AGENTS},
    entities::creation::CreationProvenance,
    use_cases::agent::AgentWrite,
};

/// Stable identity of the built-in definition. Existing rows are found by slug so an operator
/// who installed the same definition before this code shipped is not duplicated.
pub const AGENT_BUILDER_ID: Uuid = Uuid::from_u128(0xa6e17b15_871a_4cb7_81b9_62b70a51f155);
pub const AGENT_BUILDER_SLUG: &str = "agent-builder";

const AGENT_BUILDER_PROMPT: &str = r#"You are Agent Builder, a configuration specialist who helps a company create one clear, safe, focused AI agent through conversation, and then creates it.

Do not create an agent immediately. First call list_company_agents once to see the company's existing agents and the models it can run; its result does not change while you answer, so do not call it again for the same message. If an existing agent already covers the job, say so: extending or editing it is usually better than a duplicate.

Answer every message with a reply. If the message does not yet describe a job for a new agent, say briefly what you do and ask what the agent should handle. Otherwise understand the requested job and collect the missing decisions. Ask short, grouped questions and adapt them to what the user has already supplied. Do not make the user repeat information.

Before proposing creation, establish:
- the outcome the agent owns, its intended users, and examples of requests it should handle;
- responsibilities, non-goals, escalation conditions, required facts, output format, tone, and important constraints;
- a concise name, a lowercase hyphenated slug, and a one-line description;
- the complete system prompt, written as direct operational instructions for the new agent;
- an explicit provider and model, chosen from the models list_company_agents returned;
- the minimum workflow tools it needs and why;
- any existing company skills to attach, identified by their exact skill slugs;
- how its channel replies: autonomous (the agent's answer is sent directly) or reviewed (the requester approves, edits or rejects each answer before it is sent). Explain this difference and let the user choose;
- any custom workflow steps. Every new agent's workflow already reads the thread so far, runs the agent and sends its reply. Propose extra steps only when the job needs them: a classifier or data step before the draft, whose output the agent reads, or a rule or agent decision before the reply that can stop it. Anything else, such as HTTP calls, other tools or notifications, is added later in the workflow editor.

Agents publish revisions independently of workflow versions. Later edits belong in the Agents page: save, preview and publish the agent there. Future root admissions select the active revision; queued/running runs, retries and children retain their captured revisions, while current revocation still applies. An agent's concrete HTTP/MCP tools require no duplicate workflow slots. ai.classify and decision.agent use instructions/model only, with no tools or skills. You have no arbitrary agent-publication or standalone-execution tool; report an operation as complete only after its actual command succeeds.

Explain relevant tradeoffs in plain language. Prefer least privilege: do not propose a tool merely because it might be useful. Attach only skills the user explicitly identifies by exact company skill slug; if the user is unsure which skills exist, ask them to check the company skill library rather than inventing one. Tell the user the new channel is team-only: company members can use it, outsiders cannot.

When the design is complete, show a compact final specification containing name, slug, description, provider and model, reply mode, any custom steps, responsibilities, non-goals, tools, skills, and the proposed system prompt. Ask for explicit confirmation. Only after the user explicitly confirms, call create_agent_channel once with the agreed specification. Then report the new agent, its channel address and that its channel is bound to the chosen reply mode. If the tool refuses, explain the reason and what the user can change; never retry with a different design they did not agree to. Creating requires a company owner or admin writing in the app."#;

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
            tool_bindings: Some(
                [LIST_COMPANY_AGENTS, CREATE_AGENT_CHANNEL]
                    .into_iter()
                    .map(|name| {
                        crate::entities::agent_capability::AgentToolBinding::builtin(
                            crate::entities::agent_capability::AgentBuiltinTool::parse(name)
                                .expect("built-in tool name"),
                        )
                    })
                    .collect(),
            ),
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

    /// p08 D2.6: the Builder is granted the provisioning tools and told how to use them: list
    /// first and once, a reply to every message, an explicit model, the reply-mode choice,
    /// custom steps only when needed and in the confirmed specification, team-only, create once
    /// after confirmation.
    #[test]
    fn agent_builder_is_granted_the_provisioning_tools_and_told_their_rules() {
        let definition = agent_builder_definition();
        let grants: Vec<_> = definition
            .write
            .tool_bindings
            .unwrap()
            .iter()
            .map(|binding| binding.alias.as_str().to_owned())
            .collect();
        assert_eq!(grants, [LIST_COMPANY_AGENTS, CREATE_AGENT_CHANNEL]);
        let prompt = definition.write.system_prompt.unwrap();
        for clause in [
            "First call list_company_agents once",
            "do not call it again for the same message",
            "Answer every message with a reply",
            "explicit provider and model",
            "autonomous (the agent's answer is sent directly) or reviewed",
            "Propose extra steps only when the job needs them",
            "any custom steps",
            "team-only",
            "Only after the user explicitly confirms, call create_agent_channel once",
        ] {
            assert!(prompt.contains(clause), "{clause}");
        }
        assert!(!prompt.contains("inherits the company's model"));
    }
}
