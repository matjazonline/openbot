use super::{Binding, ChoiceName, Context, ContextError, ContextLimits, resolve};
use std::collections::BTreeSet;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct RuleCase {
    pub when: Binding,
    pub choice: ChoiceName,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderedRule {
    pub cases: Vec<RuleCase>,
    pub default: ChoiceName,
}

#[derive(Debug, Error)]
pub enum RuleError {
    #[error("rule must have at least one case and at most 256")]
    InvalidCases,
    #[error("rule selects undeclared choice {0}")]
    UnknownChoice(ChoiceName),
    #[error("rule predicate failed: {0}")]
    Predicate(#[from] ContextError),
    #[error("rule predicate must resolve to a boolean")]
    NonBoolean,
}

impl OrderedRule {
    pub fn check(&self, choices: &BTreeSet<ChoiceName>) -> Result<(), RuleError> {
        if self.cases.is_empty() || self.cases.len() > 256 {
            return Err(RuleError::InvalidCases);
        }
        for choice in self
            .cases
            .iter()
            .map(|case| &case.choice)
            .chain([&self.default])
        {
            if !choices.contains(choice) {
                return Err(RuleError::UnknownChoice(choice.clone()));
            }
        }
        Ok(())
    }

    pub fn decide(
        &self,
        context: &Context<'_>,
        limits: ContextLimits,
    ) -> Result<ChoiceName, RuleError> {
        for case in &self.cases {
            let serde_json::Value::Bool(matches) = resolve(&case.when, context, limits)? else {
                return Err(RuleError::NonBoolean);
            };
            if matches {
                return Ok(case.choice.clone());
            }
        }
        Ok(self.default.clone())
    }
}
