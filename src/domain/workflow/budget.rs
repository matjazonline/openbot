//! Immutable ceilings for all work descended from one admitted root.
use thiserror::Error;
use uuid::Uuid;

pub const MAX_ROOT_ACTIVATIONS: u32 = 100_000;
pub const MAX_ROOT_MODEL_CALLS: u32 = 1_000;
pub const MAX_ROOT_REPETITIONS: u32 = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenRefusal {
    Rejected,
    Expired,
    Cancelled,
}

impl TokenRefusal {
    pub const fn failure_code(self) -> &'static str {
        match self {
            Self::Rejected => "workflow.tokens_rejected",
            Self::Expired => "workflow.tokens_expired",
            Self::Cancelled => "workflow.tokens_cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetResource {
    Activation,
    ModelCall,
    Repetition,
}

impl BudgetResource {
    pub const fn maximum(self) -> u32 {
        match self {
            Self::Activation => MAX_ROOT_ACTIVATIONS,
            Self::ModelCall => MAX_ROOT_MODEL_CALLS,
            Self::Repetition => MAX_ROOT_REPETITIONS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("Root budget quantity must be positive and within its platform ceiling")]
pub struct BudgetLimitError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootBudgetLimits {
    activations: u32,
    model_calls: u32,
    repetitions: u32,
    tokens: Option<TokenAllowance>,
}

impl RootBudgetLimits {
    pub fn new(
        activations: u32,
        model_calls: u32,
        repetitions: u32,
    ) -> Result<Self, BudgetLimitError> {
        BudgetCharge::new(BudgetResource::Activation, activations)?;
        BudgetCharge::new(BudgetResource::ModelCall, model_calls)?;
        BudgetCharge::new(BudgetResource::Repetition, repetitions)?;
        Ok(Self {
            activations,
            model_calls,
            repetitions,
            tokens: None,
        })
    }

    pub fn with_tokens(mut self, tokens: TokenAllowance) -> Self {
        self.tokens = Some(tokens);
        self
    }

    pub fn tokens(&self) -> Option<&TokenAllowance> {
        self.tokens.as_ref()
    }

    pub fn limit(&self, resource: BudgetResource) -> u32 {
        match resource {
            BudgetResource::Activation => self.activations,
            BudgetResource::ModelCall => self.model_calls,
            BudgetResource::Repetition => self.repetitions,
        }
    }
}

impl Default for RootBudgetLimits {
    fn default() -> Self {
        Self {
            activations: MAX_ROOT_ACTIVATIONS,
            model_calls: MAX_ROOT_MODEL_CALLS,
            repetitions: MAX_ROOT_REPETITIONS,
            tokens: None,
        }
    }
}

/// An optional human allowance; hard model-call and execution bounds still apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenAllowance {
    max_tokens: u32,
    reviewers: Vec<Uuid>,
}

impl TokenAllowance {
    pub const MAX_TOKENS: u32 = i32::MAX as u32;
    pub const MAX_REVIEWERS: usize = 16;

    pub fn new(max_tokens: u32, reviewers: Vec<Uuid>) -> Result<Self, BudgetLimitError> {
        if max_tokens == 0
            || max_tokens > Self::MAX_TOKENS
            || reviewers.is_empty()
            || reviewers.len() > Self::MAX_REVIEWERS
            || reviewers
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != reviewers.len()
        {
            return Err(BudgetLimitError);
        }
        Ok(Self {
            max_tokens,
            reviewers,
        })
    }

    pub fn max_tokens(&self) -> u32 {
        self.max_tokens
    }
    pub fn reviewers(&self) -> &[Uuid] {
        &self.reviewers
    }
}

/// A reservation is an irreversible debit, not a measured post-work refund.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetCharge {
    resource: BudgetResource,
    quantity: u32,
}

impl BudgetCharge {
    pub fn new(resource: BudgetResource, quantity: u32) -> Result<Self, BudgetLimitError> {
        if quantity == 0 || quantity > resource.maximum() {
            return Err(BudgetLimitError);
        }
        Ok(Self { resource, quantity })
    }

    pub fn resource(self) -> BudgetResource {
        self.resource
    }
    pub fn quantity(self) -> u32 {
        self.quantity
    }

    /// Refuse invalid persisted consumption and overflow as well as exhaustion.
    pub fn consumed_after(self, limits: &RootBudgetLimits, consumed: u32) -> Option<u32> {
        consumed
            .checked_add(self.quantity)
            .filter(|total| *total <= limits.limit(self.resource))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_root_budget_boundaries_and_checked_debits() {
        for resource in [
            BudgetResource::Activation,
            BudgetResource::ModelCall,
            BudgetResource::Repetition,
        ] {
            let maximum = resource.maximum();
            assert!(BudgetCharge::new(resource, 0).is_err());
            assert!(BudgetCharge::new(resource, maximum + 1).is_err());
            let exact = BudgetCharge::new(resource, maximum).unwrap();
            assert_eq!(
                exact.consumed_after(&RootBudgetLimits::default(), 0),
                Some(maximum)
            );
            assert_eq!(exact.consumed_after(&RootBudgetLimits::default(), 1), None);
            assert_eq!(
                exact.consumed_after(&RootBudgetLimits::default(), u32::MAX),
                None
            );
            let one = BudgetCharge::new(resource, 1).unwrap();
            let lower = RootBudgetLimits::new(2, 2, 2).unwrap();
            assert_eq!(one.consumed_after(&lower, 1), Some(2));
            assert_eq!(one.consumed_after(&lower, 2), None);
        }
        for invalid in [
            (0, 1, 1),
            (1, 0, 1),
            (1, 1, 0),
            (MAX_ROOT_ACTIVATIONS + 1, 1, 1),
            (1, MAX_ROOT_MODEL_CALLS + 1, 1),
            (1, 1, MAX_ROOT_REPETITIONS + 1),
        ] {
            assert!(RootBudgetLimits::new(invalid.0, invalid.1, invalid.2).is_err());
        }
    }
}
