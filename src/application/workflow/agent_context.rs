//! Pure request projections shared with the outer provider adapter.
use super::super::agent_history::{AgentHistoryBudget, FrozenAgentHistory};
use super::*;
use crate::services::harness::runs::{LogicalSeed, untrusted_context_prompt};

#[derive(Serialize)]
pub struct AgentToolDeclaration {
    pub name: crate::application::agent::AgentModelToolName,
    pub description: String,
    pub parameters: Value,
}

impl PreparedAgent {
    pub fn declarations(&self) -> Vec<AgentToolDeclaration> {
        self.tools().iter().map(PreparedTool::declaration).collect()
    }

    pub fn history_budget(&self, seed: &LogicalSeed) -> AppResult<AgentHistoryBudget> {
        seed.validate()?;
        let contract = self.response_contract()?;
        let mut mandatory = json!({
            "preamble": crate::services::response_contract::final_answer_preamble(
                self.preamble().to_owned(), Some(&contract)),
            "tools": self.declarations(),
            "current": untrusted_context_prompt(self.logical_input().value())?,
        });
        // A recalled pack is mandatory input: history gets what remains (E7).
        if let Some(memory) = self.memory_prompt()? {
            mandatory["memory"] = Value::String(memory);
        }
        let policy = self.policy()?;
        let budget = AgentHistoryBudget {
            // Includes JSON escaping; the fixed allowance covers request/message framing.
            mandatory_tokens: (serde_json::to_vec(&mandatory)
                .map_err(|_| invalid("Invalid request budget"))?
                .len() as u64)
                * 2
                + 4096,
            history_tokens: (serde_json::to_vec(seed)
                .map_err(|_| invalid("Invalid seed budget"))?
                .len() as u64)
                * 2
                + seed.pairs().len() as u64 * 256,
            reserved_output_tokens: policy.request_output_tokens,
            request_limit: policy.request_input_tokens,
        };
        budget.validate()?;
        Ok(budget)
    }

    pub fn with_frozen_history(mut self, history: FrozenAgentHistory) -> AppResult<Self> {
        history.validate()?;
        if self.history_exchange().is_none()
            || self.history_budget(&history.seed)? != history.budget
        {
            return Err(invalid("Frozen history differs from the prepared request"));
        }
        self.history = Some(history);
        Ok(self)
    }

    pub fn frozen_history(&self) -> Option<&FrozenAgentHistory> {
        self.history.as_ref()
    }

    pub fn seed(&self) -> LogicalSeed {
        self.history
            .as_ref()
            .map(|history| history.seed.clone())
            .unwrap_or_default()
    }
}

impl PreparedTool {
    pub fn declaration(&self) -> AgentToolDeclaration {
        AgentToolDeclaration {
            name: self.declared.clone(),
            description: match self.target {
                AgentToolTarget::Knowledge => {
                    super::super::knowledge::KNOWLEDGE_TOOL_DESCRIPTION.into()
                }
                AgentToolTarget::Memory { tool } => tool.description().into(),
                _ => format!("Workflow tool {}", self.name),
            },
            parameters: self.input_schema.clone(),
        }
    }
}
