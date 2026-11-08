use super::{AgentExchangeId, AgentExchangeOrder};
use crate::{
    app_error::{AppError, AppResult},
    entities::access_level::AccessLevel,
    services::harness::runs::LogicalSeed,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedAgentExchange {
    pub id: AgentExchangeId,
    pub position: AgentExchangeOrder,
    pub access_level: AccessLevel,
}

/// Conservative byte-per-token estimates; byte and item ceilings remain separate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentHistoryBudget {
    pub mandatory_tokens: u64,
    pub history_tokens: u64,
    pub reserved_output_tokens: u64,
    pub request_limit: u64,
}
impl AgentHistoryBudget {
    pub fn validate(self) -> AppResult<()> {
        let total = self
            .mandatory_tokens
            .checked_add(self.history_tokens)
            .and_then(|n| n.checked_add(self.reserved_output_tokens));
        if total.is_none_or(|n| n > self.request_limit) {
            return Err(AppError::Execution(
                crate::app_error::ExecutionFailure::Budget,
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenAgentHistory {
    pub selected: Vec<SelectedAgentExchange>,
    pub seed: LogicalSeed,
    pub omitted_eligible: u64,
    pub selected_level: AccessLevel,
    pub content_level: AccessLevel,
    pub budget: AgentHistoryBudget,
}
impl FrozenAgentHistory {
    pub fn validate(&self) -> AppResult<()> {
        self.seed.validate()?;
        self.budget.validate()?;
        let mut identities = std::collections::HashSet::new();
        if self.selected.len() != self.seed.pairs().len()
            || self.selected.iter().any(|item| !identities.insert(item.id))
            || self
                .selected
                .windows(2)
                .any(|pair| pair[0].position >= pair[1].position)
            || self
                .selected
                .iter()
                .map(|item| item.access_level)
                .max()
                .unwrap_or(AccessLevel::PUBLIC)
                != self.selected_level
            || self.content_level < self.selected_level
            || self.omitted_eligible > i64::MAX as u64
        {
            return Err(AppError::BadRequest("Invalid frozen agent history".into()));
        }
        Ok(())
    }
}
